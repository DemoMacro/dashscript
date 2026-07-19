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
///
/// A `.ds` `number` is an IEEE-754 double — i.e. Rust `f64`. Every literal is
/// anchored to `f64` with a `_f64` suffix so a bare literal can stand where
/// rustc would otherwise see an ambiguous `{float}`: as a method receiver
/// (`5.is_finite()`) or as a `Vec` element followed by a chained method
/// (`vec![3, 1, 4].map(...)`). Consumers that need another integer type —
/// `as usize` for indexing, `as i32` for bitwise, `as u32` for a radix — already
/// cast, so anchoring is safe there too.
pub(super) fn numeric_expr(value: f64) -> Expr {
    let s = if value.is_nan() {
        "f64::NAN".to_string()
    } else if value.is_infinite() {
        if value > 0_f64 {
            "f64::INFINITY"
        } else {
            "f64::NEG_INFINITY"
        }
        .to_string()
    } else {
        format!("{value}_f64")
    };
    parse_str(&s).unwrap_or_else(|_| parse_quote!(::core::f64::NAN))
}

/// Render an integer-valued `f64` as an `i64` literal (`0_i64`, `42_i64`) —
/// the initializer of a flavor-inferred `i64` local. Only integer values reach
/// here (`flavor::literal` gates the promotion); a fractional value falls back
/// to `_f64` defensively (cargo check would flag the type mismatch anyway).
pub(super) fn numeric_expr_i64(value: f64) -> Expr {
    if value.is_finite() && value.fract() == 0.0 {
        let s = format!("{value}_i64");
        parse_str(&s).unwrap_or_else(|_| parse_quote!(0_i64))
    } else {
        numeric_expr(value)
    }
}
