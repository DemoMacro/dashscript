//! Member access: `p.x` field access, `m["k"]` HashMap / Vec index, `a?.x` chain.

use oxc_ast::ast::{ComputedMemberExpression, Expression, StaticMemberExpression};
use proc_macro2::Span;
use syn::{parse_quote, Expr};

use super::super::bindings;
use super::super::builtins;
use super::super::context::Ctx;
use super::is_hashmap;
use super::translate_expr;

/// Optional chaining `a?.field` → `a.as_ref().map(|__c| __c.field)`. The
/// receiver is an `Option`; the access maps over a reference and yields
/// another `Option`. Only a single optional field access is handled; indexed
/// access, optional calls, and chained `a?.b?.c` fall back to `todo!()`.
pub(super) fn chain_expr(elem: &oxc_ast::ast::ChainElement, ctx: &Ctx<'_>) -> Expr {
    use oxc_ast::ast::ChainElement;
    match elem {
        ChainElement::StaticMemberExpression(sm) => {
            let obj = translate_expr(&sm.object, ctx);
            let field = bindings::snake(&sm.property.name);
            parse_quote!(#obj.as_ref().map(|__c| __c.#field))
        }
        _ => parse_quote!(::core::todo!()),
    }
}

/// `p.x` → field access. (A `console.log` callee is intercepted earlier.)
pub(super) fn member_expr(sm: &StaticMemberExpression, ctx: &Ctx<'_>) -> Expr {
    let field_name: &str = &sm.property.name;
    // `tags.a` on a `Record`/HashMap local → `tags.get("a").copied().unwrap()`
    // (a TS `Record` static field access and `m["a"]` are the same lookup).
    if is_hashmap_local(&sm.object, ctx) {
        let obj = translate_expr(&sm.object, ctx);
        let key = syn::LitStr::new(field_name, Span::call_site());
        return parse_quote!(#obj.get(#key).copied().unwrap());
    }
    // `Math.PI` / `Math.E` → the corresponding Rust constant.
    if builtins::is_ident(&sm.object, "Math") {
        if let Some(p) = builtins::math_constant(field_name) {
            return p;
        }
    }
    // `Number.EPSILON` / `Number.MAX_VALUE` / `Number.NaN` / … → the matching
    // `f64` constant.
    if builtins::is_ident(&sm.object, "Number") {
        if let Some(p) = builtins::number_constant(field_name) {
            return p;
        }
    }
    // Inside a discriminated-union match arm, `s.field` reads as the `field`
    // binding the pattern destructured (TS narrowing).
    if let Expression::Identifier(id) = &sm.object {
        let scrut = bindings::snake(&id.name);
        let field = bindings::snake(field_name);
        if ctx.narrow_binds(&scrut.to_string(), &field.to_string()) {
            return parse_quote!(#field);
        }
    }
    let obj = translate_expr(&sm.object, ctx);
    // `.length` on a Vec/String maps to Rust's `.len()` (a method, not a field).
    // TS `.length` is always a `number` → `f64`; `len()` returns `usize`, so cast.
    // Index/repeat sites that need `usize` cast the whole expression again.
    if field_name == "length" {
        return parse_quote!((#obj.len() as f64));
    }
    let field = bindings::snake(field_name);
    parse_quote!(#obj.#field)
}

/// `arr[i]` → `arr[i as usize]`; `m["k"]` on a `HashMap` →
/// `m.get("k").copied().unwrap()`. A `.ds` index is `f64`; Rust indexes by
/// `usize`, so the Vec/array index is cast. A HashMap key is looked up with
/// `.get` (typed: the key is assumed present, so `unwrap` panics if absent —
/// matching the non-optional type).
pub(super) fn computed_member(cm: &ComputedMemberExpression, ctx: &Ctx<'_>) -> Expr {
    let obj = translate_expr(&cm.object, ctx);
    if is_hashmap_local(&cm.object, ctx) {
        let key = index_key(&cm.expression, ctx);
        return parse_quote!(#obj.get(#key).copied().unwrap());
    }
    let idx = translate_expr(&cm.expression, ctx);
    let idx = Expr::Cast(syn::ExprCast {
        attrs: Vec::new(),
        expr: Box::new(idx),
        as_token: syn::Token![as](Span::call_site()),
        ty: Box::new(parse_quote!(usize)),
    });
    parse_quote!(#obj[#idx])
}

/// True when `expr` is a local whose type is a `HashMap`.
pub(super) fn is_hashmap_local(expr: &Expression, ctx: &Ctx<'_>) -> bool {
    let Expression::Identifier(id) = expr else {
        return false;
    };
    let name = bindings::snake(&id.name).to_string();
    ctx.local_type(&name).is_some_and(is_hashmap)
}

/// A HashMap key: a string literal stays bare (a `&str` for `HashMap::get`);
/// any other expression gets `.as_str()`.
fn index_key(expr: &Expression, ctx: &Ctx<'_>) -> Expr {
    if let Expression::StringLiteral(s) = expr {
        let lit = syn::LitStr::new(s.value.as_str(), Span::call_site());
        return parse_quote!(#lit);
    }
    let e = translate_expr(expr, ctx);
    parse_quote!(#e.as_str())
}
