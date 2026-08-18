//! Turns one source file into [`FileFacts`] using the oxc parser and
//! semantic analyzer.
//!
//! This is the only module that touches an AST. Everything the commands need
//! — edges, bindings, evaluation-time usage, exported declaration kinds — is
//! extracted here into plain data.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use oxc_allocator::Allocator;
use oxc_ast::AstKind;
use oxc_ast::ast::{Argument, ExportDefaultDeclarationKind, Expression, TSModuleReference};
use oxc_parser::Parser;
use oxc_semantic::AstNode;
use oxc_semantic::{Semantic, SemanticBuilder};
use oxc_span::{GetSpan, SourceType, Span};
use oxc_syntax::module_record::{
    ExportExportName, ExportImportName, ExportLocalName, ImportImportName, ModuleRecord,
};
use oxc_syntax::symbol::SymbolFlags;

use crate::model::{
    BindingFact, DeclKind, DynamicImportFact, FileFacts, ImportEdgeFact, ImportedName,
    NamedReExport, NamespaceReads, ReExportSource, RequireFact, StarReExport, Usage,
};
use crate::util::LineIndex;

/// File extensions overpull understands.
pub const SOURCE_EXTENSIONS: &[&str] = &["ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs"];

pub fn is_source_file(path: &Path) -> bool {
    // Declaration files describe types only; they never load at runtime.
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if name.ends_with(".d.ts") || name.ends_with(".d.mts") || name.ends_with(".d.cts") {
        return false;
    }
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| SOURCE_EXTENSIONS.contains(&ext))
}

pub fn parse_file(path: &Path, source: &str) -> FileFacts {
    let allocator = Allocator::default();
    let source_type = SourceType::from_path(path).unwrap_or_default();
    let ret = Parser::new(&allocator, source, source_type).parse();
    let parse_ok = !ret.panicked && ret.diagnostics.is_empty();

    // `with_build_nodes` is opt-in as of oxc 0.144. Without it the AST node
    // store is empty, which is not an error — usage classification and
    // require collection would silently find nothing.
    let semantic_ret = SemanticBuilder::new()
        .with_build_nodes(true)
        .build(&ret.program);
    let semantic = semantic_ret.semantic;
    let lines = LineIndex::new(source);

    let module_record = &ret.module_record;
    let imports = collect_import_edges(module_record, &semantic, &lines);
    let (named_reexports, star_reexports, local_value_export_count) =
        collect_reexports(module_record, &lines);
    let mut export_decl_kinds = collect_export_decl_kinds(module_record, &semantic);
    let dynamic_imports = collect_dynamic_imports(module_record, source, &lines);
    let requires = collect_requires(&semantic, source_type, &mut export_decl_kinds, &lines);

    FileFacts {
        path: path.to_path_buf(),
        bytes: source.len() as u64,
        lines: lines.line(
            u32::try_from(source.len())
                .unwrap_or(u32::MAX)
                .saturating_sub(1),
        ),
        parse_ok,
        imports,
        dynamic_imports,
        requires,
        named_reexports,
        star_reexports,
        local_value_export_count,
        export_decl_kinds,
    }
}

/// Per-specifier accumulator: one import edge can come from several
/// statements, and each contributes bindings or occurrence flags.
struct Accum {
    line: u32,
    side_effect: bool,
    has_type_occurrence: bool,
    bindings: Vec<BindingFact>,
}

