//! Unary, conditional, and non-null expressions.
//! `-`/`!`/`~` → Rust unary; `cond ? a : b` → `if`; `x!` → `unwrap`.

use oxc_ast::ast::{ConditionalExpression, Expression, TSNonNullExpression, UnaryExpression};
use oxc_syntax::operator::UnaryOperator;
use syn::{parse_quote, Expr, UnOp};

use super::super::bindings;
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
        _ => parse_quote!(::core::todo!()),
    }
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
