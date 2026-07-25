//! `.ts` module imports. A relative import (`import { x } from "./other"`)
//! resolves to a local `.ts` file, so `ds build` emits one Rust module per
//! dependency (the matching `mod` declarations and `use` aliases). A `cargo:`
//! import (`import { X } from "cargo:serde"`) names a Cargo crate added via
//! `ds add`: it is not a local file (so it is excluded from module assembly
//! below) but still lowers to `use serde::X` — see [`module_ident`]. A bare
//! specifier (`lodash`) has no resolver — `check` reports it.

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    BindingIdentifier, BindingPattern, Declaration, ExportSpecifier, Function,
    ImportDeclarationSpecifier, ModuleExportName, Statement,
};
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType, Span};
use syn::Ident;

use super::{bindings, semantic::SymbolKind};

/// A `.ts` import of a local module: the Rust module name (`other`) and the
/// original source string (`"./other"`).
#[derive(Debug, Clone)]
pub struct ImportRef {
    /// Snake-cased Rust module name, derived from the source's file stem.
    pub module: String,
    /// The verbatim import source (`"./other"`).
    pub source: String,
}

/// The local modules a `.ts` file imports, in source order. Used by `ds build`
/// to emit one `src/<module>.rs` per dependency.
pub(crate) fn collect_imports(source: &str) -> Vec<ImportRef> {
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, SourceType::ts()).parse();
    ret.program
        .body
        .iter()
        .filter_map(|stmt| {
            let Statement::ImportDeclaration(imp) = stmt else {
                return None;
            };
            // Only relative imports are local `.ts` files assembled into
            // `mod` decls — `cargo:` names a crate, a bare specifier is an
            // unsupported npm import.
            if !imp.source.value.starts_with('.') {
                return None;
            }
            let module = module_ident(&imp.source.value)?.to_string();
            Some(ImportRef {
                module,
                source: imp.source.value.to_string(),
            })
        })
        .collect()
}

/// The Rust module name for an import source. Three families, aligned with
/// Deno's `npm:`/`jsr:`/`node:` markers (`.temp/architecture-proposal.md`):
/// - `cargo:adler` → the crate's module ident (`adler`; `cfg-if` → `cfg_if` —
///   a `use` path may not contain a hyphen).
/// - `./other` → the local file stem (`other`).
/// - a bare specifier (`lodash`) → `None`: an npm import DashScript has no
///   resolver for — `check` reports it, the translator emits nothing.
pub(crate) fn module_ident(source: &str) -> Option<Ident> {
    if let Some(rest) = source.strip_prefix("cargo:") {
        Some(bindings::crate_mod(rest))
    } else if source.starts_with('.') {
        let stem = source.rsplit(['/', '\\']).next()?;
        let stem = stem.trim_end_matches(".ts");
        if stem.is_empty() || stem == "." || stem == ".." {
            return None;
        }
        Some(bindings::snake(stem))
    } else {
        None
    }
}

/// The local binding of a named or default import — `import { foo }` and
/// `import foo` — in the form the imported item has in its module: a binding
/// starting uppercase names a type (interface/type alias, kept PascalCase);
/// otherwise it names a value (function, snake_cased). A namespace import
/// (`import * as ns`) is excluded — it needs its own lowering, tracked
/// separately.
pub(crate) fn named_local(spec: &ImportDeclarationSpecifier) -> Option<Ident> {
    let local = match spec {
        ImportDeclarationSpecifier::ImportSpecifier(s) => &s.local,
        ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => &s.local,
        ImportDeclarationSpecifier::ImportNamespaceSpecifier(_) => return None,
    };
    Some(casing_ident(&local.name))
}

/// Type-vs-value casing for an import name: an uppercase-first name (an
/// interface/type alias) keeps PascalCase; otherwise snake_cased (a function or
/// value). The same rule applies to both the imported name (what the source
/// module exports) and the local binding — a Rust `use` of a type is a type, a
/// `use` of a value is a value.
fn casing_ident(name: &str) -> Ident {
    if name.chars().next().is_some_and(char::is_uppercase) {
        bindings::type_ident(name)
    } else {
        bindings::snake(name)
    }
}

