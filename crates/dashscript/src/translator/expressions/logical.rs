//! Logical operators (`&&`/`||`/`??`) and truthiness. A bool operand
//! short-circuits as Rust `&&`/`||`; a value operand routes through a
//! truthiness block (TS `a || b` returns `a` when truthy, else `b`).

use oxc_ast::ast::{AssignmentTarget, Expression, LogicalExpression};
use oxc_syntax::operator::LogicalOperator;
use quote::format_ident;
use syn::{parse_quote, BinOp, Expr, Ident};

use super::super::bindings;
use super::super::context::Ctx;
use super::bool_expr;
use super::option_local_name;
use super::translate_expr;

/// Method names whose result is a `bool` — `&&`/`||` short-circuit on these
/// directly instead of routing through a truthiness block. The translator has no
/// type info for call results, so this is a curated list of common predicates.
const BOOL_METHODS: &[&str] = &[
    "includes",
    "startsWith",
    "endsWith", // string / array
    "some",
    "every",   // array
    "isArray", // Array
    "isNaN",
    "isFinite",
    "isInteger",
    "isSafeInteger", // Number
    "hasOwnProperty",
    "isPrototypeOf",
    "propertyIsEnumerable", // Object
    "isFrozen",
    "isSealed",
    "isExtensible", // Object (no-op introspection)
];

