//! Specifier resolution on top of `oxc_resolver`.
//!
//! This is the same resolution engine oxlint and Rolldown use — package.json
//! `exports`/`imports` maps, tsconfig `paths`, extension probing — so what
//! overpull follows is what a bundler or Node actually loads, not a guess.

use std::path::{Path, PathBuf};

use oxc_resolver::{
    ResolveOptions, Resolver, TsconfigDiscovery, TsconfigOptions, TsconfigReferences,
};

/// Where one specifier leads.
pub enum Target {
    /// A file inside the project (or a linked workspace package) that we can
    /// parse and keep walking through.
    Local(PathBuf),
    /// A dependency under `node_modules`, counted at the package boundary.
    External(String),
    /// A Node/Bun/Deno builtin.
    Builtin,
    /// Resolution failed — a broken import or an alias only the bundler knows.
    Unresolved,
}

pub struct SpecResolver {
    resolver: Resolver,
}

impl SpecResolver {
    pub fn new(tsconfig: Option<&Path>) -> Self {
        let tsconfig = match tsconfig {
            Some(path) => TsconfigDiscovery::Manual(TsconfigOptions {
                config_file: path.to_path_buf(),
                references: TsconfigReferences::Auto,
            }),
            None => TsconfigDiscovery::Auto,
        };
        let options = ResolveOptions {
            tsconfig: Some(tsconfig),
            extensions: vec![
                ".ts".into(),
                ".tsx".into(),
                ".mts".into(),
                ".cts".into(),
                ".js".into(),
                ".jsx".into(),
                ".mjs".into(),
                ".cjs".into(),
                ".json".into(),
            ],
            // Match the TypeScript compiler: an import of "./x.js" may live
            // on disk as "./x.ts".
            extension_alias: vec![
                (
                    ".js".into(),
                    vec![".ts".into(), ".tsx".into(), ".js".into()],
                ),
                (".mjs".into(), vec![".mts".into(), ".mjs".into()]),
                (".cjs".into(), vec![".cts".into(), ".cjs".into()]),
            ],
            condition_names: vec![
                "types".into(),
                "import".into(),
                "node".into(),
                "default".into(),
            ],
            main_fields: vec!["module".into(), "main".into()],
            ..ResolveOptions::default()
        };
        Self {
            resolver: Resolver::new(options),
        }
    }

    pub fn resolve(&self, from_dir: &Path, specifier: &str) -> Target {
        if is_builtin(specifier) {
            return Target::Builtin;
        }
        match self.resolver.resolve(from_dir, specifier) {
            Ok(resolution) => {
                let path = resolution.full_path();
                if path.components().any(|c| c.as_os_str() == "node_modules") {
                    Target::External(package_name(specifier))
                } else {
                    Target::Local(path)
                }
            }
            Err(_) => {
                if specifier.starts_with('.') || specifier.starts_with('/') {
                    Target::Unresolved
                } else if looks_like_package(specifier) {
                    // A bare specifier that did not resolve is still a
                    // dependency — likely present in production installs even
                    // when absent here.
                    Target::External(package_name(specifier))
                } else {
                    Target::Unresolved
                }
            }
        }
    }
}

/// The npm package a bare specifier belongs to: `react/jsx-runtime` → react,
/// `@scope/pkg/sub` → @scope/pkg.
pub fn package_name(specifier: &str) -> String {
    let mut parts = specifier.split('/');
    let first = parts.next().unwrap_or(specifier);
    if first.starts_with('@') {
        if let Some(second) = parts.next() {
            return format!("{first}/{second}");
        }
    }
    first.to_string()
}

fn looks_like_package(specifier: &str) -> bool {
    !specifier.is_empty()
        && !specifier.starts_with('.')
        && !specifier.starts_with('/')
        && !specifier.starts_with('#')
}

/// Node builtin modules (bare and `node:`-prefixed) plus other runtime
/// namespaces. Sorted; checked with binary search.
const NODE_BUILTINS: &[&str] = &[
    "assert",
    "assert/strict",
    "async_hooks",
    "buffer",
    "child_process",
    "cluster",
    "console",
    "constants",
    "crypto",
    "dgram",
    "diagnostics_channel",
    "dns",
    "dns/promises",
    "domain",
    "events",
    "fs",
    "fs/promises",
    "http",
    "http2",
    "https",
    "inspector",
    "module",
    "net",
    "os",
    "path",
    "path/posix",
    "path/win32",
    "perf_hooks",
    "process",
    "punycode",
    "querystring",
    "readline",
    "readline/promises",
    "repl",
    "stream",
    "stream/consumers",
    "stream/promises",
    "stream/web",
    "string_decoder",
    "sys",
    "timers",
    "timers/promises",
    "tls",
    "trace_events",
    "tty",
    "url",
    "util",
    "util/types",
    "v8",
    "vm",
    "wasi",
    "worker_threads",
    "zlib",
];

pub fn is_builtin(specifier: &str) -> bool {
    if let Some(rest) = specifier.strip_prefix("node:") {
        // Every `node:` specifier is builtin by definition (`node:test`,
        // `node:sqlite`, … exist only under the prefix).
        return !rest.is_empty();
    }
    if specifier.starts_with("bun:") || specifier.starts_with("deno:") {
        return true;
    }
    NODE_BUILTINS.binary_search(&specifier).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_names() {
        assert_eq!(package_name("react"), "react");
        assert_eq!(package_name("react/jsx-runtime"), "react");
        assert_eq!(package_name("@scope/pkg"), "@scope/pkg");
        assert_eq!(package_name("@scope/pkg/deep/file"), "@scope/pkg");
    }

    #[test]
    fn builtins() {
        assert!(is_builtin("fs"));
        assert!(is_builtin("node:fs"));
        assert!(is_builtin("node:test"));
        assert!(is_builtin("fs/promises"));
        assert!(!is_builtin("react"));
        assert!(!is_builtin("node:"));
    }

    #[test]
    fn builtin_list_is_sorted() {
        let mut sorted = NODE_BUILTINS.to_vec();
        sorted.sort_unstable();
        assert_eq!(sorted, NODE_BUILTINS);
    }
}
