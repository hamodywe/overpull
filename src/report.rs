//! Terminal, JSON and SARIF reporting.
//!
//! Every string that came out of a scanned project passes through
//! [`crate::style::sanitize`] before printing: a file name or specifier is
//! attacker-influenced text, and a report is not a place to replay escape
//! sequences.

use serde_json::{Value, json};

use crate::barrels::BarrelReport;
use crate::cost::EntryCostReport;
use crate::cycles::{BreakSuggestion, CycleAnalysis, CycleFinding, Hazard};
use crate::entries::{Entry, EntryKind};
use crate::graph::ModuleGraph;
use crate::run::Suppressed;
use crate::style::{BOLD, CYAN, DIM, GREEN, RED, Style, YELLOW, sanitize};
use crate::util::format_bytes;
use crate::why::WhyReport;

pub struct Reporter<'a> {
    pub graph: &'a ModuleGraph,
    pub style: Style,
    pub top: usize,
}

/// `1 module` / `3 modules`, without a second `format!` at every call site.
fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

impl Reporter<'_> {
    fn path(&self, idx: usize) -> String {
        sanitize(&self.graph.display_path(idx))
    }

    // ---- cost ----------------------------------------------------------

    pub fn cost_human(
        &self,
        reports: &[EntryCostReport],
        max_modules: Option<usize>,
        max_bytes: Option<u64>,
    ) -> String {
        let mut out = String::new();
        for report in reports {
            out.push_str(&format!(
                "\n{}\n",
                self.style
                    .paint(BOLD, &format!("cost  {}", self.path(report.entry)))
            ));
            out.push_str(&format!(
                "  loads {} module{} · {} of source · {} external package{}\n",
                self.style.paint(CYAN, &report.module_count.to_string()),
                plural(report.module_count),
                format_bytes(report.total_bytes),
                report.external_packages.len(),
                plural(report.external_packages.len()),
            ));
            if report.dynamic_target_count > 0 {
                let (suffix, verb) = if report.dynamic_target_count == 1 {
                    ("", "sits")
                } else {
                    ("s", "sit")
                };
                out.push_str(&format!(
                    "  {} module{suffix} {verb} behind dynamic import() and are not counted above\n",
                    report.dynamic_target_count,
                ));
            }
            if report.opaque_dynamic_imports > 0 {
                out.push_str(&self.style.paint(
                    DIM,
                    &format!(
                        "  {} dynamic import{} have computed specifiers and could not be followed\n",
                        report.opaque_dynamic_imports,
                        plural(report.opaque_dynamic_imports),
                    ),
                ));
            }
            if report.unresolved > 0 {
                out.push_str(&self.style.paint(
                    YELLOW,
                    &format!(
                        "  {} import{} did not resolve — the real cost is higher than shown\n",
                        report.unresolved,
                        plural(report.unresolved),
                    ),
                ));
            }
            out.push_str(&self.budget_lines(report, max_modules, max_bytes));

            let contributors: Vec<_> = report
                .contributors
                .iter()
                .filter(|c| c.exclusive_modules > 0)
                .take(self.top)
                .collect();
            if contributors.is_empty() {
                out.push_str(&self.style.paint(
                    DIM,
                    "  no single import owns part of this graph on its own\n",
                ));
            } else {
                out.push_str("\n  brought in by, and only by:\n");
                for c in contributors {
                    out.push_str(&format!(
                        "    {:>5}  {}  {}\n",
                        self.style.paint(CYAN, &c.exclusive_modules.to_string()),
                        sanitize(&c.specifier),
                        self.style.paint(DIM, &format!("line {}", c.line)),
                    ));
                }
                out.push_str(&self.style.paint(
                    DIM,
                    "    (modules that leave the graph entirely if that import goes)\n",
                ));
            }
        }
        out
    }

    /// The budget verdict, printed only when a budget was asked for — a
    /// number with no threshold is not a gate, and a gate that stays silent
    /// when it passes is not one either.
    fn budget_lines(
        &self,
        report: &EntryCostReport,
        max_modules: Option<usize>,
        max_bytes: Option<u64>,
    ) -> String {
        let mut out = String::new();
        if let Some(max) = max_modules {
            let over = report.module_count > max;
            out.push_str(&format!(
                "  {} {} modules, budget {max}{}\n",
                self.style.paint(
                    if over { RED } else { GREEN },
                    if over { "over" } else { "within" }
                ),
                report.module_count,
                if over {
                    format!(" — {} over", report.module_count - max)
                } else {
                    String::new()
                },
            ));
        }
        if let Some(max) = max_bytes {
            let over = report.total_bytes > max;
            out.push_str(&format!(
                "  {} {}, budget {}{}\n",
                self.style.paint(
                    if over { RED } else { GREEN },
                    if over { "over" } else { "within" }
                ),
                format_bytes(report.total_bytes),
                format_bytes(max),
                if over {
                    format!(" — {} over", format_bytes(report.total_bytes - max))
                } else {
                    String::new()
                },
            ));
        }
        out
    }

    pub fn cost_json(
        &self,
        reports: &[EntryCostReport],
        max_modules: Option<usize>,
        max_bytes: Option<u64>,
    ) -> Value {
        json!({
            "tool": "overpull",
            "version": crate::cli::VERSION,
            "command": "cost",
            "budget": {
                "maxModules": max_modules,
                "maxBytes": max_bytes,
            },
            "entries": reports.iter().map(|report| json!({
                "entry": self.path(report.entry),
                "modules": report.module_count,
                "bytes": report.total_bytes,
                "overBudget": max_modules.is_some_and(|max| report.module_count > max)
                    || max_bytes.is_some_and(|max| report.total_bytes > max),
                "externalPackages": report.external_packages,
                "dynamicTargets": report.dynamic_target_count,
                "opaqueDynamicImports": report.opaque_dynamic_imports,
                "unresolvedImports": report.unresolved,
                "contributors": report.contributors.iter()
                    .filter(|c| c.exclusive_modules > 0)
                    .map(|c| json!({
                        "specifier": c.specifier,
                        "line": c.line,
                        "target": self.path(c.target),
                        "exclusiveModules": c.exclusive_modules,
                    })).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        })
    }

    // ---- why -----------------------------------------------------------

    pub fn why_human(&self, report: &WhyReport) -> String {
        let mut out = format!(
            "\n{}\n",
            self.style
                .paint(BOLD, &format!("why  {}", self.path(report.target)))
        );
        out.push_str(&format!(
            "  once loaded it pulls {} module{} ({})\n",
            self.style.paint(CYAN, &report.cost_modules.to_string()),
            plural(report.cost_modules),
            format_bytes(report.cost_bytes),
        ));

        if report.paths.is_empty() {
            out.push_str(
                &self
                    .style
                    .paint(GREEN, "\n  no entry point loads this module at startup.\n"),
            );
        } else {
            out.push_str("\n  shortest chain from each entry point:\n");
            for path in report.paths.iter().take(self.top) {
                out.push_str(&format!(
                    "\n    {} {}\n",
                    self.style.paint(DIM, path.kind.label()),
                    self.style.paint(CYAN, &self.path(path.entry)),
                ));
                for (hop, window) in path.path.windows(2).enumerate() {
                    let line = path.lines.get(hop).copied().unwrap_or(0);
                    out.push_str(&format!(
                        "      {}:{} → {}\n",
                        self.path(window[0]),
                        line,
                        self.path(window[1]),
                    ));
                }
            }
            if report.paths.len() > self.top {
                out.push_str(&self.style.paint(
                    DIM,
                    &format!(
                        "\n  … {} more entry point{} reach it (raise --top)\n",
                        report.paths.len() - self.top,
                        plural(report.paths.len() - self.top),
                    ),
                ));
            }
        }

        if report.unreachable_entries > 0 {
            out.push_str(&self.style.paint(
                DIM,
                &format!(
                    "\n  {} entry point{} never load{} it\n",
                    report.unreachable_entries,
                    plural(report.unreachable_entries),
                    if report.unreachable_entries == 1 {
                        "s"
                    } else {
                        ""
                    },
                ),
            ));
        }
        if !report.dynamic_importers.is_empty() {
            out.push_str(&format!(
                "\n  behind a dynamic import() in {} place{}:\n",
                report.dynamic_importers.len(),
                plural(report.dynamic_importers.len()),
            ));
            for &(module, line) in report.dynamic_importers.iter().take(self.top) {
                out.push_str(&format!("    {}:{}\n", self.path(module), line));
            }
        }
        out.push_str(&format!(
            "\n  imported directly by {} module{}\n",
            report.direct_importers.len(),
            plural(report.direct_importers.len()),
        ));
        for (module, line, specifier) in report.direct_importers.iter().take(self.top) {
            out.push_str(&format!(
                "    {}:{}  {}\n",
                self.path(*module),
                line,
                self.style.paint(DIM, &sanitize(specifier)),
            ));
        }
        out
    }

    pub fn why_json(&self, report: &WhyReport) -> Value {
        json!({
            "tool": "overpull",
            "version": crate::cli::VERSION,
            "command": "why",
            "module": self.path(report.target),
            "loadCost": { "modules": report.cost_modules, "bytes": report.cost_bytes },
            "unreachableEntries": report.unreachable_entries,
            "paths": report.paths.iter().map(|path| json!({
                "entry": self.path(path.entry),
                "entryKind": path.kind.label(),
                "hops": path.path.iter().map(|&m| self.path(m)).collect::<Vec<_>>(),
                "lines": path.lines,
            })).collect::<Vec<_>>(),
            "directImporters": report.direct_importers.iter().map(|(module, line, specifier)| json!({
                "file": self.path(*module),
                "line": line,
                "specifier": specifier,
            })).collect::<Vec<_>>(),
            "dynamicImporters": report.dynamic_importers.iter().map(|(module, line)| json!({
                "file": self.path(*module),
                "line": line,
            })).collect::<Vec<_>>(),
        })
    }

    // ---- barrels -------------------------------------------------------

    pub fn barrels_human(&self, reports: &[BarrelReport], scanned: usize, hidden: usize) -> String {
        let mut out = String::new();
        if reports.is_empty() {
            return format!(
                "\n{}\n  {scanned} modules scanned, no {}barrel amplifies above the threshold.\n",
                self.style.paint(GREEN, "barrels  clean"),
                if hidden > 0 { "new " } else { "" },
            );
        }
        out.push_str(&format!(
            "\n{}\n",
            self.style
                .paint(BOLD, &format!("barrels  {} amplifying", reports.len()))
        ));
        for report in reports.iter().take(self.top) {
            out.push_str(&format!(
                "\n  {}\n",
                self.style.paint(YELLOW, &self.path(report.module))
            ));
            out.push_str(&format!(
                "    importing it loads {} modules ({}); a member costs {} — {}\n",
                self.style.paint(CYAN, &report.cost_modules.to_string()),
                format_bytes(report.cost_bytes),
                report.median_target_cost,
                self.style
                    .paint(RED, &format!("{:.1}x amplification", report.amplification)),
            ));
            out.push_str(&format!(
                "    {} re-exports ({} via export *), {} local · imported by {} module{}\n",
                report.reexport_count,
                report.star_count,
                report.local_export_count,
                report.importer_count,
                plural(report.importer_count),
            ));
        }
        if reports.len() > self.top {
            out.push_str(&self.style.paint(
                DIM,
                &format!(
                    "\n  … {} more (raise --top to see them)\n",
                    reports.len() - self.top
                ),
            ));
        }
        out
    }

    pub fn barrels_json(&self, reports: &[BarrelReport]) -> Value {
        json!({
            "tool": "overpull",
            "version": crate::cli::VERSION,
            "command": "barrels",
            "barrels": reports.iter().map(|report| json!({
                "file": self.path(report.module),
                "reexports": report.reexport_count,
                "starReexports": report.star_count,
                "localExports": report.local_export_count,
                "costModules": report.cost_modules,
                "costBytes": report.cost_bytes,
                "medianMemberCost": report.median_target_cost,
                "amplification": report.amplification,
                "importers": report.importer_count,
                "externalPackages": report.external_packages,
            })).collect::<Vec<_>>(),
        })
    }

    // ---- cycles --------------------------------------------------------

    pub fn cycles_human(
        &self,
        analysis: &CycleAnalysis,
        scanned: usize,
        entries: &[Entry],
        hidden: usize,
    ) -> String {
        let findings = &analysis.findings;
        if findings.is_empty() {
            return format!(
                "\n{}\n  {scanned} modules scanned, no {}import cycles.\n",
                self.style.paint(GREEN, "cycles  clean"),
                if hidden > 0 { "new " } else { "" },
            );
        }
        let count = |hazard: Hazard| findings.iter().filter(|f| f.hazard == hazard).count();
        let crashes = count(Hazard::Crash);

        let mut out = format!(
            "\n{}\n  {} crash · {} crash-if-loaded-first · {} undefined-read · \
             {} cjs-mixed · {} benign\n",
            self.style
                .paint(BOLD, &format!("cycles  {} found", findings.len())),
            self.style
                .paint(if crashes > 0 { RED } else { DIM }, &crashes.to_string()),
            count(Hazard::ConditionalCrash),
            count(Hazard::Undefined),
            count(Hazard::CjsMixed),
            count(Hazard::Benign),
        );
        out.push_str(&self.entry_note(analysis, entries));

        let mut sorted: Vec<&CycleFinding> = findings.iter().collect();
        sorted.sort_by(|a, b| {
            b.hazard
                .cmp(&a.hazard)
                .then_with(|| a.members[0].cmp(&b.members[0]))
        });

        for finding in sorted.iter().take(self.top) {
            out.push_str(&self.one_cycle(finding));
        }
        if findings.len() > self.top {
            out.push_str(&self.style.paint(
                DIM,
                &format!(
                    "\n  … {} more (raise --top to see them)\n",
                    findings.len() - self.top
                ),
            ));
        }
        out
    }

    /// What the verdicts were computed against. A severity that depends on
    /// entry order is only as trustworthy as the entry set, so the entry set
    /// is printed rather than assumed.
    fn entry_note(&self, analysis: &CycleAnalysis, entries: &[Entry]) -> String {
        let package = entries
            .iter()
            .filter(|e| e.kind == EntryKind::Package)
            .count();
        let tests = entries.iter().filter(|e| e.kind == EntryKind::Test).count();
        let mut note = format!(
            "  simulated from {} of {} entry point{}",
            analysis.entries_simulated,
            analysis.entries_simulated + analysis.entries_skipped,
            plural(analysis.entries_simulated + analysis.entries_skipped),
        );
        if tests > 0 {
            note.push_str(&format!(" ({package} declared, {tests} test)"));
        }
        note.push('\n');
        let mut out = self.style.paint(DIM, &note);

        // Only a skipped *declared* entry can turn a crash into a conditional
        // one; skipped test files can only cost a conditional finding, so the
        // two are not worth the same warning.
        if analysis.entries_skipped_declared > 0 {
            out.push_str(&self.style.paint(
                YELLOW,
                &format!(
                    "  {} declared entry point{} were not simulated; a crash reachable only\n\
                     \x20 from those is reported as crash-if-loaded-first. Narrow the set with --entry.\n",
                    analysis.entries_skipped_declared,
                    plural(analysis.entries_skipped_declared),
                ),
            ));
        } else if analysis.entries_skipped > 0 {
            out.push_str(&self.style.paint(
                DIM,
                &format!(
                    "  {} further test file{} were not simulated — every declared entry point was\n",
                    analysis.entries_skipped,
                    plural(analysis.entries_skipped),
                ),
            ));
        }
        out
    }

    fn one_cycle(&self, finding: &CycleFinding) -> String {
        let (color, label) = match finding.hazard {
            Hazard::Crash => (RED, "crash"),
            Hazard::ConditionalCrash => (YELLOW, "crash-if-loaded-first"),
            Hazard::Undefined => (YELLOW, "undefined-read"),
            Hazard::CjsMixed => (YELLOW, "cjs-mixed"),
            Hazard::Benign => (DIM, "benign"),
        };
        let mut out = format!("\n  {}  ", self.style.paint(color, label));
        let path: Vec<String> = finding.cycle_path.iter().map(|&m| self.path(m)).collect();
        out.push_str(&format!("{}\n", path.join(" → ")));

        if let Some(detail) = &finding.detail {
            let read = match &detail.member {
                Some(member) => format!("{}.{}", detail.binding_local, member),
                None => detail.binding_local.clone(),
            };
            let where_ = if detail.in_extends {
                " in an `extends` clause"
            } else {
                " while the module evaluates"
            };
            out.push_str(&format!(
                "    {}:{} reads `{}`{}, but {} has not run yet\n",
                self.path(detail.reader),
                detail.read_line,
                sanitize(&read),
                where_,
                self.path(detail.owner),
            ));
            let consequence = match finding.hazard {
                Hazard::Crash | Hazard::ConditionalCrash => format!(
                    "ReferenceError: Cannot access '{}' before initialization",
                    sanitize(&detail.imported_name)
                ),
                Hazard::Undefined => {
                    format!("`{}` reads as undefined at load time", sanitize(&read))
                }
                _ => String::new(),
            };
            if !consequence.is_empty() {
                out.push_str(&format!(
                    "    {} — it is a {}, so: {}\n",
                    self.style.paint(DIM, "at run time"),
                    detail.decl_kind.label(),
                    self.style.paint(color, &consequence),
                ));
            }
            if finding.hazard == Hazard::ConditionalCrash {
                // Saying this plainly is the difference between a report
                // people act on and one they learn to scroll past: from the
                // project's own entry points this order never occurs.
                //
                // A test file is the strongest form of this: a real file in
                // the repository, importing into the cycle, that produces the
                // order. It still is not a `crash`, because the test process
                // has usually evaluated the safe half long before the spec
                // runs — which is why a green suite is not a contradiction.
                let explanation = if detail.entry_kind == EntryKind::Test {
                    format!(
                        "    reachable from your test file {} — it imports into this cycle\n\
                         \x20   directly, which produces the failing order. Whether it fires\n\
                         \x20   depends on what the test process loaded first.\n",
                        self.path(detail.entry)
                    )
                } else {
                    format!(
                        "    only when {} is the first module loaded — a deep import, or a\n\
                         \x20   test importing it directly. Starting from the project's own\n\
                         \x20   entry points, the order is safe.\n",
                        self.path(detail.entry)
                    )
                };
                out.push_str(&self.style.paint(DIM, &explanation));
            }
            if detail.owner != detail.via {
                out.push_str(&self.style.paint(
                    DIM,
                    &format!(
                        "    reached through {} (re-export), which is where the import points\n",
                        self.path(detail.via)
                    ),
                ));
            }
        } else if finding.hazard == Hazard::CjsMixed {
            out.push_str(
                "    the loop crosses a require() edge — CommonJS may observe a partial\n\
                 \x20   exports object; overpull cannot prove which half you get\n",
            );
        } else {
            out.push_str(&self.style.paint(
                DIM,
                "    every binding is a hoisted function or used only inside functions\n",
            ));
        }

        out.push_str(&self.break_suggestion(finding.suggestion.as_ref()));
        out
    }

    fn break_suggestion(&self, suggestion: Option<&BreakSuggestion>) -> String {
        let fix = self.style.paint(GREEN, "fix");
        match suggestion {
            Some(BreakSuggestion::TypeOnly { from, to, line }) => format!(
                "    {fix} {}:{line} imports only types from {} — `import type` removes the edge\n",
                self.path(*from),
                self.path(*to),
            ),
            Some(BreakSuggestion::DeferImport { from, to, line }) => format!(
                "    {fix} {}:{line} uses {} only inside functions — a dynamic import() there\n\
                 \x20        breaks the load-time loop\n",
                self.path(*from),
                self.path(*to),
            ),
            Some(BreakSuggestion::ExtractShared { from, to, line }) => format!(
                "    {fix} lightest edge is {}:{line} → {}; move what both need into a\n\
                 \x20        module neither imports back\n",
                self.path(*from),
                self.path(*to),
            ),
            None => String::new(),
        }
    }

    pub fn cycles_json(&self, analysis: &CycleAnalysis) -> Value {
        json!({
            "tool": "overpull",
            "version": crate::cli::VERSION,
            "command": "cycles",
            "entriesSimulated": analysis.entries_simulated,
            "entriesSkipped": analysis.entries_skipped,
            "entriesSkippedDeclared": analysis.entries_skipped_declared,
            "cycles": analysis.findings.iter().map(|finding| {
                let mut entry = json!({
                    "hazard": finding.hazard.label(),
                    "members": finding.members.iter().map(|&m| self.path(m)).collect::<Vec<_>>(),
                    "path": finding.cycle_path.iter().map(|&m| self.path(m)).collect::<Vec<_>>(),
                });
                if let Some(detail) = &finding.detail {
                    entry["evidence"] = json!({
                        "reader": self.path(detail.reader),
                        "line": detail.read_line,
                        "binding": detail.binding_local,
                        "member": detail.member,
                        "importedName": detail.imported_name,
                        "owner": self.path(detail.owner),
                        "importPointsAt": self.path(detail.via),
                        "declarationKind": detail.decl_kind.label(),
                        "inExtendsClause": detail.in_extends,
                        "entry": self.path(detail.entry),
                        "entryKind": detail.entry_kind.label(),
                    });
                }
                entry["suggestion"] = match &finding.suggestion {
                    Some(BreakSuggestion::TypeOnly { from, to, line }) => json!({
                        "kind": "import-type", "from": self.path(*from),
                        "to": self.path(*to), "line": line,
                    }),
                    Some(BreakSuggestion::DeferImport { from, to, line }) => json!({
                        "kind": "defer-import", "from": self.path(*from),
                        "to": self.path(*to), "line": line,
                    }),
                    Some(BreakSuggestion::ExtractShared { from, to, line }) => json!({
                        "kind": "extract-shared", "from": self.path(*from),
                        "to": self.path(*to), "line": line,
                    }),
                    None => Value::Null,
                };
                entry
            }).collect::<Vec<_>>(),
        })
    }

    // ---- SARIF ---------------------------------------------------------

    /// SARIF 2.1.0, so findings land in a code-scanning dashboard with the
    /// evidence attached rather than as a line in a build log.
    ///
    /// Benign cycles are left out on purpose: a dashboard that shows every
    /// legal cycle teaches people to dismiss the whole run.
    pub fn sarif(&self, findings: &[CycleFinding], barrels: &[BarrelReport]) -> Value {
        let mut results: Vec<Value> = Vec::new();

        for finding in findings.iter().filter(|f| f.hazard != Hazard::Benign) {
            let (file, line) = match &finding.detail {
                Some(detail) => (self.path(detail.reader), detail.read_line),
                None => (self.path(finding.members[0]), 1),
            };
            let members: Vec<String> = finding.members.iter().map(|&m| self.path(m)).collect();
            let text = match &finding.detail {
                Some(detail) => format!(
                    "Import cycle ({}): `{}` is read at {}:{} before {} has evaluated. \
                     It is a {}. Cycle: {}.",
                    finding.hazard.label(),
                    detail.member.as_ref().map_or_else(
                        || detail.binding_local.clone(),
                        |m| format!("{}.{m}", detail.binding_local)
                    ),
                    file,
                    detail.read_line,
                    self.path(detail.owner),
                    detail.decl_kind.label(),
                    members.join(" → "),
                ),
                None => format!(
                    "Import cycle ({}): {}.",
                    finding.hazard.label(),
                    members.join(" → ")
                ),
            };
            results.push(sarif_result(
                finding.hazard.label(),
                sarif_level(finding.hazard),
                &text,
                &file,
                line,
            ));
        }

        for barrel in barrels {
            let file = self.path(barrel.module);
            let text = format!(
                "Barrel amplification {:.1}x: importing this file loads {} modules ({}), \
                 while importing a member costs {}. {} modules import it.",
                barrel.amplification,
                barrel.cost_modules,
                format_bytes(barrel.cost_bytes),
                barrel.median_target_cost,
                barrel.importer_count,
            );
            results.push(sarif_result(
                "barrel-amplification",
                "warning",
                &text,
                &file,
                1,
            ));
        }

        json!({
            "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
            "version": "2.1.0",
            "runs": [{
                "tool": { "driver": {
                    "name": "overpull",
                    "version": crate::cli::VERSION,
                    "informationUri": "https://github.com/hamodywe/overpull",
                    "rules": sarif_rules(),
                }},
                "results": results,
            }],
        })
    }

    // ---- shared --------------------------------------------------------

    pub fn suppressed_note(&self, suppressed: Suppressed) -> String {
        if suppressed.total() == 0 {
            return String::new();
        }
        self.style.paint(
            DIM,
            &format!(
                "\n  {} finding{} hidden by the baseline ({} cycle{}, {} barrel{})\n",
                suppressed.total(),
                plural(suppressed.total()),
                suppressed.cycles,
                plural(suppressed.cycles),
                suppressed.barrels,
                plural(suppressed.barrels),
            ),
        )
    }
}

fn sarif_level(hazard: Hazard) -> &'static str {
    match hazard {
        Hazard::Crash => "error",
        Hazard::ConditionalCrash | Hazard::Undefined => "warning",
        Hazard::CjsMixed | Hazard::Benign => "note",
    }
}

fn sarif_result(rule: &str, level: &str, text: &str, file: &str, line: u32) -> Value {
    json!({
        "ruleId": rule,
        "level": level,
        "message": { "text": text },
        "locations": [{ "physicalLocation": {
            "artifactLocation": { "uri": file },
            "region": { "startLine": line.max(1) },
        }}],
    })
}

fn sarif_rules() -> Vec<Value> {
    [
        (
            "crash",
            "Import cycle that throws at load time",
            "A `const`, `let` or `class` binding is read before the module declaring it \
             has evaluated, on the order this project's own entry points produce.",
        ),
        (
            "crash-if-loaded-first",
            "Import cycle that throws if the module is loaded first",
            "The same early read, on an order that only occurs when one of the cycle's own \
             modules is loaded before the entry point — a deep import, or a test.",
        ),
        (
            "undefined-read",
            "Import cycle producing a silent undefined",
            "A binding compiled to `var` or an enum is read before its module runs: no throw, \
             the value is simply `undefined`.",
        ),
        (
            "cjs-mixed",
            "Import cycle crossing a require() edge",
            "CommonJS evaluation order differs and a partial exports object may be observed. \
             Not statically provable either way.",
        ),
        (
            "barrel-amplification",
            "Barrel file amplifies import cost",
            "Importing this re-export file loads far more modules than importing the member \
             the caller needed.",
        ),
    ]
    .into_iter()
    .map(|(id, name, description)| {
        json!({
            "id": id,
            "name": name,
            "shortDescription": { "text": name },
            "fullDescription": { "text": description },
            "helpUri": "https://github.com/hamodywe/overpull/blob/main/docs/how-cycles-are-classified.md",
        })
    })
    .collect()
}
