//! `overpull why` — why is this module in the graph at all?
//!
//! The question people actually ask when a number surprises them is not
//! "how big is the graph" but "who dragged *that* in". This walks back from
//! each entry point to one named module and shows the shortest chain of
//! import lines that reaches it, so the answer is a route, not a count.

use crate::entries::{Entry, EntryKind};
use crate::graph::ModuleGraph;

pub struct WhyPath {
    pub entry: usize,
    pub kind: EntryKind,
    /// Modules from the entry to the target, inclusive.
    pub path: Vec<usize>,
    /// Source line of each hop; one shorter than `path`.
    pub lines: Vec<u32>,
}

pub struct WhyReport {
    pub target: usize,
    /// One shortest chain per entry point that reaches the target at load
    /// time, shortest first.
    pub paths: Vec<WhyPath>,
    /// Entry points from which the target never loads.
    pub unreachable_entries: usize,
    /// Modules importing the target directly: (module, line, specifier).
    pub direct_importers: Vec<(usize, u32, String)>,
    /// Modules reaching it only through `import()`, where it loads on demand
    /// rather than at startup: (module, line).
    pub dynamic_importers: Vec<(usize, u32)>,
    /// What the target itself costs once it is loaded.
    pub cost_modules: usize,
    pub cost_bytes: u64,
}

pub fn analyze(graph: &ModuleGraph, entries: &[Entry], target: usize) -> WhyReport {
    let mut paths: Vec<WhyPath> = Vec::new();
    let mut unreachable_entries = 0usize;

    for entry in entries {
        match graph.shortest_import_path(entry.module, target, None) {
            Some(path) => {
                let lines = hop_lines(graph, &path);
                paths.push(WhyPath {
                    entry: entry.module,
                    kind: entry.kind,
                    path,
                    lines,
                });
            }
            None => unreachable_entries += 1,
        }
    }
    // Shortest chain first: it is the one a reader can act on.
    paths.sort_by(|a, b| {
        a.path
            .len()
            .cmp(&b.path.len())
            .then_with(|| a.kind.cmp(&b.kind))
            .then_with(|| a.entry.cmp(&b.entry))
    });

    let mut direct_importers = Vec::new();
    for &importer in &graph.importers[target] {
        if let Some(edge) = graph.modules[importer]
            .edges
            .iter()
            .find(|e| e.to == target)
        {
            direct_importers.push((importer, edge.line, edge.specifier.clone()));
        }
    }

    let mut dynamic_importers = Vec::new();
    for (index, module) in graph.modules.iter().enumerate() {
        for &(to, line) in &module.dynamic_edges {
            if to == target {
                dynamic_importers.push((index, line));
            }
        }
    }

    let cost = graph.load_cost(target);
    WhyReport {
        target,
        paths,
        unreachable_entries,
        direct_importers,
        dynamic_importers,
        cost_modules: cost.modules.len(),
        cost_bytes: cost.total_bytes,
    }
}

/// The import line behind each hop of a path.
fn hop_lines(graph: &ModuleGraph, path: &[usize]) -> Vec<u32> {
    path.windows(2)
        .map(|hop| {
            graph.modules[hop[0]]
                .edges
                .iter()
                .find(|edge| edge.to == hop[1])
                .map_or(0, |edge| edge.line)
        })
        .collect()
}