fn collect_import_edges(
    record: &ModuleRecord,
    semantic: &Semantic,
    lines: &LineIndex,
) -> Vec<ImportEdgeFact> {
    // Statement spans that carry at least one import binding, per specifier —
    // an `import "x"` occurrence is side-effect-only when its statement has no
    // entry here.
    let mut statements_with_entries: HashSet<(String, Span)> = HashSet::new();
    for entry in &record.import_entries {
        statements_with_entries
            .insert((entry.module_request.name.to_string(), entry.statement_span));
    }

    let mut runtime_reexport_specs: HashSet<String> = HashSet::new();
    for entry in record
        .indirect_export_entries
        .iter()
        .chain(&record.star_export_entries)
    {
        if let Some(request) = &entry.module_request
            && !entry.is_type
        {
            runtime_reexport_specs.insert(request.name.to_string());
        }
    }

    let mut by_spec: BTreeMap<String, Accum> = BTreeMap::new();

    for (specifier, occurrences) in &record.requested_modules {
        let spec = specifier.to_string();
        for occ in occurrences {
            let line = lines.line(occ.span.start);
            let entry = by_spec.entry(spec.clone()).or_insert(Accum {
                line,
                side_effect: false,
                has_type_occurrence: false,
                bindings: Vec::new(),
            });
            entry.line = entry.line.min(line);
            if occ.is_type {
                entry.has_type_occurrence = true;
            } else if occ.is_import
                && !statements_with_entries.contains(&(spec.clone(), occ.statement_span))
            {
                entry.side_effect = true;
            }
        }
    }

    for entry in &record.import_entries {
        if entry.is_type {
            continue;
        }
        let spec = entry.module_request.name.to_string();
        let imported = match &entry.import_name {
            ImportImportName::Name(n) => ImportedName::Named(n.name.to_string()),
            ImportImportName::Default(_) => ImportedName::Default,
            ImportImportName::NamespaceObject => ImportedName::Namespace,
        };
        let local = entry.local_name.name.to_string();
        let is_namespace = imported == ImportedName::Namespace;
        let facts = classify_usage(semantic, &local, entry.local_name.span, is_namespace, lines);
        let accum = by_spec.entry(spec).or_insert(Accum {
            line: lines.line(entry.statement_span.start),
            side_effect: false,
            has_type_occurrence: false,
            bindings: Vec::new(),
        });
        accum.bindings.push(BindingFact {
            local,
            imported,
            line: lines.line(entry.local_name.span.start),
            usage: facts.usage,
            namespace_reads: facts.namespace_reads,
        });
    }

    let mut edges: Vec<ImportEdgeFact> = by_spec
        .into_iter()
        .map(|(specifier, accum)| {
            let runtime_reexport = runtime_reexport_specs.contains(&specifier);
            let has_runtime = !accum.bindings.is_empty() || accum.side_effect || runtime_reexport;
            ImportEdgeFact {
                specifier,
                line: accum.line,
                type_only: !has_runtime,
                side_effect_only: accum.side_effect && accum.bindings.is_empty(),
                bindings: accum.bindings,
            }
        })
        .collect();
    edges.sort_by_key(|e| e.line);
    edges
}

/// What one imported binding's references say about evaluation time.
struct UsageFacts {
    usage: Usage,
    namespace_reads: NamespaceReads,
}

/// Classifies how an imported binding is used relative to module evaluation:
/// a reference inside a function body waits until the function is called; a
/// reference in a top-level statement, an `extends` clause, a decorator, a
/// static class member, or an immediately-invoked function expression runs
/// while the module itself evaluates.
///
/// For a namespace binding it also records which members are touched. Holding
/// a namespace object is always safe — the object exists from instantiation —
/// so only a member read can throw, and only when that member is in the
/// temporal dead zone.
fn classify_usage(
    semantic: &Semantic,
    local_name: &str,
    binding_span: Span,
    is_namespace: bool,
    lines: &LineIndex,
) -> UsageFacts {
    let scoping = semantic.scoping();
    let Some(symbol_id) = scoping.symbol_ids().find(|id| {
        scoping.symbol_span(*id) == binding_span && scoping.symbol_name(*id) == local_name
    }) else {
        return UsageFacts {
            usage: Usage::Unused,
            namespace_reads: NamespaceReads::default(),
        };
    };

    let mut saw_type = false;
    let mut saw_deferred = false;
    let mut immediate: Option<(u32, bool)> = None;
    let mut namespace_reads = NamespaceReads::default();

    for &reference_id in scoping.get_resolved_reference_ids(symbol_id) {
        let reference = scoping.get_reference(reference_id);
        if reference.is_type() {
            saw_type = true;
            continue;
        }
        let node_id = reference.node_id();
        let ref_span = semantic.nodes().get_node(node_id).kind().span();
        let ancestors: Vec<&AstNode> = semantic.nodes().ancestors(node_id).collect();
        let (deferred, in_extends) = evaluation_context(&ancestors, ref_span);
        if deferred {
            saw_deferred = true;
            continue;
        }
        if is_namespace {
            record_namespace_read(&ancestors, ref_span, &mut namespace_reads);
        }
        let line = lines.line(ref_span.start);
        match immediate {
            // Prefer reporting an `extends` read: it is the clearest crash.
            Some((_, true)) => {}
            Some(_) if in_extends => immediate = Some((line, true)),
            Some(_) => {}
            None => immediate = Some((line, in_extends)),
        }
    }

    let usage = if let Some((line, in_extends)) = immediate {
        Usage::Immediate { line, in_extends }
    } else if saw_deferred {
        Usage::Deferred
    } else if saw_type {
        Usage::TypeOnly
    } else {
        Usage::Unused
    };
    UsageFacts {
        usage,
        namespace_reads,
    }
}

