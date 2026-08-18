//! CLI tests that spawn the real binary.
//!
//! Capturing output in-process would let a mistake in exit-code handling
//! pass unnoticed; spawning costs a few milliseconds and tests the contract
//! a user and a CI job actually see.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn binary() -> PathBuf {
    // The integration-test binary lives next to the CLI binary.
    let mut path = std::env::current_exe().expect("test binary path");
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.join(format!("overpull{}", std::env::consts::EXE_SUFFIX))
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn run(args: &[&str]) -> Output {
    Command::new(binary())
        .args(args)
        .env("NO_COLOR", "1")
        .output()
        .expect("failed to run overpull")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

#[test]
fn help_exits_zero_and_lists_commands() {
    let output = run(&["--help"]);
    assert_eq!(output.status.code(), Some(0));
    let text = stdout(&output);
    for command in ["cost", "barrels", "cycles", "check"] {
        assert!(text.contains(command), "help must document `{command}`");
    }
}

#[test]
fn no_arguments_prints_help_rather_than_nothing() {
    // The failure this guards: a binary that exits 0 having printed nothing
    // is indistinguishable from a broken install.
    let output = run(&[]);
    assert_eq!(output.status.code(), Some(0));
    assert!(stdout(&output).contains("USAGE"));
}

#[test]
fn version_matches_the_package() {
    let output = run(&["--version"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(stdout(&output).contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn unknown_command_exits_two_with_guidance() {
    let output = run(&["frobnicate"]);
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknown command"));
    assert!(stderr.contains("--help"));
}

#[test]
fn missing_entry_exits_two_and_names_the_root() {
    let root = fixture("clean-project");
    let output = run(&["cost", "src/nope.ts", "--root", root.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("not found"));
}

#[test]
fn crashing_cycle_fails_the_build() {
    let root = fixture("crashing-cycle");
    let output = run(&["cycles", "--root", root.to_str().unwrap()]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "a crashing cycle must fail CI by default"
    );
    let text = stdout(&output);
    assert!(text.contains("crash"));
    assert!(text.contains("SERVICE_NAME"));
    assert!(text.contains("Cannot access"));
}

#[test]
fn benign_cycle_passes_at_default_severity() {
    let root = fixture("benign-cycle");
    let output = run(&["cycles", "--root", root.to_str().unwrap()]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "a legal cycle must not fail CI"
    );
    assert!(stdout(&output).contains("benign"));
}

#[test]
fn benign_cycle_fails_only_when_asked() {
    let root = fixture("benign-cycle");
    let output = run(&[
        "cycles",
        "--root",
        root.to_str().unwrap(),
        "--fail-on",
        "any",
    ]);
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn fail_on_levels_are_ordered() {
    // A benign cycle must pass `crash` and `hazard` but fail `any`; the
    // levels are useless if they do not actually differ.
    let root = fixture("benign-cycle");
    let path = root.to_str().unwrap();
    assert_eq!(
        run(&["cycles", "--root", path, "--fail-on", "crash"])
            .status
            .code(),
        Some(0)
    );
    assert_eq!(
        run(&["cycles", "--root", path, "--fail-on", "hazard"])
            .status
            .code(),
        Some(0)
    );
    assert_eq!(
        run(&["cycles", "--root", path, "--fail-on", "any"])
            .status
            .code(),
        Some(1)
    );
}

#[test]
fn unknown_fail_on_level_is_rejected() {
    let root = fixture("clean-project");
    let output = run(&[
        "cycles",
        "--root",
        root.to_str().unwrap(),
        "--fail-on",
        "loud",
    ]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("hazard"));
}

#[test]
fn clean_project_is_silent_and_passes_at_every_severity() {
    let root = fixture("clean-project");
    for level in ["never", "crash", "hazard", "any"] {
        let output = run(&[
            "check",
            "--root",
            root.to_str().unwrap(),
            "--fail-on",
            level,
        ]);
        assert_eq!(
            output.status.code(),
            Some(0),
            "clean project must pass at --fail-on {level}"
        );
    }
    let text = stdout(&run(&["check", "--root", root.to_str().unwrap()]));
    assert!(text.contains("no import cycles"));
    assert!(text.contains("no barrel amplifies"));
}

#[test]
fn json_output_is_valid_and_carries_the_evidence() {
    let root = fixture("crashing-cycle");
    let output = run(&["cycles", "--root", root.to_str().unwrap(), "--json"]);
    let value: serde_json::Value =
        serde_json::from_str(&stdout(&output)).expect("output must be valid JSON");

    assert_eq!(value["tool"], "overpull");
    assert_eq!(value["command"], "cycles");
    let cycle = &value["cycles"][0];
    assert_eq!(cycle["hazard"], "crash");
    assert_eq!(cycle["evidence"]["binding"], "SERVICE_NAME");
    assert_eq!(cycle["evidence"]["declarationKind"], "const/let");
    assert!(
        cycle["evidence"]["owner"]
            .as_str()
            .unwrap()
            .ends_with("service.mjs")
    );
}

#[test]
fn check_json_bundles_both_analyses() {
    let root = fixture("barrel-project");
    let output = run(&["check", "--root", root.to_str().unwrap(), "--json"]);
    let value: serde_json::Value =
        serde_json::from_str(&stdout(&output)).expect("output must be valid JSON");
    let results = value["results"]
        .as_array()
        .expect("check emits an array of results");
    assert_eq!(results.len(), 2);
    assert_eq!(results[0]["barrels"][0]["amplification"], 12.5);
}

#[test]
fn cost_reports_the_barrel_as_the_sole_contributor() {
    let root = fixture("barrel-project");
    let output = run(&[
        "cost",
        "src/app.ts",
        "--root",
        root.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(output.status.code(), Some(0), "cost never fails a build");
    let value: serde_json::Value = serde_json::from_str(&stdout(&output)).unwrap();
    let entry = &value["entries"][0];
    assert_eq!(entry["modules"], 26);
    assert_eq!(entry["contributors"][0]["exclusiveModules"], 25);
}

#[test]
fn no_color_output_has_no_escape_sequences() {
    let root = fixture("crashing-cycle");
    let output = run(&["cycles", "--root", root.to_str().unwrap(), "--no-color"]);
    assert!(!stdout(&output).contains('\u{1b}'));
}

// ---- 0.2.0: budgets, why, baseline, SARIF ------------------------------

#[test]
fn a_cost_budget_fails_the_build_and_a_met_one_does_not() {
    let root = fixture("barrel-project");
    let path = root.to_str().unwrap();
    let over = run(&["cost", "src/app.ts", "--root", path, "--max-modules", "10"]);
    assert_eq!(
        over.status.code(),
        Some(1),
        "26 modules must bust a 10 budget"
    );
    assert!(stdout(&over).contains("over"));

    let within = run(&["cost", "src/app.ts", "--root", path, "--max-modules", "100"]);
    assert_eq!(within.status.code(), Some(0));
    assert!(
        stdout(&within).contains("within"),
        "a budget that passes must still say so"
    );
}

#[test]
fn byte_budgets_accept_human_sizes() {
    let root = fixture("barrel-project");
    let path = root.to_str().unwrap();
    let output = run(&["cost", "src/app.ts", "--root", path, "--max-bytes", "1kb"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stdout(&output).contains("1000 B"));
}

#[test]
fn cost_rejects_fail_on_instead_of_ignoring_it() {
    // A flag that silently does nothing is worse than one that is refused.
    let root = fixture("barrel-project");
    let output = run(&[
        "cost",
        "src/app.ts",
        "--root",
        root.to_str().unwrap(),
        "--fail-on",
        "any",
    ]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("--max-modules"));
}

#[test]
fn why_names_the_chain_and_the_line() {
    let root = fixture("barrel-project");
    let output = run(&[
        "why",
        "src/internal/h07.ts",
        "--root",
        root.to_str().unwrap(),
        "--entry",
        "src/app.ts",
    ]);
    assert_eq!(output.status.code(), Some(0));
    let text = stdout(&output);
    assert!(text.contains("src/app.ts"));
    assert!(text.contains("src/components/c07.ts"));
    assert!(
        text.contains("imported directly by"),
        "why must name who imports it, not only how it is reached"
    );
}

#[test]
fn why_json_carries_the_hops() {
    let root = fixture("barrel-project");
    let output = run(&[
        "why",
        "src/internal/h07.ts",
        "--root",
        root.to_str().unwrap(),
        "--json",
    ]);
    let value: serde_json::Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(value["command"], "why");
    assert_eq!(value["module"], "src/internal/h07.ts");
    let hops = value["paths"][0]["hops"].as_array().expect("a chain");
    assert_eq!(hops.last().unwrap(), "src/internal/h07.ts");
}

#[test]
fn a_baseline_hides_known_findings_and_says_how_many() {
    let root = fixture("namespace-cycle");
    let path = root.to_str().unwrap();
    let baseline = std::env::temp_dir().join("overpull-baseline-test.json");

    let first = run(&["check", "--root", path, "--json"]);
    assert_eq!(
        first.status.code(),
        Some(1),
        "the crash is there to begin with"
    );
    std::fs::write(&baseline, stdout(&first)).unwrap();

    let second = run(&[
        "check",
        "--root",
        path,
        "--baseline",
        baseline.to_str().unwrap(),
    ]);
    assert_eq!(
        second.status.code(),
        Some(0),
        "nothing new means nothing to fail on"
    );
    let text = stdout(&second);
    assert!(text.contains("hidden by the baseline"));
    assert!(
        text.contains("no new import cycles"),
        "a suppressed run must not claim to be clean"
    );
    let _ = std::fs::remove_file(&baseline);
}

#[test]
fn a_missing_baseline_says_how_to_make_one() {
    let root = fixture("clean-project");
    let output = run(&[
        "check",
        "--root",
        root.to_str().unwrap(),
        "--baseline",
        "no-such-file.json",
    ]);
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--json"));
}

#[test]
fn sarif_output_is_valid_and_locates_the_read() {
    let root = fixture("crashing-cycle");
    let output = run(&["check", "--root", root.to_str().unwrap(), "--sarif"]);
    let value: serde_json::Value =
        serde_json::from_str(&stdout(&output)).expect("SARIF must be valid JSON");

    assert_eq!(value["version"], "2.1.0");
    let run_node = &value["runs"][0];
    assert_eq!(run_node["tool"]["driver"]["name"], "overpull");
    let result = &run_node["results"][0];
    assert_eq!(result["ruleId"], "crash");
    assert_eq!(result["level"], "error");
    let location = &result["locations"][0]["physicalLocation"];
    assert!(
        location["artifactLocation"]["uri"]
            .as_str()
            .unwrap()
            .ends_with("registry.mjs")
    );
    assert!(location["region"]["startLine"].as_u64().unwrap() >= 1);
    // Every emitted ruleId must be declared, or a dashboard drops the result.
    let declared: Vec<&str> = run_node["tool"]["driver"]["rules"]
        .as_array()
        .unwrap()
        .iter()
        .map(|rule| rule["id"].as_str().unwrap())
        .collect();
    for emitted in run_node["results"].as_array().unwrap() {
        assert!(declared.contains(&emitted["ruleId"].as_str().unwrap()));
    }
}

#[test]
fn json_and_sarif_together_are_refused() {
    let root = fixture("clean-project");
    let output = run(&[
        "check",
        "--root",
        root.to_str().unwrap(),
        "--json",
        "--sarif",
    ]);
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn an_entry_override_changes_the_verdict_it_should_change() {
    // With the real entry named, the namespace fixture still reports its one
    // crash; with only the safe entry named, there is nothing to fail on.
    let root = fixture("namespace-cycle");
    let path = root.to_str().unwrap();
    let unsafe_entry = run(&["cycles", "--root", path, "--entry", "unsafe-main.mjs"]);
    assert_eq!(unsafe_entry.status.code(), Some(1));

    let safe_entry = run(&["cycles", "--root", path, "--entry", "safe-main.mjs"]);
    assert_eq!(
        safe_entry.status.code(),
        Some(0),
        "the unsafe cycle is unreachable from the safe entry point"
    );
}
