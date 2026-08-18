//! The per-file fact model every command consumes.
//!
//! `FileFacts` is deliberately plain data: the parser produces it, the graph
//! and analyses read it. Nothing downstream touches an AST.

use std::collections::BTreeMap;
use std::path::PathBuf;

/// Everything overpull knows about one source file after parsing.
pub struct FileFacts {
    pub path: PathBuf,
    pub bytes: u64,
    pub lines: u32,
    /// False when the parser reported syntax errors. The file still
    /// participates in the graph with whatever was recovered, but reports
    /// flag it so a truncated parse is never mistaken for a clean one.
    pub parse_ok: bool,
    /// Static `import`/`export ... from` edges, one per distinct specifier.
    pub imports: Vec<ImportEdgeFact>,
    /// `import(...)` expressions.
    pub dynamic_imports: Vec<DynamicImportFact>,
    /// `require("...")` calls and `import x = require("...")`.
    pub requires: Vec<RequireFact>,
    /// `export { a as b } from "mod"` / `export * as ns from "mod"`.
    pub named_reexports: Vec<NamedReExport>,
    /// `export * from "mod"`.
    pub star_reexports: Vec<StarReExport>,
    /// Number of runtime (non-type) exports declared locally in this file.
    pub local_value_export_count: usize,
    /// Declaration kind behind each locally exported name, for cycle-hazard
    /// classification: importing a hoisted function before its module runs is
    /// safe; importing a `const` is a `ReferenceError`.
    pub export_decl_kinds: BTreeMap<String, DeclKind>,
}

pub struct ImportEdgeFact {
    pub specifier: String,
    pub line: u32,
    /// Every binding and every occurrence of this specifier is type-only, so
    /// the edge vanishes at runtime.
    pub type_only: bool,
    /// `import "mod"` with no bindings anywhere for this specifier.
    pub side_effect_only: bool,
    /// Value bindings this file imports from the specifier.
    pub bindings: Vec<BindingFact>,
}

pub struct BindingFact {
    pub local: String,
    pub imported: ImportedName,
    pub line: u32,
    pub usage: Usage,
}

#[derive(Clone, PartialEq, Eq)]
pub enum ImportedName {
    Named(String),
    Default,
    Namespace,
}

impl ImportedName {
    pub fn display(&self) -> String {
        match self {
            Self::Named(n) => n.clone(),
            Self::Default => "default".to_string(),
            Self::Namespace => "*".to_string(),
        }
    }
}

/// How an imported binding is used relative to module evaluation time.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Usage {
    /// Never referenced (or only re-exported).
    Unused,
    /// Referenced only in type positions.
    TypeOnly,
    /// Referenced only inside function bodies or instance-property
    /// initializers — nothing runs while the module itself evaluates.
    Deferred,
    /// Read while the module evaluates: top-level statement, initializer,
    /// `extends` clause, decorator, or static class member.
    Immediate {
        line: u32,
        /// The read is a class `extends` clause — the classic cycle crash.
        in_extends: bool,
    },
}

pub struct DynamicImportFact {
    pub line: u32,
    /// `Some` when the argument is a plain string literal; `None` when the
    /// specifier is computed and cannot be followed statically.
    pub specifier: Option<String>,
}

pub struct RequireFact {
    pub specifier: String,
    pub line: u32,
}

pub struct NamedReExport {
    /// Name this file exports (`b` in `export { a as b } from "m"`), or the
    /// namespace name for `export * as ns from "m"`.
    pub export_name: String,
    pub source: ReExportSource,
    pub specifier: String,
    pub is_type: bool,
    pub line: u32,
}

#[derive(Clone, PartialEq, Eq)]
pub enum ReExportSource {
    Named(String),
    Default,
    /// `export * as ns from "m"` — the whole namespace object.
    Namespace,
}

pub struct StarReExport {
    pub specifier: String,
    pub is_type: bool,
    pub line: u32,
}

/// What kind of declaration stands behind an exported name.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeclKind {
    /// `function f() {}` — hoisted and callable before the module body runs.
    HoistedFunction,
    /// `class C {}` — in the temporal dead zone until the module runs.
    Class,
    /// `const` / `let` — in the temporal dead zone until the module runs.
    ConstLet,
    /// `var` or `enum` — reads as `undefined` before the module runs.
    VarLike,
    /// Interface / type alias — erased at runtime, always safe.
    TypeOnly,
    Unknown,
}

impl DeclKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::HoistedFunction => "function",
            Self::Class => "class",
            Self::ConstLet => "const/let",
            Self::VarLike => "var/enum",
            Self::TypeOnly => "type",
            Self::Unknown => "value",
        }
    }
}