/// Walks the ancestor chain of one reference: does it run while the module
/// evaluates, and does it sit in a class `extends` clause?
fn evaluation_context(ancestors: &[&AstNode], ref_span: Span) -> (bool, bool) {
    let mut in_extends = false;
    let mut i = 0usize;
    while i < ancestors.len() {
        match ancestors[i].kind() {
            AstKind::Function(function) => {
                match immediately_invoked(
                    ancestors,
                    i,
                    function.span,
                    function.r#async || function.generator,
                ) {
                    // An IIFE runs where it is written, so the walk continues
                    // from the call site rather than stopping here.
                    Some(next) => {
                        i = next;
                        continue;
                    }
                    None => return (true, in_extends),
                }
            }
            AstKind::ArrowFunctionExpression(arrow) => {
                match immediately_invoked(ancestors, i, arrow.span, arrow.r#async) {
                    Some(next) => {
                        i = next;
                        continue;
                    }
                    None => return (true, in_extends),
                }
            }
            AstKind::PropertyDefinition(prop) if !prop.r#static => {
                // An instance-property initializer runs at construction, not
                // at class evaluation. The key side still evaluates
                // immediately (computed keys), so only the value defers.
                if let Some(value) = &prop.value
                    && covers(value.span(), ref_span)
                {
                    return (true, in_extends);
                }
            }
            AstKind::Class(class) => {
                if let Some(heritage) = &class.heritage
                    && covers(heritage.expression.span(), ref_span)
                {
                    in_extends = true;
                }
            }
            _ => {}
        }
        i += 1;
    }
    (false, in_extends)
}

