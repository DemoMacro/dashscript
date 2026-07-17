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
pub(in crate::translator) fn math_method(name: &str, args: &[Argument], ctx: &Ctx<'_>) -> Option<Expr> {
    match name {
        "floor" | "ceil" | "round" | "abs" | "sqrt" | "trunc" | "exp"
        | "log10" | "log2" | "sin" | "cos" | "tan" | "asin" | "acos" | "atan" | "cbrt"
        | "sinh" | "cosh" | "tanh" | "asinh" | "acosh" | "atanh" => {
            let recv = math_receiver(args.first()?, ctx);
            Some(method_call(recv, name, Vec::new()))
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
        "max" | "min" => {
            let a = math_receiver(args.first()?, ctx);
            let b = translate_argument(args.get(1)?, ctx);
            Some(method_call(a, name, vec![b]))
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
        // `Math.hypot(a, b)` → `(a.powi(2) + b.powi(2)).sqrt()` (std has no
        // `f64::hypot`; the Pythagorean form is exact for finite inputs). Both
        // args go through `math_receiver` so a bare literal like `4` gets an
        // `_f64` suffix — otherwise `4.powi(2)` is an ambiguous `{float}`.
        "hypot" => {
            let a = math_receiver(args.first()?, ctx);
            let b = math_receiver(args.get(1)?, ctx);
            Some(parse_quote!((#a.powi(2) + #b.powi(2)).sqrt()))
        }
        // `Math.clz32(x)` → count of leading zero bits of `x as u32`.
        "clz32" => {
            let recv = math_receiver(args.first()?, ctx);
            Some(parse_quote!((#recv as u32).leading_zeros() as f64))
        }
        // `Math.fround(x)` → round-trip through `f32`: `x as f32 as f64`.
        "fround" => {
            let recv = math_receiver(args.first()?, ctx);
            Some(parse_quote!((#recv as f32) as f64))
        }
        // `Math.imul(a, b)` → 32-bit wrapping multiply of `a as i32`, `b as i32`.
        "imul" => {
            let a = math_receiver(args.first()?, ctx);
            let b = math_receiver(args.get(1)?, ctx);
            Some(parse_quote!((#a as i32).wrapping_mul(#b as i32) as f64))
        }
        _ => None,
    }
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
    if let Argument::NumericLiteral(n) = arg {
        let s = format!("{}_f64", n.value);
        return parse_str(&s).unwrap_or_else(|_| parse_quote!(::core::f64::NAN));
    }
    translate_argument(arg, ctx)
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
