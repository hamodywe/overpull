//! The resolved module graph and the queries every command runs on it.
//!
//! Nodes are project source files. Edges are resolved imports, split by what
//! they mean at runtime: static ESM edges load with the importer, dynamic
//! `import()` edges load later, `require` edges load synchronously with CJS
//! semantics. External packages are counted at the boundary, not traversed —
//! overpull measures *your* graph; `node_modules` is a separate bill.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

use rayon::prelude::*;

use crate::model::{DeclKind, FileFacts, ReExportSource};
use crate::parse::{is_source_file, parse_file};
use crate::resolve::{SpecResolver, Target};

/// How far a name is followed through re-export chains before the search
/// gives up. Real barrels nest a handful deep; anything past this is either
/// generated or adversarial, and either way is not worth a deep stack.
const MAX_REEXPORT_DEPTH: usize = 64;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EdgeKind {
    /// `import`/`export … from` — evaluated with the importer.
    Static,
    /// `require()` / `import x = require()` — loads with the importer, CJS
    /// evaluation semantics.
    Require,
}

pub struct Edge {
    pub to: usize,
    pub kind: EdgeKind,
    pub line: u32,
    pub specifier: String,
    /// Index into the importer's `facts.imports`; `None` for require edges.
    pub import_idx: Option<usize>,
}

pub struct Module {
    pub facts: FileFacts,
    /// Runtime edges to other project modules, in source-line order.
    pub edges: Vec<Edge>,
    /// Resolved dynamic-import edges: (target module, line).
    pub dynamic_edges: Vec<(usize, u32)>,
    /// Dynamic imports whose specifier is computed and cannot be followed.
    pub opaque_dynamic_imports: usize,
    /// Distinct external packages this module pulls at load time.
    pub external_packages: BTreeSet<String>,
    /// External packages behind dynamic imports (deferred).
    pub dynamic_external_packages: BTreeSet<String>,
    /// Type-only edges resolved to project modules (never load at runtime).
    pub type_edges: Vec<usize>,
    /// Specifiers that failed to resolve: (specifier, line).
    pub unresolved: Vec<(String, u32)>,
}

pub struct ModuleGraph {
    pub root: PathBuf,
    pub modules: Vec<Module>,
    pub index_by_path: HashMap<PathBuf, usize>,
    /// Reverse runtime adjacency: for each module, who imports it.
    pub importers: Vec<Vec<usize>>,
}

/// Result of walking everything a module loads at startup.
pub struct Cost {
    /// Project modules evaluated, including the entry itself.
    pub modules: Vec<usize>,
    pub total_bytes: u64,
    pub external_packages: BTreeSet<String>,
    /// Dynamic boundaries reached (modules only loaded on demand).
    pub dynamic_targets: BTreeSet<usize>,
    pub opaque_dynamic_imports: usize,
    pub unresolved: usize,
}

