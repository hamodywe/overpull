//! Analysis tests over the fixtures.
//!
//! Every expectation here is also written down in the fixture's own README,
//! before the tool could produce it. A fixture whose numbers were filled in
//! afterwards just agrees with whatever the code does.

use std::path::{Path, PathBuf};

use overpull::barrels::{self, Thresholds};
use overpull::cost;
use overpull::cycles::{self, CycleFinding, Hazard};
use overpull::entries::{self, EntryKind};
use overpull::graph::ModuleGraph;
use overpull::model::DeclKind;
use overpull::resolve::SpecResolver;
use overpull::util::normalize;
use overpull::walk::discover_sources;
use overpull::why;

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

fn cycles_for(graph: &ModuleGraph) -> Vec<CycleFinding> {
    let entry_set = entries::classify(graph, &[]);
    cycles::analyze(graph, &entry_set).findings
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
    let findings = cycles_for(&graph);
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
        "namespace-cycle",
        "iife-cycle",
        "entry-points",
        "test-entry-cycle",
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
    let findings = cycles_for(&graph);
    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings[0].hazard,
        Hazard::Benign,
        "hoisted functions across a cycle are legal and must not be reported as a crash"
    );
    assert!(findings[0].detail.is_none());
}

// ---- namespace-cycle ---------------------------------------------------

#[test]
fn namespace_member_reads_are_judged_by_the_member_not_the_namespace() {
    // An ES module namespace object exists from instantiation, so holding it
    // never throws. `ns.hoistedFunction()` in a cycle is legal;
    // `ns.someConst` is not. verify.mjs proves both halves under Node.
    let graph = graph_for("namespace-cycle");
    let findings = cycles_for(&graph);
    assert_eq!(findings.len(), 2, "two independent cycles in this fixture");

    let safe = findings
        .iter()
        .find(|f| {
            f.members
                .iter()
                .any(|&m| graph.display_path(m).starts_with("safe-"))
        })
        .expect("the safe cycle must be reported");
    assert_eq!(
        safe.hazard,
        Hazard::Benign,
        "reading a hoisted function off a namespace is legal in a cycle"
    );

    let unsafe_cycle = findings
        .iter()
        .find(|f| {
            f.members
                .iter()
                .any(|&m| graph.display_path(m).starts_with("unsafe-"))
        })
        .expect("the unsafe cycle must be reported");
    assert_eq!(unsafe_cycle.hazard, Hazard::Crash);
    let detail = unsafe_cycle.detail.as_ref().expect("evidence");
    assert_eq!(detail.binding_local, "b");
    assert_eq!(detail.member.as_deref(), Some("PREFIX"));
    assert_eq!(detail.imported_name, "PREFIX");
    assert_eq!(detail.decl_kind, DeclKind::ConstLet);
    assert_eq!(graph.display_path(detail.reader), "unsafe-a.mjs");
    assert_eq!(graph.display_path(detail.owner), "unsafe-b.mjs");
}

// ---- iife-cycle --------------------------------------------------------

#[test]
fn an_invoked_arrow_runs_now_and_an_uninvoked_one_does_not() {
    // The question is not "is the read inside a function" but "does that
    // function run while the module evaluates". verify.mjs proves Node throws
    // on the invoked half and loads the deferred half cleanly.
    let graph = graph_for("iife-cycle");
    let findings = cycles_for(&graph);
    assert_eq!(findings.len(), 2);

    let invoked = findings
        .iter()
        .find(|f| {
            f.members
                .iter()
                .any(|&m| graph.display_path(m).starts_with("invoked-"))
        })
        .expect("the invoked cycle must be reported");
    assert_eq!(invoked.hazard, Hazard::Crash);
    let detail = invoked.detail.as_ref().expect("evidence");
    assert_eq!(detail.imported_name, "NAME");
    assert_eq!(graph.display_path(detail.reader), "invoked-a.mjs");
    assert_eq!(graph.display_path(detail.owner), "invoked-b.mjs");

    let deferred = findings
        .iter()
        .find(|f| {
            f.members
                .iter()
                .any(|&m| graph.display_path(m).starts_with("deferred-"))
        })
        .expect("the deferred cycle must be reported");
    assert_eq!(
        deferred.hazard,
        Hazard::Benign,
        "an arrow function nobody calls at module scope defers its reads"
    );
}

// ---- test-entry-cycle --------------------------------------------------

