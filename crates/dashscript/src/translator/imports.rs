//! `.ds` module imports. A relative import (`import { x } from "./other"`)
//! resolves to a local `.ds` file, so `ds build` emits one Rust module per
//! dependency (the matching `mod` declarations and `use` aliases). A *bare*
//! specifier (`import { X } from "serde"`) is a crate added via `ds add`: it is
//! not a local file (so it is excluded from module assembly below) but still
//! lowers to `use serde::X` — see [`module_ident`].

use oxc_allocator::Allocator;
use oxc_ast::ast::{ImportDeclarationSpecifier, Statement};
use oxc_parser::Parser;
use oxc_span::{SourceType, Span};
use syn::Ident;

use super::bindings;

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
            Some(ImportRef { module, source: imp.source.value.to_string() })
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
                    Some(CrateImportSymbol { name, span: local.span })
                })
                .collect();
            Some(CrateImport { module, symbols })
        })
        .collect()
}