/// If the function at `index` is called where it stands — `(f)()`,
/// `(f).call(this)`, `new (f)()` — returns the ancestor index of the call so
/// the walk continues from the call site.
///
/// Async and generator functions stay deferred: only the part before the
/// first `await`/`yield` runs synchronously, and overpull does not report
/// what it cannot prove.
fn immediately_invoked(
    ancestors: &[&AstNode],
    index: usize,
    fn_span: Span,
    suspends: bool,
) -> Option<usize> {
    if suspends {
        return None;
    }
    let call = skip_parens(ancestors, index + 1)?;
    match ancestors.get(call)?.kind() {
        AstKind::CallExpression(expr) if covers(expr.callee.span(), fn_span) => Some(call),
        AstKind::NewExpression(expr) if covers(expr.callee.span(), fn_span) => Some(call),
        // `(function () { … }).call(this)` — the member expression is the
        // callee of the call one level further up.
        AstKind::StaticMemberExpression(member)
            if covers(member.object.span(), fn_span)
                && matches!(member.property.name.as_str(), "call" | "apply") =>
        {
            let outer = skip_parens(ancestors, call + 1)?;
            match ancestors.get(outer)?.kind() {
                AstKind::CallExpression(expr) if covers(expr.callee.span(), member.span) => {
                    Some(outer)
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// First ancestor index at or after `from` that is not a parenthesis node.
/// The parser preserves parentheses, and every IIFE has them.
fn skip_parens(ancestors: &[&AstNode], from: usize) -> Option<usize> {
    let mut i = from;
    while matches!(
        ancestors.get(i)?.kind(),
        AstKind::ParenthesizedExpression(_)
    ) {
        i += 1;
    }
    Some(i)
}

fn covers(outer: Span, inner: Span) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}

/// Records what one evaluation-time reference to a namespace binding reads:
/// a named member, or the whole object.
fn record_namespace_read(ancestors: &[&AstNode], ref_span: Span, reads: &mut NamespaceReads) {
    match ancestors.first().map(|node| node.kind()) {
        Some(AstKind::StaticMemberExpression(member)) if covers(member.object.span(), ref_span) => {
            reads.members.push(member.property.name.to_string());
        }
        // A computed key, a spread, a call argument, a `console.log`: any of
        // these can observe every export at once.
        _ => reads.whole_object = true,
    }
}

fn collect_reexports(
    record: &ModuleRecord,
    lines: &LineIndex,
) -> (Vec<NamedReExport>, Vec<StarReExport>, usize) {
    let mut named = Vec::new();
    for entry in &record.indirect_export_entries {
        let Some(request) = &entry.module_request else {
            continue;
        };
        let export_name = match &entry.export_name {
            ExportExportName::Name(n) => n.name.to_string(),
            ExportExportName::Default(_) => "default".to_string(),
            ExportExportName::Null => continue,
        };
        let source = match &entry.import_name {
            ExportImportName::Name(n) if n.name == "default" => ReExportSource::Default,
            ExportImportName::Name(n) => ReExportSource::Named(n.name.to_string()),
            ExportImportName::All => ReExportSource::Namespace,
            ExportImportName::AllButDefault | ExportImportName::Null => continue,
        };
        named.push(NamedReExport {
            export_name,
            source,
            specifier: request.name.to_string(),
            is_type: entry.is_type,
            line: lines.line(entry.span.start),
        });
    }
    named.sort_by(|a, b| {
        a.line
            .cmp(&b.line)
            .then_with(|| a.export_name.cmp(&b.export_name))
    });

    let mut stars = Vec::new();
    for entry in &record.star_export_entries {
        let Some(request) = &entry.module_request else {
            continue;
        };
        stars.push(StarReExport {
            specifier: request.name.to_string(),
            is_type: entry.is_type,
            line: lines.line(entry.span.start),
        });
    }
    stars.sort_by_key(|s| s.line);

    let local_value_export_count = record
        .local_export_entries
        .iter()
        .filter(|e| !e.is_type)
        .count();
    (named, stars, local_value_export_count)
}

fn collect_export_decl_kinds(
    record: &ModuleRecord,
    semantic: &Semantic,
) -> BTreeMap<String, DeclKind> {
    let scoping = semantic.scoping();
    let mut kinds = BTreeMap::new();
    for entry in &record.local_export_entries {
        if entry.is_type {
            continue;
        }
        let export_name = match &entry.export_name {
            ExportExportName::Name(n) => n.name.to_string(),
            ExportExportName::Default(_) => "default".to_string(),
            ExportExportName::Null => continue,
        };
        let kind = match &entry.local_name {
            ExportLocalName::Name(local) | ExportLocalName::Default(local) => scoping
                .symbol_ids()
                .find(|id| {
                    scoping.symbol_span(*id) == local.span
                        && scoping.symbol_name(*id) == local.name.as_str()
                })
                .map_or(DeclKind::Unknown, |id| decl_kind(scoping.symbol_flags(id))),
            // `export default <expression>` — initialized when the module
            // body runs, so it behaves like a `const` for early access.
            ExportLocalName::Null => DeclKind::ConstLet,
        };
        kinds.insert(export_name, kind);
    }
    kinds
}

fn decl_kind(flags: SymbolFlags) -> DeclKind {
    if flags.contains(SymbolFlags::Function) {
        DeclKind::HoistedFunction
    } else if flags.contains(SymbolFlags::Class) {
        DeclKind::Class
    } else if flags.intersects(SymbolFlags::RegularEnum | SymbolFlags::ConstEnum) {
        DeclKind::VarLike
    } else if flags.contains(SymbolFlags::ConstVariable)
        || flags.contains(SymbolFlags::BlockScopedVariable)
    {
        DeclKind::ConstLet
    } else if flags.contains(SymbolFlags::FunctionScopedVariable) {
        DeclKind::VarLike
    } else if flags.intersects(SymbolFlags::TypeAlias | SymbolFlags::Interface) {
        DeclKind::TypeOnly
    } else {
        DeclKind::Unknown
    }
}

fn collect_dynamic_imports(
    record: &ModuleRecord,
    source: &str,
    lines: &LineIndex,
) -> Vec<DynamicImportFact> {
    record
        .dynamic_imports
        .iter()
        .map(|dynamic| {
            let text = source
                .get(dynamic.module_request.start as usize..dynamic.module_request.end as usize)
                .unwrap_or("");
            DynamicImportFact {
                line: lines.line(dynamic.span.start),
                specifier: literal_specifier(text),
            }
        })
        .collect()
}

/// Extracts the string from a plain literal specifier expression: `"./x"`,
/// `'./x'`, or a template literal with no substitutions. Anything else is a
/// computed specifier that static analysis cannot follow.
fn literal_specifier(text: &str) -> Option<String> {
    let text = text.trim();
    let bytes = text.as_bytes();
    if bytes.len() < 2 {
        return None;
    }
    let quote = bytes[0];
    if (quote == b'"' || quote == b'\'' || quote == b'`') && bytes[bytes.len() - 1] == quote {
        let inner = &text[1..text.len() - 1];
        if inner.contains(char::from(quote)) || inner.contains('\\') {
            return None;
        }
        if quote == b'`' && inner.contains("${") {
            return None;
        }
        return Some(inner.to_string());
    }
    None
}

fn collect_requires(
    semantic: &Semantic,
    source_type: SourceType,
    export_decl_kinds: &mut BTreeMap<String, DeclKind>,
    lines: &LineIndex,
) -> Vec<RequireFact> {
    let mut requires = Vec::new();
    for node in semantic.nodes() {
        match node.kind() {
            AstKind::CallExpression(call) => {
                let Expression::Identifier(ident) = &call.callee else {
                    continue;
                };
                if ident.name != "require" || call.arguments.len() != 1 {
                    continue;
                }
                // Only a global `require` counts; a local binding named
                // `require` is someone else's function.
                let reference = semantic.scoping().get_reference(ident.reference_id());
                if reference.symbol_id().is_some() {
                    continue;
                }
                if let Some(Expression::StringLiteral(lit)) =
                    call.arguments.first().and_then(Argument::as_expression)
                {
                    requires.push(RequireFact {
                        specifier: lit.value.to_string(),
                        line: lines.line(call.span.start),
                    });
                }
            }
            AstKind::TSImportEqualsDeclaration(decl) => {
                if decl.import_kind.is_type() {
                    continue;
                }
                if let TSModuleReference::ExternalModuleReference(external) = &decl.module_reference
                {
                    requires.push(RequireFact {
                        specifier: external.expression.value.to_string(),
                        line: lines.line(decl.span.start),
                    });
                }
            }
            AstKind::ExportDefaultDeclaration(decl) => {
                // An anonymous `export default function () {}` is still a
                // hoisted function declaration; only expression defaults are
                // TDZ-prone. Overrides the ConstLet fallback recorded from
                // the module record.
                let kind = match &decl.declaration {
                    ExportDefaultDeclarationKind::FunctionDeclaration(_) => {
                        DeclKind::HoistedFunction
                    }
                    ExportDefaultDeclarationKind::ClassDeclaration(_) => DeclKind::Class,
                    _ => continue,
                };
                export_decl_kinds.insert("default".to_string(), kind);
            }
            _ => {}
        }
    }
    // `module.exports` shapes are not modeled; requires are collected for the
    // load graph only, which is all CommonJS interop needs here.
    let _ = source_type;
    requires.sort_by_key(|r| r.line);
    requires
}
