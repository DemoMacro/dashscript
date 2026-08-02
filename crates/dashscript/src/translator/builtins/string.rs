//! String methods on a `.ts` string. Mirrors
//! `test/built-ins/String/prototype/` plus `String.<static>`.

use oxc_ast::ast::{Argument, Expression, StaticMemberExpression};
use proc_macro2::Span;
use syn::{parse_quote, Expr};

use super::super::context::Ctx;
use super::super::expressions::{
    array_elem_arg, option_unwrap_object, regex_lit_parts, translate_argument, translate_expr,
};
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
        // An `Option<String>` receiver is treated as non-null (the TS type plus
        // a `.method()` call imply a prior null guard) — unwrap so a `&self`
        // str method lands on the inner `String`. A narrowed-some local keeps
        // its own read form; anything else translates as-is.
        option_unwrap_object(obj, ctx).unwrap_or_else(|| translate_expr(obj, ctx))
    }
}

/// The chars ES `String.prototype.trim` strips: WhiteSpace + LineTerminator
/// (ECMA-262 §7.2, §7.3). Rust's `char::is_whitespace` omits U+FEFF (ES
/// WhiteSpace) and includes U+0085 NEL (ES does not trim), so emit the set
/// explicitly for `trim`/`trimStart`/`trimEnd`.
fn es_trim_chars() -> Expr {
    parse_quote!(|c: char| matches!(
        c,
        '\u{9}'
            | '\u{A}'
            | '\u{B}'
            | '\u{C}'
            | '\u{D}'
            | '\u{20}'
            | '\u{A0}'
            | '\u{FEFF}'
            | '\u{1680}'
            | '\u{2000}'
            ..='\u{200A}' | '\u{2028}' | '\u{2029}' | '\u{202F}' | '\u{205F}' | '\u{3000}'
    ))
}

