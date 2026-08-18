//! Source-file discovery.
//!
//! Walks the project tree collecting files the parser understands, skipping
//! the directories that hold other people's code or build output. The skip
//! list is deliberate and short — overpull measures the graph of code the
//! project *evaluates from source*, and `node_modules` is counted at the
//! package boundary instead of being traversed.

use std::fs;
use std::path::{Path, PathBuf};

use crate::parse::is_source_file;

/// Directories never descended into.
const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "dist",
    "build",
    "out",
    "coverage",
    "target",
    "vendor",
    "__snapshots__",
];

pub fn discover_sources(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                // Dot-directories (.git, .next, .turbo, …) are tool state,
                // not source.
                if name.starts_with('.') || SKIP_DIRS.contains(&name) {
                    continue;
                }
                stack.push(path);
            } else if file_type.is_file() && is_source_file(&path) {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_node_modules_and_dot_dirs() {
        let dir = std::env::temp_dir().join("overpull-walk-test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::create_dir_all(dir.join("node_modules/pkg")).unwrap();
        fs::create_dir_all(dir.join(".git")).unwrap();
        fs::write(dir.join("src/a.ts"), "export {}").unwrap();
        fs::write(dir.join("src/a.d.ts"), "export {}").unwrap();
        fs::write(dir.join("node_modules/pkg/index.js"), "").unwrap();
        fs::write(dir.join(".git/config.js"), "").unwrap();

        let found = discover_sources(&dir);
        assert_eq!(found.len(), 1);
        assert!(found[0].ends_with("a.ts"));
        let _ = fs::remove_dir_all(&dir);
    }
}
