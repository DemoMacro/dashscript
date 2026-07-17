//! `Math.<method>` and `Math.<constant>` → idiomatic Rust float operations.
//! Mirrors `test/built-ins/Math/`.

use oxc_ast::ast::Argument;
use quote::{format_ident, quote};
use syn::{parse_quote, parse_str, Expr};

use super::super::context::Ctx;
use super::super::expressions::translate_argument;

/// `Math.<m>(args)` → the idiomatic Rust float operation. Single-arg methods
/// (`floor`, `ceil`, `abs`, …) become a method on the argument; `max`/`min`
/// become `a.max(b)`; `pow` becomes `a.powf(b)`. Returns `None` when unmapped.
pub(in crate::translator) fn math_method(
    name: &str,
    args: &[Argument],
    ctx: &Ctx<'_>,
) -> Option<Expr> {
    match name {
        "floor" | "ceil" | "abs" | "sqrt" | "trunc" | "exp" | "log10" | "log2" | "sin" | "cos"
        | "tan" | "asin" | "acos" | "atan" | "cbrt" | "sinh" | "cosh" | "tanh" | "asinh"
        | "acosh" | "atanh" => {
            let recv = math_receiver(args.first()?, ctx);
            Some(method_call(recv, name, Vec::new()))
        }
        // `Math.round(x)` → JS rounds half toward +∞ (`Math.round(2.5)` = 3),
        // not Rust's away-from-zero (`(-0.5).round()` = -1). The `(x + 0.5).floor()`
        // form matches JS; and when the result is 0 with a negative input JS
        // returns -0 (`Math.round(-0.5)` = -0), so mirror that — Rust's `-0.0`
        // already prints "-0".
        "round" => {
            let recv = math_receiver(args.first()?, ctx);
            Some(parse_quote!({
                let __x = (#recv) as f64;
                let __r = (__x + 0.5).floor();
                if __r == 0.0 && __x.is_sign_negative() { -0.0f64 } else { __r }
            }))
        }
        // `Math.sign(x)` → `x.signum()` (Rust spells it `signum`, not `sign`).
        "sign" => {
            let recv = math_receiver(args.first()?, ctx);
            Some(method_call(recv, "signum", Vec::new()))
        }
        // `Math.log(x)` (TS natural log) → `x.ln()` (Rust spells it `ln`).
        "log" => {
            let recv = math_receiver(args.first()?, ctx);
            Some(method_call(recv, "ln", Vec::new()))
        }
        // `Math.log1p(x)` → `x.ln_1p()`; `Math.expm1(x)` → `x.exp_m1()`
        // (Rust's f64 names differ from JS).
        "log1p" => {
            let recv = math_receiver(args.first()?, ctx);
            Some(method_call(recv, "ln_1p", Vec::new()))
        }
        "expm1" => {
            let recv = math_receiver(args.first()?, ctx);
            Some(method_call(recv, "exp_m1", Vec::new()))
        }
        // `Math.max`/`min` are variadic in JS: `max()` = -∞ / `min()` = +∞
        // (the identity element), one arg is the value itself, and
        // `max(a, b, c, …)` folds binary `f64::max`/`min` left to right. Every
        // arg goes through `math_receiver` so a bare/negative literal anchors
        // to f64; a variable keeps whatever type its context pins.
        "max" | "min" => {
            if args.is_empty() {
                return Some(if name == "max" {
                    parse_quote!(::core::f64::NEG_INFINITY)
                } else {
                    parse_quote!(::core::f64::INFINITY)
                });
            }
            let mut recv = math_receiver(args.first()?, ctx);
            for arg in args.iter().skip(1) {
                let b = math_receiver(arg, ctx);
                recv = method_call(recv, name, vec![b]);
            }
            Some(recv)
        }
        "pow" => {
            let a = math_receiver(args.first()?, ctx);
            let b = translate_argument(args.get(1)?, ctx);
            Some(method_call(a, "powf", vec![b]))
        }
        // `Math.atan2(y, x)` → `y.atan2(x)` (2-arg, unlike the 1-arg `atan`).
        "atan2" => {
            let y = math_receiver(args.first()?, ctx);
            let x = translate_argument(args.get(1)?, ctx);
            Some(method_call(y, "atan2", vec![x]))
        }
        // `Math.hypot` is variadic: `hypot()` = 0, `hypot(a)` = |a|, and the
        // general case is √(Σ aᵢ²) (std has no `f64::hypot`; the Pythagorean
        // form is exact for finite inputs). Fold the sum of squares from the
        // first arg so the 2-arg form stays `(a² + b²).sqrt()`. Each arg goes
        // through `math_receiver` so a bare/negative literal anchors to f64.
        "hypot" => {
            if args.is_empty() {
                return Some(parse_quote!(0.0f64));
            }
            // JS Math.hypot returns +∞ if any arg is ±∞ (hypot(∞, NaN) = ∞,
            // not the NaN Rust's (Inf² + NaN²).sqrt() yields), so bind each arg
            // once and guard; the finite path is √(Σ aᵢ²). Binding also avoids
            // re-evaluating a side-effecting argument.
            let recvs: Vec<Expr> = args.iter().map(|a| math_receiver(a, ctx)).collect();
            let lets = recvs
                .iter()
                .enumerate()
                .map(|(i, r)| {
                    let id = format_ident!("__h{i}");
                    parse_quote!(let #id = #r;)
                })
                .collect::<Vec<syn::Stmt>>();
            let idents = (0..recvs.len())
                .map(|i| format_ident!("__h{i}"))
                .collect::<Vec<_>>();
            let infs = idents
                .iter()
                .map(|id| quote!(#id.is_infinite()))
                .collect::<Vec<_>>();
            let sqs = idents
                .iter()
                .map(|id| quote!(#id.powi(2)))
                .collect::<Vec<_>>();
            Some(parse_quote!({
                #(#lets)*
                if #(#infs)||* {
                    ::core::f64::INFINITY
                } else {
                    (#(#sqs)+*).sqrt()
                }
            }))
        }
        // `Math.clz32(x)` → leading zero bits of ToUint32(x) (see
        // `to_uint32_expr`). JS applies ToUint32 (mod 2³²), not Rust's
        // saturating `as u32`: `clz32(2³²)` = 32, `clz32(-1)` = 0.
        "clz32" => {
            let n = to_uint32_expr(math_receiver(args.first()?, ctx));
            Some(parse_quote!((#n).leading_zeros() as f64))
        }
        // `Math.fround(x)` → round-trip through `f32`: `x as f32 as f64`.
        "fround" => {
            let recv = math_receiver(args.first()?, ctx);
            Some(parse_quote!((#recv as f32) as f64))
        }
        // `Math.imul(a, b)` → 32-bit wrapping multiply of ToInt32(a), ToInt32(b).
        // JS applies ToInt32 (ToUint32 bit-reinterpreted as signed), not Rust's
        // saturating `as i32` — so large/negative args wrap like JS.
        "imul" => {
            let a = to_uint32_expr(math_receiver(args.first()?, ctx));
            let b = to_uint32_expr(math_receiver(args.get(1)?, ctx));
            Some(parse_quote!(((#a) as i32).wrapping_mul((#b) as i32) as f64))
        }
        _ => None,
    }
}

/// `ToUint32(x)` as a Rust `u32` expression — trunc toward zero, then mod 2³²
/// (JS semantics), unlike Rust's saturating `as u32`. Non-finite → 0. Shared by
/// `Math.clz32` (then `.leading_zeros()`) and `Math.imul` (as `i32`).
fn to_uint32_expr(recv: Expr) -> Expr {
    parse_quote!({
        let n = ((#recv) as f64).trunc();
        (if !n.is_finite() {
            0.0f64
        } else {
            ((n % 4294967296.0) + 4294967296.0) % 4294967296.0
        }) as u32
    })
}

/// Build `recv.method(args)` as an `ExprMethodCall` so `prettyplease`
/// parenthesizes the receiver by precedence — `(a + b).sqrt()`, not
/// `a + b.sqrt()` (which would bind `.sqrt()` to `b` only).
fn method_call(recv: Expr, method: &str, args: Vec<Expr>) -> Expr {
    Expr::MethodCall(syn::ExprMethodCall {
        attrs: Vec::new(),
        receiver: Box::new(recv),
        dot_token: Default::default(),
        method: format_ident!("{}", method),
        turbofish: None,
        args: args.into_iter().collect(),
        paren_token: Default::default(),
    })
}

/// A `Math.` receiver: a numeric literal gets an `_f64` suffix so a bare
/// literal like `3` isn't an ambiguous `{float}` (`3.0.max(7.0)` won't infer);
/// any other receiver translates normally (its context already pins `f64`).
fn math_receiver(arg: &Argument, ctx: &Ctx<'_>) -> Expr {
    if let Some(v) = math_numeric_literal(arg) {
        let s = format!("{}_f64", v);
        return parse_str(&s).unwrap_or_else(|_| parse_quote!(::core::f64::NAN));
    }
    translate_argument(arg, ctx)
}

/// The literal spelling of a `Math.` receiver, sign included: `3` → `3`,
/// `-0`/`-1.5` → `-0`/`-1.5`. oxc parses a negative literal as
/// `UnaryExpression(-, NumericLiteral)` rather than a `NumericLiteral`, so the
/// plain-literal branch in `math_receiver` misses it and it would land as an
/// un-anchored `-0` → E0689 (`can't call method 'abs' on ambiguous {float}`).
/// Non-literals (variables, `NaN`, …) → `None` so they translate normally and
/// keep whatever type anchor their context already provides.
fn math_numeric_literal(arg: &Argument) -> Option<String> {
    use oxc_ast::ast::Expression;
    use oxc_syntax::operator::UnaryOperator;
    match arg {
        Argument::NumericLiteral(n) => Some(format!("{}", n.value)),
        // oxc parses a signed literal as `UnaryExpression(-/+, NumericLiteral)`.
        // `+` matters too: test262 spells `Math.acosh(+1)` (unary plus), which
        // would otherwise land as an un-anchored integer → E0689.
        Argument::UnaryExpression(un)
            if matches!(
                un.operator,
                UnaryOperator::UnaryNegation | UnaryOperator::UnaryPlus
            ) =>
        {
            let sign = if matches!(un.operator, UnaryOperator::UnaryNegation) {
                "-"
            } else {
                ""
            };
            match &un.argument {
                Expression::NumericLiteral(n) => Some(format!("{sign}{}", n.value)),
                _ => None,
            }
        }
        _ => None,
    }
}

/// `Math.PI` → `std::f64::consts::PI`, `Math.E` → `…::E`, and the rest of the
/// JS `Math` constants map to the matching `f64::consts` (Rust spells them with
/// underscores: `LN10`→`LN_10`, `LOG10E`→`LOG10_E`, `SQRT1_2`→`FRAC_1_SQRT_2`).
pub(in crate::translator) fn math_constant(name: &str) -> Option<Expr> {
    let path = match name {
        "PI" => quote!(::std::f64::consts::PI),
        "E" => quote!(::std::f64::consts::E),
        "LN10" => quote!(::std::f64::consts::LN_10),
        "LN2" => quote!(::std::f64::consts::LN_2),
        "LOG10E" => quote!(::std::f64::consts::LOG10_E),
        "LOG2E" => quote!(::std::f64::consts::LOG2_E),
        "SQRT2" => quote!(::std::f64::consts::SQRT_2),
        "SQRT1_2" => quote!(::std::f64::consts::FRAC_1_SQRT_2),
        _ => return None,
    };
    syn::parse2(path).ok()
}
