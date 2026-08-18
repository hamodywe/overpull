//! Import-cycle analysis with evaluation-order simulation.
//!
//! Detecting a cycle is the easy part; every tool does it. What matters is
//! whether the cycle *breaks anything*: ES module cycles are legal and most
//! are harmless, because function declarations hoist across module
//! boundaries. The ones that hurt read a `const`, `let`, or `class` binding
//! from a module that has not evaluated yet — that compiles clean and throws
//! `ReferenceError: Cannot access 'X' before initialization` at runtime.
//!
//! For each strongly connected component this module simulates the ESM
//! evaluation order from each realistic entry point, finds edges evaluated
//! "too early", and checks how each imported binding is used (at module
//! evaluation time or deferred inside a function) and what kind of
//! declaration stands behind it. Only the combination *immediate use × TDZ
//! declaration × not-yet-evaluated owner* is reported as a crash.

use std::collections::{HashMap, HashSet};

use crate::graph::{EdgeKind, ModuleGraph, strongly_connected_components};
use crate::model::{DeclKind, Usage};

/// How many entry points are simulated per class. Orders repeat quickly;
/// this bounds work on pathological graphs without losing findings in
/// practice.
const MAX_ENTRIES_SIMULATED: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Hazard {
    /// Only hoisted functions / deferred uses — the cycle is ugly but works.
    Benign,
    /// The cycle crosses a `require()` edge; CJS evaluation order differs and
    /// a partial exports object may be observed. Not statically provable.
    CjsMixed,
    /// A `const`/`let`/`class` binding is read too early, but only when one
    /// of the cycle's own modules is loaded first — a deep import, or a test
    /// file importing an internal module directly. Starting from the
    /// project's real entry points, the order is safe.
    ///
    /// This distinction is the difference between a live bug and a trap: a
    /// tool that calls both "crash" is wrong about working code, and gets
    /// scrolled past.
    ConditionalCrash,
    /// A binding compiled to `var`/enum is read before its module runs — no
    /// throw, silently `undefined`.
    Undefined,
    /// A `const`/`let`/`class` binding is read before its module runs, on the
    /// evaluation order the project's own entry points produce —
    /// `ReferenceError` at load time.
    Crash,
}

impl Hazard {
    pub fn label(self) -> &'static str {
        match self {
            Self::Benign => "benign",
            Self::CjsMixed => "cjs-mixed",
            Self::ConditionalCrash => "crash-if-loaded-first",
            Self::Undefined => "undefined-read",
            Self::Crash => "crash",
        }
    }
}

pub struct HazardDetail {
    /// Module whose top-level code performs the early read.
    pub reader: usize,
    pub read_line: u32,
    pub binding_local: String,
    pub imported_name: String,
    /// Module that declares the binding (after following re-exports).
    pub owner: usize,
    pub decl_kind: DeclKind,
    /// The direct import target (differs from `owner` through a barrel).
    pub via: usize,
    /// Component entry whose evaluation order produces the failure.
    pub entry: usize,
    pub in_extends: bool,
}

pub enum BreakSuggestion {
    /// Every binding on the edge is used only in type positions.
    TypeOnly { from: usize, to: usize, line: u32 },
    /// Every binding on the edge is used only inside deferred code.
    DeferImport { from: usize, to: usize, line: u32 },
    /// No mechanical fix — point at the lightest edge to redesign.
    ExtractShared { from: usize, to: usize, line: u32 },
}

pub struct CycleFinding {
    pub members: Vec<usize>,
    pub hazard: Hazard,
    pub detail: Option<HazardDetail>,
    /// One concrete loop through the component, for display.
    pub cycle_path: Vec<usize>,
    pub suggestion: Option<BreakSuggestion>,
}

pub fn analyze(graph: &ModuleGraph) -> Vec<CycleFinding> {
    // Real entry points first: a module nothing imports is where the project
    // actually starts, and the order it produces decides whether a cycle is a
    // live bug or a trap waiting for someone to deep-import into it.
    // Computed once and shared by every component.
    let root_orders: Vec<RootOrder> = (0..graph.modules.len())
        .filter(|&i| graph.importers[i].is_empty())
        .take(MAX_ENTRIES_SIMULATED)
        .map(|entry| RootOrder {
            entry,
            order: evaluation_order(graph, entry, None),
        })
        .collect();

    strongly_connected_components(graph)
        .into_iter()
        .map(|members| analyze_component(graph, members, &root_orders))
        .collect()
}

/// One project entry point and the module evaluation order it produces.
struct RootOrder {
    entry: usize,
    /// Module → position, 0 = evaluated first.
    order: HashMap<usize, usize>,
}

struct ScanState {
    worst: Hazard,
    detail: Option<HazardDetail>,
}

