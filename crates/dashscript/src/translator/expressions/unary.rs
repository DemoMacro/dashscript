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
        UnaryOperator::UnaryPlus => arg,
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
        // `~a` → `!(a as i32) as f64` (TS `~` is 32-bit bitwise NOT).
        UnaryOperator::BitwiseNot => parse_quote!((!(#arg as i32)) as f64),
        // `typeof x` is a compile-time type query (DashScript is statically
        // typed), so the JS type string is known from the operand's spelling.
        UnaryOperator::Typeof => type_of_expr(&un.argument),
        _ => parse_quote!(::core::todo!()),
    }
}

/// `typeof x` — the JS type string, known at translate time from the
/// operand's spelling (DashScript is statically typed, so this is a compile-
/// time query, not a runtime check). `typeof <number>` → `"number"`,
/// `<string>` → `"string"`, `<boolean>` → `"boolean"`, `typeof null` →
/// `"object"` (the JS quirk), `typeof Math.<const>` → `"number"`, `typeof
/// Math.<method>` → `"function"` (a function reference). Anything else falls
/// back to `"object"`. Returned as a Rust `String`.
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
        Expression::FunctionExpression(_) | Expression::ArrowFunctionExpression(_) => "function",
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
