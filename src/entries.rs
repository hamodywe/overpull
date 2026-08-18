//! Which modules the project actually starts from.
//!
//! Every order-dependent verdict overpull makes depends on this: a cycle that
//! throws when the app boots is a bug, and the same cycle reached only by
//! deep-importing an internal module is a trap that has not sprung yet. The
//! two deserve different words, so the entry set has to be right.
//!
//! Entries come from three places, in order of confidence: the paths
//! `package.json` names, conventional source entry points, and — when neither
//! exists — every module nothing imports.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use serde_json::Value;

use crate::graph::ModuleGraph;
use crate::util::normalize;

/// Where an entry point came from, which decides how much a finding reached
/// through it is worth.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum EntryKind {
    /// Named by `package.json`, given with `--entry`, or sitting at a
    /// conventional entry path. Running the project loads it.
    Package,
    /// A test or benchmark file. Nothing imports it, so it is a real deep
    /// import into whatever it reaches — but a test *process* has already
    /// loaded other modules by the time it runs, so the order a spec file
    /// produces on its own is not one the project is known to produce.
    Test,
    /// Imported by nothing and named by nothing: a script, a leftover, or a
    /// module only ever reached through a deep import.
    Orphan,
}

impl EntryKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Package => "entry point",
            Self::Test => "test file",
            Self::Orphan => "unreferenced module",
        }
    }

    /// Whether the project is *known* to produce this module's evaluation
    /// order. Only a declared entry point qualifies: a hazard found from one
    /// fires by starting the app, and that is what `crash` claims.
    ///
    /// A test file is deliberately excluded. Verified on `vitejs/vite`, whose
    /// `build.spec.ts` imports `build.ts` before `config.ts` and would throw
    /// loaded on its own — while the suite passes, because the vitest process
    /// has already evaluated `config.ts` by then. Calling that a crash would
    /// be calling a green test suite broken.
    pub fn is_real_start(self) -> bool {
        matches!(self, Self::Package)
    }

    /// Whether this entry is worth simulating at all. Test files are: they
    /// produce real orders nothing else does, and naming one is far better
    /// evidence for a conditional verdict than "some deep import".
    pub fn is_simulated(self) -> bool {
        matches!(self, Self::Package | Self::Test)
    }
}

pub struct Entry {
    pub module: usize,
    pub kind: EntryKind,
}

/// Directory names whose contents run under a test runner, not in production.
const TEST_DIRS: &[&str] = &[
    "test",
    "tests",
    "__tests__",
    "__mocks__",
    "spec",
    "specs",
    "e2e",
    "cypress",
];

/// File stems that conventionally mean "this is where the program starts".
const ENTRY_STEMS: &[&str] = &["index", "main", "cli", "app", "server", "entry"];

/// Classifies every plausible entry point, most trustworthy first.
///
/// `explicit` are paths the user named with `--entry`; when given they are the
/// entry set, and nothing is guessed.
pub fn classify(graph: &ModuleGraph, explicit: &[usize]) -> Vec<Entry> {
    if !explicit.is_empty() {
        let mut entries: Vec<Entry> = explicit
            .iter()
            .map(|&module| Entry {
                module,
                kind: EntryKind::Package,
            })
            .collect();
        entries.sort_by_key(|e| e.module);
        entries.dedup_by_key(|e| e.module);
        return entries;
    }

    let declared = declared_entries(graph);
    let mut entries: Vec<Entry> = Vec::new();
    let mut taken: BTreeSet<usize> = BTreeSet::new();

    for &module in &declared {
        if taken.insert(module) {
            entries.push(Entry {
                module,
                kind: EntryKind::Package,
            });
        }
    }

    for module in 0..graph.modules.len() {
        if taken.contains(&module) || !graph.importers[module].is_empty() {
            continue;
        }
        let path = graph.display_path(module);
        let kind = if is_test_path(&path) {
            EntryKind::Test
        } else if declared.is_empty() {
            // Nothing declares an entry point, so there is no basis for
            // calling one root more real than another. Treating them all as
            // program starts is what the project's own `node x.js` does.
            EntryKind::Package
        } else {
            EntryKind::Orphan
        };
        taken.insert(module);
        entries.push(Entry { module, kind });
    }

    entries.sort_by_key(|e| (e.kind, e.module));
    entries
}

