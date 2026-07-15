//! `TSType` → `syn::Type` — the one-to-one mapping table.

use oxc_ast::ast::TSType;
use syn::{parse_quote, Type};

/// Map a TypeScript type annotation to its Rust equivalent as a `syn::Type`.
///
/// Unmapped types fall back to `_` (inference placeholder) so a missing
/// mapping surfaces as a `cargo check` error rather than silent miscompilation.
#[allow(clippy::match_same_arms)]
pub fn translate_type(ty: &TSType) -> Type {
    match ty {
        TSType::TSStringKeyword(_) => parse_quote!(String),
        TSType::TSNumberKeyword(_) => parse_quote!(f64),
        TSType::TSBooleanKeyword(_) => parse_quote!(bool),
        TSType::TSVoidKeyword(_) | TSType::TSUndefinedKeyword(_) => parse_quote!(()),
        _ => parse_quote!(_),
    }
}
