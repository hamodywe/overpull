//! `--baseline` — report only what is new.
//!
//! The hard part of adopting any analyser on an existing codebase is the
//! first run: two hundred findings, none of them today's problem, and the
//! gate gets switched off before lunch. A baseline records what was already
//! there so a pull request is judged on what it adds. Findings that got
//! *worse* than the baseline are still reported — a cycle that was benign
//! and now crashes is new information about an old cycle.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde_json::Value;

use crate::cycles::Hazard;

pub struct Baseline {
    /// Cycle key → the hazard recorded for it.
    cycles: HashMap<String, Hazard>,
    /// Barrel file → the amplification recorded for it.
    barrels: HashMap<String, f64>,
}

/// Amplification has to grow by more than this to count as a regression;
/// below it, the difference is a file gaining one line.
const AMPLIFICATION_SLACK: f64 = 0.05;

impl Baseline {
    /// Reads a baseline from a document produced by `--json`, in either
    /// shape: a single command's output, or the `check` envelope.
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = fs::read_to_string(path).map_err(|error| {
            format!(
                "cannot read --baseline `{}`: {error}\n\
                 Create one with: overpull check --json > {}",
                path.display(),
                path.display()
            )
        })?;
        let document: Value = serde_json::from_str(&text)
            .map_err(|error| format!("--baseline `{}` is not JSON: {error}", path.display()))?;

        let mut baseline = Self {
            cycles: HashMap::new(),
            barrels: HashMap::new(),
        };
        match document.get("results").and_then(Value::as_array) {
            Some(parts) => {
                for part in parts {
                    baseline.absorb(part);
                }
            }
            None => baseline.absorb(&document),
        }
        if baseline.cycles.is_empty() && baseline.barrels.is_empty() {
            return Err(format!(
                "--baseline `{}` records no findings.\n\
                 A baseline is the JSON output of a previous run: \
                 overpull check --json > {}",
                path.display(),
                path.display()
            ));
        }
        Ok(baseline)
    }

    fn absorb(&mut self, document: &Value) {
        if let Some(cycles) = document.get("cycles").and_then(Value::as_array) {
            for cycle in cycles {
                let Some(members) = cycle.get("members").and_then(Value::as_array) else {
                    continue;
                };
                let paths: Vec<String> = members
                    .iter()
                    .filter_map(|m| m.as_str().map(str::to_string))
                    .collect();
                let hazard = cycle
                    .get("hazard")
                    .and_then(Value::as_str)
                    .map_or(Hazard::Benign, hazard_from_label);
                self.cycles.insert(cycle_key(&paths), hazard);
            }
        }
        if let Some(barrels) = document.get("barrels").and_then(Value::as_array) {
            for barrel in barrels {
                let Some(file) = barrel.get("file").and_then(Value::as_str) else {
                    continue;
                };
                let amplification = barrel
                    .get("amplification")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0);
                self.barrels.insert(file.to_string(), amplification);
            }
        }
    }

    /// True when this cycle was in the baseline and has not become worse.
    pub fn covers_cycle(&self, members: &[String], hazard: Hazard) -> bool {
        self.cycles
            .get(&cycle_key(members))
            .is_some_and(|&known| hazard <= known)
    }

    /// True when this barrel was in the baseline and has not amplified more.
    pub fn covers_barrel(&self, file: &str, amplification: f64) -> bool {
        self.barrels
            .get(file)
            .is_some_and(|&known| amplification <= known + AMPLIFICATION_SLACK)
    }
}

/// A cycle is the same cycle when it has the same members, whatever order the
/// graph happened to number them in this run.
fn cycle_key(members: &[String]) -> String {
    let mut sorted: Vec<&str> = members.iter().map(String::as_str).collect();
    sorted.sort_unstable();
    sorted.join("|")
}

fn hazard_from_label(label: &str) -> Hazard {
    match label {
        "crash" => Hazard::Crash,
        "undefined-read" => Hazard::Undefined,
        "crash-if-loaded-first" => Hazard::ConditionalCrash,
        "cjs-mixed" => Hazard::CjsMixed,
        _ => Hazard::Benign,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn baseline_from(json: &str) -> Baseline {
        let mut baseline = Baseline {
            cycles: HashMap::new(),
            barrels: HashMap::new(),
        };
        baseline.absorb(&serde_json::from_str(json).unwrap());
        baseline
    }

    #[test]
    fn member_order_does_not_change_the_key() {
        assert_eq!(
            cycle_key(&["b.ts".into(), "a.ts".into()]),
            cycle_key(&["a.ts".into(), "b.ts".into()])
        );
    }

    #[test]
    fn a_known_cycle_that_got_worse_is_still_reported() {
        let baseline =
            baseline_from(r#"{"cycles":[{"hazard":"benign","members":["a.ts","b.ts"]}]}"#);
        let members = vec!["a.ts".to_string(), "b.ts".to_string()];
        assert!(baseline.covers_cycle(&members, Hazard::Benign));
        assert!(!baseline.covers_cycle(&members, Hazard::Crash));
        assert!(!baseline.covers_cycle(&["c.ts".to_string()], Hazard::Benign));
    }

    #[test]
    fn a_barrel_that_amplifies_more_is_still_reported() {
        let baseline =
            baseline_from(r#"{"barrels":[{"file":"src/index.ts","amplification":10.0}]}"#);
        assert!(baseline.covers_barrel("src/index.ts", 10.0));
        assert!(baseline.covers_barrel("src/index.ts", 10.04));
        assert!(!baseline.covers_barrel("src/index.ts", 12.0));
        assert!(!baseline.covers_barrel("src/other.ts", 1.0));
    }

    #[test]
    fn the_check_envelope_is_unwrapped() {
        let mut baseline = Baseline {
            cycles: HashMap::new(),
            barrels: HashMap::new(),
        };
        let document: Value = serde_json::from_str(
            r#"{"command":"check","results":[
                 {"barrels":[{"file":"src/index.ts","amplification":5.0}]},
                 {"cycles":[{"hazard":"crash","members":["a.ts","b.ts"]}]}]}"#,
        )
        .unwrap();
        for part in document["results"].as_array().unwrap() {
            baseline.absorb(part);
        }
        assert!(baseline.covers_barrel("src/index.ts", 5.0));
        assert!(baseline.covers_cycle(&["a.ts".to_string(), "b.ts".to_string()], Hazard::Crash));
    }
}
