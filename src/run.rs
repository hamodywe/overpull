//! Command orchestration: discover, build the graph, analyze, report.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::barrels::{self, BarrelReport, Thresholds};
use crate::baseline::Baseline;
use crate::cli::{Command, FailOn, Options};
use crate::cost::{self, EntryCostReport};
use crate::cycles::{self, CycleAnalysis, Hazard};
use crate::entries::{self, Entry};
use crate::graph::ModuleGraph;
use crate::report::Reporter;
use crate::resolve::SpecResolver;
use crate::style::Style;
use crate::walk::discover_sources;
use crate::why::{self, WhyReport};

pub struct Outcome {
    pub output: String,
    pub should_fail: bool,
}

/// How many findings a baseline hid, so a quiet report is never mistaken for
/// a clean one.
#[derive(Default, Clone, Copy)]
pub struct Suppressed {
    pub cycles: usize,
    pub barrels: usize,
}

impl Suppressed {
    pub fn total(self) -> usize {
        self.cycles + self.barrels
    }
}

/// Everything the requested command produced, before any of it is formatted.
#[derive(Default)]
struct Analyses {
    costs: Vec<EntryCostReport>,
    barrels: Vec<BarrelReport>,
    cycles: Option<CycleAnalysis>,
    why: Option<WhyReport>,
    suppressed: Suppressed,
    budget_exceeded: bool,
}

impl Analyses {
    fn worst_hazard(&self) -> Hazard {
        self.cycles.as_ref().map_or(Hazard::Benign, |analysis| {
            analysis
                .findings
                .iter()
                .map(|f| f.hazard)
                .max()
                .unwrap_or(Hazard::Benign)
        })
    }

    fn should_fail(&self, options: &Options) -> bool {
        if self.budget_exceeded {
            return true;
        }
        match options.fail_on {
            FailOn::Never => false,
            // The default gate is the narrowest true statement the tool can
            // make: this throws when your own entry point loads it. A silent
            // undefined read and a crash that needs a deep import are both
            // real, and both live one level up — grouping them under "crash"
            // would fail builds that work, which is how a gate gets switched
            // off.
            FailOn::Crash => self.worst_hazard() == Hazard::Crash,
            FailOn::Hazard => self.worst_hazard() >= Hazard::ConditionalCrash,
            FailOn::Any => {
                self.cycles.as_ref().is_some_and(|a| !a.findings.is_empty())
                    || !self.barrels.is_empty()
            }
        }
    }
}

pub fn run(options: &Options) -> Result<Outcome, String> {
    let root = validate_root(options)?;
    let resolver = SpecResolver::new(options.tsconfig.as_deref());
    let seeds = seed_files(options, &root)?;
    let graph = ModuleGraph::build(&root, &seeds, &resolver);
    if graph.modules.is_empty() {
        return Err("nothing to analyze: no module could be parsed".into());
    }

    let baseline = match &options.baseline {
        Some(path) => Some(Baseline::load(path)?),
        None => None,
    };
    let explicit = module_indices(&graph, &root, &options.entry_files, "--entry")?;
    let entry_set = entries::classify(&graph, &explicit);
    let analyses = analyze(options, &graph, &root, &entry_set, baseline.as_ref())?;

    let style = if options.json || options.sarif {
        Style::plain()
    } else {
        Style::detect(options.no_color)
    };
    let reporter = Reporter {
        graph: &graph,
        style,
        top: options.top,
    };

    Ok(Outcome {
        should_fail: analyses.should_fail(options),
        output: render(
            &reporter,
            options,
            &entry_set,
            &analyses,
            graph.modules.len(),
        ),
    })
}

fn analyze(
    options: &Options,
    graph: &ModuleGraph,
    root: &Path,
    entry_set: &[Entry],
    baseline: Option<&Baseline>,
) -> Result<Analyses, String> {
    let mut analyses = Analyses::default();

    match options.command {
        Command::Cost => {
            let indices = module_indices(graph, root, &options.entries, "entry")?;
            analyses.costs = cost::analyze(graph, &indices);
            analyses.budget_exceeded = analyses.costs.iter().any(|report| {
                options
                    .max_modules
                    .is_some_and(|max| report.module_count > max)
                    || options
                        .max_bytes
                        .is_some_and(|max| report.total_bytes > max)
            });
        }
        Command::Why => {
            let target = *module_indices(graph, root, &options.entries, "module")?
                .first()
                .ok_or_else(|| "no module to explain".to_string())?;
            analyses.why = Some(why::analyze(graph, entry_set, target));
        }
        Command::Barrels | Command::Cycles | Command::Check => {}
    }

    if matches!(options.command, Command::Barrels | Command::Check) {
        let thresholds = Thresholds {
            min_amplification: options.min_amplification,
            min_cost: options.min_cost,
            ..Thresholds::default()
        };
        analyses.barrels = barrels::analyze(graph, &thresholds);
        if let Some(baseline) = baseline {
            let before = analyses.barrels.len();
            analyses.barrels.retain(|r| {
                !baseline.covers_barrel(&graph.display_path(r.module), r.amplification)
            });
            analyses.suppressed.barrels = before - analyses.barrels.len();
        }
    }

    if matches!(options.command, Command::Cycles | Command::Check) {
        let mut found = cycles::analyze(graph, entry_set);
        if let Some(baseline) = baseline {
            let before = found.findings.len();
            found.findings.retain(|finding| {
                let members: Vec<String> = finding
                    .members
                    .iter()
                    .map(|&m| graph.display_path(m))
                    .collect();
                !baseline.covers_cycle(&members, finding.hazard)
            });
            analyses.suppressed.cycles = before - found.findings.len();
        }
        analyses.cycles = Some(found);
    }

    Ok(analyses)
}

