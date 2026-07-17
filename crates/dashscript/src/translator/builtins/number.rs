//! Methods and constants on a `.ds` `number` (`f64`). Mirrors
//! `test/built-ins/Number/` (instance methods + static methods + constants).

use oxc_ast::ast::{Argument, StaticMemberExpression};
use proc_macro2::Span;
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
        // `(n).toExponential(fracDigits)` → scientific notation with that many
        // digits after the point. Rust's `{:.*e}` takes precision then value,
        // matching TS's `fracDigits` semantics.
        "toExponential" => {
            let prec = usize_arg(args.first()?, ctx);
            parse_quote!(format!("{:.*e}", #prec, #recv))
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
        "isNaN" => parse_quote!(#x.is_nan()),
        "isFinite" => parse_quote!(#x.is_finite()),
        "isInteger" => parse_quote!(#x.is_finite() && #x.fract() == 0.0),
        "isSafeInteger" => {
            parse_quote!(#x.is_finite() && #x.fract() == 0.0 && #x.abs() <= 9_007_199_254_740_991.0)
        }
        // `Number.parseFloat(s)` ≡ the global `parseFloat(s)` — base-10 f64
        // parse, NaN on a malformed string (never a throw, as in TS).
        "parseFloat" => parse_quote!(#x.trim().parse::<f64>().unwrap_or(f64::NAN)),
        // `Number.parseInt(s)` / `Number.parseInt(s, radix)` ≡ the global
        // `parseInt` — base-10 by default, `i64::from_str_radix` with a radix.
        "parseInt" => match args.get(1) {
            Some(radix) => {
                let r = translate_argument(radix, ctx);
                parse_quote!(
                    i64::from_str_radix(#x.trim(), #r as u32)
                        .map(|v| v as f64)
                        .unwrap_or(f64::NAN)
                )
            }
            None => parse_quote!(#x.trim().parse::<f64>().unwrap_or(f64::NAN)),
        },
        _ => return None,
    })
}