/// One `use` tree for a named or default import — a bare `foo`, or
/// `foo as fooA` when the local binding renames the imported item. A namespace
/// import (`import * as ns`) returns `None` here: it has no in-group form and
/// is emitted as its own `use mod as ns;` item. The path segment is the
/// imported name (what the source module exports); the alias is the local
/// binding. When they match (no `as`), a bare name keeps the rendered output
/// brace-free (`use other::foo`, not `use other::{foo as foo}`).
pub(crate) fn named_use_tree(spec: &ImportDeclarationSpecifier) -> Option<syn::UseTree> {
    match spec {
        ImportDeclarationSpecifier::ImportNamespaceSpecifier(_) => None,
        ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
            // A default import has no separate imported name — the local binding
            // names the crate item directly, so a bare tree (path == alias).
            Some(use_tree_from(&s.local.name, &s.local.name))
        }
        ImportDeclarationSpecifier::ImportSpecifier(s) => {
            let imported = module_export_name_str(&s.imported);
            Some(use_tree_from(&imported, &s.local.name))
        }
    }
}

/// One `use` tree for a named export specifier — `export { foo }` (bare) or
/// `export { foo as bar }` (rename). The path segment is the `local` name (in
/// the source module, or the local binding when there is no `from`); the alias
/// is the `exported` name exposed to importers — the mirror of an import's
/// `imported` → `local` pair.
pub(crate) fn export_use_tree(spec: &ExportSpecifier) -> syn::UseTree {
    let local = module_export_name_str(&spec.local);
    let exported = module_export_name_str(&spec.exported);
    use_tree_from(&local, &exported)
}

/// The Rust alias ident for an `export * as <name>` namespace re-export — the
/// `name` a `pub use mod as <name>;` exposes, with the type-vs-value casing.
pub(crate) fn export_alias_ident(name: &ModuleExportName) -> Ident {
    casing_ident(&module_export_name_str(name))
}

/// A `ModuleExportName` as a plain string — the three oxc forms (an identifier
/// name, an identifier reference, or a string literal) all carry a `.name` /
/// `.value`. Shared by import and export specifier lowering.
fn module_export_name_str(name: &ModuleExportName) -> String {
    match name {
        ModuleExportName::IdentifierName(id) => id.name.to_string(),
        ModuleExportName::IdentifierReference(id) => id.name.to_string(),
        ModuleExportName::StringLiteral(s) => s.value.to_string(),
    }
}

/// A `use` tree from a (path, alias) name pair: a bare `name` when they match,
/// else `path as alias`. Shared by import (`imported` → `local`) and export
/// (`local` → `exported`) lowering — both names take the type-vs-value casing.
fn use_tree_from(path: &str, alias: &str) -> syn::UseTree {
    use syn::UseTree;
    let path_ident = casing_ident(path);
    let alias_ident = casing_ident(alias);
    if path_ident == alias_ident {
        UseTree::Name(syn::UseName { ident: alias_ident })
    } else {
        UseTree::Rename(syn::UseRename {
            ident: path_ident,
            as_token: Default::default(),
            rename: alias_ident,
        })
    }
}

/// The local alias of a namespace import (`import * as ns`) — snake_cased, the
/// name the body uses as a module-path prefix (`ns.foo` → `ns::foo`). `None`
/// when the specifiers hold no namespace import.
pub(crate) fn namespace_local(specs: &[ImportDeclarationSpecifier]) -> Option<Ident> {
    specs.iter().find_map(|spec| {
        if let ImportDeclarationSpecifier::ImportNamespaceSpecifier(ns) = spec {
            Some(bindings::snake(&ns.local.name))
        } else {
            None
        }
    })
}

/// One symbol brought in by a `cargo:` import (`import { X } from "cargo:crate"`),
/// in the form the translator emits in the Rust `use` clause, plus the byte
/// span of the local binding in the `.ts` source — so the language server can
/// map a cursor position onto the symbol.
#[derive(Debug, Clone)]
pub struct CrateImportSymbol {
    /// The symbol name as it appears in the emitted `use crate::NAME;`
    /// (PascalCase types kept; values snake_cased — same rule as `named_local`).
    pub name: String,
    /// The `.ts` byte span of the local binding, for cursor hit-testing.
    pub span: Span,
}

