//! Array literals: `[1, 2, 3]` → `vec![…]`, with spread (`[...xs, 4]`) support.

use oxc_ast::ast::{ArrayExpression, ArrayExpressionElement};
use syn::{parse_quote, Expr};

use super::super::context::Ctx;
use super::{array_elem_expr, translate_expr};

/// `[1, 2, 3]` → `vec![1_f64, 2_f64, 3_f64]`. A spread (`[...xs, 4]`) builds via
/// slice concat: `[xs.as_slice(), &[4_f64][..]].concat()`.
pub(super) fn array_expr(arr: &ArrayExpression, ctx: &Ctx<'_>) -> Expr {
    if arr
        .elements
        .iter()
        .any(|e| matches!(e, ArrayExpressionElement::SpreadElement(_)))
    {
        return spread_array(arr, ctx);
    }
    let elems: Vec<Expr> = arr
        .elements
        .iter()
        .filter_map(|e| array_element(e, ctx))
        .collect();
    parse_quote!(vec![#(#elems),*])
}

/// A spread-free inline array literal as a borrowed slice `&[…]`, for `for …
/// in` iteration (idiomatic; avoids clippy::useless_vec). Returns `None` when
/// the array has a spread (those need a `Vec` concat).
pub(in crate::translator) fn array_slice_expr(
    arr: &ArrayExpression,
    ctx: &Ctx<'_>,
) -> Option<Expr> {
    if arr
        .elements
        .iter()
        .any(|e| matches!(e, ArrayExpressionElement::SpreadElement(_)))
    {
        return None;
    }
    let elems: Vec<Expr> = arr
        .elements
        .iter()
        .filter_map(|e| array_element(e, ctx))
        .collect();
    Some(parse_quote!(&[#(#elems),*]))
}

/// `[...xs, 4]` → `[xs.as_slice(), &[4_f64][..]].concat()`: consecutive literals
/// batch into one `&[..]` slice, each spread into `arg.as_slice()`.
fn spread_array(arr: &ArrayExpression, ctx: &Ctx<'_>) -> Expr {
    let mut segments: Vec<Expr> = Vec::new();
    let mut literals: Vec<Expr> = Vec::new();
    for e in &arr.elements {
        match e {
            ArrayExpressionElement::SpreadElement(sp) => {
                flush_literals(&mut literals, &mut segments);
                let arg = translate_expr(&sp.argument, ctx);
                segments.push(parse_quote!(#arg.as_slice()));
            }
            other => {
                if let Some(expr) = array_element(other, ctx) {
                    literals.push(expr);
                }
            }
        }
    }
    flush_literals(&mut literals, &mut segments);
    parse_quote!([#(#segments),*].concat())
}

/// Flush pending literals into a `&[a, b, ..]` slice segment.
fn flush_literals(literals: &mut Vec<Expr>, segments: &mut Vec<Expr>) {
    if literals.is_empty() {
        return;
    }
    let owned = std::mem::take(literals);
    segments.push(parse_quote!(&[#(#owned),*][..]))
}

fn array_element(elem: &ArrayExpressionElement, ctx: &Ctx<'_>) -> Option<Expr> {
    // A spread element is handled earlier by `spread_array`; an elision (array
    // hole) has no Rust equivalent and is dropped. Any other element is an
    // expression — translate it through the main expression path so an array
    // literal may hold any expression, not just value literals.
    match elem {
        ArrayExpressionElement::SpreadElement(_) | ArrayExpressionElement::Elision(_) => None,
        // An element coerces to `f64` when numeric, so `[i, j]` where `i`/`j`
        // are flavor-promoted `i64` scalars still builds a `Vec<f64>`.
        _ => Some(array_elem_expr(elem.as_expression()?, ctx)),
    }
}