impl ModuleGraph {
    /// Builds the graph from `roots` (entry files or a whole discovered file
    /// set), following resolved local edges to closure — a resolved import
    /// may lead outside the initial set (a sibling workspace package) and is
    /// parsed on demand.
    pub fn build(root: &Path, seeds: &[PathBuf], resolver: &SpecResolver) -> Self {
        // Parse the seed set in parallel first; the closure loop below picks
        // up stragglers serially (rare in practice).
        let parsed: Vec<FileFacts> = seeds
            .par_iter()
            .map(PathBuf::as_path)
            .filter_map(|path| {
                fs::read_to_string(path)
                    .ok()
                    .map(|src| parse_file(path, &src))
            })
            .collect();

        let mut modules: Vec<Module> = Vec::with_capacity(parsed.len());
        let mut index_by_path: HashMap<PathBuf, usize> = HashMap::new();
        let mut queue: VecDeque<usize> = VecDeque::new();

        for facts in parsed {
            let path = facts.path.clone();
            if index_by_path.contains_key(&path) {
                continue;
            }
            let idx = modules.len();
            index_by_path.insert(path, idx);
            modules.push(new_module(facts));
            queue.push_back(idx);
        }

        while let Some(idx) = queue.pop_front() {
            let dir = modules[idx]
                .facts
                .path
                .parent()
                .map_or_else(|| root.to_path_buf(), Path::to_path_buf);

            // Requests are collected first so the borrow on modules[idx]
            // ends before the loop below mutates the vec.
            let (requests, opaque) = collect_requests(&modules[idx].facts);
            modules[idx].opaque_dynamic_imports = opaque;

            for req in requests {
                match resolver.resolve(&dir, &req.specifier) {
                    Target::Local(path) => {
                        let path = crate::util::normalize(&path);
                        // A resolved local non-source file (JSON, CSS, an
                        // asset) is a leaf: it loads, but has no edges to
                        // follow, so it does not enter the graph.
                        let target = match index_by_path.get(&path) {
                            Some(&existing) => Some(existing),
                            None if is_source_file(&path) => {
                                fs::read_to_string(&path).ok().map(|source| {
                                    let facts = parse_file(&path, &source);
                                    let new = modules.len();
                                    index_by_path.insert(path.clone(), new);
                                    modules.push(new_module(facts));
                                    queue.push_back(new);
                                    new
                                })
                            }
                            None => None,
                        };
                        if let Some(target) = target {
                            attach_local(&mut modules[idx], target, req);
                        }
                    }
                    Target::External(package) => match req.kind {
                        ReqKind::Dynamic => {
                            modules[idx].dynamic_external_packages.insert(package);
                        }
                        ReqKind::TypeOnly => {}
                        ReqKind::Static | ReqKind::Require => {
                            modules[idx].external_packages.insert(package);
                        }
                    },
                    Target::Builtin => {}
                    Target::Unresolved => {
                        if !matches!(req.kind, ReqKind::TypeOnly) {
                            modules[idx].unresolved.push((req.specifier, req.line));
                        }
                    }
                }
            }
        }

        let mut importers = vec![Vec::new(); modules.len()];
        for (from, module) in modules.iter().enumerate() {
            for edge in &module.edges {
                importers[edge.to].push(from);
            }
        }
        for list in &mut importers {
            list.sort_unstable();
            list.dedup();
        }

        Self {
            root: root.to_path_buf(),
            modules,
            index_by_path,
            importers,
        }
    }

    /// Everything `entry` loads at startup, following static and require
    /// edges. Dynamic boundaries are recorded, not crossed.
    pub fn load_cost(&self, entry: usize) -> Cost {
        self.load_cost_excluding(entry, None)
    }

    /// Same walk with one outgoing edge of the entry removed — the basis for
    /// per-import attribution ("cut this line, save that subgraph").
    pub fn load_cost_excluding(&self, entry: usize, skip_edge: Option<(usize, usize)>) -> Cost {
        let mut visited: HashSet<usize> = HashSet::new();
        let mut stack = vec![entry];
        let mut total_bytes = 0u64;
        let mut external_packages = BTreeSet::new();
        let mut dynamic_targets = BTreeSet::new();
        let mut opaque = 0usize;
        let mut unresolved = 0usize;

        while let Some(idx) = stack.pop() {
            if !visited.insert(idx) {
                continue;
            }
            let module = &self.modules[idx];
            total_bytes += module.facts.bytes;
            external_packages.extend(module.external_packages.iter().cloned());
            opaque += module.opaque_dynamic_imports;
            unresolved += module.unresolved.len();
            for (target, _) in &module.dynamic_edges {
                dynamic_targets.insert(*target);
            }
            for edge in &module.edges {
                if skip_edge == Some((idx, edge.to)) {
                    continue;
                }
                stack.push(edge.to);
            }
        }

        let mut modules: Vec<usize> = visited.into_iter().collect();
        modules.sort_unstable();
        Cost {
            modules,
            total_bytes,
            external_packages,
            dynamic_targets,
            opaque_dynamic_imports: opaque,
            unresolved,
        }
    }

    /// Follows a named export through re-export chains to the module that
    /// declares it, returning the owner and its declaration kind. Star
    /// exports are searched breadth-first; cycles and depth are bounded.
    pub fn resolve_export(&self, module: usize, name: &str) -> Option<(usize, DeclKind)> {
        let mut seen: HashSet<(usize, String)> = HashSet::new();
        self.resolve_export_inner(module, name, &mut seen, 0)
    }