/// A `cargo:` import (`import { X } from "cargo:serde"`) — not a local `.ts`
/// file but a crate fetched via `ds add`. The module ident is hyphen-normalized
/// (`cfg-if` → `cfg_if`); each symbol name matches what the translator writes
/// in the `use` clause.
#[derive(Debug, Clone)]
pub struct CrateImport {
    /// The crate module ident (`serde`, `cfg_if`) used as the `use` path.
    pub module: String,
    /// The symbols imported from this crate, with their `.ts` byte spans.
    pub symbols: Vec<CrateImportSymbol>,
    /// The `.ts` byte span of the import source string (`"cargo:adler"`), for
    /// cursor hit-testing on the crate name (go-to-definition → crate root).
    pub source_span: Span,
}

/// The `cargo:` imports in a `.ts` file (`import { X } from "cargo:crate"`),
/// with each symbol's `.ts` byte span. Used by `ds lsp` to resolve a
/// go-to-definition request on an import specifier to the crate's source.
pub(crate) fn collect_crate_imports(source: &str) -> Vec<CrateImport> {
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, SourceType::ts()).parse();
    ret.program
        .body
        .iter()
        .filter_map(|stmt| {
            let Statement::ImportDeclaration(imp) = stmt else {
                return None;
            };
            // Only `cargo:` imports are crate imports — a bare specifier is an
            // unsupported npm import, a relative import is a local `.ts` module.
            imp.source.value.strip_prefix("cargo:")?;
            let module = module_ident(&imp.source.value)?.to_string();
            let symbols = imp
                .specifiers
                .as_ref()?
                .iter()
                .filter_map(|spec| {
                    let local = match spec {
                        ImportDeclarationSpecifier::ImportSpecifier(s) => &s.local,
                        ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => &s.local,
                        ImportDeclarationSpecifier::ImportNamespaceSpecifier(_) => return None,
                    };
                    let name = named_local(spec)?.to_string();
                    Some(CrateImportSymbol {
                        name,
                        span: local.span,
                    })
                })
                .collect();
            Some(CrateImport {
                module,
                symbols,
                source_span: imp.source.span,
            })
        })
        .collect()
}

/// A locally declarable name — `function`, `interface`, `type`, an `export`ed
/// form, or an `import` binding — with the byte span of its binding. Used by
/// `ds lsp` for in-file go-to-definition (the rust-analyzer backend handles
/// crate imports; this handles everything declared inside the `.ts` file).
#[derive(Debug, Clone)]
pub struct LocalSymbol {
    /// The bound name as written in `.ts` (e.g. `foo`, `Point`).
    pub name: String,
    /// The `.ts` byte span of the binding identifier.
    pub span: Span,
    /// What the symbol declares — drives the document-symbol icon and hover.
    pub kind: SymbolKind,
    /// A function's parameter list and return type (source slices), for
    /// signature help and hover. `None` for non-functions.
    pub signature: Option<Signature>,
    /// The full declaration span (`interface Point { … }`, `type Id = …`),
    /// for hover to show the complete type. `None` when the hover is a
    /// signature or header (functions, imports).
    pub decl_span: Option<Span>,
}

/// A function's signature as written in `.ts` — parameter names, their type
/// annotation (verbatim source slice, e.g. `number`, `string[]`), and the
/// return type. Powers LSP signature help and hover for user functions.
#[derive(Debug, Clone)]
pub struct Signature {
    pub params: Vec<ParamInfo>,
    pub return_type: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ParamInfo {
    pub name: String,
    pub type_text: Option<String>,
    pub optional: bool,
}

impl Signature {
    /// `(name: type, opt?: type): return` — the one-line signature used by
    /// document-symbol detail, hover, and signature-help labels. An untyped
    /// parameter renders as `any`; a missing return type renders as `void`.
    pub fn label(&self) -> String {
        let params: Vec<String> = self.params.iter().map(render_param).collect();
        let ret = self
            .return_type
            .clone()
            .unwrap_or_else(|| "void".to_string());
        format!("({}): {}", params.join(", "), ret)
    }
}

/// One parameter rendered as `name: type` (or `name?: type`, `name: any`).
fn render_param(p: &ParamInfo) -> String {
    let ty = p.type_text.clone().unwrap_or_else(|| "any".to_string());
    if p.optional {
        format!("{}?: {}", p.name, ty)
    } else {
        format!("{}: {}", p.name, ty)
    }
}

/// Whether the `.ts` source declares a top-level `function main()`.
///
/// Under pure-TS execution semantics, `function main` is an ordinary
/// declaration — it is renamed `__ds_main` and does **not** itself become the
/// cargo entry. The translator always emits an implicit `fn main` that collects
/// the file's top-level executable statements (empty for a declarations-only
/// file). This predicate therefore no longer decides whether a binary entry
/// exists; it only reports whether a binding literally named `main` was
/// declared, for callers that still want that signal. AST-level, so a
/// `main_loop` helper or a `"fn main"` string literal never trips a match.
pub(crate) fn has_main(source: &str) -> bool {
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, SourceType::ts()).parse();
    ret.program.body.iter().any(has_main_stmt)
}

