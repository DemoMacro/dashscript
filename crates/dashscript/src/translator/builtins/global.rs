//! Global conversion functions called as plain identifiers (`String(x)`,
//! `parseInt(s)`, `Number(s)`, `Boolean(x)`, `isNaN`/`isFinite`). These are ES
//! globals — not a `Math`/`Object` member — so they live here rather than under
//! a named built-in file.

use oxc_ast::ast::{
    Argument, Expression, IdentifierReference, ObjectExpression, ObjectPropertyKind, PropertyKey,
};
use proc_macro2::Span;
use syn::{parse_quote, Expr};

use super::super::bindings;
use super::super::context::Ctx;
use super::super::expressions::{
    bool_expr, is_number_arg, is_request_local, regex_lit_parts, string_expr, translate_argument,
    translate_expr, translate_number_to,
};
use super::super::flavor::NumberFlavor;

/// Global conversion functions called as plain identifiers: `String(x)` →
/// `format!("{}", x)`; `parseInt(s)`/`parseFloat(s)` → `s.trim().parse::<f64>()`
/// (`.ts` `number` is `f64`, so both share one parse path). Returns `None` for
/// any other name (falls through to a plain call).
pub(in crate::translator) fn global_function(
    id: &IdentifierReference,
    args: &[Argument],
    ctx: &Ctx<'_>,
) -> Option<Expr> {
    if let Some(expr) = super::web::timer_function(id, args, ctx) {
        return Some(expr);
    }
    let name: &str = &id.name;
    Some(match name {
        "String" => {
            let a = args.first()?;
            // `String(null)` → "null", `String(undefined)` → "undefined" — both
            // lower to Rust `None`, whose `Display` is "None" (not what TS
            // prints). Other values go through `format!`, like the `.call`
            // idiom's `to_string_expr`.
            match a {
                Argument::NullLiteral(_) => parse_quote!("null".to_string()),
                Argument::Identifier(id) if id.name.as_str() == "undefined" => {
                    parse_quote!("undefined".to_string())
                }
                _ => {
                    // `String(<number>)` is ES NumberToString — route through the
                    // helper (coerced to `f64` so a flavor-promoted `i64` arg
                    // compiles); other values use `format!` (Rust `Display`
                    // already matches ES for string/bool).
                    if is_number_arg(a, ctx) {
                        let e = translate_number_to(a.as_expression()?, NumberFlavor::F64, ctx);
                        parse_quote!(crate::__ds::number_to_string(#e))
                    } else {
                        let e = translate_argument(a, ctx);
                        parse_quote!(::std::format!("{}", #e))
                    }
                }
            }
        }
        // `parseFloat(s)` — full ES semantics: longest valid decimal-literal
        // prefix (truncation), `±Infinity`, NaN if none. See `parse_float_expr`.
        "parseFloat" => parse_float_expr(es_to_string_arg(args.first()?, ctx)),
        // `parseInt(s[, radix])` — full ES semantics: trim, sign, `0x`
        // auto-detect (radix 0/16), truncate at the first non-digit. See
        // `parse_int_expr`.
        "parseInt" => parse_int_expr(
            es_to_string_arg(args.first()?, ctx),
            args.get(1).map(|r| translate_argument(r, ctx)),
        ),
        // `Number(x)` is the ES ToNumber coercion (§7.1.3). A string runs
        // StringToNumber (§7.1.4.1): empty/whitespace → +0, else parse with
        // NaN on malformed; `true`/`false` → 1/+0; `null` → +0; a number
        // passes through. (A bare `undefined` arg would need NaN, but that
        // lowering is shaped by the surrounding type system, not this arm.)
        "Number" => {
            let a = args.first()?;
            if matches!(a, Argument::StringLiteral(_)) || ident_string_local(a, ctx) {
                to_number_expr(translate_argument(a, ctx))
            } else if let Argument::BooleanLiteral(b) = a {
                if b.value {
                    parse_quote!(1_f64)
                } else {
                    parse_quote!(0_f64)
                }
            } else if let Argument::NullLiteral(_) = a {
                parse_quote!(0_f64)
            } else {
                translate_argument(a, ctx)
            }
        }
        // `Boolean(x)` → the Rust truthiness of `x` (see `bool_cast`).
        "Boolean" => bool_cast(args.first()?, ctx),
        // `isNaN(x)` → `x.is_nan()` (DashScript's `number` is `f64`, so the TS
        // global's ToNumber coercion is already done).
        "isNaN" => {
            // `.is_nan()` is `f64`-only; coerce a flavor-promoted `i64` arg.
            let a = translate_number_to(args.first()?.as_expression()?, NumberFlavor::F64, ctx);
            parse_quote!(#a.is_nan())
        }
        // `isFinite(x)` → `x.is_finite()`.
        "isFinite" => {
            let a = translate_number_to(args.first()?.as_expression()?, NumberFlavor::F64, ctx);
            parse_quote!(#a.is_finite())
        }
        // `RegExp(pattern[, flags])` — the ES RegExp constructor (no `new`);
        // lowered to the same `__ds::regex` helper as `/pat/` literals. See
        // [`reg_exp_constructor`].
        "RegExp" => return reg_exp_constructor(args, ctx),
        // `atob(s)` / `btoa(s)` — WinterTC (Ecma TC55) base64 globals. `atob`
        // forgiving-decodes (strip ASCII whitespace, pad, base64-decode → a
        // Latin-1 string); `btoa` base64-encodes the string's ≤U+00FF code
        // units. Both coerce the arg via ES `ToString`. See `BASE64_HELPER`.
        "atob" => {
            let s = es_to_string_arg(args.first()?, ctx);
            parse_quote!(crate::__ds::b64_decode(#s))
        }
        "btoa" => {
            let s = es_to_string_arg(args.first()?, ctx);
            parse_quote!(crate::__ds::b64_encode(#s))
        }
        // `structuredClone(v)` — WinterTC deep clone. DashScript's subset
        // (primitives, plain records, arrays — all `Clone`) lowers to
        // `v.clone()`; a non-`Clone` value surfaces honestly at `cargo check`.
        "structuredClone" => {
            let v = translate_argument(args.first()?, ctx);
            parse_quote!(#v.clone())
        }
        // `reportError(e)` — WinterTC (HTML §5) global. Dispatches an `"error"`
        // event to the global `self` EventTarget (an `addEventListener("error",
        // …)` listener receives it); if no listener cancels it, writes the error
        // to stderr. Reuses the `EventTarget` runtime dep's `ds_report_error`
        // helper (no new dep — it lives in `EVENT_TARGET_HELPER`). The payload is
        // `Display`d, so an ES `Error`/`DOMException` (`DsError`) and a primitive
        // all type-check; a non-`Display` value surfaces honestly at `cargo
        // check`.
        "reportError" => {
            let e = translate_argument(args.first()?, ctx);
            parse_quote!(crate::__ds::ds_report_error(&#e))
        }
        // `fetch` — WinterTC (Ecma TC55) Web API. ES `fetch` returns
        // `Promise<Response>`; the caller's `await` supplies the `.await`.
        // Three arg shapes: `fetch(request)` (a `Request` object arg unwrapped
        // via `ds_fetch_request`), `fetch(url, init)` (a plain-object `init`
        // → `ds_fetch_with`), and `fetch(url)` (→ `ds_fetch`). See
        // `fetch_request_arg` / `fetch_init` / `DS_FETCH_HELPER`.
        "fetch" => {
            // `fetch(request)` — a `Request` object arg unwraps via
            // `ds_fetch_request` (the same method/body/headers path as
            // `fetch(url, init)`); a string URL or a `(url, init)` pair falls
            // through to the paths below.
            if let Some(req) = fetch_request_arg(args.first()?, ctx) {
                req
            } else {
                let url = translate_argument(args.first()?, ctx);
                // `fetch(url, init)` — a plain object `init` lowers to
                // `ds_fetch_with` (method/body/headers); anything else (no
                // second arg, a non-literal) stays the GET `ds_fetch` path.
                match args.get(1) {
                    Some(Argument::ObjectExpression(obj)) => {
                        let (method, body, headers) = fetch_init(obj, ctx);
                        parse_quote!(crate::__ds::ds_fetch_with(#url, #method, #body, #headers))
                    }
                    _ => parse_quote!(crate::__ds::ds_fetch(#url)),
                }
            }
        }
        _ => return None,
    })
}

/// `fetch(request)` when arg0 is a `DsRequest` local → `ds_fetch_request(&req)`
/// (the same method/body/headers network path as `fetch(url, init)`, unwrapped
/// from the `Request` object `new Request(…)` built). `None` for any other arg0
/// shape (a string URL, etc.) so the `fetch(url)` / `fetch(url, init)` path runs.
fn fetch_request_arg(arg: &Argument, ctx: &Ctx<'_>) -> Option<Expr> {
    let e = arg.as_expression()?;
    if !is_request_local(e, ctx) {
        return None;
    }
    let v = translate_argument(arg, ctx);
    Some(parse_quote!(crate::__ds::ds_fetch_request(&#v)))
}

/// `fetch(url, init)`'s second argument — the ES `init` object — lowered to the
/// `(method, body, headers)` triple `ds_fetch_with` takes. `method` defaults to
/// `"GET"`; `body` to `None`; `headers` (only a plain object literal here) to an
/// empty list. Each header name/value is ES ToString-coerced (`.to_string()`)
/// the way `fetch` itself stringifies before sending.
pub(in crate::translator) fn fetch_init(
    obj: &ObjectExpression<'_>,
    ctx: &Ctx<'_>,
) -> (Expr, Expr, Expr) {
    let mut method: Option<Expr> = None;
    let mut body: Option<Expr> = None;
    let mut header_pairs: Vec<Expr> = Vec::new();
    for prop in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(op) = prop else {
            continue;
        };
        let Some(name) = init_key_name(&op.key) else {
            continue;
        };
        match name.as_str() {
            "method" => method = Some(translate_expr(&op.value, ctx)),
            "body" => body = Some(translate_expr(&op.value, ctx)),
            "headers" => {
                if let Expression::ObjectExpression(ho) = &op.value {
                    for hp in &ho.properties {
                        let ObjectPropertyKind::ObjectProperty(hop) = hp else {
                            continue;
                        };
                        let Some(k) = init_key_name(&hop.key) else {
                            continue;
                        };
                        let v = translate_expr(&hop.value, ctx);
                        let k_lit = syn::LitStr::new(&k, Span::call_site());
                        header_pairs.push(parse_quote!((#k_lit.to_string(), (#v).to_string())));
                    }
                }
            }
            _ => {}
        }
    }
    let method = method.unwrap_or_else(|| parse_quote!("GET".to_string()));
    let body = body
        .map(|b| parse_quote!(::std::option::Option::Some(#b)))
        .unwrap_or_else(|| parse_quote!(::std::option::Option::None));
    let headers = parse_quote!(::std::vec![#(#header_pairs),*]);
    (method, body, headers)
}

/// An `init` object's property key as a string — a static identifier (`method`)
/// or a string literal (`"method"`); computed/shorthand keys have no static
/// name here. Shared by `fetch_init` and the `Response` ctor's init parser.
pub(in crate::translator) fn init_key_name(key: &PropertyKey<'_>) -> Option<String> {
    match key {
        PropertyKey::StaticIdentifier(id) => Some(id.name.to_string()),
        PropertyKey::StringLiteral(s) => Some(s.value.to_string()),
        _ => None,
    }
}

/// ES ToNumber applied to a string (§7.1.4.1): trim; empty → +0; otherwise a
/// signed StrDecimalLiteral (Rust's `parse` covers decimal, `Infinity`, NaN)
/// or a `0x`/`0b`/`0o` integer literal Rust's parse rejects —
/// `Number("0xff")` is 255, not NaN. Shared by `Number(string)` and the unary
/// `+` operator, both of which run ToNumber on a string.
pub(in crate::translator) fn to_number_expr(e: Expr) -> Expr {
    parse_quote!({
        // Bind the (possibly temporary) string first: `#e` may be
        // `"x".to_string()`, and `.trim()` would borrow a value freed at the
        // end of that expression (E0716). Borrow rather than move so a `let n
        // = Number(value)` does not consume `value` for later reads (the
        // enclosing function's `String(n) === value` would otherwise be a
        // borrow-of-moved). `let __s = &#e` extends a temporary's lifetime and
        // borrows an lvalue, so both shapes compile.
        let __s = &#e;
        let __t = __s.trim();
        if __t.is_empty() {
            0_f64
        } else {
            // ES StringToNumber (§7.1.4.1): a `0x`/`0b`/`0o` integer literal
            // is unsigned — a leading `+`/`-` makes the whole string a signed
            // StrDecimalLiteral, never a radix literal. So `Number("+0b1")` /
            // `Number("-0xff")` are NaN, not 1 / -255. Track whether a sign was
            // stripped and only honor a radix prefix when none was.
            let (__sign, __body, __signed): (f64, &str, bool) =
                if let Some(__r) = __t.strip_prefix('-') {
                    (-1_f64, __r, true)
                } else if let Some(__r) = __t.strip_prefix('+') {
                    (1_f64, __r, true)
                } else {
                    (1_f64, __t, false)
                };
            let __radix: Option<(&str, u32)> = if __signed {
                None
            } else if let Some(__r) =
                __body.strip_prefix("0x").or_else(|| __body.strip_prefix("0X"))
            {
                Some((__r, 16))
            } else if let Some(__r) = __body
                .strip_prefix("0b")
                .or_else(|| __body.strip_prefix("0B"))
            {
                Some((__r, 2))
            } else if let Some(__r) = __body
                .strip_prefix("0o")
                .or_else(|| __body.strip_prefix("0O"))
            {
                Some((__r, 8))
            } else {
                None
            };
            let __val = match __radix {
                Some((__rest, __r)) => {
                    let mut __v: f64 = 0_f64;
                    let mut __ok = !__rest.is_empty();
                    for __c in __rest.chars() {
                        match __c.to_digit(__r) {
                            Some(__d) => __v = __v * __r as f64 + __d as f64,
                            None => {
                                __ok = false;
                                break;
                            }
                        }
                    }
                    if __ok { __v } else { f64::NAN }
                }
                None => __body.parse::<f64>().unwrap_or(f64::NAN),
            };
            __sign * __val
        }
    })
}

/// True when `arg` is an identifier bound to a `string` local.
fn ident_string_local(arg: &Argument, ctx: &Ctx<'_>) -> bool {
    let Argument::Identifier(id) = arg else {
        return false;
    };
    let name = bindings::snake(&id.name).to_string();
    ctx.local_type(&name).is_some_and(|p| p.is_ident("String"))
}

/// `Boolean(x)` → the Rust truthiness of `x`. A literal folds at compile time
/// when possible: a number (`0`/`NaN` → `false`, else `true`), a string
/// (`!is_empty()`), `true`/`false` to itself. An identifier dispatches on its
/// known type: a `Vec`/`HashMap`/`String` → `!is_empty()`, an `Option` →
/// `is_some()`, a `bool` → itself, a number → `!= 0`. An expression of unknown
/// type falls back to `__ds::truthy` — the compiler picks the impl.
fn bool_cast(arg: &Argument, ctx: &Ctx<'_>) -> Expr {
    match arg {
        Argument::BooleanLiteral(b) => bool_expr(b.value),
        Argument::NumericLiteral(n) => bool_expr(n.value != 0_f64 && !n.value.is_nan()),
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
                Some("f64") => parse_quote!(#name != 0_f64),
                Some(
                    "i64" | "i32" | "i16" | "i8" | "isize" | "usize" | "u64" | "u32" | "u16" | "u8",
                ) => parse_quote!(#name != 0),
                _ => parse_quote!(crate::__ds::truthy(&#name)),
            }
        }
        _ => {
            let e = translate_argument(arg, ctx);
            parse_quote!(crate::__ds::truthy(&#e))
        }
    }
}

/// `parseInt(s[, radix])` — full ES semantics (ECMA-262 §19.2.5): trim leading
/// whitespace, parse a sign, auto-detect a `0x`/`0X` prefix (radix 0 or 16),
/// and truncate at the first character that is not a digit in the radix (NOT a
/// whole-string parse — `parseInt("12ab")` is `12`, not `NaN`). A radix outside
/// `[2, 36]` yields `NaN`. Inlined as a closure so each call site is
/// self-contained (a top-level `fn` would clash when two `parseInt` calls share
/// one translated scope).
/// ES `ToString` for a `parseInt`/`parseFloat` argument, so the parse closures
/// see the ECMAScript string — not Rust `Display`. A `number` arg routes
/// through `__ds::number_to_string` (ryu-js): `Display` diverges for `-0`
/// (`"-0"` vs `"0"`), `1e21`, `1e-7`, … — so `parseInt(-0)` returns `+0`
/// (matching `ToString(-0) = "0"`), not `-0`. Any other value lowers as-is;
/// the closure's own `.to_string()` then coerces (`String` clones; `bool`/
/// `null` Display is well-formed enough to parse to `NaN`).
pub(in crate::translator) fn es_to_string_arg(arg: &Argument, ctx: &Ctx<'_>) -> Expr {
    if is_number_arg(arg, ctx) {
        if let Some(e) = arg.as_expression() {
            let n = translate_number_to(e, NumberFlavor::F64, ctx);
            return parse_quote!(crate::__ds::number_to_string(#n));
        }
    }
    // ES `ToString` (§7.1.17): `null` → "null", `undefined` → "undefined".
    // Other values pass through `translate_argument`; the receiving API's own
    // `Display`/`AsRef<str>` finishes the coercion. Without this a `null`
    // lowers as `Option::None`, which fails `AsRef<str>` on `URLSearchParams`
    // / `JSON.parse` / `parseInt` arguments (`params.append(null, null)`,
    // `JSON.parse(null)`).
    match arg {
        Argument::NullLiteral(_) => parse_quote!("null".to_string()),
        Argument::Identifier(id) if id.name.as_str() == "undefined" => {
            parse_quote!("undefined".to_string())
        }
        _ => translate_argument(arg, ctx),
    }
}

pub(in crate::translator) fn parse_int_expr(a: Expr, radix: Option<Expr>) -> Expr {
    // ES parseInt step 6 applies `ToInt32` to the radix, so ±Inf/NaN map to
    // +0 (→ default 10), and a value like 2^32+2 wraps to 2 — neither matches a
    // plain `f64 as i32`, which *saturates* out-of-range floats to `i32::MAX`/
    // `MIN` (out of `[2,36]` → `NaN`). Gate non-finite values to 0, otherwise
    // hop through `i64` so the `i64 as i32` truncation wraps mod 2³² (matching
    // `ToInt32`).
    let radix_arg: Expr = match radix {
        Some(r) => parse_quote!({
            let __rd: f64 = (#r) as f64;
            if __rd.is_finite() {
                (__rd as i64) as i32
            } else {
                0
            }
        }),
        None => parse_quote!(0_i32),
    };
    parse_quote!({
        let __pi = |__s: &str, __radix: i32| -> f64 {
            // ES StringUnicodeWhitespaceTrim:  / - / / /
            // 　 count as whitespace for parseInt. They are multi-byte
            // UTF-8, so the old `(byte as char).is_whitespace()` walked raw
            // bytes and missed them — `parseInt(" 1")` returned NaN instead
            // of 1. Walk code points, advancing the byte offset past each
            // whitespace char; everything after (sign, radix prefix, digits) is
            // ASCII, so byte indexing from `__i` stays correct below.
            let mut __i = 0_usize;
            for (__off, __c) in __s.char_indices() {
                if __c.is_whitespace() {
                    __i = __off + __c.len_utf8();
                } else {
                    break;
                }
            }
            let __b = __s.as_bytes();
            let mut __sign = 1_f64;
            if __i < __b.len() && (__b[__i] == b'+' || __b[__i] == b'-') {
                if __b[__i] == b'-' {
                    __sign = -1_f64;
                }
                __i += 1;
            }
            let mut __r = if __radix == 0 { 10 } else { __radix };
            if __r == 16 && __i + 1 < __b.len() && __b[__i] == b'0' && matches!(__b[__i + 1], b'x' | b'X')
            {
                __i += 2;
            } else if __r == 10
                && __i + 1 < __b.len()
                && __b[__i] == b'0'
                && matches!(__b[__i + 1], b'x' | b'X')
            {
                __r = 16;
                __i += 2;
            }
            if !(2..=36).contains(&__r) {
                return f64::NAN;
            }
            let mut __acc: f64 = 0_f64;
            let mut __any = false;
            while __i < __b.len() {
                match (__b[__i] as char).to_digit(__r as u32) {
                    Some(__d) => {
                        __acc = __acc * (__r as f64) + __d as f64;
                        __any = true;
                        __i += 1;
                    }
                    None => break,
                }
            }
            if __any { __sign * __acc } else { f64::NAN }
        };
        let __arg = #a;
        __pi(&__arg.to_string(), #radix_arg)
    })
}

/// `parseFloat(s)` — full ES semantics (ECMA-262 §19.2.4): trim leading
/// whitespace, then take the longest valid decimal-literal prefix
/// (`[+-]?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?` or `±Infinity`), truncating at the
/// first char that cannot extend it. `NaN` if no valid prefix (so `parseFloat`
/// truncates: `"3.14abc"` → `3.14`, `"12ab"` → `12`). Inlined as a closure for
/// the same reason as [`parse_int_expr`].
pub(in crate::translator) fn parse_float_expr(a: Expr) -> Expr {
    parse_quote!({
        let __pf = |__s: &str| -> f64 {
            let __t = __s.trim_start();
            let __b = __t.as_bytes();
            let mut __i = 0_usize;
            if __i < __b.len() && (__b[__i] == b'+' || __b[__i] == b'-') {
                __i += 1;
            }
            if __t[__i..].starts_with("Infinity") {
                return __t[..__i + 8].parse::<f64>().unwrap_or(f64::NAN);
            }
            let __int0 = __i;
            while __i < __b.len() && __b[__i].is_ascii_digit() {
                __i += 1;
            }
            let __has_int = __i > __int0;
            let __has_frac = if __i < __b.len() && __b[__i] == b'.' {
                __i += 1;
                let __f0 = __i;
                while __i < __b.len() && __b[__i].is_ascii_digit() {
                    __i += 1;
                }
                __i > __f0
            } else {
                false
            };
            if !__has_int && !__has_frac {
                return f64::NAN;
            }
            if __i < __b.len() && (__b[__i] == b'e' || __b[__i] == b'E') {
                let __e0 = __i;
                let mut __j = __i + 1;
                if __j < __b.len() && (__b[__j] == b'+' || __b[__j] == b'-') {
                    __j += 1;
                }
                if __j < __b.len() && __b[__j].is_ascii_digit() {
                    while __j < __b.len() && __b[__j].is_ascii_digit() {
                        __j += 1;
                    }
                    __i = __j;
                } else {
                    __i = __e0;
                }
            }
            __t[..__i].parse::<f64>().unwrap_or(f64::NAN)
        };
        let __arg = #a;
        __pf(&__arg.to_string())
    })
}

/// `RegExp(pattern[, flags])` / `new RegExp(...)` → `__ds::regex`. ES RegExp
/// takes a runtime string pattern (a variable, not just a literal); flags
/// default to empty. `RegExp(/pat/)` copies the literal's pattern and, absent a
/// flags arg, its flags. Shared by the global `RegExp(...)` call and the
/// `new RegExp(...)` lowering.
pub(in crate::translator) fn reg_exp_constructor(args: &[Argument], ctx: &Ctx<'_>) -> Option<Expr> {
    let first = args.first()?;
    let pat = reg_exp_str_arg(first, ctx);
    let fl = match args.get(1) {
        Some(a) => reg_exp_str_arg(a, ctx),
        None => match first {
            // `RegExp(/pat/)` with no flags arg copies the literal's flags.
            Argument::RegExpLiteral(re) => {
                let (_, f) = regex_lit_parts(re);
                parse_quote!(#f)
            }
            _ => parse_quote!(""),
        },
    };
    Some(parse_quote!(crate::__ds::regex(#pat, #fl)))
}

/// A `&str` expression for a `RegExp` constructor argument: a string or regex
/// literal is emitted as a `&str` literal; any other argument is ToString'd and
/// borrowed (`&{ … }.to_string()` is a `&String` that deref-coerces to `&str`).
fn reg_exp_str_arg(arg: &Argument, ctx: &Ctx<'_>) -> Expr {
    match arg {
        Argument::StringLiteral(s) => {
            let lit = syn::LitStr::new(s.value.as_str(), Span::call_site());
            parse_quote!(#lit)
        }
        Argument::RegExpLiteral(re) => {
            let (p, _) = regex_lit_parts(re);
            parse_quote!(#p)
        }
        _ => {
            let e = translate_argument(arg, ctx);
            parse_quote!(&{ #e }.to_string())
        }
    }
}

/// `RegExp.<method>(...)` static methods. `RegExp.escape(s)` (TC39 Stage 3)
/// backslash-escapes every regex metacharacter so `s` is safe to splice into a
/// pattern; inlined so it pulls no runtime dep. Returns `None` for any other
/// name (falls through; `RegExp.<other>` surfaces as E0425 honestly).
pub(in crate::translator) fn reg_exp_static(
    method: &str,
    args: &[Argument],
    ctx: &Ctx<'_>,
) -> Option<Expr> {
    if method != "escape" {
        return None;
    }
    let s = args.first()?;
    let e = translate_argument(s, ctx);
    Some(parse_quote!({
        // ES2025 RegExp.escape (sec-regexp.escape): if the first code point is
        // a decimal digit or ASCII letter, emit it as `\xHH` (so the result is
        // safe after a `\0`/`\1` escape); every other code point runs through
        // EncodeForRegExpEscape — ControlEscape, SyntaxCharacter or `/`,
        // otherPunctuators, WhiteSpace, LineTerminator and surrogate code
        // points become `\xHH` (≤ U+FF) or `\uHHHH` (> U+FF); the rest pass
        // through. Isolated surrogates cannot occur in a Rust `String` (UTF-8),
        // so the surrogate branch is unreachable here — those fixtures stay
        // partial honestly.
        fn __hex2(__o: &mut String, __cp: u32) {
            __o.push('\\');
            __o.push('x');
            __o.push(char::from_digit((__cp >> 4) & 0xF, 16).unwrap());
            __o.push(char::from_digit(__cp & 0xF, 16).unwrap());
        }
        fn __hex4(__o: &mut String, __cp: u32) {
            __o.push_str("\\u");
            __o.push(char::from_digit((__cp >> 12) & 0xF, 16).unwrap());
            __o.push(char::from_digit((__cp >> 8) & 0xF, 16).unwrap());
            __o.push(char::from_digit((__cp >> 4) & 0xF, 16).unwrap());
            __o.push(char::from_digit(__cp & 0xF, 16).unwrap());
        }
        let __s = #e;
        let mut __out = String::with_capacity(__s.len());
        let mut __first = true;
        for __c in __s.chars() {
            if __first {
                __first = false;
                if __c.is_ascii_digit() || __c.is_ascii_alphabetic() {
                    __hex2(&mut __out, __c as u32);
                    continue;
                }
            }
            match __c {
                '\t' => __out.push_str("\\t"),
                '\n' => __out.push_str("\\n"),
                '\u{B}' => __out.push_str("\\v"),
                '\u{C}' => __out.push_str("\\f"),
                '\r' => __out.push_str("\\r"),
                '^' | '$' | '\\' | '.' | '*' | '+' | '?' | '(' | ')' | '[' | ']'
                    | '{' | '}' | '|' | '/' => {
                    __out.push('\\');
                    __out.push(__c);
                }
                _ => {
                    let __cp = __c as u32;
                    let __need = matches!(
                        __c,
                        ',' | '-' | '=' | '<' | '>' | '#' | '&' | '!' | '%' | ':' | ';'
                            | '@' | '~' | '\'' | '`' | '"'
                    ) || matches!(
                        __cp,
                        0x20 | 0xA0 | 0xFEFF | 0x1680 | 0x2000..=0x200A | 0x2028 | 0x2029
                            | 0x202F | 0x205F | 0x3000
                    );
                    if __need {
                        if __cp <= 0xFF {
                            __hex2(&mut __out, __cp);
                        } else if __cp > 0xFFFF {
                            let __v = __cp - 0x10000;
                            __hex4(&mut __out, 0xD800 + (__v >> 10));
                            __hex4(&mut __out, 0xDC00 + (__v & 0x3FF));
                        } else {
                            __hex4(&mut __out, __cp);
                        }
                    } else {
                        __out.push(__c);
                    }
                }
            }
        }
        __out
    }))
}