/// True if `arg` is a replacement string whose ES `GetSubstitution` `$` patterns
/// (`$$`/`$&`/`` $` ``/`$'`) need expanding: a string literal containing `$`, or
/// a non-literal (whose runtime value may contain `$`). A `$`-free literal is
/// the fast path — Rust's `str::replace`/`replacen` treat `repl` literally,
/// which is exactly ES semantics when no `$` is present.
fn repl_needs_substitution(arg: &Argument) -> bool {
    match arg {
        Argument::StringLiteral(s) => s.value.as_str().contains('$'),
        _ => true,
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
    string_method_on(obj, &sm.property.name, args, ctx)
}

/// A string method applied to a given receiver expression. Shared by
/// `s.method(...)` and the `String.prototype.method.call(r)` idiom — the JS
/// pattern of borrowing a prototype method via `.call` lowers to
/// `String(r).method(...)` (the receiver is ToString-coerced first).
pub(in crate::translator) fn string_method_on(
    obj: Expr,
    name: &str,
    args: &[Argument],
    ctx: &Ctx<'_>,
) -> Option<Expr> {
    Some(match name {
        // `.trim()`/`.trimStart()`/`.trimEnd()` strip ES WhiteSpace +
        // LineTerminator (§7.2/§7.3). Rust's `char::is_whitespace` differs
        // (omits U+FEFF, includes U+0085 NEL), so emit the explicit char set.
        "trim" => {
            let ws = es_trim_chars();
            parse_quote!(#obj.trim_matches(#ws))
        }
        "trimStart" => {
            let ws = es_trim_chars();
            parse_quote!(#obj.trim_start_matches(#ws))
        }
        "trimEnd" => {
            let ws = es_trim_chars();
            parse_quote!(#obj.trim_end_matches(#ws))
        }
        // `.includes(s)` / `.includes(s, pos)` — search from byte offset `pos`
        // (a negative `pos` is clamped to 0, an over-length one to `len`;
        // ASCII matches TS char index). `pos` is taken as `f64` then routed
        // through `i64` so a negative value survives (a direct `as usize`
        // would wrap it to a huge offset).
        "includes" => {
            let a = str_method_arg(args.first()?, ctx);
            match args.get(1) {
                Some(pos) => {
                    let p = array_elem_arg(pos, ctx);
                    // `.get` borrows the receiver (`&self`), so a `String` local
                    // isn't moved — later reads in the same fixture still work.
                    parse_quote!({
                        let __p = ((#p) as i64).max(0) as usize;
                        let __p = __p.min((#obj).len());
                        (#obj).get(__p..).is_some_and(|t| t.contains(#a))
                    })
                }
                None => parse_quote!(#obj.contains(#a)),
            }
        }
        // `.startsWith(s)` / `.startsWith(s, pos)` — match begins at `pos`.
        "startsWith" => {
            let a = str_method_arg(args.first()?, ctx);
            match args.get(1) {
                Some(pos) => {
                    let p = array_elem_arg(pos, ctx);
                    parse_quote!({
                        let __p = ((#p) as i64).max(0) as usize;
                        let __p = __p.min((#obj).len());
                        (#obj).get(__p..).is_some_and(|t| t.starts_with(#a))
                    })
                }
                None => parse_quote!(#obj.starts_with(#a)),
            }
        }
        // `.endsWith(s)` / `.endsWith(s, endPos)` — only the first `endPos`
        // bytes are considered (clamped to `[0, len]`).
        "endsWith" => {
            let a = str_method_arg(args.first()?, ctx);
            match args.get(1) {
                Some(pos) => {
                    let p = array_elem_arg(pos, ctx);
                    parse_quote!({
                        let __p = ((#p) as i64).max(0) as usize;
                        let __p = __p.min((#obj).len());
                        (#obj).get(..__p).is_some_and(|t| t.ends_with(#a))
                    })
                }
                None => parse_quote!(#obj.ends_with(#a)),
            }
        }
        // `.match(/pat/)` (non-global) → `Option<DsMatch>` (ES: the match or
        // `null`). Only a regex-literal argument is lowered; a non-regex arg
        // falls through (returns None, leaving the call unmapped).
        "match" => {
            let Argument::RegExpLiteral(re) = args.first()? else {
                return None;
            };
            let (pat, fl) = regex_lit_parts(re);
            // `obj` is a bare `&str` literal or a `String` expr; pass as `&str`.
            let text: Expr = if matches!(
                &obj,
                Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(_),
                    ..
                })
            ) {
                obj
            } else {
                parse_quote!(#obj.as_str())
            };
            parse_quote!(crate::__ds::regex_match(#pat, #fl, #text))
        }
        // `.search(/pat/)` → the byte index of the first match, or -1. Only a
        // regex-literal argument is lowered; a non-regex arg falls through.
        "search" => {
            let Argument::RegExpLiteral(re) = args.first()? else {
                return None;
            };
            let (pat, fl) = regex_lit_parts(re);
            let text: Expr = if matches!(
                &obj,
                Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(_),
                    ..
                })
            ) {
                obj
            } else {
                parse_quote!(#obj.as_str())
            };
            parse_quote!(crate::__ds::regex_search(#pat, #fl, #text))
        }
        "replace" => {
            let b = str_method_arg(args.get(1)?, ctx);
            if let Argument::RegExpLiteral(re) = args.first()? {
                // `.replace(/pat/, repl)` — the first match, or every match when
                // the regex carries the global flag (`g`); `$` patterns in `repl`
                // are expanded. `regex_replace` reads the flags itself.
                let (pat, fl) = regex_lit_parts(re);
                let text: Expr = if matches!(
                    &obj,
                    Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Str(_),
                        ..
                    })
                ) {
                    obj
                } else {
                    parse_quote!(#obj.as_str())
                };
                parse_quote!(crate::__ds::regex_replace(#pat, #fl, #text, #b))
            } else {
                let a = str_method_arg(args.first()?, ctx);
                if repl_needs_substitution(args.get(1)?) {
                    // The replacement carries an ES `$` pattern (`$$`/`$&`/
                    // `` $` ``/`$'`); Rust's `replacen` treats it literally, so
                    // route through `ds_replace`, which applies GetSubstitution.
                    let text: Expr = if matches!(
                        &obj,
                        Expr::Lit(syn::ExprLit {
                            lit: syn::Lit::Str(_),
                            ..
                        })
                    ) {
                        obj
                    } else {
                        parse_quote!(#obj.as_str())
                    };
                    parse_quote!(crate::__ds::ds_replace(#text, #a, #b))
                } else {
                    parse_quote!(#obj.replacen(#a, #b, 1))
                }
            }
        }
        // `.replaceAll(a, b)` → `replace` (all matches; TS `replace` does one).
        // A replacement with `$` routes through `ds_replace_all` (GetSubstitution
        // at each match); otherwise Rust's literal `replace` is exact.
        "replaceAll" => {
            if let Argument::RegExpLiteral(re) = args.first()? {
                // `.replaceAll(/pat/g, repl)` — replace every match. ES requires
                // a global regex (else TypeError); `regex_replace` honors `g`,
                // and `$<name>` named groups expand via `expand_replacement`.
                let (pat, fl) = regex_lit_parts(re);
                let mut flags = fl.value();
                if !flags.contains('g') {
                    flags.push('g');
                }
                let gl = syn::LitStr::new(&flags, Span::call_site());
                let b = str_method_arg(args.get(1)?, ctx);
                let text: Expr = if matches!(
                    &obj,
                    Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Str(_),
                        ..
                    })
                ) {
                    obj
                } else {
                    parse_quote!(#obj.as_str())
                };
                return Some(parse_quote!(crate::__ds::regex_replace(#pat, #gl, #text, #b)));
            }
            let a = str_method_arg(args.first()?, ctx);
            let b_arg = args.get(1)?;
            let b = str_method_arg(b_arg, ctx);
            if repl_needs_substitution(b_arg) {
                let text: Expr = if matches!(
                    &obj,
                    Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Str(_),
                        ..
                    })
                ) {
                    obj
                } else {
                    parse_quote!(#obj.as_str())
                };
                parse_quote!(crate::__ds::ds_replace_all(#text, #a, #b))
            } else {
                parse_quote!(#obj.replace(#a, #b))
            }
        }
        "repeat" => {
            let n = usize_arg(args.first()?, ctx);
            parse_quote!(#obj.repeat(#n))
        }
        "split" => {
            // `.split(/pat/, limit?)` — split on regex matches. A string
            // separator stays on the str `split` path below.
            if let Argument::RegExpLiteral(re) = args.first()? {
                let (pat, fl) = regex_lit_parts(re);
                let text: Expr = if matches!(
                    &obj,
                    Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Str(_),
                        ..
                    })
                ) {
                    obj
                } else {
                    parse_quote!(#obj.as_str())
                };
                return Some(match args.get(1) {
                    Some(lim) => {
                        let l = usize_arg(lim, ctx);
                        parse_quote!(crate::__ds::regex_split(#pat, #fl, #text, Some(#l)))
                    }
                    None => parse_quote!(crate::__ds::regex_split(#pat, #fl, #text, None)),
                });
            }
            // `split` yields `&str`; map to owned so the result is `Vec<String>`.
            // A `limit` caps the segment count (TS `split(sep, n)`).
            let delim = str_method_arg(args.first()?, ctx);
            match args.get(1) {
                Some(lim) => {
                    let l = usize_arg(lim, ctx);
                    parse_quote!(#obj.split(#delim).take(#l).map(|part| part.to_string()).collect::<Vec<String>>())
                }
                None => {
                    parse_quote!(#obj.split(#delim).map(|part| part.to_string()).collect::<Vec<String>>())
                }
            }
        }
        // `.indexOf(s)` → byte offset (ASCII == char index), or -1.
        // `.indexOf(s, from)` starts searching at byte offset `from` (a negative
        // `from` is clamped to 0, as in TS). `from` routes through `i64` first
        // so a negative value is preserved (a direct `as usize` would wrap it).
        "indexOf" => {
            let needle = str_method_arg(args.first()?, ctx);
            match args.get(1) {
                Some(from) => {
                    let f = array_elem_arg(from, ctx);
                    parse_quote!({
                        let __from = ((#f) as i64).max(0) as usize;
                        (#obj).get(__from..).and_then(|t| t.find(#needle)).map(|b| (b + __from) as f64).unwrap_or(-1_f64)
                    })
                }
                None => parse_quote!(#obj.find(#needle).map(|b| b as f64).unwrap_or(-1_f64)),
            }
        }
        // `.slice(a, b)` → byte slice. TS `slice` treats a negative index as an
        // offset from the end (clamped to 0); `a..b` order is preserved (empty
        // when a >= b). ASCII byte offsets match char indices. The receiver is
        // bound once so it is not re-evaluated for the length and the index.
        "slice" => {
            let start = array_elem_arg(args.first()?, ctx);
            match args.get(1) {
                Some(end) => {
                    let end = array_elem_arg(end, ctx);
                    parse_quote!({
                        let __s = &(#obj);
                        let __n = __s.len() as f64;
                        let __a = { let v = #start; if v < 0_f64 { (__n + v).max(0_f64) } else { v.min(__n) } } as usize;
                        let __b = { let v = #end; if v < 0_f64 { (__n + v).max(0_f64) } else { v.min(__n) } } as usize;
                        if __a < __b { __s.get(__a..__b).unwrap_or("").to_string() } else { String::new() }
                    })
                }
                None => parse_quote!({
                    let __s = &(#obj);
                    let __n = __s.len() as f64;
                    let __a = { let v = #start; if v < 0_f64 { (__n + v).max(0_f64) } else { v.min(__n) } } as usize;
                    __s.get(__a..).unwrap_or("").to_string()
                }),
            }
        }
        // `.substring(a, b)` → byte slice. TS `substring` maps negatives to 0,
        // clamps to `[0, len]`, and swaps the bounds when `a > b` (unlike slice,
        // which never swaps). ASCII byte offsets match char indices.
        "substring" => {
            let start = array_elem_arg(args.first()?, ctx);
            match args.get(1) {
                Some(end) => {
                    let end = array_elem_arg(end, ctx);
                    parse_quote!({
                        let __s = &(#obj);
                        let __n = __s.len() as f64;
                        let mut __a = { let v = #start; if v < 0_f64 { 0_f64 } else { v }.min(__n) } as usize;
                        let mut __b = { let v = #end; if v < 0_f64 { 0_f64 } else { v }.min(__n) } as usize;
                        if __a > __b {
                            ::core::mem::swap(&mut __a, &mut __b);
                        }
                        __s.get(__a..__b).unwrap_or("").to_string()
                    })
                }
                None => parse_quote!({
                    let __s = &(#obj);
                    let __n = __s.len() as f64;
                    let __a = { let v = #start; if v < 0_f64 { 0_f64 } else { v }.min(__n) } as usize;
                    __s.get(__a..).unwrap_or("").to_string()
                }),
            }
        }
        // `.charAt(i)` → the `i`-th char as a `String` ("" if out of range).
        // ASCII fast path indexes raw bytes in O(1) (a byte is a char); the
        // `is_ascii` scan is SIMD and cheap per call, and ASCII covers the
        // common hot loop (scanners, parsers). Non-ASCII keeps `chars().nth`.
        "charAt" => {
            let i = usize_arg(args.first()?, ctx);
            parse_quote!({
                let __s = &(#obj);
                let __i = #i;
                if __s.is_ascii() {
                    match __s.as_bytes().get(__i) {
                        Some(&__b) => (__b as char).to_string(),
                        None => ::std::string::String::new(),
                    }
                } else {
                    __s.chars().nth(__i).map(|c| c.to_string()).unwrap_or_default()
                }
            })
        }
        // `.charCodeAt(i)` → the `i`-th UTF-16 code unit as `f64` (`NaN` out of
        // range). The ASCII fast path indexes raw bytes in O(1) — `is_ascii` is a
        // SIMD scan, cheap enough to run per call, and ASCII bytes are identical
        // to UTF-16 units. Non-ASCII encodes to UTF-16 first, because ES indexes
        // code units: a non-BMP character is a surrogate *pair*, so
        // `charCodeAt(0)` of "𝌆" is 0xD834 (the high surrogate), not the code
        // point — `chars().nth` would wrongly return the scalar (0x1D306). This
        // is the hot loop of bit-vector string algorithms (Myers–Levenshtein),
        // whose peq tables are pure ASCII.
        "charCodeAt" => {
            let i = usize_arg(args.first()?, ctx);
            parse_quote!({
                let __s = &#obj;
                let __i = #i;
                if __s.is_ascii() {
                    __s.as_bytes()
                        .get(__i)
                        .map(|&b| b as f64)
                        .unwrap_or(f64::NAN)
                } else {
                    __s.encode_utf16()
                        .nth(__i)
                        .map(|u| u as f64)
                        .unwrap_or(f64::NAN)
                }
            })
        }
        // `.codePointAt(i)` → the code point at UTF-16 index `i`, merging a
        // lead/trail surrogate pair into its code point (0x10000+). ES indexes
        // UTF-16 code units, so Rust's `chars().nth` (scalar values) is wrong:
        // `\uD800\uDBFF`.codePointAt(0) yields the lead surrogate 0xD800, not
        // the replacement char a UTF-32 read would give. ASCII fast path is
        // O(1); non-ASCII iterates `encode_utf16()` without materializing a
        // `Vec<u16>` (the trail unit is re-derived only in the rare surrogate
        // case) — so a hot loop pays zero per-call allocation.
        "codePointAt" => {
            let i = translate_argument(args.first()?, ctx);
            parse_quote!({
                let __s = &(#obj);
                // ES: position < 0 or >= length → undefined. Keep the index as
                // f64 so a negative literal (e.g. -1) is not saturated to 0 by
                // `as usize` (Rust 1.45+ saturating float→int cast).
                let __idx: f64 = #i;
                if __idx < 0.0 {
                    None
                } else {
                    let __i = __idx as usize;
                    if __s.is_ascii() {
                        __s.as_bytes().get(__i).map(|&b| b as f64)
                    } else {
                        __s.encode_utf16()
                            .nth(__i)
                            .map(|__c1| {
                                let __c1 = __c1 as u32;
                                if (0xD800..=0xDBFF).contains(&__c1) {
                                    if let Some(__c2) = __s.encode_utf16().nth(__i + 1) {
                                        let __c2 = __c2 as u32;
                                        if (0xDC00..=0xDFFF).contains(&__c2) {
                                            return (0x10000
                                                + ((__c1 - 0xD800) << 10)
                                                + (__c2 - 0xDC00))
                                                as f64;
                                        }
                                    }
                                }
                                __c1 as f64
                            })
                    }
                }
            })
        }
        // `.padStart(n)` → right-align to width `n` (fills on the left with
        // spaces, TS's default pad). `.padStart(n, undefined)` is the same — an
        // undefined fill falls back to the space default.
        "padStart"
            if args.len() <= 1
                || matches!(args.get(1), Some(Argument::Identifier(id)) if id.name.as_str() == "undefined") =>
        {
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
                let __s = &(#obj);
                let __need = (#n).saturating_sub(__s.chars().count());
                format!("{}{}", #ch.chars().cycle().take(__need).collect::<String>(), __s)
            })
        }
        // `.padEnd(n)` → left-align to width `n` (fills on the right). An
        // undefined fill (`padEnd(n, undefined)`) uses the space default too.
        "padEnd"
            if args.len() <= 1
                || matches!(args.get(1), Some(Argument::Identifier(id)) if id.name.as_str() == "undefined") =>
        {
            let n = usize_arg(args.first()?, ctx);
            parse_quote!(format!("{:<1$}", #obj, #n))
        }
        // `.padEnd(n, ch)` → fill the right with `ch` (cycled) to width `n`.
        "padEnd" if args.len() >= 2 => {
            let n = usize_arg(args.first()?, ctx);
            let ch = str_method_arg(args.get(1)?, ctx);
            parse_quote!({
                let __s = &(#obj);
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
            let i = array_elem_arg(args.first()?, ctx);
            parse_quote!({
                let __s = &(#obj);
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
            parse_quote!(#obj.rfind(#needle).map(|b| b as f64).unwrap_or(-1_f64))
        }
        // `.valueOf()` → the string itself (a String identity).
        "valueOf" if args.is_empty() => parse_quote!(#obj),
        // `.isWellFormed()` → `true` (a Rust `&str`/`String` is always valid
        // UTF-8, so it is always well-formed — lone surrogates can't occur).
        "isWellFormed" if args.is_empty() => parse_quote!(true),
        // `.toWellFormed()` → the string unchanged (already well-formed).
        "toWellFormed" if args.is_empty() => parse_quote!(#obj.to_string()),
        // `toLocaleLowerCase`/`toLocaleUpperCase` are NOT handled here — a
        // locale-less `s.toLocaleUpperCase()` falls through to
        // `mod.rs::map_method`, which lowers it to the default casing (per
        // ECMA-262 §22.1.3 a locale-less `toLocale*` ≡ `toUpperCase`/
        // `toLowerCase`). A locale argument is intercepted by `check` (no ICU
        // locale table). `toLocaleString` has no mapping, so `cargo check`
        // rejects it honestly.
        _ => return None,
    })
}

/// `String.<m>(…)`: `fromCharCode(n…)`/`fromCodePoint(n…)` → a `String` built
/// from every code-point argument (`fromCodePoint(65, 90)` === `"AZ"`). Rust's
/// `char` is a Unicode scalar value, so valid code points map directly; lone
/// surrogates (invalid in Rust's UTF-8) fall back to U+FFFD — the closest a
/// Rust `String` can get to ES's permissive UTF-16. Returns `None` for other
/// static names.
pub(in crate::translator) fn string_static(
    name: &str,
    args: &[Argument],
    ctx: &Ctx<'_>,
) -> Option<Expr> {
    Some(match name {
        "fromCharCode" | "fromCodePoint" => {
            let parts: Vec<Expr> = args
                .iter()
                .map(|a| {
                    let n = translate_argument(a, ctx);
                    parse_quote!(char::from_u32((#n) as u32).unwrap_or('\u{FFFD}'))
                })
                .collect();
            parse_quote!([#(#parts),*].into_iter().collect::<String>())
        }
        _ => return None,
    })
}