/// One statement declares `function main` (bare, or `export function main`).
fn has_main_stmt(stmt: &Statement) -> bool {
    match stmt {
        Statement::FunctionDeclaration(f) => is_named_main(&f.id),
        Statement::ExportNamedDeclaration(exp) => matches!(
            &exp.declaration,
            Some(Declaration::FunctionDeclaration(f)) if is_named_main(&f.id)
        ),
        _ => false,
    }
}

fn is_named_main(id: &Option<BindingIdentifier>) -> bool {
    id.as_ref().is_some_and(|id| id.name.as_str() == "main")
}

/// Every declarable name in a `.ts` file with its binding span, kind, and (for
/// functions) signature. Used by `ds lsp` for in-file go-to-definition,
/// document symbols, hover, and signature help.
pub(crate) fn collect_declarations(source: &str) -> Vec<LocalSymbol> {
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, SourceType::ts()).parse();
    let mut out = Vec::new();
    for stmt in &ret.program.body {
        collect_from_statement(stmt, source, &mut out);
    }
    out
}

fn collect_from_statement(stmt: &Statement, source: &str, out: &mut Vec<LocalSymbol>) {
    match stmt {
        Statement::FunctionDeclaration(f) => extend_binding(
            &f.id,
            SymbolKind::Function,
            function_signature(f, source),
            out,
        ),
        Statement::TSInterfaceDeclaration(i) => {
            out.push(symbol_decl(&i.id, SymbolKind::Interface, i.span()))
        }
        Statement::TSTypeAliasDeclaration(t) => {
            out.push(symbol_decl(&t.id, SymbolKind::TypeAlias, t.span()))
        }
        Statement::ImportDeclaration(imp) => {
            if let Some(specs) = &imp.specifiers {
                for spec in specs {
                    let local = match spec {
                        ImportDeclarationSpecifier::ImportSpecifier(s) => Some(&s.local),
                        ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => Some(&s.local),
                        ImportDeclarationSpecifier::ImportNamespaceSpecifier(_) => None,
                    };
                    if let Some(local) = local {
                        out.push(LocalSymbol {
                            name: local.name.to_string(),
                            span: local.span,
                            kind: SymbolKind::Other,
                            signature: None,
                            decl_span: None,
                        });
                    }
                }
            }
        }
        Statement::ExportNamedDeclaration(exp) => {
            if let Some(decl) = &exp.declaration {
                collect_from_declaration(decl, source, out);
            }
        }
        _ => {}
    }
}

fn collect_from_declaration(decl: &Declaration, source: &str, out: &mut Vec<LocalSymbol>) {
    match decl {
        Declaration::FunctionDeclaration(f) => extend_binding(
            &f.id,
            SymbolKind::Function,
            function_signature(f, source),
            out,
        ),
        Declaration::TSInterfaceDeclaration(i) => {
            out.push(symbol_decl(&i.id, SymbolKind::Interface, i.span()))
        }
        Declaration::TSTypeAliasDeclaration(t) => {
            out.push(symbol_decl(&t.id, SymbolKind::TypeAlias, t.span()))
        }
        _ => {}
    }
}

fn extend_binding(
    id: &Option<BindingIdentifier>,
    kind: SymbolKind,
    signature: Option<Signature>,
    out: &mut Vec<LocalSymbol>,
) {
    if let Some(id) = id {
        out.push(symbol_with(id, kind, signature));
    }
}

