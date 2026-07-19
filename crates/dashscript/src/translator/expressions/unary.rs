//! Unary, conditional, and non-null expressions.
//! `-`/`!`/`~` → Rust unary; `cond ? a : b` → `if`; `x!` → `unwrap`.

use oxc_ast::ast::{ConditionalExpression, Expression, TSNonNullExpression, UnaryExpression};
use oxc_syntax::operator::UnaryOperator;
use proc_macro2::Span;
use syn::{parse_quote, Expr, LitStr, UnOp};

use super::super::bindings;
use super::super::builtins;
use super::super::context::Ctx;
use super::translate_expr;

/// Unary `-`/`!`/`~`. (`+` is a no-op; `typeof`, `void`, `delete` are unmapped.)
pub(super) fn unary_expr(un: &UnaryExpression, ctx: &Ctx<'_>) -> Expr {
    let arg = translate_expr(&un.argument, ctx);
    match un.operator {
        // `+x` is ES ToNumber. A string operand needs the full StringToNumber
        // (hex/binary/octal/decimal/Infinity) — `+"0xff"` is 255, not the
        // string "0xff"; a number operand passes through unchanged.
        UnaryOperator::UnaryPlus => {
            if expr_is_string(&un.argument, ctx) {
                builtins::to_number_expr(arg)
            } else {
                arg
            }
        }
        UnaryOperator::UnaryNegation => Expr::Unary(syn::ExprUnary {
            attrs: Vec::new(),
            op: UnOp::Neg(Default::default()),
            expr: Box::new(arg),
        }),
        UnaryOperator::LogicalNot => Expr::Unary(syn::ExprUnary {
            attrs: Vec::new(),
            op: UnOp::Not(Default::default()),
            expr: Box::new(arg),
        }),
        // `~a` → `!ToInt32(a) as f64` (TS `~` is 32-bit bitwise NOT). The
        // operand casts via `bitwise_operand` (f64 → i64 → i32 for the JS
        // `ToInt32` wrap; i64 skips the hop); bound to a local so `as` never
        // binds into a compound operand.
        UnaryOperator::BitwiseNot => {
            let a = super::bitwise_operand(&un.argument, ctx, true);
            parse_quote!({
                let __a = #a;
                (!__a) as f64
            })
        }
        // `typeof x` is a compile-time type query (DashScript is statically
        // typed), so the JS type string is known from the operand's spelling.
        UnaryOperator::Typeof => type_of_expr(&un.argument),
        _ => parse_quote!(::core::todo!()),
    }
}

/// True when `expr` evaluates to a string: a string literal (possibly
/// parenthesized), or an identifier bound to a `string` local. Drives unary
/// `+` to run ToNumber only on a string operand (a number is a no-op).
fn expr_is_string(e: &Expression, ctx: &Ctx<'_>) -> bool {
    match e {
        Expression::StringLiteral(_) => true,
        Expression::ParenthesizedExpression(p) => expr_is_string(&p.expression, ctx),
        Expression::Identifier(id) => {
            let name = bindings::snake(&id.name).to_string();
            ctx.local_type(&name).is_some_and(|p| p.is_ident("String"))
        }
        _ => false,
    }
}

