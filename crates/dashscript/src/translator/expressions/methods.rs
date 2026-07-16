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
use super::{arrow_expr, bool_expr, string_expr, translate_argument, translate_expr};

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
        // `.flatMap(cb)` → `flat_map` then collect; `cb` returns a `Vec` per
        // element (TS `flatMap` requires an array return), flattened into one.
        "flatMap" => {
            let cb = translate_argument(args.first()?, ctx);
            parse_quote!(#recv.iter().copied().flat_map(#cb).collect::<Vec<_>>())
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
        // `.at(i)` → element at signed index `i` (TS allows negatives to count
        // from the end). `i` is `f64`; bind it once so a side-effecting index
        // expression evaluates only once, then branch on its sign.
        "at" => {
            let idx = translate_argument(args.first()?, ctx);
            parse_quote!({
                let __at_i = #idx;
                #recv[if __at_i >= 0.0 { __at_i as usize } else { (#recv.len() as f64 + __at_i) as usize }]
            })
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
        // `.lastIndexOf(x)` → last index of `x`, or -1 (`rposition`).
        "lastIndexOf" => {
            let needle = translate_argument(args.first()?, ctx);
            parse_quote!(
                #recv
                    .iter()
                    .copied()
                    .rposition(|y| y == #needle)
                    .map(|i| i as f64)
                    .unwrap_or(-1.0)
            )
        }
        // `.findIndex(cb)` → first index where cb holds, or -1. `position` takes
        // the item by value (after `.copied()`), so the param is `|n|`.
        "findIndex" => {
            let Argument::ArrowFunctionExpression(arrow) = args.first()? else {
                return None;
            };
            let cb = arrow_expr(arrow, ctx, false);
            parse_quote!(
                #recv
                    .iter()
                    .copied()
                    .position(#cb)
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
        // `.forEach(cb)` → `for_each` (side-effecting; returns `()`). The
        // callback takes the item by value (after `.copied()`), so `|n|`.
        "forEach" => {
            let Argument::ArrowFunctionExpression(arrow) = args.first()? else {
                return None;
            };
            let cb = arrow_expr(arrow, ctx, false);
            parse_quote!(#recv.iter().copied().for_each(#cb))
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
        // `.findLast(cb)` → last match as `Option<T>` (reverse `find`); `find`
        // takes `&Item`, so the closure param destructures as `|&n|`.
        "findLast" => {
            let Argument::ArrowFunctionExpression(arrow) = args.first()? else {
                return None;
            };
            let cb = arrow_expr(arrow, ctx, true);
            parse_quote!(#recv.iter().copied().rev().find(#cb))
        }
        // `.findLastIndex(cb)` → last index where cb holds, or -1 (`rposition`
        // searches from the end and returns the original index).
        "findLastIndex" => {
            let Argument::ArrowFunctionExpression(arrow) = args.first()? else {
                return None;
            };
            let cb = arrow_expr(arrow, ctx, false);
            parse_quote!(
                #recv
                    .iter()
                    .copied()
                    .rposition(#cb)
                    .map(|i| i as f64)
                    .unwrap_or(-1.0)
            )
        }
        // `.reduceRight(cb, init)` → reverse `fold`; `.reduceRight(cb)` → reverse
        // `reduce` (yields `Option<T>`, as the no-seed form does).
        "reduceRight" => {
            let Argument::ArrowFunctionExpression(arrow) = args.first()? else {
                return None;
            };
            let cb = arrow_expr(arrow, ctx, false);
            match args.get(1) {
                Some(init) => {
                    let init = translate_argument(init, ctx);
                    parse_quote!(#recv.iter().copied().rev().fold(#init, #cb))
                }
                None => parse_quote!(#recv.iter().copied().rev().reduce(#cb)),
            }
        }
        // `.flat()` → flatten one level (`Vec<Vec<T>>::concat` → `Vec<T>`).
        // A depth argument is unsupported.
        "flat" if args.is_empty() => parse_quote!(#recv.concat()),
        // `.concat(ys, …)` → concatenate slices into a new `Vec`. Arguments
        // are assumed to be arrays; scalar concat args are unsupported.
        "concat" => {
            let parts: Vec<Expr> = args
                .iter()
                .map(|a| {
                    let e = translate_argument(a, ctx);
                    parse_quote!(#e.as_slice())
                })
                .collect();
            parse_quote!([#recv.as_slice(), #(#parts),*].concat())
        }
        // `.fill(v)` → in-place `Vec::fill` (every element set to `v`). Mutates;
        // a start/end range is unsupported.
        "fill" if args.len() == 1 => {
            let v = translate_argument(args.first()?, ctx);
            parse_quote!(#recv.fill(#v))
        }
        // `.reverse()` → in-place `Vec::reverse`. Mutates; needs a mutable
        // (`let`) array. TS returns the same reference — DashScript uses it
        // statement-style.
        "reverse" if args.is_empty() => parse_quote!(#recv.reverse()),
        // `.sort()` → in-place numeric ascending sort (TS default sort is
        // lexicographic; DashScript treats number arrays numerically). A
        // comparator argument is unsupported — it would return `Ordering`.
        "sort" if args.is_empty() => {
            // `partial_cmp` is `None` for NaN; fall back to `Equal` so a NaN
            // element never panics (TS sort never throws on NaN).
            parse_quote!(#recv.sort_by(|a, b| a.partial_cmp(&b).unwrap_or(::core::cmp::Ordering::Equal)))
        }
        _ => return None,
    })
}

/// Methods on a `.ds` `number` (`f64`). `.toFixed(n)` → a formatted string
/// with `n` decimal places. Returns `None` for an unmapped name.
pub(super) fn number_method(
    sm: &StaticMemberExpression,
    args: &[Argument],
    ctx: &Ctx<'_>,
) -> Option<Expr> {
    let recv = translate_expr(&sm.object, ctx);
    Some(match sm.property.name.as_str() {
        // `(3.14).toFixed(2)` → `format!("{:.*}", n, …)`. In Rust the `*`
        // precision argument comes first, the value second.
        "toFixed" => {
            let prec = usize_arg(args.first()?, ctx);
            parse_quote!(format!("{:.*}", #prec, #recv))
        }
        // `(255).toString(radix)` → a format with the matching base. Only a
        // literal radix of 2/8/16/10 maps; the f64 is cast to `u32` first (TS
        // truncates the fractional part). A non-literal radix is unmapped.
        "toString" if !args.is_empty() => {
            let radix = match args.first()? {
                Argument::NumericLiteral(n) => n.value,
                _ => return None,
            };
            let fmt = match radix as u64 {
                16 => "{:x}",
                2 => "{:b}",
                8 => "{:o}",
                10 => "{}",
                _ => return None,
            };
            let fmt_lit = syn::LitStr::new(fmt, Span::call_site());
            parse_quote!(format!(#fmt_lit, (#recv) as u32))
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
        // `.toString()` → `.to_string()` (Rust's `Display`). A numeric receiver
        // with a radix (`(255).toString(16)`) is handled in `number_method`.
        "toString" => "to_string",
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
        // `.replaceAll(a, b)` → `replace` (all matches; TS `replace` does one).
        "replaceAll" => {
            let a = str_method_arg(args.first()?, ctx);
            let b = str_method_arg(args.get(1)?, ctx);
            parse_quote!(#obj.replace(#a, #b))
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
        // `.charCodeAt(i)` → the `i`-th char's code point as `f64` (TS returns
        // `NaN` out of range; UTF-16 vs Rust's `char` differ for non-BMP).
        "charCodeAt" => {
            let i = usize_arg(args.first()?, ctx);
            parse_quote!(#obj.chars().nth(#i).map(|c| c as u32 as f64).unwrap_or(f64::NAN))
        }
        // `.padStart(n)` → right-align to width `n` (fills on the left with
        // spaces, TS's default pad).
        "padStart" if args.len() <= 1 => {
            let n = usize_arg(args.first()?, ctx);
            parse_quote!(format!("{:>1$}", #obj, #n))
        }
        // `.padStart(n, ch)` → fill the left with `ch` (its chars cycled) to
        // width `n`. Rust's `format!` fill must be a literal char, so a dynamic
        // pad is built by cycling `ch`'s chars to the needed width (matches TS:
        // `"5".padStart(6, "ab")` → `"ababa5"`).
        "padStart" if args.len() >= 2 => {
            let n = usize_arg(args.first()?, ctx);
            let ch = str_method_arg(args.get(1)?, ctx);
            parse_quote!({
                let __s = #obj;
                let __need = (#n).saturating_sub(__s.chars().count());
                format!("{}{}", #ch.chars().cycle().take(__need).collect::<String>(), __s)
            })
        }
        // `.padEnd(n)` → left-align to width `n` (fills on the right).
        "padEnd" if args.len() <= 1 => {
            let n = usize_arg(args.first()?, ctx);
            parse_quote!(format!("{:<1$}", #obj, #n))
        }
        // `.padEnd(n, ch)` → fill the right with `ch` (cycled) to width `n`.
        "padEnd" if args.len() >= 2 => {
            let n = usize_arg(args.first()?, ctx);
            let ch = str_method_arg(args.get(1)?, ctx);
            parse_quote!({
                let __s = #obj;
                let __need = (#n).saturating_sub(__s.chars().count());
                format!("{}{}", __s, #ch.chars().cycle().take(__need).collect::<String>())
            })
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

/// `Object.<m>(record)` on a `Record` (a `HashMap`): `keys` → the map's keys
/// as `Vec<String>`, `values` → its values (cloned, so Copy and Clone both
/// work), `entries` → `(K, V)` pairs. Returns `None` for any other member.
pub(super) fn object_method(name: &str, args: &[Argument], ctx: &Ctx<'_>) -> Option<Expr> {
    let r = translate_argument(args.first()?, ctx);
    Some(match name {
        "keys" => parse_quote!(#r.keys().map(|k| k.to_string()).collect::<Vec<_>>()),
        "values" => parse_quote!(#r.values().cloned().collect::<Vec<_>>()),
        "entries" => {
            parse_quote!(#r.iter().map(|(k, v)| (k.clone(), v.clone())).collect::<Vec<_>>())
        }
        _ => return None,
    })
}

/// `String.<m>(…)`: `fromCharCode(n)` → a one-char `String` from the code
/// point (or `""` if `n` isn't a valid `char`). Returns `None` otherwise.
pub(super) fn string_static(name: &str, args: &[Argument], ctx: &Ctx<'_>) -> Option<Expr> {
    let n = translate_argument(args.first()?, ctx);
    Some(match name {
        "fromCharCode" => {
            parse_quote!(char::from_u32(#n as u32).map(|c| c.to_string()).unwrap_or_default())
        }
        _ => return None,
    })
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
        // A malformed string yields NaN in TS, never a throw — `unwrap_or`
        // matches that without a runtime panic.
        "parseInt" | "parseFloat" => {
            let a = translate_argument(args.first()?, ctx);
            parse_quote!(#a.trim().parse::<f64>().unwrap_or(f64::NAN))
        }
        // `Number(s)` parses a string; `Number(n)` passes a number through.
        "Number" => {
            let a = args.first()?;
            let e = translate_argument(a, ctx);
            if matches!(a, Argument::StringLiteral(_)) || ident_string_local(a, ctx) {
                parse_quote!(#e.trim().parse::<f64>().unwrap_or(f64::NAN))
            } else {
                e
            }
        }
        // `Boolean(x)` → the Rust truthiness of `x` (see `bool_cast`).
        "Boolean" => bool_cast(args.first()?, ctx),
        _ => return None,
    })
}

/// `Boolean(x)` → the Rust truthiness of `x`. A literal folds at compile time
/// when possible: a number (`0`/`NaN` → `false`, else `true`), a string
/// (`!is_empty()`), `true`/`false` to itself. An identifier dispatches on its
/// known type: a `Vec`/`HashMap`/`String` → `!is_empty()`, an `Option` →
/// `is_some()`, a `bool` → itself, anything else (an `f64`) → `!= 0.0`. An
/// expression of unknown type falls back to `!= 0.0` (TS `Boolean` is most
/// often applied to numbers).
fn bool_cast(arg: &Argument, ctx: &Ctx<'_>) -> Expr {
    match arg {
        Argument::BooleanLiteral(b) => bool_expr(b.value),
        Argument::NumericLiteral(n) => bool_expr(n.value != 0.0 && !n.value.is_nan()),
        Argument::StringLiteral(s) => {
            let e = string_expr(s);
            parse_quote!(!#e.is_empty())
        }
        Argument::Identifier(id) => {
            let name = bindings::snake(&id.name);
            let last = ctx
                .local_type(&name.to_string())
                .and_then(|p| p.segments.last())
                .map(|s| s.ident.to_string());
            match last.as_deref() {
                Some("Vec") | Some("HashMap") | Some("String") => parse_quote!(!#name.is_empty()),
                Some("Option") => parse_quote!(#name.is_some()),
                Some("bool") => parse_quote!(#name),
                _ => parse_quote!(#name != 0.0),
            }
        }
        _ => {
            let e = translate_argument(arg, ctx);
            parse_quote!(#e != 0.0)
        }
    }
}

/// True when `arg` is an identifier bound to a `string` local.
fn ident_string_local(arg: &Argument, ctx: &Ctx<'_>) -> bool {
    let Argument::Identifier(id) = arg else { return false };
    let name = bindings::snake(&id.name).to_string();
    ctx.local_type(&name).is_some_and(|p| p.is_ident("String"))
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