    fn resolve_export_inner(
        &self,
        module: usize,
        name: &str,
        seen: &mut HashSet<(usize, String)>,
        depth: usize,
    ) -> Option<(usize, DeclKind)> {
        // The `seen` set bounds total work but not stack depth: a chain of
        // modules each re-exporting the next would recurse once per link.
        // Source is untrusted input, so the depth is capped outright.
        if depth > MAX_REEXPORT_DEPTH || !seen.insert((module, name.to_string())) {
            return None;
        }
        let facts = &self.modules[module].facts;
        if let Some(kind) = facts.export_decl_kinds.get(name) {
            return Some((module, *kind));
        }
        for reexport in &facts.named_reexports {
            if reexport.export_name != name || reexport.is_type {
                continue;
            }
            let Some(target) = self.edge_target(module, &reexport.specifier) else {
                continue;
            };
            return match &reexport.source {
                ReExportSource::Named(original) => {
                    self.resolve_export_inner(target, original, seen, depth + 1)
                }
                ReExportSource::Default => {
                    self.resolve_export_inner(target, "default", seen, depth + 1)
                }
                // `export * as ns` — the namespace object materializes when
                // the target module evaluates; early access behaves like a
                // const read.
                ReExportSource::Namespace => Some((target, DeclKind::ConstLet)),
            };
        }
        for star in &facts.star_reexports {
            if star.is_type {
                continue;
            }
            let Some(target) = self.edge_target(module, &star.specifier) else {
                continue;
            };
            if let Some(found) = self.resolve_export_inner(target, name, seen, depth + 1) {
                return Some(found);
            }
        }
        None
    }

    /// Every runtime export this module offers, resolved to the module that
    /// declares it and its declaration kind.
    ///
    /// Used when a namespace object is read as a whole (`{...ns}`,
    /// `console.log(ns)`, a computed key): that observes every export at
    /// once, so every export is a candidate for a temporal-dead-zone read.
    pub fn runtime_exports(&self, module: usize) -> Vec<(String, usize, DeclKind)> {
        let mut names: BTreeSet<String> = BTreeSet::new();
        let mut seen: HashSet<usize> = HashSet::new();
        self.collect_export_names(module, &mut names, &mut seen, 0);
        names
            .into_iter()
            .filter_map(|name| {
                self.resolve_export(module, &name)
                    .map(|(owner, kind)| (name, owner, kind))
            })
            .collect()
    }

    fn collect_export_names(
        &self,
        module: usize,
        out: &mut BTreeSet<String>,
        seen: &mut HashSet<usize>,
        depth: usize,
    ) {
        if depth > MAX_REEXPORT_DEPTH || !seen.insert(module) {
            return;
        }
        let facts = &self.modules[module].facts;
        out.extend(facts.export_decl_kinds.keys().cloned());
        for reexport in &facts.named_reexports {
            if !reexport.is_type {
                out.insert(reexport.export_name.clone());
            }
        }
        for star in &facts.star_reexports {
            if star.is_type {
                continue;
            }
            if let Some(target) = self.edge_target(module, &star.specifier) {
                self.collect_export_names(target, out, seen, depth + 1);
            }
        }
    }

    /// Shortest chain of runtime imports from `from` to `to`, inclusive.
    ///
    /// `scope`, when given, restricts the walk to a set of modules — a cycle
    /// analysis asks about paths inside one component, `overpull why` asks
    /// about the whole graph.
    pub fn shortest_import_path(
        &self,
        from: usize,
        to: usize,
        scope: Option<&HashSet<usize>>,
    ) -> Option<Vec<usize>> {
        let mut previous: HashMap<usize, usize> = HashMap::new();
        let mut queue: VecDeque<usize> = VecDeque::from([from]);
        let mut seen: HashSet<usize> = HashSet::from([from]);
        while let Some(current) = queue.pop_front() {
            if current == to {
                let mut path = vec![to];
                let mut node = to;
                while node != from {
                    node = previous[&node];
                    path.push(node);
                }
                path.reverse();
                return Some(path);
            }
            for edge in &self.modules[current].edges {
                if scope.is_none_or(|set| set.contains(&edge.to)) && seen.insert(edge.to) {
                    previous.insert(edge.to, current);
                    queue.push_back(edge.to);
                }
            }
        }
        None
    }

    fn edge_target(&self, module: usize, specifier: &str) -> Option<usize> {
        self.modules[module]
            .edges
            .iter()
            .find(|e| e.specifier == specifier)
            .map(|e| e.to)
    }

    pub fn display_path(&self, idx: usize) -> String {
        crate::util::display_path(&self.root, &self.modules[idx].facts.path)
    }
}