fn render(
    reporter: &Reporter,
    options: &Options,
    entry_set: &[Entry],
    analyses: &Analyses,
    scanned: usize,
) -> String {
    // `why` answers a question rather than reporting findings, so it has no
    // SARIF shape; JSON is its machine-readable form.
    if let Some(report) = &analyses.why {
        return if options.json || options.sarif {
            render_json(vec![reporter.why_json(report)])
        } else {
            format!("{}\n", reporter.why_human(report))
        };
    }

    if options.sarif {
        let findings = analyses
            .cycles
            .as_ref()
            .map_or(&[][..], |a| a.findings.as_slice());
        return format!(
            "{}\n",
            serde_json::to_string_pretty(&reporter.sarif(findings, &analyses.barrels))
                .unwrap_or_default()
        );
    }

    if options.json {
        return render_json(json_parts(reporter, options, analyses));
    }

    let mut text = String::new();
    if options.command == Command::Cost {
        text.push_str(&reporter.cost_human(
            &analyses.costs,
            options.max_modules,
            options.max_bytes,
        ));
    }
    if matches!(options.command, Command::Barrels | Command::Check) {
        text.push_str(&reporter.barrels_human(
            &analyses.barrels,
            scanned,
            analyses.suppressed.barrels,
        ));
    }
    if let Some(analysis) = &analyses.cycles {
        text.push_str(&reporter.cycles_human(
            analysis,
            scanned,
            entry_set,
            analyses.suppressed.cycles,
        ));
    }
    text.push_str(&reporter.suppressed_note(analyses.suppressed));
    text.push('\n');
    text
}

fn json_parts(reporter: &Reporter, options: &Options, analyses: &Analyses) -> Vec<Value> {
    let mut parts = Vec::new();
    if options.command == Command::Cost {
        parts.push(reporter.cost_json(&analyses.costs, options.max_modules, options.max_bytes));
    }
    if matches!(options.command, Command::Barrels | Command::Check) {
        parts.push(reporter.barrels_json(&analyses.barrels));
    }
    if let Some(analysis) = &analyses.cycles {
        parts.push(reporter.cycles_json(analysis));
    }
    parts
}

/// A single analysis emits its own document; `check` wraps both in an
/// envelope, dropping the per-part header that would only repeat it.
fn render_json(mut parts: Vec<Value>) -> String {
    let payload = if parts.len() == 1 {
        parts.pop().unwrap_or(Value::Null)
    } else {
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
    };
    format!(
        "{}\n",
        serde_json::to_string_pretty(&payload).unwrap_or_default()
    )
}

fn validate_root(options: &Options) -> Result<PathBuf, String> {
    if !options.root.exists() {
        return Err(format!("cannot read --root `{}`", options.root.display()));
    }
    let root = crate::util::normalize(&options.root);
    if !root.is_dir() {
        return Err(format!("--root `{}` is not a directory", root.display()));
    }
    if let Some(tsconfig) = &options.tsconfig
        && !tsconfig.is_file()
    {
        return Err(format!(
            "--tsconfig `{}` does not exist",
            tsconfig.display()
        ));
    }
    Ok(root)
}

/// `cost` seeds from the named entries only, so the graph is exactly what
/// those entries reach. Every other command seeds from the whole project:
/// `why` and `cycles` both need to know who imports what.
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

/// Resolves user-given paths to modules already in the graph.
fn module_indices(
    graph: &ModuleGraph,
    root: &Path,
    paths: &[String],
    label: &str,
) -> Result<Vec<usize>, String> {
    let mut indices = Vec::with_capacity(paths.len());
    for path in resolve_entries(root, paths)? {
        let index = graph.index_by_path.get(&path).copied().ok_or_else(|| {
            format!(
                "{label} `{}` is not in the analyzed graph.\n\
                 It may sit in a skipped directory (node_modules, dist, build, \
                 out, coverage, target, vendor, or a dot-directory).",
                crate::util::display_path(root, &path)
            )
        })?;
        indices.push(index);
    }
    Ok(indices)
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
