//! String methods on a `.ds` string. Mirrors
//! `test/built-ins/String/prototype/` plus `String.<static>`.

use oxc_ast::ast::{Argument, Expression, StaticMemberExpression};
use proc_macro2::Span;
use syn::{parse_quote, Expr};

use super::super::context::Ctx;
use super::super::expressions::{translate_argument, translate_expr};
use super::{str_method_arg, usize_arg};

/// A string method's receiver. A string literal stays a bare `&str` — every
/// `str` method is `&self`, so the literal needs no `.to_string()` (which would
/// trip clippy `unnecessary_to_string`). Any other receiver (a `String` local,
/// a call result) is translated as-is.
fn str_receiver(obj: &Expression, ctx: &Ctx<'_>) -> Expr {
    if let Expression::StringLiteral(s) = obj {
        let lit = syn::LitStr::new(s.value.as_str(), Span::call_site());
        parse_quote!(#lit)
    } else {
        translate_expr(obj, ctx)
    }
}

/// String methods whose arguments need adapting to Rust's `&str`-oriented API:
/// `includes`/`startsWith`/`endsWith` → `contains`/`starts_with`/`ends_with`;
/// `replace` → `replacen(.., 1)` (TS replaces the first match only); `repeat`
/// → `repeat(n as usize)`. Returns `None` for unmapped names.
pub(in crate::translator) fn string_method(
    sm: &StaticMemberExpression,
    args: &[Argument],
    ctx: &Ctx<'_>,
) -> Option<Expr> {
    let obj = str_receiver(&sm.object, ctx);
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
        // `.charCodeAt(i)` / `.codePointAt(i)` → the `i`-th char's code point as
        // `f64` (TS returns `NaN` out of range; UTF-16 vs Rust's `char` differ
        // for non-BMP — Rust's `char` is already a Unicode scalar, so the two TS
        // methods lower to the same `chars().nth().as u32`).
        "charCodeAt" | "codePointAt" => {
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
        // `.concat(a, b, …)` → `format!("{s}{a}{b}…")` (Rust `+` needs `&str`).
        "concat" => {
            let mut fmt = String::from("{}");
            let mut parts: Vec<Expr> = Vec::new();
            for arg in args {
                fmt.push_str("{}");
                parts.push(str_method_arg(arg, ctx));
            }
            let fmt_lit = syn::LitStr::new(&fmt, Span::call_site());
            parse_quote!(format!(#fmt_lit, #obj, #(#parts),*))
        }
        // `.at(i)` → the i-th char as a String ("" if out of range). TS allows
        // a negative `i` to count from the end; `obj` is bound once so it isn't
        // re-evaluated for the length lookup.
        "at" => {
            let i = translate_argument(args.first()?, ctx);
            parse_quote!({
                let __s = #obj;
                let __n = (#i) as i64;
                let __idx = if __n >= 0 {
                    __n as usize
                } else {
                    (__s.chars().count() as i64 + __n) as usize
                };
                __s.chars().nth(__idx).map(|c| c.to_string()).unwrap_or_default()
            })
        }
        // `.lastIndexOf(s)` → last byte offset of `s`, or -1 (`rfind`).
        "lastIndexOf" => {
            let needle = str_method_arg(args.first()?, ctx);
            parse_quote!(#obj.rfind(#needle).map(|b| b as f64).unwrap_or(-1.0))
        }
        // `.valueOf()` → the string itself (a String identity).
        "valueOf" if args.is_empty() => parse_quote!(#obj),
        // `.isWellFormed()` → `true` (a Rust `&str`/`String` is always valid
        // UTF-8, so it is always well-formed — lone surrogates can't occur).
        "isWellFormed" if args.is_empty() => parse_quote!(true),
        // `.toWellFormed()` → the string unchanged (already well-formed).
        "toWellFormed" if args.is_empty() => parse_quote!(#obj.to_string()),
        // NOTE: `toLocaleLowerCase`/`toLocaleUpperCase`/`toLocaleString` are
        // intentionally NOT mapped — they are locale-dependent (thousands
        // separators, Turkish İ) and Rust has no locale, so any default would
        // silently change a program's output. They fall through to a plain
        // call, which `cargo check` rejects, surfacing the gap honestly.
        _ => return None,
    })
}

/// `String.<m>(…)`: `fromCharCode(n)`/`fromCodePoint(n)` → a one-char
/// `String` from the code point (or `""` if `n` isn't a valid `char`). Rust's
/// `char` is a Unicode scalar value, so the two TS methods lower identically
/// (`fromCharCode`'s UTF-16 surrogate distinction doesn't arise). Returns
/// `None` otherwise.
pub(in crate::translator) fn string_static(
    name: &str,
    args: &[Argument],
    ctx: &Ctx<'_>,
) -> Option<Expr> {
    let n = translate_argument(args.first()?, ctx);
    Some(match name {
        "fromCharCode" | "fromCodePoint" => {
            parse_quote!(char::from_u32(#n as u32).map(|c| c.to_string()).unwrap_or_default())
        }
        _ => return None,
    })
}