/// `typeof x` — the JS type string, known at translate time from the
/// operand's spelling (DashScript is statically typed, so this is a compile-
/// time query, not a runtime check). `typeof <number>` → `"number"`,
/// `<string>` → `"string"`, `<boolean>` → `"boolean"`, `typeof null` →
/// `"object"` (the JS quirk), `typeof Math.<const>`/`Number.<const>` →
/// `"number"`, `typeof Math.<method>`/`Number.<method>` → `"function"` (a
/// function reference), `typeof Array`/`Object`/… → `"function"` (a global
/// builtin constructor is callable). Anything else falls back to `"object"`.
/// Returned as a Rust `String`.
fn type_of_expr(arg: &Expression) -> Expr {
    let s: &str = match arg {
        Expression::NumericLiteral(_) => "number",
        Expression::StringLiteral(_) => "string",
        Expression::BooleanLiteral(_) => "boolean",
        // JS `typeof null === "object"` — the famous bug, kept for conformance.
        Expression::NullLiteral(_) => "object",
        Expression::StaticMemberExpression(sm) if builtins::is_ident(&sm.object, "Math") => {
            // `Math.<constant>` is a number; `Math.<method>` is a function ref.
            if builtins::math_constant(&sm.property.name).is_some() {
                "number"
            } else {
                "function"
            }
        }
        Expression::StaticMemberExpression(sm) if builtins::is_ident(&sm.object, "Number") => {
            // `Number.<constant>` (MAX_VALUE/EPSILON/…) is a number;
            // `Number.<method>` (isInteger/parseInt/…) is a function ref.
            if builtins::number_constant(&sm.property.name).is_some() {
                "number"
            } else {
                "function"
            }
        }
        Expression::FunctionExpression(_) | Expression::ArrowFunctionExpression(_) => "function",
        // A global builtin constructor is callable (`typeof Array === "function"`).
        // Namespace objects (`Math`/`JSON`/`Reflect`/`Atomics`/`Intl`/`globalThis`)
        // are not — `typeof === "object"`; a user identifier also falls back to
        // "object" (a precise answer for a user symbol needs type inference).
        Expression::Identifier(id) => match id.name.as_str() {
            // Namespace objects — not callable, `typeof === "object"`.
            "Math" | "JSON" | "Reflect" | "Atomics" | "Intl" | "globalThis" => "object",
            // Global constructors — callable, `typeof === "function"`.
            "Array"
            | "Object"
            | "String"
            | "Number"
            | "Boolean"
            | "Symbol"
            | "Function"
            | "Date"
            | "RegExp"
            | "Error"
            | "TypeError"
            | "RangeError"
            | "SyntaxError"
            | "ReferenceError"
            | "EvalError"
            | "URIError"
            | "AggregateError"
            | "SuppressedError"
            | "Promise"
            | "Map"
            | "Set"
            | "WeakMap"
            | "WeakSet"
            | "WeakRef"
            | "FinalizationRegistry"
            | "ArrayBuffer"
            | "SharedArrayBuffer"
            | "DataView"
            | "BigInt"
            | "Proxy"
            | "Int8Array"
            | "Uint8Array"
            | "Uint8ClampedArray"
            | "Int16Array"
            | "Uint16Array"
            | "Int32Array"
            | "Uint32Array"
            | "Float32Array"
            | "Float64Array"
            | "BigInt64Array"
            | "BigUint64Array" => "function",
            _ => "object",
        },
        _ => "object",
    };
    let lit = LitStr::new(s, Span::call_site());
    parse_quote!(#lit.to_string())
}

/// `cond ? a : b` → `if cond { a } else { b }` — Rust's `if` is an expression.
pub(super) fn conditional_expr(c: &ConditionalExpression, ctx: &Ctx<'_>) -> Expr {
    let test = translate_expr(&c.test, ctx);
    let then = translate_expr(&c.consequent, ctx);
    let els = translate_expr(&c.alternate, ctx);
    parse_quote!(if #test { #then } else { #els })
}

/// `x!` (TS non-null assertion) → `x.unwrap()`. The author asserts non-null, so
/// a panic on `None` is their explicit choice, not an implicit assumption.
pub(super) fn nonnull_expr(nn: &TSNonNullExpression, ctx: &Ctx<'_>) -> Expr {
    // Inside an `if (opt)` narrowing, `opt!` reads the bound inner value
    // directly — no `Option::unwrap` after an `is_some` check.
    if let Expression::Identifier(id) = &nn.expression {
        if ctx.is_narrowed_some(&bindings::snake(&id.name).to_string()) {
            return translate_expr(&nn.expression, ctx);
        }
    }
    let inner = translate_expr(&nn.expression, ctx);
    parse_quote!(#inner.unwrap())
}