fn symbol_with(
    id: &BindingIdentifier,
    kind: SymbolKind,
    signature: Option<Signature>,
) -> LocalSymbol {
    LocalSymbol {
        name: id.name.to_string(),
        span: id.span,
        kind,
        signature,
        decl_span: None,
    }
}

/// A symbol with a full declaration span — interface/type aliases, so hover
/// can show the complete definition (`interface Point { x: number }`).
fn symbol_decl(id: &BindingIdentifier, kind: SymbolKind, decl_span: Span) -> LocalSymbol {
    LocalSymbol {
        name: id.name.to_string(),
        span: id.span,
        kind,
        signature: None,
        decl_span: Some(decl_span),
    }
}

/// A function's signature from its AST: parameter names, their type annotation
/// (verbatim source slice, e.g. `number`, `string[]`), and the return type.
/// Destructuring parameters (`{ x }`) show as `_`. Slices the source by the
/// type's span so the text matches what the developer wrote.
fn function_signature(f: &Function, source: &str) -> Option<Signature> {
    let params = f
        .params
        .items
        .iter()
        .map(|fp| {
            let name = match &fp.pattern {
                BindingPattern::BindingIdentifier(id) => id.name.to_string(),
                _ => "_".to_string(),
            };
            ParamInfo {
                name,
                type_text: fp
                    .type_annotation
                    .as_ref()
                    .map(|ta| source[ta.type_annotation.span()].to_string()),
                optional: fp.optional,
            }
        })
        .collect();
    Some(Signature {
        params,
        return_type: f
            .return_type
            .as_ref()
            .map(|ta| source[ta.type_annotation.span()].to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn function_signature_with_params_and_return() {
        let src = "function greet(name: string, times?: number): string { return name; }";
        let decls = collect_declarations(src);
        let greet = decls.iter().find(|d| d.name == "greet").expect("greet");
        assert_eq!(greet.kind, SymbolKind::Function);
        let sig = greet.signature.as_ref().expect("signature");
        assert_eq!(sig.params.len(), 2);
        assert_eq!(sig.params[0].name, "name");
        assert_eq!(sig.params[0].type_text.as_deref(), Some("string"));
        assert_eq!(sig.params[1].name, "times");
        assert!(sig.params[1].optional, "times is optional");
        assert_eq!(sig.return_type.as_deref(), Some("string"));
    }

    #[test]
    fn interface_and_type_alias_kinds_no_signature() {
        let src = "interface Point { x: number } type Id = number;";
        let decls = collect_declarations(src);
        let p = decls.iter().find(|d| d.name == "Point").expect("Point");
        assert_eq!(p.kind, SymbolKind::Interface);
        assert!(p.signature.is_none());
        let id = decls.iter().find(|d| d.name == "Id").expect("Id");
        assert_eq!(id.kind, SymbolKind::TypeAlias);
        assert!(id.signature.is_none());
    }

    #[test]
    fn import_binding_is_other() {
        let src = "import { foo } from \"./other\";";
        let decls = collect_declarations(src);
        let foo = decls.iter().find(|d| d.name == "foo").expect("foo");
        assert_eq!(foo.kind, SymbolKind::Other);
        assert!(foo.signature.is_none());
    }

    #[test]
    fn function_without_return_type() {
        let src = "function f(x: number) { return x; }";
        let decls = collect_declarations(src);
        let f = decls.iter().find(|d| d.name == "f").expect("f");
        let sig = f.signature.as_ref().expect("sig");
        assert!(sig.return_type.is_none());
        assert_eq!(sig.params[0].type_text.as_deref(), Some("number"));
    }

    #[test]
    fn signature_label_renders_params_and_return() {
        let src = "function greet(name: string, times?: number): string { return name; }";
        let decls = collect_declarations(src);
        let greet = decls.iter().find(|d| d.name == "greet").expect("greet");
        let sig = greet.signature.as_ref().expect("sig");
        assert_eq!(sig.label(), "(name: string, times?: number): string");
    }

    #[test]
    fn signature_label_void_when_no_return() {
        let src = "function f() {}";
        let decls = collect_declarations(src);
        let f = decls.iter().find(|d| d.name == "f").expect("f");
        assert_eq!(f.signature.as_ref().expect("sig").label(), "(): void");
    }
}
