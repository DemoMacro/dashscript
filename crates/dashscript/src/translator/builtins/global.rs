//! Global conversion functions called as plain identifiers (`String(x)`,
//! `parseInt(s)`, `Number(s)`, `Boolean(x)`, `isNaN`/`isFinite`). These are ES
//! globals — not a `Math`/`Object` member — so they live here rather than under
//! a named built-in file.

use oxc_ast::ast::{Argument, IdentifierReference};
use syn::{parse_quote, Expr};

use super::super::bindings;
use super::super::context::Ctx;
use super::super::expressions::{bool_expr, is_number_arg, string_expr, translate_argument};

/// Global conversion functions called as plain identifiers: `String(x)` →
/// `format!("{}", x)`; `parseInt(s)`/`parseFloat(s)` → `s.trim().parse::<f64>()`
/// (`.ds` `number` is `f64`, so both share one parse path). Returns `None` for
/// any other name (falls through to a plain call).
pub(in crate::translator) fn global_function(
    id: &IdentifierReference,
    args: &[Argument],
    ctx: &Ctx<'_>,
) -> Option<Expr> {
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
                    let e = translate_argument(a, ctx);
                    // `String(<number>)` is ES NumberToString — route through the
                    // helper; other values use `format!` (Rust `Display` already
                    // matches ES for string/bool).
                    if is_number_arg(a, ctx) {
                        parse_quote!(crate::__ds::number_to_string(#e))
                    } else {
                        parse_quote!(::std::format!("{}", #e))
                    }
                }
            }
        }
        // `parseFloat(s)` — full ES semantics: longest valid decimal-literal
        // prefix (truncation), `±Infinity`, NaN if none. See `parse_float_expr`.
        "parseFloat" => parse_float_expr(translate_argument(args.first()?, ctx)),
        // `parseInt(s[, radix])` — full ES semantics: trim, sign, `0x`
        // auto-detect (radix 0/16), truncate at the first non-digit. See
        // `parse_int_expr`.
        "parseInt" => parse_int_expr(
            translate_argument(args.first()?, ctx),
            args.get(1).map(|r| translate_argument(r, ctx)),
        ),
        // `Number(s)` parses a string; `Number(n)` passes a number through.
        // ToNumber coercion: an empty or whitespace-only string is `0` (not
        // NaN — `Number("")` / `Number("  ")` are both `0`); anything else
        // parses as `f64`, NaN on a malformed string (a throw is never raised).
        "Number" => {
            let a = args.first()?;
            let e = translate_argument(a, ctx);
            if matches!(a, Argument::StringLiteral(_)) || ident_string_local(a, ctx) {
                parse_quote!({
                    // Bind the (possibly temporary) string first: `#e` may be
                    // `"x".to_string()`, and `.trim()` would borrow a value
                    // freed at the end of that expression (E0716).
                    let __s = #e;
                    let __t = __s.trim();
                    if __t.is_empty() {
                        0_f64
                    } else {
                        __t.parse::<f64>().unwrap_or(f64::NAN)
                    }
                })
            } else {
                e
            }
        }
        // `Boolean(x)` → the Rust truthiness of `x` (see `bool_cast`).
        "Boolean" => bool_cast(args.first()?, ctx),
        // `isNaN(x)` → `x.is_nan()` (DashScript's `number` is `f64`, so the TS
        // global's ToNumber coercion is already done).
        "isNaN" => {
            let a = translate_argument(args.first()?, ctx);
            parse_quote!(#a.is_nan())
        }
        // `isFinite(x)` → `x.is_finite()`.
        "isFinite" => {
            let a = translate_argument(args.first()?, ctx);
            parse_quote!(#a.is_finite())
        }
        _ => return None,
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
/// `is_some()`, a `bool` → itself, anything else (an `f64`) → `!= 0_f64`. An
/// expression of unknown type falls back to `!= 0_f64` (TS `Boolean` is most
/// often applied to numbers).
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
                _ => parse_quote!(#name != 0_f64),
            }
        }
        _ => {
            let e = translate_argument(arg, ctx);
            parse_quote!(#e != 0_f64)
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
pub(in crate::translator) fn parse_int_expr(a: Expr, radix: Option<Expr>) -> Expr {
    let radix_arg: Expr = match radix {
        Some(r) => parse_quote!((#r) as i32),
        None => parse_quote!(0_i32),
    };
    parse_quote!({
        let __pi = |__s: &str, __radix: i32| -> f64 {
            let __b = __s.as_bytes();
            let mut __i = 0_usize;
            while __i < __b.len() && (__b[__i] as char).is_whitespace() {
                __i += 1;
            }
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