/// Entry modules the project declares: `package.json` fields that resolve to
/// a module in the graph, plus conventional source entry paths.
fn declared_entries(graph: &ModuleGraph) -> Vec<usize> {
    let mut found: BTreeSet<usize> = BTreeSet::new();

    for relative in package_json_paths(&graph.root) {
        let candidate = normalize(&graph.root.join(relative.trim_start_matches("./")));
        if let Some(&module) = graph.index_by_path.get(&candidate) {
            found.insert(module);
        }
    }

    // A published `main` usually points at build output the walker never
    // sees, so the source-side convention carries most of the weight here.
    for module in 0..graph.modules.len() {
        if is_conventional_entry(&graph.display_path(module)) {
            found.insert(module);
        }
    }

    found.into_iter().collect()
}

/// String paths named by the fields of `package.json` that mean "load this":
/// `main`, `module`, `browser`, `bin`, and every leaf of `exports`.
fn package_json_paths(root: &Path) -> Vec<String> {
    let Ok(text) = fs::read_to_string(root.join("package.json")) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };
    let mut paths = Vec::new();
    for field in ["main", "module", "browser", "bin", "exports"] {
        if let Some(node) = value.get(field) {
            collect_paths(node, &mut paths, 0);
        }
    }
    paths
}

/// Walks a `package.json` value collecting every relative path string in it.
/// `exports` nests conditions arbitrarily deep; the depth cap keeps a hostile
/// manifest from recursing without bound.
fn collect_paths(node: &Value, out: &mut Vec<String>, depth: usize) {
    if depth > 8 {
        return;
    }
    match node {
        Value::String(text) => {
            if text.starts_with("./") {
                out.push(text.clone());
            }
        }
        Value::Object(map) => {
            for child in map.values() {
                collect_paths(child, out, depth + 1);
            }
        }
        Value::Array(items) => {
            for child in items {
                collect_paths(child, out, depth + 1);
            }
        }
        _ => {}
    }
}

/// `src/index.ts`, `packages/ui/src/main.tsx`, `cli.js` — a file whose name
/// and location both say "start here".
fn is_conventional_entry(display: &str) -> bool {
    let (directory, file) = match display.rsplit_once('/') {
        Some((directory, file)) => (directory, file),
        None => ("", display),
    };
    let stem = file.split('.').next().unwrap_or("");
    if !ENTRY_STEMS.contains(&stem) {
        return false;
    }
    // A barrel deep in the tree is also called `index`; only the project root
    // and a `src` directory carry the entry-point meaning.
    directory.is_empty() || directory == "src" || directory.ends_with("/src")
}

fn is_test_path(display: &str) -> bool {
    let file = display.rsplit('/').next().unwrap_or(display);
    let stem = file.split('.').next().unwrap_or("");
    if [".test.", ".spec.", ".bench.", "-test.", "_test."]
        .iter()
        .any(|marker| file.contains(marker))
    {
        return true;
    }
    if stem == "setupTests" || stem == "vitest" || stem == "jest" {
        return true;
    }
    display
        .split('/')
        .any(|segment| TEST_DIRS.contains(&segment))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conventional_entries_are_roots_and_src_only() {
        assert!(is_conventional_entry("src/index.ts"));
        assert!(is_conventional_entry("index.js"));
        assert!(is_conventional_entry("packages/ui/src/main.tsx"));
        assert!(is_conventional_entry("src/cli.ts"));
        // A barrel, not an entry point.
        assert!(!is_conventional_entry("src/components/index.ts"));
        assert!(!is_conventional_entry("src/helpers.ts"));
    }

    #[test]
    fn test_paths_are_recognised_by_name_and_by_directory() {
        assert!(is_test_path("src/user.test.ts"));
        assert!(is_test_path("src/user.spec.tsx"));
        assert!(is_test_path("tests/smoke.mjs"));
        assert!(is_test_path("src/__tests__/user.ts"));
        assert!(is_test_path("e2e/login.ts"));
        assert!(!is_test_path("src/latest.ts"));
        assert!(!is_test_path("src/protest/index.ts"));
    }

    #[test]
    fn package_json_paths_are_collected_from_nested_exports() {
        let value: Value = serde_json::from_str(
            r#"{"main":"./dist/index.js","exports":{".":{"import":"./src/index.ts"},
                "./sub":{"node":{"default":"./src/sub.ts"}}},"bin":{"cli":"./src/cli.ts"},
                "version":"1.0.0"}"#,
        )
        .unwrap();
        let mut paths = Vec::new();
        for field in ["main", "module", "browser", "bin", "exports"] {
            if let Some(node) = value.get(field) {
                collect_paths(node, &mut paths, 0);
            }
        }
        paths.sort();
        assert_eq!(
            paths,
            vec![
                "./dist/index.js",
                "./src/cli.ts",
                "./src/index.ts",
                "./src/sub.ts"
            ]
        );
    }
}
