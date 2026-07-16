//! `.ds` translatability check — the middle layer of the three-layer
//! correctness chain (structure → translatability → `cargo check`).
//!
//! It reuses the translator's own mapping as the single source of truth: any
//! top-level statement [`super::functions::translate_statement`] cannot lower is
//! reported as a diagnostic, alongside the syntax errors `oxc_parser` already
//! surfaced. This answers "can this `.ds` become valid Rust?" — which
//! eslint-style rules cannot express, and which `oxc_linter` (not on crates.io)
//! is therefore not used for.

use oxc_allocator::Allocator;
use oxc_ast::ast::Statement;
use oxc_diagnostics::OxcDiagnostic;
use oxc_parser::Parser;
use oxc_span::{SourceType, Span};

use super::{functions, registry};

/// Check `.ds` source for translatability. Returns syntax errors from
/// `oxc_parser` plus one diagnostic per top-level statement the translator
/// cannot map. An empty result means the file lowers to valid Rust (as far as
/// DashScript can tell — `cargo check` is still the final arbiter).
pub(super) fn check(source: &str) -> Vec<OxcDiagnostic> {
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, SourceType::ts()).parse();

    // Layer 1 — structure: oxc_parser syntax errors.
    let mut diagnostics = ret.diagnostics.into_vec();

    // Layer 2 — translatability: the translator is the source of truth (its
    // `None` means "not mapped"); the match only adds a human message + span.
    let registry = registry::build_registry(&ret.program.body);
    for stmt in &ret.program.body {
        if functions::translate_statement(stmt, &registry).is_none() {
            diagnostics.push(unmapped_top_level(stmt));
        }
    }
    diagnostics
}

/// A human message + span for a top-level statement the translator skips.
fn unmapped_top_level(stmt: &Statement) -> OxcDiagnostic {
    match stmt {
        Statement::ImportDeclaration(s) => err("module `import` is not supported yet", s.span),
        Statement::ExportNamedDeclaration(s) => err("module `export` is not supported yet", s.span),
        Statement::ExportDefaultDeclaration(s) => {
            err("module `export default` is not supported yet", s.span)
        }
        Statement::ExportAllDeclaration(s) => err("module `export *` is not supported yet", s.span),
        Statement::ClassDeclaration(s) => err("classes are not supported yet", s.span),
        Statement::TSEnumDeclaration(s) => {
            err("TypeScript `enum` is not supported (use a union type instead)", s.span)
        }
        Statement::ExpressionStatement(s) => err(
            "a top-level expression is not allowed — only function/interface/type \
             declarations may sit at module scope",
            s.span,
        ),
        Statement::VariableDeclaration(s) => err(
            "a top-level variable declaration is not allowed — move it into a function",
            s.span,
        ),
        _ => OxcDiagnostic::error("this top-level statement cannot be translated to Rust"),
    }
}

fn err(message: &'static str, span: Span) -> OxcDiagnostic {
    OxcDiagnostic::error(message).with_label(span)
}
