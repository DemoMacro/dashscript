//! WHATWG `Headers` API — `new Headers(init?)` constructor + instance methods
//! (FETCH §5.1, a WinterTC Web API). The constructor builds a
//! `crate::__ds::DsHeaders` (an ordered, case-insensitive-by-name
//! `(name, value)` list) injected by the `Headers` runtime dep.
//! `headers_ctor_type` carries the name → type mapping the `new` lowering
//! needs; `headers_method` dispatches the instance methods on the receiver's
//! resolved type. ES coerces each name/value argument via `ToString`
//! (`es_to_string_arg`, the same path `URLSearchParams` takes), so a numeric
//! or `null` argument type-checks against the `&str` parameters.

use oxc_ast::ast::{
    Argument, ArrayExpression, Expression, ObjectExpression, ObjectPropertyKind, PropertyKey,
    StaticMemberExpression,
};
use syn::{parse_quote, Expr, Type};

use super::super::super::context::Ctx;
use super::super::super::expressions::{is_headers_local, translate_argument, translate_expr};
use super::super::es_to_string_arg;

/// The Rust type a WHATWG `Headers` constructor builds, if `name` is `Headers`:
/// `crate::__ds::DsHeaders`. `None` otherwise (the `new` lowering falls
/// through to the generic `Foo::new` path and surfaces at `cargo check`).
pub(in crate::translator) fn headers_ctor_type(name: &str) -> Option<Type> {
    match name {
        "Headers" => Some(parse_quote!(crate::__ds::DsHeaders)),
        _ => None,
    }
}

