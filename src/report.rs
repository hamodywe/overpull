//! Terminal and JSON reporting.
//!
//! Every string that came out of a scanned project passes through
//! [`crate::style::sanitize`] before printing: a file name or specifier is
//! attacker-influenced text, and a report is not a place to replay escape
//! sequences.

use serde_json::{Value, json};

use crate::barrels::BarrelReport;
use crate::cost::EntryCostReport;
use crate::cycles::{BreakSuggestion, CycleFinding, Hazard};
use crate::graph::ModuleGraph;
use crate::style::{BOLD, CYAN, DIM, GREEN, RED, Style, YELLOW, sanitize};
use crate::util::format_bytes;

pub struct Reporter<'a> {
    pub graph: &'a ModuleGraph,
    pub style: Style,
    pub top: usize,
}

impl Reporter<'_> {
    fn path(&self, idx: usize) -> String {
        sanitize(&self.graph.display_path(idx))
    }

    // ---- cost ----------------------------------------------------------

    pub fn cost_human(&self, reports: &[EntryCostReport]) -> String {
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
                if report.module_count == 1 { "" } else { "s" },
                format_bytes(report.total_bytes),
                report.external_packages.len(),
                if report.external_packages.len() == 1 {
                    ""
                } else {
                    "s"
                }
            ));
            if report.dynamic_target_count > 0 {
                let (plural, verb) = if report.dynamic_target_count == 1 {
                    ("", "sits")
                } else {
                    ("s", "sit")
                };
                out.push_str(&format!(
                    "  {} module{plural} {verb} behind dynamic import() and are not counted above\n",
                    report.dynamic_target_count,
                ));
            }
            if report.opaque_dynamic_imports > 0 {
                out.push_str(&self.style.paint(
                    DIM,
                    &format!(
                        "  {} dynamic import{} have computed specifiers and could not be followed\n",
                        report.opaque_dynamic_imports,
                        if report.opaque_dynamic_imports == 1 { "" } else { "s" }
                    ),
                ));
            }
            if report.unresolved > 0 {
                out.push_str(&self.style.paint(
                    YELLOW,
                    &format!(
                        "  {} import{} did not resolve — the real cost is higher than shown\n",
                        report.unresolved,
                        if report.unresolved == 1 { "" } else { "s" }
                    ),
                ));
            }

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

    pub fn cost_json(&self, reports: &[EntryCostReport]) -> Value {
        json!({
            "tool": "overpull",
            "version": crate::cli::VERSION,
            "command": "cost",
            "entries": reports.iter().map(|report| json!({
                "entry": self.path(report.entry),
                "modules": report.module_count,
                "bytes": report.total_bytes,
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

    // ---- barrels -------------------------------------------------------

    pub fn barrels_human(&self, reports: &[BarrelReport], scanned: usize) -> String {
        let mut out = String::new();
        if reports.is_empty() {
            return format!(
                "\n{}\n  {} modules scanned, no barrel amplifies above the threshold.\n",
                self.style.paint(GREEN, "barrels  clean"),
                scanned
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
                    .paint(RED, &format!("{}x amplification", report.amplification)),
            ));
            out.push_str(&format!(
                "    {} re-exports ({} via export *), {} local · imported by {} module{}\n",
                report.reexport_count,
                report.star_count,
                report.local_export_count,
                report.importer_count,
                if report.importer_count == 1 { "" } else { "s" },
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

    pub fn cycles_human(&self, findings: &[CycleFinding], scanned: usize) -> String {
        if findings.is_empty() {
            return format!(
                "\n{}\n  {} modules scanned, no import cycles.\n",
                self.style.paint(GREEN, "cycles  clean"),
                scanned
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

    fn one_cycle(&self, finding: &CycleFinding) -> String {
        {
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
                let where_ = if detail.in_extends {
                    " in an `extends` clause"
                } else {
                    " while the module evaluates"
                };
                out.push_str(&format!(
                    "    {}:{} reads `{}`{}, but {} has not run yet\n",
                    self.path(detail.reader),
                    detail.read_line,
                    sanitize(&detail.binding_local),
                    where_,
                    self.path(detail.owner),
                ));
                let consequence = match finding.hazard {
                    Hazard::Crash | Hazard::ConditionalCrash => format!(
                        "ReferenceError: Cannot access '{}' before initialization",
                        sanitize(&detail.imported_name)
                    ),
                    Hazard::Undefined => format!(
                        "`{}` reads as undefined at load time",
                        sanitize(&detail.binding_local)
                    ),
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
                    // people act on and one they learn to scroll past: from
                    // the project's own entry points this order never occurs.
                    out.push_str(&self.style.paint(
                        DIM,
                        &format!(
                            "    only when {} is the first module loaded — a deep import, or a\n\
                             \x20   test importing it directly. Starting from the project's own\n\
                             \x20   entry points, the order is safe.\n",
                            self.path(detail.entry)
                        ),
                    ));
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

    pub fn cycles_json(&self, findings: &[CycleFinding]) -> Value {
        json!({
            "tool": "overpull",
            "version": crate::cli::VERSION,
            "command": "cycles",
            "cycles": findings.iter().map(|finding| {
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
                        "importedName": detail.imported_name,
                        "owner": self.path(detail.owner),
                        "importPointsAt": self.path(detail.via),
                        "declarationKind": detail.decl_kind.label(),
                        "inExtendsClause": detail.in_extends,
                        "entry": self.path(detail.entry),
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
}