/// Records a resolved in-project target on the importing module, in the slot
/// its request kind belongs to.
fn attach_local(module: &mut Module, target: usize, req: Req) {
    match req.kind {
        ReqKind::Static => module.edges.push(Edge {
            to: target,
            kind: EdgeKind::Static,
            line: req.line,
            specifier: req.specifier,
            import_idx: req.import_idx,
        }),
        ReqKind::Require => module.edges.push(Edge {
            to: target,
            kind: EdgeKind::Require,
            line: req.line,
            specifier: req.specifier,
            import_idx: None,
        }),
        ReqKind::Dynamic => module.dynamic_edges.push((target, req.line)),
        ReqKind::TypeOnly => module.type_edges.push(target),
    }
}

/// One specifier a module asks for, tagged with what it means at runtime.
struct Req {
    specifier: String,
    line: u32,
    kind: ReqKind,
    /// Index into `facts.imports`, so an edge can find its bindings again.
    import_idx: Option<usize>,
}

enum ReqKind {
    Static,
    TypeOnly,
    Dynamic,
    Require,
}

/// Flattens a file's imports, requires and dynamic imports into resolution
/// requests. Returns the requests and the count of dynamic imports whose
/// specifier is computed and cannot be followed.
fn collect_requests(facts: &FileFacts) -> (Vec<Req>, usize) {
    let mut requests = Vec::with_capacity(facts.imports.len() + facts.requires.len());
    for (i, import) in facts.imports.iter().enumerate() {
        requests.push(Req {
            specifier: import.specifier.clone(),
            line: import.line,
            kind: if import.type_only {
                ReqKind::TypeOnly
            } else {
                ReqKind::Static
            },
            import_idx: Some(i),
        });
    }
    for require in &facts.requires {
        requests.push(Req {
            specifier: require.specifier.clone(),
            line: require.line,
            kind: ReqKind::Require,
            import_idx: None,
        });
    }
    let mut opaque = 0usize;
    for dynamic in &facts.dynamic_imports {
        match &dynamic.specifier {
            Some(specifier) => requests.push(Req {
                specifier: specifier.clone(),
                line: dynamic.line,
                kind: ReqKind::Dynamic,
                import_idx: None,
            }),
            None => opaque += 1,
        }
    }
    (requests, opaque)
}

fn new_module(facts: FileFacts) -> Module {
    Module {
        facts,
        edges: Vec::new(),
        dynamic_edges: Vec::new(),
        opaque_dynamic_imports: 0,
        external_packages: BTreeSet::new(),
        dynamic_external_packages: BTreeSet::new(),
        type_edges: Vec::new(),
        unresolved: Vec::new(),
    }
}

/// Strongly connected components of the runtime graph with more than one
/// module (or a self-loop), via iterative Tarjan. Deterministic: components
/// and their members come out in stable order.
pub fn strongly_connected_components(graph: &ModuleGraph) -> Vec<Vec<usize>> {
    let n = graph.modules.len();
    let mut index = vec![usize::MAX; n];
    let mut low = vec![0usize; n];
    let mut on_stack = vec![false; n];
    let mut stack: Vec<usize> = Vec::new();
    let mut next_index = 0usize;
    let mut components: Vec<Vec<usize>> = Vec::new();

    // Iterative Tarjan: (node, edge cursor) frames.
    let mut call_stack: Vec<(usize, usize)> = Vec::new();
    for start in 0..n {
        if index[start] != usize::MAX {
            continue;
        }
        call_stack.push((start, 0));
        while let Some(&mut (v, ref mut cursor)) = call_stack.last_mut() {
            if *cursor == 0 {
                index[v] = next_index;
                low[v] = next_index;
                next_index += 1;
                stack.push(v);
                on_stack[v] = true;
            }
            if let Some(edge) = graph.modules[v].edges.get(*cursor) {
                *cursor += 1;
                let w = edge.to;
                if index[w] == usize::MAX {
                    call_stack.push((w, 0));
                } else if on_stack[w] {
                    low[v] = low[v].min(index[w]);
                }
            } else {
                if low[v] == index[v] {
                    let mut component = Vec::new();
                    while let Some(w) = stack.pop() {
                        on_stack[w] = false;
                        component.push(w);
                        if w == v {
                            break;
                        }
                    }
                    let self_loop =
                        component.len() == 1 && graph.modules[v].edges.iter().any(|e| e.to == v);
                    if component.len() > 1 || self_loop {
                        component.sort_unstable();
                        components.push(component);
                    }
                }
                call_stack.pop();
                if let Some(&mut (parent, _)) = call_stack.last_mut() {
                    low[parent] = low[parent].min(low[v]);
                }
            }
        }
    }
    components.sort_by_key(|c| c[0]);
    components
}
