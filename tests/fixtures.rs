//! Analysis tests over the fixtures.
//!
//! Every expectation here is also written down in the fixture's own README,
//! before the tool could produce it. A fixture whose numbers were filled in
//! afterwards just agrees with whatever the code does.

use std::path::{Path, PathBuf};

use overpull::barrels::{self, Thresholds};
use overpull::cost;
use overpull::cycles::{self, Hazard};
use overpull::graph::ModuleGraph;
use overpull::resolve::SpecResolver;
use overpull::util::normalize;
use overpull::walk::discover_sources;

fn fixture(name: &str) -> PathBuf {
    normalize(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
}

fn graph_for(name: &str) -> ModuleGraph {
    let root = fixture(name);
    let resolver = SpecResolver::new(None);
    let seeds = discover_sources(&root);
    assert!(!seeds.is_empty(), "fixture {name} has no source files");
    ModuleGraph::build(&root, &seeds, &resolver)
}

fn index_of(graph: &ModuleGraph, relative: &str) -> usize {
    (0..graph.modules.len())
        .find(|&i| graph.display_path(i) == relative)
        .unwrap_or_else(|| panic!("module {relative} not in graph"))
}

// ---- crashing-cycle ----------------------------------------------------

#[test]
fn crashing_cycle_is_reported_as_a_crash() {
    let graph = graph_for("crashing-cycle");
    let findings = cycles::analyze(&graph);
    assert_eq!(findings.len(), 1, "expected exactly one cycle");

    let finding = &findings[0];
    assert_eq!(finding.hazard, Hazard::Crash);
    assert_eq!(finding.members.len(), 2);

    let detail = finding
        .detail
        .as_ref()
        .expect("crash finding must carry evidence");
    // The binding, the reader and the owner must match what Node actually
    // throws — verify.mjs in the fixture proves the message.
    assert_eq!(detail.binding_local, "SERVICE_NAME");
    assert_eq!(detail.imported_name, "SERVICE_NAME");
    assert!(graph.display_path(detail.reader).ends_with("registry.mjs"));
    assert!(graph.display_path(detail.owner).ends_with("service.mjs"));
    assert_eq!(detail.decl_kind.label(), "const/let");
}

#[test]
fn no_fixture_produces_duplicate_modules() {
    // A path-normalization slip inserts the same file under two keys and
    // silently splits the graph in half — every count downstream is then
    // wrong, with nothing in the output pointing at the cause.
    for name in [
        "crashing-cycle",
        "benign-cycle",
        "clean-project",
        "barrel-project",
    ] {
        let graph = graph_for(name);
        let mut paths: Vec<String> = (0..graph.modules.len())
            .map(|i| graph.display_path(i))
            .collect();
        let total = paths.len();
        paths.sort();
        paths.dedup();
        assert_eq!(
            paths.len(),
            total,
            "{name} has the same file in the graph twice"
        );
    }
}

// ---- benign-cycle ------------------------------------------------------

#[test]
fn mutual_recursion_is_benign() {
    let graph = graph_for("benign-cycle");
    let findings = cycles::analyze(&graph);
    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings[0].hazard,
        Hazard::Benign,
        "hoisted functions across a cycle are legal and must not be reported as a crash"
    );
    assert!(findings[0].detail.is_none());
}

// ---- clean-project -----------------------------------------------------

#[test]
fn clean_project_reports_nothing() {
    let graph = graph_for("clean-project");
    assert!(
        cycles::analyze(&graph).is_empty(),
        "clean project must have no cycles"
    );
    assert!(
        barrels::analyze(&graph, &Thresholds::default()).is_empty(),
        "a two-export barrel is not worth reporting"
    );
}

#[test]
fn type_only_imports_do_not_create_runtime_edges() {
    let graph = graph_for("clean-project");
    let format = index_of(&graph, "src/user/format.ts");
    let types = index_of(&graph, "src/user/types.ts");
    assert!(
        graph.modules[format].edges.iter().all(|e| e.to != types),
        "`import type` must not become a runtime edge"
    );
    assert!(graph.modules[format].type_edges.contains(&types));
}

#[test]
fn dynamic_imports_and_builtins_are_excluded_from_load_cost() {
    let graph = graph_for("clean-project");
    let load = index_of(&graph, "src/config/load.ts");
    let cost = graph.load_cost(load);
    // load.ts + text/case.ts. plugins.ts is behind `await import()`, and
    // node:fs/promises is a builtin, not a package.
    assert_eq!(cost.modules.len(), 2);
    assert_eq!(cost.dynamic_targets.len(), 1);
    assert!(cost.external_packages.is_empty());
}

// ---- barrel-project ----------------------------------------------------

#[test]
fn barrel_amplification_matches_the_fixture_spec() {
    let graph = graph_for("barrel-project");
    assert_eq!(graph.modules.len(), 26);

    let reports = barrels::analyze(&graph, &Thresholds::default());
    assert_eq!(reports.len(), 1, "only the index barrel should be reported");

    let report = &reports[0];
    assert_eq!(graph.display_path(report.module), "src/index.ts");
    assert_eq!(report.reexport_count, 12);
    assert_eq!(report.cost_modules, 25);
    assert_eq!(report.median_target_cost, 2);
    assert!(
        (report.amplification - 12.5).abs() < f64::EPSILON,
        "expected 12.5x, got {}",
        report.amplification
    );
}

#[test]
fn cost_attributes_the_whole_library_to_one_import() {
    let graph = graph_for("barrel-project");
    let app = index_of(&graph, "src/app.ts");
    let reports = cost::analyze(&graph, &[app]);
    let report = &reports[0];

    assert_eq!(
        report.module_count, 26,
        "one component through a barrel loads everything"
    );
    let top = report
        .contributors
        .first()
        .expect("app.ts imports the barrel");
    assert_eq!(top.specifier, "./index.js");
    assert_eq!(top.exclusive_modules, 25);
}
