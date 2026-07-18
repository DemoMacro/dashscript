//! Methods and constants on a `.ds` `number` (`f64`). Mirrors
//! `test/built-ins/Number/` (instance methods + static methods + constants).

use oxc_ast::ast::{Argument, StaticMemberExpression};
use syn::{parse_quote, Expr};

use super::super::context::Ctx;
use super::super::expressions::{translate_argument, translate_expr};
use super::usize_arg;

/// Methods on a `.ds` `number` (`f64`). `.toFixed(n)` → a formatted string
/// with `n` decimal places. Returns `None` for an unmapped name.
pub(in crate::translator) fn number_method(
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
                let __r = (#radix) as u32;
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
                    let prec = usize_arg(a, ctx);
                    parse_quote!(format!("{:.*e}", #prec, #recv))
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
    let x = translate_argument(args.first()?, ctx);
    Some(match name {
        // `(#x)` — the argument may be a negated literal (`-Infinity`); a bare
        // `#x.is_finite()` would parse as `-(x.is_finite())` because method-call
        // binds tighter than unary `-`, applying `-` to the resulting `bool`
        // (E0600). Parenthesise so the receiver is the whole argument.
        "isNaN" => parse_quote!((#x).is_nan()),
        "isFinite" => parse_quote!((#x).is_finite()),
        "isInteger" => parse_quote!((#x).is_finite() && (#x).fract() == 0_f64),
        "isSafeInteger" => {
            parse_quote!((#x).is_finite() && (#x).fract() == 0_f64 && (#x).abs() <= 9_007_199_254_740_991_f64)
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
