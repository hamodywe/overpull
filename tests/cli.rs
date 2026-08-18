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