/// `&&`/`||` are a separate `LogicalExpression` in oxc (not `BinaryExpression`).
/// `??` (nullish coalescing) maps to `Option::unwrap_or_else` (see `coalesce_expr`).
pub(super) fn logical_expr(log: &LogicalExpression, ctx: &Ctx<'_>) -> Expr {
    if matches!(log.operator, LogicalOperator::Coalesce) {
        return coalesce_expr(&log.left, &log.right, ctx);
    }
    // A `bool` left operand short-circuits as Rust `&&`/`||` (TS `bool || bool`
    // is itself a bool). A value operand uses a truthiness block: TS `a || b`
    // returns `a` when truthy else `b`, so bind `a` once and branch.
    if expr_is_bool(&log.left, ctx) {
        let left = translate_expr(&log.left, ctx);
        let right = translate_expr(&log.right, ctx);
        let op = match log.operator {
            LogicalOperator::And => BinOp::And(Default::default()),
            LogicalOperator::Or => BinOp::Or(Default::default()),
            LogicalOperator::Coalesce => unreachable!(),
        };
        return Expr::Binary(syn::ExprBinary {
            attrs: Vec::new(),
            left: Box::new(left),
            op,
            right: Box::new(right),
        });
    }
    let left = translate_expr(&log.left, ctx);
    let right = translate_expr(&log.right, ctx);
    let cond = truthy_cond(&log.left, ctx);
    match log.operator {
        // `a || b`: a truthy → a, else b
        LogicalOperator::Or => parse_quote!({ let __l = #left; if #cond { __l } else { #right } }),
        // `a && b`: a truthy → b, else a
        LogicalOperator::And => parse_quote!({ let __l = #left; if #cond { #right } else { __l } }),
        LogicalOperator::Coalesce => unreachable!(),
    }
}

/// `a ?? b`: when `a` is an `Option`-typed local, `a.unwrap_or_else(|| b)`; when
/// `a` is non-nullable the result is just `a` (a `??` on a never-null value is a
/// no-op).
fn coalesce_expr(left: &Expression, right: &Expression, ctx: &Ctx<'_>) -> Expr {
    let right_val = translate_expr(right, ctx);
    if let Some(name) = option_local_name(left, ctx) {
        let ident = bindings::snake(name);
        return parse_quote!(#ident.unwrap_or_else(|| #right_val));
    }
    // An optional-chain result is itself an `Option`, so the right side
    // supplies the default via `unwrap_or`.
    if matches!(left, Expression::ChainExpression(_)) {
        let left_val = translate_expr(left, ctx);
        return parse_quote!(#left_val.unwrap_or(#right_val));
    }
    translate_expr(left, ctx)
}

/// A truthiness test for the block-local `__l`, picking the check by the
/// original left operand's type — used by `||`/`&&` value semantics. Mirrors
/// `builtins::global`'s `bool_cast` but references `__l` rather than
/// re-evaluating the operand.
pub(super) fn truthy_cond(left: &Expression, ctx: &Ctx<'_>) -> Expr {
    let l: Ident = format_ident!("__l");
    match left {
        Expression::StringLiteral(_) => parse_quote!(!#l.is_empty()),
        Expression::Identifier(id) => {
            let name = bindings::snake(&id.name).to_string();
            let last = ctx
                .local_type(&name)
                .and_then(|p| p.segments.last())
                .map(|s| s.ident.to_string());
            match last.as_deref() {
                Some("Vec") | Some("HashMap") | Some("String") => parse_quote!(!#l.is_empty()),
                Some("Option") => parse_quote!(#l.is_some()),
                Some("bool") => parse_quote!(#l),
                _ => parse_quote!(#l != 0_f64),
            }
        }
        Expression::CallExpression(call)
            if matches!(&call.callee, Expression::StaticMemberExpression(sm)
                if BOOL_METHODS.contains(&sm.property.name.as_str())) =>
        {
            parse_quote!(#l)
        }
        Expression::NumericLiteral(n) => bool_expr(n.value != 0_f64 && !n.value.is_nan()),
        Expression::BooleanLiteral(b) => bool_expr(b.value),
        _ => parse_quote!(#l != 0_f64),
    }
}

/// True when `expr` is a `bool` operand (a `BooleanLiteral`, a comparison, a
/// logical not, a predicate method call, or a local annotated `boolean`) —
/// those short-circuit as Rust `&&`/`||` instead of routing through a
/// truthiness block (which would produce `bool != 0_f64` and fail to compile).
pub(super) fn expr_is_bool(expr: &Expression, ctx: &Ctx<'_>) -> bool {
    match expr {
        Expression::BooleanLiteral(_) => true,
        // `a && b` / `a || b` of bool operands is itself bool — a predicate
        // chain like `isInteger(n) && isFinite(n)` short-circuits as Rust `&&`.
        Expression::LogicalExpression(log)
            if matches!(
                log.operator,
                oxc_ast::ast::LogicalOperator::And | oxc_ast::ast::LogicalOperator::Or
            ) =>
        {
            expr_is_bool(&log.left, ctx) && expr_is_bool(&log.right, ctx)
        }
        // A comparison (`<`, `>`, `==`, `!=`, `<=`, `>=`, strict or not) yields
        // bool — `v > 5 && v < 25` short-circuits as Rust `&&`.
        Expression::BinaryExpression(b)
            if matches!(
                b.operator,
                oxc_ast::ast::BinaryOperator::LessThan
                    | oxc_ast::ast::BinaryOperator::GreaterThan
                    | oxc_ast::ast::BinaryOperator::LessEqualThan
                    | oxc_ast::ast::BinaryOperator::GreaterEqualThan
                    | oxc_ast::ast::BinaryOperator::Equality
                    | oxc_ast::ast::BinaryOperator::Inequality
                    | oxc_ast::ast::BinaryOperator::StrictEquality
                    | oxc_ast::ast::BinaryOperator::StrictInequality
            ) =>
        {
            true
        }
        // `!x` (logical not) yields bool.
        Expression::UnaryExpression(u)
            if matches!(u.operator, oxc_ast::ast::UnaryOperator::LogicalNot) =>
        {
            true
        }
        // A predicate method *call* (`s.includes(...)`, `xs.some(...)`) returns
        // bool — the outer node is a `CallExpression` whose callee is the member.
        Expression::CallExpression(call) => match &call.callee {
            Expression::StaticMemberExpression(sm) => {
                BOOL_METHODS.contains(&sm.property.name.as_str())
            }
            _ => false,
        },
        Expression::Identifier(id) => {
            let name = bindings::snake(&id.name).to_string();
            ctx.local_type(&name)
                .and_then(|p| p.segments.last())
                .is_some_and(|s| s.ident == "bool")
        }
        _ => false,
    }
}

/// The truthiness test for an assignment target, picking the check by its
/// declared type (an identifier local) — used by `||=`/`&&=`. Falls back to a
/// numeric `!= 0_f64` when the type is unknown or the target isn't an identifier.
pub(super) fn assign_truthy(left: &AssignmentTarget, target: &Expr, ctx: &Ctx<'_>) -> Expr {
    if let AssignmentTarget::AssignmentTargetIdentifier(id) = left {
        let name = bindings::snake(&id.name).to_string();
        let last = ctx
            .local_type(&name)
            .and_then(|p| p.segments.last())
            .map(|s| s.ident.to_string());
        return match last.as_deref() {
            Some("Vec") | Some("HashMap") | Some("String") => parse_quote!(!#target.is_empty()),
            Some("Option") => parse_quote!(#target.is_some()),
            Some("bool") => parse_quote!(#target),
            _ => parse_quote!(#target != 0_f64),
        };
    }
    parse_quote!(#target != 0_f64)
}
