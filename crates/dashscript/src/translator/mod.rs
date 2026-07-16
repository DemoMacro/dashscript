//! oxc AST → idiomatic Rust source, emitted through `syn` + `prettyplease`.
//!
//! Translation is one file per AST category — `declarations`, `functions`,
//! `types`, `expressions`, `bindings` — so each oxc node maps to a `syn` node
//! one-to-one. The `syn` tree is the project's hub: the translator builds it
//! (oxc → syn), `prettyplease` prints it, and the future `bindgen` parses
//! Rust crates into the same `syn` tree (syn → .ds) — one AST, two
//! directions. Parsing reuses `oxc_parser`; DashScript never parses itself.

mod analysis;
mod check;
pub mod bindings;
pub mod context;
pub mod declarations;
pub mod expressions;
pub mod functions;
pub mod imports;
pub mod registry;
pub mod types;

use oxc_allocator::Allocator;
use oxc_codegen::Codegen;
use oxc_diagnostics::OxcDiagnostic;
use oxc_parser::Parser;
use oxc_span::SourceType;

/// Translates a TypeScript-flavored `.ds` program into Rust source.
#[derive(Default)]
pub struct Translator;

impl Translator {
    /// Create a translator with default options.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Parse `.ds` source with oxc and translate the AST to Rust source.
    ///
    /// # Errors
    /// Returns an error string if oxc reports parse diagnostics.
    pub fn translate(&self, source: &str) -> Result<String, String> {
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, source, SourceType::ts()).parse();

        if !ret.diagnostics.is_empty() {
            return Err(format!(
                "dashscript: oxc reported {} parse diagnostic(s)",
                ret.diagnostics.len()
            ));
        }

        // First pass: collect discriminated-union enum shapes so later
        // expression translation can build variant constructors.
        let registry = registry::build_registry(&ret.program.body);
        let items = ret
            .program
            .body
            .iter()
            .filter_map(|s| functions::translate_statement(s, &registry))
            .collect();
        let file = syn::File { shebang: None, attrs: Vec::new(), items };
        Ok(prettyplease::unparse(&file))
    }

    /// Check `.ds` source for translatability without emitting Rust.
    ///
    /// Returns syntax errors from `oxc_parser` plus one diagnostic per
    /// top-level statement the translator cannot map. An empty `Vec` means the
    /// file is translatable to valid Rust (as far as DashScript can tell).
    #[must_use]
    pub fn check(&self, source: &str) -> Vec<OxcDiagnostic> {
        check::check(source)
    }

    /// Format `.ds` source with `oxc_codegen` (pretty-print, not minified).
    ///
    /// # Errors
    /// Returns an error string if `oxc_parser` reports syntax diagnostics — a
    /// file with syntax errors cannot be formatted.
    pub fn format(&self, source: &str) -> Result<String, String> {
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, source, SourceType::ts()).parse();
        if !ret.diagnostics.is_empty() {
            return Err(format!(
                "dashscript: oxc reported {} parse diagnostic(s) — fix syntax before formatting",
                ret.diagnostics.len()
            ));
        }
        Ok(Codegen::new().build(&ret.program).code)
    }

    /// The local `.ds` modules this file imports (`import { x } from "./other"`
    /// → `other`), for `ds build` to assemble one Rust module per dependency.
    #[must_use]
    pub fn imports(&self, source: &str) -> Vec<imports::ImportRef> {
        imports::collect_imports(source)
    }
}

#[cfg(test)]
mod tests;
