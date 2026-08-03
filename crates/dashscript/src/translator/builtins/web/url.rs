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

use oxc_ast::ast::{Argument, Expression, StaticMemberExpression};
use syn::{parse_quote, Expr, Type};

use super::super::super::context::Ctx;
use super::super::super::expressions::{is_url_local, is_url_search_params_local, translate_expr};
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

/// `url.searchParams.<method>(...)` — a URLSearchParams method invoked through
/// a DsUrl's live `searchParams` view. The receiver is `<DsUrl>.searchParams`
/// (a `StaticMemberExpression` whose object is a `DsUrl` local and whose
/// property is `searchParams`); the method mutates the URL's query in place,
/// so it lowers to a `DsUrl::sp_<method>(&self, …)` call on the URL local.
/// Returns `None` for any other receiver shape or an unmapped name, so the call
/// falls through to a plain method call (cargo check rejects it honestly).
pub(in crate::translator) fn url_search_params_on_url_method(
    sm: &StaticMemberExpression,
    args: &[Argument],
    ctx: &Ctx<'_>,
) -> Option<Expr> {
    let Expression::StaticMemberExpression(inner) = &sm.object else {
        return None;
    };
    if inner.property.name.as_str() != "searchParams" {
        return None;
    }
    if !is_url_local(&inner.object, ctx) {
        return None;
    }
    let url = translate_expr(&inner.object, ctx);
    let name = sm.property.name.as_str();
    Some(match name {
        "get" => {
            let k = es_to_string_arg(args.first()?, ctx);
            parse_quote!(#url.sp_get(#k))
        }
        "has" => {
            let k = es_to_string_arg(args.first()?, ctx);
            if value_arg_absent(args.get(1)) {
                parse_quote!(#url.sp_has(#k))
            } else {
                let v = es_to_string_arg(args.get(1)?, ctx);
                parse_quote!(#url.sp_has_value(#k, #v))
            }
        }
        "set" => {
            let k = es_to_string_arg(args.first()?, ctx);
            let v = es_to_string_arg(args.get(1)?, ctx);
            parse_quote!({ #url.sp_set(#k, #v); })
        }
        "append" => {
            let k = es_to_string_arg(args.first()?, ctx);
            let v = es_to_string_arg(args.get(1)?, ctx);
            parse_quote!({ #url.sp_append(#k, #v); })
        }
        "delete" => {
            let k = es_to_string_arg(args.first()?, ctx);
            if value_arg_absent(args.get(1)) {
                parse_quote!({ #url.sp_delete(#k); })
            } else {
                let v = es_to_string_arg(args.get(1)?, ctx);
                parse_quote!({ #url.sp_delete_value(#k, #v); })
            }
        }
        "getAll" => {
            let k = es_to_string_arg(args.first()?, ctx);
            parse_quote!(#url.sp_get_all(#k))
        }
        "sort" if args.is_empty() => parse_quote!({ #url.sp_sort(); }),
        "toString" if args.is_empty() => parse_quote!(#url.sp_to_string()),
        _ => return None,
    })
}

/// `URL.<static method>(...)` — a static method on the `URL` constructor object
/// (a WinterTC WHATWG URL API). The callee is `<URL>.<method>`, where `URL` is
/// the constructor identifier — not translated as a value (it has no Rust
/// binding, only the `DsUrl` type and the `url` crate), so the dispatch
/// intercepts it and lowers directly to the `__ds::url_*` free helper. Each
/// argument is coerced via ES `ToString` (`es_to_string_arg`), so a `number` or
/// `undefined` argument type-checks against the helper's `AsRef<str>`.
/// `URL.parse` lowers to `Option<DsUrl>` (ES `null` on a parse failure, not a
/// throw). Returns `None` for a non-`URL` receiver or an unmapped name, so the
/// call falls through to a plain method call (cargo check rejects it honestly).
pub(in crate::translator) fn url_static_method(
    sm: &StaticMemberExpression,
    args: &[Argument],
    ctx: &Ctx<'_>,
) -> Option<Expr> {
    match &sm.object {
        Expression::Identifier(id) if id.name.as_str() == "URL" => {}
        _ => return None,
    }
    let name = sm.property.name.as_str();
    Some(match name {
        "canParse" => {
            let url = es_to_string_arg(args.first()?, ctx);
            match args.get(1) {
                None => parse_quote!(crate::__ds::DsUrl::can_parse(#url)),
                Some(base) => {
                    let base = es_to_string_arg(base, ctx);
                    parse_quote!(crate::__ds::DsUrl::can_parse_with_base(#url, #base))
                }
            }
        }
        "parse" => {
            let url = es_to_string_arg(args.first()?, ctx);
            match args.get(1) {
                None => parse_quote!(crate::__ds::DsUrl::parse_opt(#url)),
                Some(base) => {
                    let base = es_to_string_arg(base, ctx);
                    parse_quote!(crate::__ds::DsUrl::parse_opt_with_base(#url, #base))
                }
            }
        }
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
