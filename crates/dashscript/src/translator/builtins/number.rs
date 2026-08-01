//! Methods and constants on a `.ts` `number` (`f64`). Mirrors
//! `test/built-ins/Number/` (instance methods + static methods + constants).

use oxc_ast::ast::{Argument, StaticMemberExpression};
use syn::{parse_quote, Expr};

use super::super::context::Ctx;
use super::super::expressions::{translate_argument, translate_number_to};
use super::super::flavor::NumberFlavor;

/// Methods on a `.ts` `number` (`f64`). `.toFixed(n)` → a formatted string
/// with `n` decimal places. Returns `None` for an unmapped name.
pub(in crate::translator) fn number_method(
    sm: &StaticMemberExpression,
    args: &[Argument],
    ctx: &Ctx<'_>,
) -> Option<Expr> {
    // `number` methods (`toString`/`toPrecision`/…) emit `f64`-only operations
    // (`.is_nan()`, `as i64`, …); coerce a flavor-promoted `i64` receiver to
    // `f64` so those sites compile (an `i64 → f64` widening is exact below 2^53,
    // the ES safe-integer range).
    let recv = translate_number_to(&sm.object, NumberFlavor::F64, ctx);
    Some(match sm.property.name.as_str() {
        // `(3.14).toFixed(2)` → `format!("{:.*}", n, …)`. In Rust the `*`
        // precision argument comes first, the value second. ES throws
        // `RangeError` when the digit count is outside [0, 100].
        "toFixed" => {
            let f = translate_argument(args.first()?, ctx);
            parse_quote!({
                let __f = (#f) as f64;
                if !(__f >= 0.0 && __f <= 100.0) {
                    ::std::panic::panic_any(crate::__ds::DsError::new(
                        "RangeError",
                        "toFixed() digits must be between 0 and 100",
                    ));
                }
                format!("{:.*}", __f as usize, #recv)
            })
        }
        // `(n).toString(radix)` → a base-`radix` integer string (radix 2-36).
        // The receiver is cast to `i64` (TS truncates the fractional part of
        // the receiver before converting); a variable radix lowers to a
        // runtime conversion with digits `0-9a-z`, so `i.toString(radix)` in
        // a loop works — not just literal radices. A fractional receiver
        // loses its fraction, matching TS (`(3.5).toString(2)` first drops to
        // the integer `3`). `NaN`/`±Infinity` keep their names in any radix
        // (TS does not convert them); `f64 as i64` would turn them into 0 /
        // i64::MAX, so they are intercepted first.
        "toString" if !args.is_empty() => {
            let radix = translate_argument(args.first()?, ctx);
            parse_quote!({
                let __x = #recv;
                let __r = (#radix) as f64;
                if !(__r >= 2.0 && __r <= 36.0) {
                    ::std::panic::panic_any(crate::__ds::DsError::new(
                        "RangeError",
                        "toString() radix must be between 2 and 36",
                    ));
                }
                let __r = __r as u32;
                if __x.is_nan() {
                    "NaN".to_string()
                } else if __x.is_infinite() {
                    if __x < 0_f64 {
                        "-Infinity".to_string()
                    } else {
                        "Infinity".to_string()
                    }
                } else {
                    let __n = __x as i64;
                    let __digits = b"0123456789abcdefghijklmnopqrstuvwxyz";
                    let mut __m = __n.unsigned_abs();
                    let mut __buf: Vec<u8> = Vec::new();
                    if __m == 0 {
                        __buf.push(b'0');
                    }
                    while __m > 0 {
                        __buf.push(__digits[(__m % __r as u64) as usize]);
                        __m /= __r as u64;
                    }
                    __buf.reverse();
                    let mut __s = String::from_utf8(__buf).unwrap();
                    if __n < 0 {
                        __s.insert(0, '-');
                    }
                    __s
                }
            })
        }
        // `(n).toExponential(fracDigits)` / `(n).toExponential()` → scientific
        // notation. Rust's `{:e}` prints a sign-less exponent (`1e4`); TS always
        // signs it (`1e+4`), so a bare exponent gets a `+` prepended (a `-` and
        // an explicit `+` are left alone).
        "toExponential" => {
            let formatted: Expr = match args.first() {
                Some(a) => {
                    let prec = translate_argument(a, ctx);
                    parse_quote!({
                        let __p = (#prec) as f64;
                        if !(__p >= 0.0 && __p <= 100.0) {
                            ::std::panic::panic_any(crate::__ds::DsError::new(
                                "RangeError",
                                "toExponential() digits must be between 0 and 100",
                            ));
                        }
                        format!("{:.*e}", __p as usize, #recv)
                    })
                }
                None => parse_quote!(format!("{:e}", #recv)),
            };
            parse_quote!({
                let __s = #formatted;
                if let Some((__m, __e)) = __s.split_once('e') {
                    if __e.starts_with('-') || __e.starts_with('+') {
                        __s
                    } else {
                        format!("{}e+{}", __m, __e)
                    }
                } else {
                    __s
                }
            })
        }
        // `(n).toPrecision(p)` / `(n).toPrecision()` → ES precision
        // formatting. No arg ≡ `toString()` (ES NumberToString via the shared
        // ryu-js helper, matching Node's oracle). With a `p` arg, `p`
        // significant digits are rendered, choosing fixed-point for exponents
        // in [-6, p) and scientific notation outside that — mirroring
        // ECMAScript's toPrecision. Rust's `{:.prec$e}` supplies `p`
        // significant digits (prec = p-1 fractional places); the digits +
        // exponent are then reshaped to ES form. (Rust's round-half-to-even
        // may diverge from ES round-half-away on exact `.5` boundaries — the
        // common case; test262 is the arbiter.)
        "toPrecision" if args.is_empty() => {
            parse_quote!(crate::__ds::number_to_string(#recv))
        }
        "toPrecision" => {
            let prec = translate_argument(args.first()?, ctx);
            parse_quote!({
                let __p_f = (#prec) as f64;
                if !(__p_f >= 1.0 && __p_f <= 100.0) {
                    ::std::panic::panic_any(crate::__ds::DsError::new(
                        "RangeError",
                        "toPrecision() precision must be between 1 and 100",
                    ));
                }
                let __x = #recv;
                let __p: usize = __p_f as usize;
                if __x.is_nan() {
                    "NaN".to_string()
                } else if __x.is_infinite() {
                    if __x < 0_f64 {
                        "-Infinity".to_string()
                    } else {
                        "Infinity".to_string()
                    }
                } else {
                    let __neg = __x < 0_f64;
                    let __ax = __x.abs();
                    let __sci = format!("{:.*e}", __p.saturating_sub(1), __ax);
                    let (__mant, __exp_str) = __sci
                        .split_once('e')
                        .unwrap_or((__sci.as_str(), "0"));
                    let __exp: i32 = __exp_str.parse().unwrap();
                    let __digits: String = __mant.chars().filter(|c| *c != '.').collect();
                    let __m = if __exp < -6 || __exp >= __p as i32 {
                        let __body = if __p == 1 {
                            __digits.clone()
                        } else {
                            format!("{}.{}", &__digits[..1], &__digits[1..])
                        };
                        let __esign = if __exp >= 0 { "+" } else { "-" };
                        format!("{}e{}{}", __body, __esign, __exp.abs())
                    } else if __exp == __p as i32 - 1 {
                        __digits.clone()
                    } else if __exp >= 0 {
                        let __i = __exp as usize + 1;
                        format!("{}.{}", &__digits[..__i], &__digits[__i..])
                    } else {
                        format!(
                            "0.{}{}",
                            "0".repeat(((-__exp) - 1) as usize),
                            __digits.as_str()
                        )
                    };
                    let __sign = if __neg { "-" } else { "" };
                    format!("{}{}", __sign, __m)
                }
            })
        }
        // `(n).valueOf()` → the number itself (an `f64` identity).
        // `toLocaleString` is locale-dependent (thousands separators) and
        // intentionally not mapped — see `string_method`.
        "valueOf" if args.is_empty() => parse_quote!(#recv),
        _ => return None,
    })
}

/// `Number.<CONST>` → the matching `f64` constant. TS's `Number.EPSILON` /
/// `MAX_VALUE` / `NaN` / `±INFINITY` map directly to `f64`'s associated
/// constants; `MAX_SAFE_INTEGER` / `MIN_SAFE_INTEGER` are 2^53 − 1 (the
/// largest integer exactly representable in f64). Returns `None` otherwise.
pub(in crate::translator) fn number_constant(name: &str) -> Option<Expr> {
    Some(match name {
        "EPSILON" => parse_quote!(::std::f64::EPSILON),
        "MAX_SAFE_INTEGER" => parse_quote!(9_007_199_254_740_991f64),
        "MAX_VALUE" => parse_quote!(::std::f64::MAX),
        "MIN_SAFE_INTEGER" => parse_quote!(-9_007_199_254_740_991f64),
        "MIN_VALUE" => parse_quote!(::std::f64::MIN_POSITIVE),
        "NaN" => parse_quote!(::std::f64::NAN),
        "NEGATIVE_INFINITY" => parse_quote!(::std::f64::NEG_INFINITY),
        "POSITIVE_INFINITY" => parse_quote!(::std::f64::INFINITY),
        _ => return None,
    })
}

/// Translate a number `Argument` coerced to `f64`. The type-check arms below
/// emit `f64`-only methods (`.is_nan()` / `.fract()` / `.abs()`); a flavor-
/// promoted `i64` local would otherwise lack them.
fn number_arg_to_f64(arg: &Argument, ctx: &Ctx<'_>) -> Option<Expr> {
    Some(translate_number_to(
        arg.as_expression()?,
        NumberFlavor::F64,
        ctx,
    ))
}

/// `Number.<m>(x)`: static type checks on an `f64`. `isNaN` → `is_nan`,
/// `isFinite` → `is_finite`, `isInteger` → a finite value with no fractional
/// part, `isSafeInteger` adds the ±(2^53 − 1) bound. `parseFloat`/`parseInt`
/// mirror the global functions (TS `Number.parseFloat === parseFloat`).
/// Returns `None` otherwise.
pub(in crate::translator) fn number_static(
    name: &str,
    args: &[Argument],
    ctx: &Ctx<'_>,
) -> Option<Expr> {
    let arg = args.first()?;
    // `parseFloat` / `parseInt` take a string — keep the raw argument. The
    // type-check arms re-translate at `f64` (see `number_arg_to_f64`).
    let x = translate_argument(arg, ctx);
    Some(match name {
        "isNaN" => {
            let n = number_arg_to_f64(arg, ctx)?;
            parse_quote!((#n).is_nan())
        }
        "isFinite" => {
            let n = number_arg_to_f64(arg, ctx)?;
            parse_quote!((#n).is_finite())
        }
        "isInteger" => {
            let n = number_arg_to_f64(arg, ctx)?;
            parse_quote!((#n).is_finite() && (#n).fract() == 0_f64)
        }
        "isSafeInteger" => {
            let n = number_arg_to_f64(arg, ctx)?;
            parse_quote!((#n).is_finite() && (#n).fract() == 0_f64 && (#n).abs() <= 9_007_199_254_740_991_f64)
        }
        // `Number.parseFloat(s)` ≡ the global `parseFloat` — full ES
        // truncation semantics (see `global::parse_float_expr`).
        "parseFloat" => return Some(super::global::parse_float_expr(x)),
        // `Number.parseInt(s[, radix])` ≡ the global `parseInt` — full ES
        // trim/sign/`0x`/truncation semantics (see `global::parse_int_expr`).
        "parseInt" => {
            return Some(super::global::parse_int_expr(
                x,
                args.get(1).map(|r| translate_argument(r, ctx)),
            ));
        }
        _ => return None,
    })
}
