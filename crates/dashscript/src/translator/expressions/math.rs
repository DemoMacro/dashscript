//! `Math.<method>` and `Math.<constant>` → idiomatic Rust float operations.

use oxc_ast::ast::Argument;
use quote::{format_ident, quote};
use syn::{parse_quote, parse_str, Expr};

use super::super::context::Ctx;
use super::translate_argument;

/// `Math.<m>(args)` → the idiomatic Rust float operation. Single-arg methods
/// (`floor`, `ceil`, `abs`, …) become a method on the argument; `max`/`min`
/// become `a.max(b)`; `pow` becomes `a.powf(b)`. Returns `None` when unmapped.
pub(super) fn math_method(name: &str, args: &[Argument], ctx: &Ctx<'_>) -> Option<Expr> {
    match name {
        "floor" | "ceil" | "round" | "abs" | "sqrt" | "trunc" | "sign" | "exp" | "ln"
        | "log10" | "log2" | "sin" | "cos" | "tan" | "asin" | "acos" | "atan" | "cbrt" => {
            let recv = math_receiver(args.first()?, ctx);
            Some(method_call(recv, name, Vec::new()))
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

/// `Math.PI` → `std::f64::consts::PI`, `Math.E` → `…::E`.
pub(super) fn math_constant(name: &str) -> Option<Expr> {
    let path = match name {
        "PI" => quote!(::std::f64::consts::PI),
        "E" => quote!(::std::f64::consts::E),
        _ => return None,
    };
    syn::parse2(path).ok()
}