/// Checks one evaluation order for bindings read before the module that
/// declares them has run, keeping the worst hazard seen so far.
fn scan_order(
    graph: &ModuleGraph,
    members: &[usize],
    member_set: &HashSet<usize>,
    entry: usize,
    order: &HashMap<usize, usize>,
    conditional: bool,
    state: &mut ScanState,
) {
    for &module in members {
        let Some(&module_pos) = order.get(&module) else {
            continue;
        };
        for edge in &graph.modules[module].edges {
            if edge.kind != EdgeKind::Static || !member_set.contains(&edge.to) {
                continue;
            }
            let Some(&target_pos) = order.get(&edge.to) else {
                continue;
            };
            if target_pos <= module_pos {
                continue; // target evaluates first — its bindings are live
            }
            let Some(import_idx) = edge.import_idx else {
                continue;
            };
            for binding in &graph.modules[module].facts.imports[import_idx].bindings {
                let Usage::Immediate { line, in_extends } = binding.usage else {
                    continue;
                };
                let name = binding.imported.display();
                let resolved = if name == "*" {
                    // A namespace object read at top level observes the
                    // uninitialized namespace: property reads throw for TDZ
                    // bindings. Treat as const-like on the target.
                    Some((edge.to, DeclKind::ConstLet))
                } else {
                    graph.resolve_export(edge.to, &name)
                };
                let Some((owner, kind)) = resolved else {
                    continue;
                };
                // The owner itself must still be un-evaluated at read time;
                // through a re-export chain it may already have run.
                if owner != edge.to {
                    match order.get(&owner) {
                        Some(&p) if p > module_pos => {}
                        _ => continue,
                    }
                }
                let hazard = match kind {
                    DeclKind::ConstLet | DeclKind::Class => {
                        if conditional {
                            Hazard::ConditionalCrash
                        } else {
                            Hazard::Crash
                        }
                    }
                    DeclKind::VarLike | DeclKind::Unknown => Hazard::Undefined,
                    DeclKind::HoistedFunction | DeclKind::TypeOnly => continue,
                };
                let better = hazard > state.worst
                    || (hazard == state.worst
                        && in_extends
                        && state.detail.as_ref().is_some_and(|d| !d.in_extends));
                if better {
                    state.worst = hazard;
                    state.detail = Some(HazardDetail {
                        reader: module,
                        read_line: line,
                        binding_local: binding.local.clone(),
                        imported_name: name,
                        owner,
                        decl_kind: kind,
                        via: edge.to,
                        entry,
                        in_extends,
                    });
                }
            }
        }
    }
}

fn analyze_component(
    graph: &ModuleGraph,
    members: Vec<usize>,
    root_orders: &[RootOrder],
) -> CycleFinding {
    let member_set: HashSet<usize> = members.iter().copied().collect();

    let has_require_edge = members.iter().any(|&m| {
        graph.modules[m]
            .edges
            .iter()
            .any(|e| e.kind == EdgeKind::Require && member_set.contains(&e.to))
    });

    let mut state = ScanState {
        worst: Hazard::Benign,
        detail: None,
    };

    // Pass one: the evaluation orders the project's own entry points
    // produce. A hazard found here fires by simply starting the app.
    for root in root_orders {
        scan_order(
            graph,
            &members,
            &member_set,
            root.entry,
            &root.order,
            false,
            &mut state,
        );
    }

    // Pass two: each member as if it were loaded first — a deep import, or a
    // test file importing an internal module directly. Real, but conditional
    // on entry order, so it is reported one level down. Calling this a crash
    // would mean calling working code broken.
    for &entry in members.iter().take(MAX_ENTRIES_SIMULATED) {
        let order = evaluation_order(graph, entry, Some(&member_set));
        scan_order(
            graph,
            &members,
            &member_set,
            entry,
            &order,
            true,
            &mut state,
        );
    }

    let (mut worst, detail) = (state.worst, state.detail);
    if worst == Hazard::Benign && has_require_edge {
        worst = Hazard::CjsMixed;
    }

    let cycle_path = match &detail {
        Some(d) => cycle_through_edge(graph, &member_set, d.reader, d.via),
        None => shortest_cycle(graph, &member_set, members[0]),
    };
    let suggestion = suggest_break(graph, &cycle_path, &member_set);

    CycleFinding {
        members,
        hazard: worst,
        detail,
        cycle_path,
        suggestion,
    }
}

