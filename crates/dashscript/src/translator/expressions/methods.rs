//! Method-call mappings for `.ds` expressions: array callbacks
//! (`.map`/`.filter`/`.slice`/`.indexOf`), string methods
//! (`.includes`/`.split`/…), and name-only renames (`.toUpperCase`).

use oxc_ast::ast::{Argument, Expression, StaticMemberExpression};
use proc_macro2::Span;
use quote::format_ident;
use syn::{parse_quote, Expr, Ident};

use super::super::bindings;
use super::super::context::Ctx;
use super::{arrow_expr, translate_argument, translate_expr};

/// Array methods on a `Vec` of Copy elements: `.map`/`.filter` take a callback
/// (`xs.iter().copied().<m>(f).collect::<Vec<_>>()`, with `.filter`'s param
/// destructured since its closure receives `&Item`); `.slice(a, b)` →
/// `xs[a as usize..b as usize].to_vec()` (`.slice(a)` / `.slice()` clamp the
/// range / clone). Returns `None` otherwise.
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

/// True when `callee` is `console.log` (a static member access).
pub(super) fn is_console_log(callee: &Expression) -> bool {
    let Expression::StaticMemberExpression(member) = callee else {
        return false;
    };
    is_ident(&member.object, "console") && {
        let prop: &str = &member.property.name;
        prop == "log"
    }
}

/// True when `expr` is an `Identifier` whose name equals `expected`.
pub(super) fn is_ident(expr: &Expression, expected: &str) -> bool {
    let Expression::Identifier(ident) = expr else {
        return false;
    };
    let name: &str = &ident.name;
    name == expected
}
