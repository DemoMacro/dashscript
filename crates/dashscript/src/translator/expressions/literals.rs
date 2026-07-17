//! Value-literal renderers: `.ds` string / number / boolean → `syn::Expr`.

use oxc_ast::ast::StringLiteral;
use proc_macro2::Span;
use syn::{parse_quote, parse_str, Expr};

/// `.ds` string literal → Rust `String` (`"…".to_string()`).
pub(in crate::translator) fn string_expr(s: &StringLiteral) -> Expr {
    let lit = syn::LitStr::new(s.value.as_str(), Span::call_site());
    parse_quote!(#lit.to_string())
}

pub(in crate::translator) fn bool_expr(value: bool) -> Expr {
    parse_quote!(#value)
}

/// Render an `f64` as a valid Rust literal expression.
pub(super) fn numeric_expr(value: f64) -> Expr {
    let s = if value.is_nan() {
        "f64::NAN".to_string()
    } else if value.is_infinite() {
        if value > 0.0 { "f64::INFINITY" } else { "f64::NEG_INFINITY" }.to_string()
    } else {
        let s = format!("{value}");
        if s.contains('.') || s.contains('e') || s.contains('E') { s } else { format!("{s}.0") }
    };
    parse_str(&s).unwrap_or_else(|_| parse_quote!(::core::f64::NAN))
}
