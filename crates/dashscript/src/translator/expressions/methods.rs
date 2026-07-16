//! Method-call mappings for `.ds` expressions: array callbacks
//! (`.map`/`.filter`/`.find`/`.some`/`.every`/`.reduce`/`.join`/`.slice`/
//! `.indexOf`/`.includes`), string methods (`.includes`/`.split`/…), and
//! name-only renames (`.toUpperCase`).

use oxc_ast::ast::{Argument, Expression, IdentifierReference, StaticMemberExpression};
use proc_macro2::Span;
use quote::format_ident;
use syn::{parse_quote, Expr, Ident};

use super::super::bindings;
use super::super::context::Ctx;
use super::{arrow_expr, translate_argument, translate_expr};

/// Array methods on a `Vec` of Copy elements. The callback methods share a
/// `xs.iter().copied().<m>(f)` core: `.map`/`.filter` collect back into a
/// `Vec`; `.find`/`.some`/`.every`/`.reduce` return a scalar. A closure that
/// receives `&Item` (`filter`/`find`) destructures its param as `|&n|`; one that
/// receives the item by value (`map`/`some`/`every`/`reduce`) uses a plain
/// `|n|`. `.slice(a, b)` → `xs[a as usize..b as usize].to_vec()`; `.join(sep)`
/// stringifies each element first; `.indexOf`/`.includes` test membership.
/// Returns `None` for an unmapped name.
pub(super) fn array_method(
    sm: &StaticMemberExpression,
    args: &[Argument],
    ctx: &Ctx<'_>,
) -> Option<Expr> {
    let Expression::Identifier(id) = &sm.object else {
        return None;
    };
    let recv = bindings::snake(&id.name);
    let path = ctx.local_type(&recv.to_string())?;
    let is_vec = path.segments.last().is_some_and(|s| s.ident == "Vec");
    if !is_vec {
        return None;
    }
    Some(match sm.property.name.as_str() {
        "map" => {
            let cb = translate_argument(args.first()?, ctx);
            parse_quote!(#recv.iter().copied().map(#cb).collect::<Vec<_>>())
        }
        "filter" => {
            let Argument::ArrowFunctionExpression(arrow) = args.first()? else {
                return None;
            };
            let cb = arrow_expr(arrow, ctx, true);
            parse_quote!(#recv.iter().copied().filter(#cb).collect::<Vec<_>>())
        }
        // `.ds` indices are `f64`; cast each bound to `usize`.
        "slice" => {
            let start = args.first().map(|a| usize_arg(a, ctx));
            let end = args.get(1).map(|a| usize_arg(a, ctx));
            match (start, end) {
                (Some(s), Some(e)) => parse_quote!(#recv[#s..#e].to_vec()),
                (Some(s), None) => parse_quote!(#recv[#s..].to_vec()),
                (None, _) => parse_quote!(#recv.clone()),
            }
        }
        // `.indexOf(x)` → first index of `x`, or -1 (TS returns a `number`).
        "indexOf" => {
            let needle = translate_argument(args.first()?, ctx);
            parse_quote!(
                #recv
                    .iter()
                    .copied()
                    .position(|y| y == #needle)
                    .map(|i| i as f64)
                    .unwrap_or(-1.0)
            )
        }
        // `.includes(x)` → Vec::contains (by reference).
        "includes" => {
            let needle = translate_argument(args.first()?, ctx);
            parse_quote!(#recv.contains(&#needle))
        }
        // `.find(cb)` → first match as `Option<T>` (TS `T | undefined`); the
        // closure receives `&Item`, so its param destructures as `|&n|`.
        "find" => {
            let Argument::ArrowFunctionExpression(arrow) = args.first()? else {
                return None;
            };
            let cb = arrow_expr(arrow, ctx, true);
            parse_quote!(#recv.iter().copied().find(#cb))
        }
        // `.some(cb)` → `any` (true if any element matches); `any` takes the
        // item by value (after `.copied()`), so the param is a plain `|n|`.
        "some" => {
            let Argument::ArrowFunctionExpression(arrow) = args.first()? else {
                return None;
            };
            let cb = arrow_expr(arrow, ctx, false);
            parse_quote!(#recv.iter().copied().any(#cb))
        }
        // `.every(cb)` → `all` (true if all elements match); same value param.
        "every" => {
            let Argument::ArrowFunctionExpression(arrow) = args.first()? else {
                return None;
            };
            let cb = arrow_expr(arrow, ctx, false);
            parse_quote!(#recv.iter().copied().all(#cb))
        }
        // `.join(sep)` → `Vec<String>.join(sep)` (each element stringified first).
        "join" => {
            let sep = str_method_arg(args.first()?, ctx);
            parse_quote!(#recv.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(#sep))
        }
        // `.reduce(cb, init)` → `fold`; `.reduce(cb)` (no seed) → `reduce`,
        // which yields `Option<T>` (an empty `.ds` array has no first element).
        "reduce" => {
            let Argument::ArrowFunctionExpression(arrow) = args.first()? else {
                return None;
            };
            let cb = arrow_expr(arrow, ctx, false);
            match args.get(1) {
                Some(init) => {
                    let init = translate_argument(init, ctx);
                    parse_quote!(#recv.iter().copied().fold(#init, #cb))
                }
                None => parse_quote!(#recv.iter().copied().reduce(#cb)),
            }
        }
        _ => return None,
    })
}

/// A handful of TS method names map to a different Rust method name; the
/// receiver and arguments are passed through unchanged. Unmapped methods fall
/// through to a plain call on the receiver expression.
pub(super) fn map_method(name: &str) -> Option<Ident> {
    let mapped = match name {
        "toUpperCase" => "to_uppercase",
        "toLowerCase" => "to_lowercase",
        "trim" => "trim",
        "trimStart" => "trim_start",
        "trimEnd" => "trim_end",
        "push" => "push",
        "pop" => "pop",
        _ => return None,
    };
    Some(format_ident!("{}", mapped))
}

/// String methods whose arguments need adapting to Rust's `&str`-oriented API:
/// `includes`/`startsWith`/`endsWith` → `contains`/`starts_with`/`ends_with`;
/// `replace` → `replacen(.., 1)` (TS replaces the first match only); `repeat`
/// → `repeat(n as usize)`. Returns `None` for unmapped names.
pub(super) fn string_method(
    sm: &StaticMemberExpression,
    args: &[Argument],
    ctx: &Ctx<'_>,
) -> Option<Expr> {
    let obj = translate_expr(&sm.object, ctx);
    let name: &str = &sm.property.name;
    Some(match name {
        "includes" => {
            let a = str_method_arg(args.first()?, ctx);
            parse_quote!(#obj.contains(#a))
        }
        "startsWith" => {
            let a = str_method_arg(args.first()?, ctx);
            parse_quote!(#obj.starts_with(#a))
        }
        "endsWith" => {
            let a = str_method_arg(args.first()?, ctx);
            parse_quote!(#obj.ends_with(#a))
        }
        "replace" => {
            let a = str_method_arg(args.first()?, ctx);
            let b = str_method_arg(args.get(1)?, ctx);
            parse_quote!(#obj.replacen(#a, #b, 1))
        }
        "repeat" => {
            let n = usize_arg(args.first()?, ctx);
            parse_quote!(#obj.repeat(#n))
        }
        "split" => {
            // `split` yields `&str`; map to owned so the result is `Vec<String>`.
            let delim = str_method_arg(args.first()?, ctx);
            parse_quote!(#obj.split(#delim).map(|part| part.to_string()).collect::<Vec<String>>())
        }
        // `.indexOf(s)` → byte offset (ASCII == char index), or -1.
        "indexOf" => {
            let needle = str_method_arg(args.first()?, ctx);
            parse_quote!(#obj.find(#needle).map(|b| b as f64).unwrap_or(-1.0))
        }
        // `.slice(a, b)` / `.substring` → byte slice `s[a..b]` (ASCII), owned.
        "slice" | "substring" => {
            let start = usize_arg(args.first()?, ctx);
            match args.get(1) {
                Some(end) => {
                    let end = usize_arg(end, ctx);
                    parse_quote!(#obj[#start..#end].to_string())
                }
                None => parse_quote!(#obj[#start..].to_string()),
            }
        }
        // `.charAt(i)` → the `i`-th char as a `String` ("" if out of range).
        "charAt" => {
            let i = usize_arg(args.first()?, ctx);
            parse_quote!(#obj.chars().nth(#i).map(|c| c.to_string()).unwrap_or_default())
        }
        _ => return None,
    })
}

/// A string-method argument as a `&str`: a string literal stays a bare literal
/// (a perfect `Pattern`); any other expression (a `String` var or call) gets
/// `.as_str()` so it satisfies Rust's `&str`-typed string APIs.
fn str_method_arg(arg: &Argument, ctx: &Ctx<'_>) -> Expr {
    if let Argument::StringLiteral(s) = arg {
        let lit = syn::LitStr::new(s.value.as_str(), Span::call_site());
        return parse_quote!(#lit);
    }
    let e = translate_argument(arg, ctx);
    parse_quote!(#e.as_str())
}

/// A `.ds` `number` argument cast to `usize` (e.g. for `repeat`).
fn usize_arg(arg: &Argument, ctx: &Ctx<'_>) -> Expr {
    let e = translate_argument(arg, ctx);
    parse_quote!(#e as usize)
}

/// Global conversion functions called as plain identifiers: `String(x)` →
/// `format!("{}", x)`; `parseInt(s)`/`parseFloat(s)` → `s.trim().parse::<f64>()`
/// (`.ds` `number` is `f64`, so both share one parse path). Returns `None` for
/// any other name (falls through to a plain call).
pub(super) fn global_function(
    id: &IdentifierReference,
    args: &[Argument],
    ctx: &Ctx<'_>,
) -> Option<Expr> {
    let name: &str = &id.name;
    Some(match name {
        "String" => {
            let a = translate_argument(args.first()?, ctx);
            parse_quote!(::std::format!("{}", #a))
        }
        "parseInt" | "parseFloat" => {
            let a = translate_argument(args.first()?, ctx);
            parse_quote!(#a.trim().parse::<f64>().unwrap())
        }
        _ => return None,
    })
}

/// The Rust macro for a `console.<m>(…)` call: `log` → `println!`, `warn`/
/// `error` → `eprintln!`. Returns `None` for any other member.
pub(super) fn console_method(callee: &Expression) -> Option<Ident> {
    let Expression::StaticMemberExpression(member) = callee else {
        return None;
    };
    if !is_ident(&member.object, "console") {
        return None;
    }
    let name = match member.property.name.as_str() {
        "log" => "println",
        "warn" | "error" => "eprintln",
        _ => return None,
    };
    Some(format_ident!("{}", name))
}

/// True when `expr` is an `Identifier` whose name equals `expected`.
pub(super) fn is_ident(expr: &Expression, expected: &str) -> bool {
    let Expression::Identifier(ident) = expr else {
        return false;
    };
    let name: &str = &ident.name;
    name == expected
}