#[test]
fn a_hazard_only_a_test_file_reaches_is_not_a_crash() {
    // Reduced from vitejs/vite. The order is real and the test file that
    // produces it is named — but a test process has usually evaluated the
    // safe half long before the spec runs, which is why vite's suite is
    // green. Calling this a crash would mean calling working code broken.
    let graph = graph_for("test-entry-cycle");
    let findings = cycles_for(&graph);
    assert_eq!(findings.len(), 1);

    let finding = &findings[0];
    assert_eq!(
        finding.hazard,
        Hazard::ConditionalCrash,
        "a test-file entry must not promote a finding to `crash`"
    );
    let detail = finding.detail.as_ref().expect("evidence");
    assert_eq!(detail.imported_name, "DEFAULTS");
    assert_eq!(graph.display_path(detail.reader), "src/config.mjs");
    assert_eq!(graph.display_path(detail.owner), "src/build.mjs");
    assert_eq!(
        graph.display_path(detail.entry),
        "tests/build.test.mjs",
        "the evidence must name the file that produces the order"
    );
    assert_eq!(detail.entry_kind, EntryKind::Test);
}

// ---- entry classification ----------------------------------------------

#[test]
fn entry_points_are_classified_by_what_declares_them() {
    // Severity depends on which module is assumed to load first, so a stray
    // root file must not be able to displace the project's real entry point.
    let graph = graph_for("entry-points");
    let entry_set = entries::classify(&graph, &[]);
    let kind_of = |relative: &str| {
        let module = index_of(&graph, relative);
        entry_set
            .iter()
            .find(|e| e.module == module)
            .map(|e| e.kind)
    };

    assert_eq!(kind_of("src/index.ts"), Some(EntryKind::Package));
    assert_eq!(kind_of("src/cli.ts"), Some(EntryKind::Package));
    assert_eq!(
        kind_of("src/legacy.ts"),
        Some(EntryKind::Orphan),
        "a root file the project never declares is not a program start"
    );
    assert_eq!(
        kind_of("tests/core.test.ts"),
        Some(EntryKind::Test),
        "nothing imports a test file, but the test runner loads it"
    );
    assert_eq!(
        kind_of("src/core.ts"),
        None,
        "a module with importers is not an entry point"
    );
}

#[test]
fn explicit_entries_replace_the_guesses() {
    let graph = graph_for("barrel-project");
    let app = index_of(&graph, "src/app.ts");
    let entry_set = entries::classify(&graph, &[app]);
    assert_eq!(entry_set.len(), 1);
    assert_eq!(entry_set[0].module, app);
    assert_eq!(entry_set[0].kind, EntryKind::Package);
}

// ---- why ----------------------------------------------------------------

#[test]
fn why_names_the_import_line_that_pulls_a_module_in() {
    let graph = graph_for("barrel-project");
    let target = index_of(&graph, "src/internal/h07.ts");
    let entry_set = entries::classify(&graph, &[index_of(&graph, "src/app.ts")]);
    let report = why::analyze(&graph, &entry_set, target);

    let path = report
        .paths
        .first()
        .expect("app.ts reaches every component through the barrel");
    let hops: Vec<String> = path.path.iter().map(|&m| graph.display_path(m)).collect();
    assert_eq!(hops.first().unwrap(), "src/app.ts");
    assert_eq!(hops.last().unwrap(), "src/internal/h07.ts");
    assert_eq!(
        path.lines.len(),
        path.path.len() - 1,
        "one import line per hop"
    );
    assert!(
        !report.direct_importers.is_empty(),
        "something must import it directly"
    );
}

// ---- clean-project -----------------------------------------------------

#[test]
fn clean_project_reports_nothing() {
    let graph = graph_for("clean-project");
    assert!(
        cycles_for(&graph).is_empty(),
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

#[test]
fn runtime_exports_reach_through_the_barrel() {
    // What a whole-namespace read observes: every name the barrel offers,
    // resolved to the module that declares it.
    let graph = graph_for("barrel-project");
    let barrel = index_of(&graph, "src/index.ts");
    let exports = graph.runtime_exports(barrel);
    assert!(
        exports.len() >= 12,
        "the barrel re-exports twelve components, got {}",
        exports.len()
    );
    assert!(
        exports.iter().all(|(_, owner, _)| *owner != barrel),
        "a barrel declares nothing itself; every export must resolve elsewhere"
    );
}
