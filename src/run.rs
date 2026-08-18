//! Command orchestration: discover, build the graph, analyze, report.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::barrels::{self, Thresholds};
use crate::cli::{Command, FailOn, Options};
use crate::cost;
use crate::cycles::{self, Hazard};
use crate::graph::ModuleGraph;
use crate::report::Reporter;
use crate::resolve::SpecResolver;
use crate::style::Style;
use crate::walk::discover_sources;

pub struct Outcome {
    pub output: String,
    pub should_fail: bool,
}

pub fn run(options: &Options) -> Result<Outcome, String> {
    let root = validate_root(options)?;
    let resolver = SpecResolver::new(options.tsconfig.as_deref());
    let seeds = seed_files(options, &root)?;
    let graph = ModuleGraph::build(&root, &seeds, &resolver);
    if graph.modules.is_empty() {
        return Err("nothing to analyze: no module could be parsed".into());
    }

    let style = if options.json {
        Style::plain()
    } else {
        Style::detect(options.no_color)
    };
    let reporter = Reporter {
        graph: &graph,
        style,
        top: options.top,
    };
    let scanned = graph.modules.len();

    let mut output = String::new();
    let mut json_parts: Vec<Value> = Vec::new();
    let mut worst_hazard = Hazard::Benign;
    let mut cycle_count = 0usize;
    let mut barrel_count = 0usize;

    if options.command == Command::Cost {
        let entries = resolve_entries(&root, &options.entries)?;
        let indices: Vec<usize> = entries
            .iter()
            .filter_map(|path| graph.index_by_path.get(path).copied())
            .collect();
        if indices.is_empty() {
            return Err("none of the given entries could be parsed".into());
        }
        let reports = cost::analyze(&graph, &indices);
        if options.json {
            json_parts.push(reporter.cost_json(&reports));
        } else {
            output.push_str(&reporter.cost_human(&reports));
        }
    }

    if matches!(options.command, Command::Barrels | Command::Check) {
        let thresholds = Thresholds {
            min_amplification: options.min_amplification,
            min_cost: options.min_cost,
            ..Thresholds::default()
        };
        let reports = barrels::analyze(&graph, &thresholds);
        barrel_count = reports.len();
        if options.json {
            json_parts.push(reporter.barrels_json(&reports));
        } else {
            output.push_str(&reporter.barrels_human(&reports, scanned));
        }
    }

    if matches!(options.command, Command::Cycles | Command::Check) {
        let findings = cycles::analyze(&graph);
        cycle_count = findings.len();
        worst_hazard = findings
            .iter()
            .map(|f| f.hazard)
            .max()
            .unwrap_or(Hazard::Benign);
        if options.json {
            json_parts.push(reporter.cycles_json(&findings));
        } else {
            output.push_str(&reporter.cycles_human(&findings, scanned));
        }
    }

    let should_fail = match options.fail_on {
        FailOn::Never => false,
        // The default gate is the narrowest true statement the tool can
        // make: this throws when your own entry point loads it. A silent
        // undefined read and a crash that needs a deep import are both real,
        // and both live one level up — grouping them under "crash" would
        // fail builds that work, which is how a gate gets switched off.
        FailOn::Crash => worst_hazard == Hazard::Crash,
        FailOn::Hazard => worst_hazard >= Hazard::ConditionalCrash,
        FailOn::Any => cycle_count > 0 || barrel_count > 0,
    };

    if options.json {
        let payload = json_payload(json_parts);
        output = format!(
            "{}\n",
            serde_json::to_string_pretty(&payload).unwrap_or_default()
        );
    } else {
        output.push('\n');
    }

    Ok(Outcome {
        output,
        should_fail,
    })
}

/// A single analysis emits its own document; `check` wraps both in an
/// envelope, dropping the per-part header that would only repeat it.
fn json_payload(mut parts: Vec<Value>) -> Value {
    if parts.len() == 1 {
        return parts.pop().unwrap_or(Value::Null);
    }
    for part in &mut parts {
        if let Some(object) = part.as_object_mut() {
            object.remove("tool");
            object.remove("version");
        }
    }
    json!({
        "tool": "overpull",
        "version": crate::cli::VERSION,
        "command": "check",
        "results": parts,
    })
}

fn validate_root(options: &Options) -> Result<PathBuf, String> {
    if !options.root.exists() {
        return Err(format!("cannot read --root `{}`", options.root.display()));
    }
    let root = crate::util::normalize(&options.root);
    if !root.is_dir() {
        return Err(format!("--root `{}` is not a directory", root.display()));
    }
    if let Some(tsconfig) = &options.tsconfig {
        if !tsconfig.is_file() {
            return Err(format!(
                "--tsconfig `{}` does not exist",
                tsconfig.display()
            ));
        }
    }
    Ok(root)
}

/// `cost` seeds from the named entries only, so the graph is exactly what
/// those entries reach. The project-wide commands seed from everything.
fn seed_files(options: &Options, root: &Path) -> Result<Vec<PathBuf>, String> {
    if options.command == Command::Cost {
        return resolve_entries(root, &options.entries);
    }
    let discovered = discover_sources(root);
    if discovered.is_empty() {
        return Err(format!(
            "no source files found under `{}`.\n\
             overpull reads .ts/.tsx/.mts/.cts/.js/.jsx/.mjs/.cjs; check --root.",
            root.display()
        ));
    }
    Ok(discovered)
}

fn resolve_entries(root: &Path, entries: &[String]) -> Result<Vec<PathBuf>, String> {
    let mut paths = Vec::with_capacity(entries.len());
    for entry in entries {
        let candidate = if Path::new(entry).is_absolute() {
            PathBuf::from(entry)
        } else {
            root.join(entry)
        };
        if !candidate.exists() {
            return Err(format!(
                "entry `{entry}` not found.\n\
                 Paths are relative to --root (`{}`).",
                root.display()
            ));
        }
        let resolved = crate::util::normalize(&candidate);
        if !resolved.is_file() {
            return Err(format!("entry `{entry}` is not a file"));
        }
        if !crate::parse::is_source_file(&resolved) {
            return Err(format!(
                "entry `{entry}` is not a source file overpull can read\n\
                 (expected .ts/.tsx/.mts/.cts/.js/.jsx/.mjs/.cjs, and not a .d.ts)"
            ));
        }
        paths.push(resolved);
    }
    Ok(paths)
}
