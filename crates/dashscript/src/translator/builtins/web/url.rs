//! WHATWG URL API — `new URLSearchParams(...)` constructor + instance methods
//! (a WinterTC Web API). The constructor builds a stateful
//! `crate::__ds::DsUrlSearchParams` (a `Vec<(String, String)>` —
//! insertion-ordered name/value pairs) injected by the `Url` runtime dep.
//! `url_ctor_type` carries the name → type mapping the `new` lowering needs;
//! `url_search_params_method` dispatches the instance methods on the
//! receiver's resolved type. ES coerces each name/value argument via ToString
//! (a `number` routes through `number_to_string`), so the arguments go through
//! `es_to_string_arg` before the inherent methods — otherwise a numeric value
//! (`params.set("id", 0)`) fails `AsRef<str>`. `new URL(input[, base])` lowers
//! to `DsUrl::parse`/`parse_with_base` (a `url::Url` wrapper, injected by the
//! same `Url` runtime dep); its component accessors (`href`/`origin`/
//! `protocol`/…) are dispatched in `member.rs`.

use oxc_ast::ast::{Argument, StaticMemberExpression};
use syn::{parse_quote, Expr, Type};

use super::super::super::context::Ctx;
use super::super::super::expressions::{is_url_search_params_local, translate_expr};
use super::super::es_to_string_arg;

/// The Rust type a WHATWG URL API constructor builds, if `name` is one:
/// `URLSearchParams` → `crate::__ds::DsUrlSearchParams`, `URL` →
/// `crate::__ds::DsUrl`. `None` for any other name (the `new` lowering falls
/// through to the generic `Foo::new` path and surfaces at `cargo check`).
pub(in crate::translator) fn url_ctor_type(name: &str) -> Option<Type> {
    match name {
        "URLSearchParams" => Some(parse_quote!(crate::__ds::DsUrlSearchParams)),
        "URL" => Some(parse_quote!(crate::__ds::DsUrl)),
        _ => None,
    }
}

/// A `URLSearchParams` instance method, dispatched on the receiver's resolved
/// type. Returns `None` for a non-`DsUrlSearchParams` receiver or an unmapped
/// name, so the call falls through to a plain method call (cargo check
/// rejects it honestly). Each name/value argument is coerced via ES `ToString`
/// (`es_to_string_arg`): a `number` becomes `number_to_string(…)`, a `string`
/// passes through, so `params.set("id", 0)` and `params.append("a", "b")` both
/// type-check against the `AsRef<str>` parameters.
pub(in crate::translator) fn url_search_params_method(
    sm: &StaticMemberExpression,
    args: &[Argument],
    ctx: &Ctx<'_>,
) -> Option<Expr> {
    if !is_url_search_params_local(&sm.object, ctx) {
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
            // ES2024 `has(name, value)`: a present, non-undefined `value`
            // matches a (name, value) pair; an absent or `undefined` second
            // arg falls back to the name-only check.
            if value_arg_absent(args.get(1)) {
                parse_quote!(#obj.has(#k))
            } else {
                let v = es_to_string_arg(args.get(1)?, ctx);
                parse_quote!(#obj.has_value(#k, #v))
            }
        }
        "set" => {
            let k = es_to_string_arg(args.first()?, ctx);
            let v = es_to_string_arg(args.get(1)?, ctx);
            parse_quote!({ #obj.set(#k, #v); })
        }
        "append" => {
            let k = es_to_string_arg(args.first()?, ctx);
            let v = es_to_string_arg(args.get(1)?, ctx);
            parse_quote!({ #obj.append(#k, #v); })
        }
        "delete" => {
            let k = es_to_string_arg(args.first()?, ctx);
            // ES2024 `delete(name, value)`: same absent/undefined fallback as
            // `has`; a concrete `value` removes only matching pairs.
            if value_arg_absent(args.get(1)) {
                parse_quote!({ #obj.delete(#k); })
            } else {
                let v = es_to_string_arg(args.get(1)?, ctx);
                parse_quote!({ #obj.delete_value(#k, #v); })
            }
        }
        "getAll" => {
            let k = es_to_string_arg(args.first()?, ctx);
            parse_quote!(#obj.get_all(#k))
        }
        "sort" if args.is_empty() => parse_quote!({ #obj.sort(); }),
        "toString" if args.is_empty() => parse_quote!(#obj.to_string()),
        _ => return None,
    })
}

/// Whether the optional second argument of `has`/`delete` is absent or the
/// `undefined` global — the ES2024 fallback condition: both treat an
/// absent-or-undefined `value` as no value filter (name only).
fn value_arg_absent(arg: Option<&Argument>) -> bool {
    match arg {
        None => true,
        Some(Argument::Identifier(id)) if id.name.as_str() == "undefined" => true,
        _ => false,
    }
}
