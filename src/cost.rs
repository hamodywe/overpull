//! `overpull cost` — what importing a module actually loads.
//!
//! For each entry: the transitive set of project modules evaluated at
//! startup, total source bytes, external packages touched, and — the part
//! nothing else reports — per-import attribution: for every direct import of
//! the entry, how many modules disappear if that one line goes away.

use crate::graph::ModuleGraph;

pub struct EntryCostReport {
    pub entry: usize,
    pub module_count: usize,
    pub total_bytes: u64,
    pub external_packages: Vec<String>,
    pub dynamic_target_count: usize,
    pub opaque_dynamic_imports: usize,
    pub unresolved: usize,
    /// (target module, line, specifier, modules only reachable through this
    /// edge) — sorted by exclusive contribution, largest first.
    pub contributors: Vec<Contributor>,
}

pub struct Contributor {
    pub target: usize,
    pub line: u32,
    pub specifier: String,
    pub exclusive_modules: usize,
}

pub fn analyze(graph: &ModuleGraph, entries: &[usize]) -> Vec<EntryCostReport> {
    entries
        .iter()
        .map(|&entry| {
            let full = graph.load_cost(entry);
            let mut contributors: Vec<Contributor> = graph.modules[entry]
                .edges
                .iter()
                .map(|edge| {
                    let without = graph.load_cost_excluding(entry, Some((entry, edge.to)));
                    Contributor {
                        target: edge.to,
                        line: edge.line,
                        specifier: edge.specifier.clone(),
                        exclusive_modules: full.modules.len().saturating_sub(without.modules.len()),
                    }
                })
                .collect();
            contributors.sort_by(|a, b| {
                b.exclusive_modules
                    .cmp(&a.exclusive_modules)
                    .then_with(|| a.line.cmp(&b.line))
            });

            EntryCostReport {
                entry,
                module_count: full.modules.len(),
                total_bytes: full.total_bytes,
                external_packages: full.external_packages.into_iter().collect(),
                dynamic_target_count: full.dynamic_targets.len(),
                opaque_dynamic_imports: full.opaque_dynamic_imports,
                unresolved: full.unresolved,
                contributors,
            }
        })
        .collect()
}