/// ESM evaluation order starting at `entry`: depth-first, imports in source
/// order, each dependency fully evaluated before its importer — except back
/// edges into a module already in progress, which is exactly where cycles
/// bite. Returns evaluation positions (0 = first to run).
///
/// `scope` restricts the walk to a set of modules. Pass `None` to simulate a
/// real program start over the whole graph; pass a component's members to
/// ask what happens when one of them is loaded on its own.
fn evaluation_order(
    graph: &ModuleGraph,
    entry: usize,
    scope: Option<&HashSet<usize>>,
) -> HashMap<usize, usize> {
    let mut pos: HashMap<usize, usize> = HashMap::new();
    let mut visiting: HashSet<usize> = HashSet::new();
    // Frame: (module, next edge cursor).
    let mut stack: Vec<(usize, usize)> = vec![(entry, 0)];
    visiting.insert(entry);

    while let Some(&mut (module, ref mut cursor)) = stack.last_mut() {
        let next_target = graph.modules[module]
            .edges
            .iter()
            .skip(*cursor)
            .enumerate()
            .find_map(|(offset, edge)| {
                let in_scope = scope.is_none_or(|set| set.contains(&edge.to))
                    && !visiting.contains(&edge.to)
                    && !pos.contains_key(&edge.to);
                in_scope.then_some((offset, edge.to))
            });
        if let Some((offset, target)) = next_target {
            *cursor += offset + 1;
            visiting.insert(target);
            stack.push((target, 0));
        } else {
            visiting.remove(&module);
            let position = pos.len();
            pos.insert(module, position);
            stack.pop();
        }
    }
    pos
}

/// A concrete loop that contains the edge `from → to`: the shortest static
/// path `to → … → from` inside the component, closed by the edge itself.
/// Returned as [from, to, …, from].
fn cycle_through_edge(
    graph: &ModuleGraph,
    members: &HashSet<usize>,
    from: usize,
    to: usize,
) -> Vec<usize> {
    let path = shortest_path(graph, members, to, from).unwrap_or_else(|| vec![to, from]);
    let mut cycle = vec![from];
    cycle.extend(path);
    cycle
}

fn shortest_cycle(graph: &ModuleGraph, members: &HashSet<usize>, start: usize) -> Vec<usize> {
    // Shortest way back to `start` over any of its in-component edges.
    let mut best: Option<Vec<usize>> = None;
    for edge in &graph.modules[start].edges {
        if !members.contains(&edge.to) {
            continue;
        }
        if edge.to == start {
            return vec![start, start];
        }
        if let Some(back) = shortest_path(graph, members, edge.to, start) {
            let mut cycle = vec![start];
            cycle.extend(back);
            if best.as_ref().is_none_or(|b| cycle.len() < b.len()) {
                best = Some(cycle);
            }
        }
    }
    best.unwrap_or_else(|| vec![start])
}

/// BFS over runtime edges restricted to the component. Returns the node path
/// from `from` to `to` inclusive.
fn shortest_path(
    graph: &ModuleGraph,
    members: &HashSet<usize>,
    from: usize,
    to: usize,
) -> Option<Vec<usize>> {
    let mut previous: HashMap<usize, usize> = HashMap::new();
    let mut queue = std::collections::VecDeque::from([from]);
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
        for edge in &graph.modules[current].edges {
            if members.contains(&edge.to) && seen.insert(edge.to) {
                previous.insert(edge.to, current);
                queue.push_back(edge.to);
            }
        }
    }
    None
}

/// Ranks the edges of the representative loop for breakability. A cycle edge
/// whose bindings are all type-level disappears with `import type`; one whose
/// bindings are only used inside functions can move into them; otherwise the
/// edge with the fewest immediately-used bindings is the redesign point.
fn suggest_break(
    graph: &ModuleGraph,
    cycle_path: &[usize],
    members: &HashSet<usize>,
) -> Option<BreakSuggestion> {
    let mut fallback: Option<(usize, usize, usize, u32)> = None; // (immediate_count, from, to, line)
    let mut defer: Option<BreakSuggestion> = None;

    for window in cycle_path.windows(2) {
        let (from, to) = (window[0], window[1]);
        if !members.contains(&from) || !members.contains(&to) {
            continue;
        }
        let Some(edge) = graph.modules[from]
            .edges
            .iter()
            .find(|e| e.to == to && e.kind == EdgeKind::Static)
        else {
            continue;
        };
        let Some(import_idx) = edge.import_idx else {
            continue;
        };
        let bindings = &graph.modules[from].facts.imports[import_idx].bindings;
        if bindings.is_empty() {
            continue; // side-effect import: nothing mechanical to suggest
        }
        let all_type = bindings
            .iter()
            .all(|b| matches!(b.usage, Usage::TypeOnly | Usage::Unused));
        if all_type {
            return Some(BreakSuggestion::TypeOnly {
                from,
                to,
                line: edge.line,
            });
        }
        let all_deferred = bindings
            .iter()
            .all(|b| matches!(b.usage, Usage::Deferred | Usage::TypeOnly | Usage::Unused));
        if all_deferred && defer.is_none() {
            defer = Some(BreakSuggestion::DeferImport {
                from,
                to,
                line: edge.line,
            });
        }
        let immediate_count = bindings
            .iter()
            .filter(|b| matches!(b.usage, Usage::Immediate { .. }))
            .count();
        if fallback.is_none_or(|(count, ..)| immediate_count < count) {
            fallback = Some((immediate_count, from, to, edge.line));
        }
    }

    defer.or(fallback.map(|(_, from, to, line)| BreakSuggestion::ExtractShared { from, to, line }))
}