/// `new Headers(init?)` → `DsHeaders::new()` (no init) or
/// `DsHeaders::from_pairs(vec)` (a Record `{ name: value, … }` or a
/// `[[name, value], …]` sequence). Each name/value is coerced via ES
/// `ToString`. A `Headers`-copy or dynamic-value init is not statically
/// lowered (the static translator cannot copy an arbitrary header set without
/// a `Clone` deep-copy it does not model); it panics with the `TypeError`
/// message ES would throw (the WPT verdict reads the panic prefix) rather than
/// emitting a phantom constructor.
pub(in crate::translator) fn headers_ctor(args: &[Argument], ctx: &Ctx<'_>) -> Expr {
    match args.first().and_then(Argument::as_expression) {
        None => parse_quote!(crate::__ds::DsHeaders::new()),
        Some(Expression::ObjectExpression(obj)) => {
            let pairs = record_pairs(obj, ctx);
            parse_quote!(crate::__ds::DsHeaders::from_pairs(#pairs))
        }
        Some(Expression::ArrayExpression(arr)) => {
            let pairs = sequence_pairs(arr, ctx);
            parse_quote!(crate::__ds::DsHeaders::from_pairs(#pairs))
        }
        Some(_) => parse_quote!(::core::panic!(
            "TypeError: Headers construct: init must be a record, a sequence, or absent"
        )),
    }
}

/// A `Headers` instance method, dispatched on the receiver's resolved type.
/// Returns `None` for a non-`DsHeaders` receiver or an unmapped name, so the
/// call falls through to a plain method call (cargo check rejects it
/// honestly). Each name/value argument is coerced via ES `ToString`
/// (`es_to_string_arg`): a `number` becomes `number_to_string(…)`, a `string`
/// passes through, so `headers.set("id", 0)` and `headers.append("a", "b")`
/// both type-check against the `&str` parameters. Iteration (`keys`/`values`/
/// `entries`) lowers to a `Vec`-returning method — the static path trades an
/// ES iterator wrapper (a closure state machine) for a materialized `Vec`.
pub(in crate::translator) fn headers_method(
    sm: &StaticMemberExpression,
    args: &[Argument],
    ctx: &Ctx<'_>,
) -> Option<Expr> {
    if !is_headers_local(&sm.object, ctx) {
        return None;
    }
    let name = sm.property.name.as_str();
    let obj = translate_expr(&sm.object, ctx);
    Some(match name {
        "get" => {
            let k = es_to_string_arg(args.first()?, ctx);
            parse_quote!(#obj.get(#k))
        }
        "has" => {
            let k = es_to_string_arg(args.first()?, ctx);
            parse_quote!(#obj.has(#k))
        }
        "set" => {
            let k = es_to_string_arg(args.first()?, ctx);
            let v = es_to_string_arg(args.get(1)?, ctx);
            parse_quote!({
                #obj.set(#k, #v);
            })
        }
        "append" => {
            let k = es_to_string_arg(args.first()?, ctx);
            let v = es_to_string_arg(args.get(1)?, ctx);
            parse_quote!({
                #obj.append(#k, #v);
            })
        }
        "delete" => {
            let k = es_to_string_arg(args.first()?, ctx);
            parse_quote!({
                #obj.delete(#k);
            })
        }
        // `forEach(cb)` → `for_each(cb)`: value-first/name-second (ES order).
        // The callback lowers via `function_expr_to_closure`; the optional
        // `thisArg` (arg 1) is reflection the static path drops.
        "forEach" => {
            let cb = translate_argument(args.first()?, ctx);
            parse_quote!(#obj.for_each(#cb))
        }
        "keys" if args.is_empty() => parse_quote!(#obj.keys_vec()),
        "values" if args.is_empty() => parse_quote!(#obj.values_vec()),
        "entries" if args.is_empty() => parse_quote!(#obj.entries_vec()),
        _ => return None,
    })
}

/// Collect `(name, value)` pairs from a Record init (`{ name: value, … }`).
/// Each value is coerced via ES `ToString` (`expr_to_string`). Only literal
/// keys (identifier or string property names) lower — the common WPT shape; a
/// computed key is skipped (a non-literal name has no static lower).
fn record_pairs(obj: &ObjectExpression, ctx: &Ctx<'_>) -> Expr {
    let mut pairs: Vec<Expr> = Vec::new();
    for kind in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(p) = kind else {
            continue;
        };
        let key = match &p.key {
            PropertyKey::StaticIdentifier(id) => id.name.as_str(),
            PropertyKey::StringLiteral(s) => s.value.as_str(),
            _ => continue,
        };
        let key_lit = syn::LitStr::new(key, proc_macro2::Span::call_site());
        let val = expr_to_string(&p.value, ctx);
        pairs.push(parse_quote!((::std::string::String::from(#key_lit), #val)));
    }
    parse_quote!(::std::vec![#(#pairs),*])
}

/// Collect `(name, value)` pairs from a sequence init (`[[name, value], …]`).
/// Each element must be a 2-element array literal; a non-array / wrong-arity
/// element is skipped. Each value is coerced via ES `ToString`.
fn sequence_pairs(arr: &ArrayExpression, ctx: &Ctx<'_>) -> Expr {
    let mut pairs: Vec<Expr> = Vec::new();
    for el in &arr.elements {
        let Some(Expression::ArrayExpression(pair)) = el.as_expression() else {
            continue;
        };
        let (Some(Some(key_expr)), Some(Some(val_expr))) = (
            pair.elements.first().map(|e| e.as_expression()),
            pair.elements.get(1).map(|e| e.as_expression()),
        ) else {
            continue;
        };
        let key = expr_to_string(key_expr, ctx);
        let val = expr_to_string(val_expr, ctx);
        pairs.push(parse_quote!((#key, #val)));
    }
    parse_quote!(::std::vec![#(#pairs),*])
}

/// Coerce an arbitrary `Expression` to a `String`-typed expression via ES
/// `ToString`. A `number` routes through `number_to_string` (the precise ES
/// form: `-0` → `"0"`, `1e21` → `"1e+21"`); any other expression lowers via
/// `translate_expr` then `.to_string()` (a `String` clones, `bool`/`null`
/// `Display` matches ES closely enough for header values).
fn expr_to_string(expr: &Expression, ctx: &Ctx<'_>) -> Expr {
    match expr {
        Expression::NumericLiteral(_) => {
            let n = translate_expr(expr, ctx);
            parse_quote!(crate::__ds::number_to_string(#n))
        }
        _ => {
            let e = translate_expr(expr, ctx);
            parse_quote!((#e).to_string())
        }
    }
}
