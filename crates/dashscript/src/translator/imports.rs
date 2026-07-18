//! `.ds` module imports. A relative import (`import { x } from "./other"`)
//! resolves to a local `.ds` file, so `ds build` emits one Rust module per
//! dependency (the matching `mod` declarations and `use` aliases). A *bare*
//! specifier (`import { X } from "serde"`) is a crate added via `ds add`: it is
//! not a local file (so it is excluded from module assembly below) but still
//! lowers to `use serde::X` — see [`module_ident`].

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    BindingIdentifier, BindingPattern, Declaration, Function, ImportDeclarationSpecifier, Statement,
};
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType, Span};
use syn::Ident;

use super::{bindings, semantic::SymbolKind};

/// A `.ds` import of a local module: the Rust module name (`other`) and the
/// original source string (`"./other"`).
#[derive(Debug, Clone)]
pub struct ImportRef {
    /// Snake-cased Rust module name, derived from the source's file stem.
    pub module: String,
    /// The verbatim import source (`"./other"`).
    pub source: String,
}

/// The local modules a `.ds` file imports, in source order. Used by `ds build`
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
            // A bare specifier is a crate (provided by cargo via `ds add`), not
            // a local `.ds` file — only relative imports are assembled into
            // `mod` decls.
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

/// The Rust module name for an import source. A relative path (`./other`) maps
/// to the local file stem (`other`); a bare specifier (`serde`, `cfg-if`) maps
/// to the crate's module ident (`serde`, `cfg_if` — hyphens become underscores,
/// since a `use` path may not contain `-`).
pub(crate) fn module_ident(source: &str) -> Option<Ident> {
    if source.starts_with('.') {
        let stem = source.rsplit(['/', '\\']).next()?;
        let stem = stem.trim_end_matches(".ds").trim_end_matches(".ts");
        if stem.is_empty() || stem == "." || stem == ".." {
            return None;
        }
        Some(bindings::snake(stem))
    } else {
        // Bare specifier: a crate, fetched by `ds add` and resolved by cargo.
        Some(bindings::crate_mod(source))
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
    let name: &str = &local.name;
    if name.chars().next().is_some_and(char::is_uppercase) {
        Some(bindings::type_ident(name))
    } else {
        Some(bindings::ident_of(local))
    }
}

/// One symbol brought in by a bare-crate import (`import { X } from "crate"`),
/// in the form the translator emits in the Rust `use` clause, plus the byte
/// span of the local binding in the `.ds` source — so the language server can
/// map a cursor position onto the symbol.
#[derive(Debug, Clone)]
pub struct CrateImportSymbol {
    /// The symbol name as it appears in the emitted `use crate::NAME;`
    /// (PascalCase types kept; values snake_cased — same rule as `named_local`).
    pub name: String,
    /// The `.ds` byte span of the local binding, for cursor hit-testing.
    pub span: Span,
}

/// A bare-crate import (`import { X } from "serde"`) — not a local `.ds` file
/// but a crate fetched via `ds add`. The module ident is hyphen-normalized
/// (`cfg-if` → `cfg_if`); each symbol name matches what the translator writes
/// in the `use` clause.
#[derive(Debug, Clone)]
pub struct CrateImport {
    /// The crate module ident (`serde`, `cfg_if`) used as the `use` path.
    pub module: String,
    /// The symbols imported from this crate, with their `.ds` byte spans.
    pub symbols: Vec<CrateImportSymbol>,
    /// The `.ds` byte span of the import source string (`"adler"`), for
    /// cursor hit-testing on the crate name (go-to-definition → crate root).
    pub source_span: Span,
}

/// The bare-crate imports in a `.ds` file (`import { X } from "crate"`), with
/// each symbol's `.ds` byte span. Used by `ds lsp` to resolve a
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
            // Relative imports are local modules, not crates.
            if imp.source.value.starts_with('.') {
                return None;
            }
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
/// crate imports; this handles everything declared inside the `.ds` file).
#[derive(Debug, Clone)]
pub struct LocalSymbol {
    /// The bound name as written in `.ds` (e.g. `foo`, `Point`).
    pub name: String,
    /// The `.ds` byte span of the binding identifier.
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

/// A function's signature as written in `.ds` — parameter names, their type
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

/// Whether the `.ds` source declares a top-level `function main()` — the
/// entry point a `[[bin]]` target compiles. Used by `ds build`/`ds run` (a bin
/// must have `main`) and the conformance harness. AST-level, so a `main_loop`
/// helper or a `"fn main"` string literal never trips a substring match.
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

/// Every declarable name in a `.ds` file with its binding span, kind, and (for
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
