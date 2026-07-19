//! Number flavor inference: which `.ds` `number` locals are pure integers
//! (`i64`) vs doubles (`f64`). Conservative — the default is `f64` (an ES
//! `number` is an IEEE-754 double); a local is promoted to `i64` only when it
//! is initialized with an integer-valued expression and every value later
//! assigned to it is also integer-valued. A `: number` annotation or any
//! fractional / division / `Math.*` value forces `f64`.
//!
//! This closes the perf gap on integer-loop benches (`factorial`,
//! `loop-data-dependent`) where emitting the loop counter / accumulator as
//! `f64` costs a `f64 → i64 → i32 → f64 → usize` cast chain per iteration. See
//! the plan in `idempotent-crafting-bunny.md`.
//!
//! `usize` for indexing is a site-cast, never a flavor. Flavor inference is
//! scalar-only in the MVP — array elements stay `Vec<f64>` (an ES array's
//! element type is `number`).

use std::collections::{HashMap, HashSet};

use oxc_ast::ast::{AssignmentTarget, Expression, ForStatementInit, Statement, TSType};
use oxc_syntax::operator::{AssignmentOperator, BinaryOperator, UnaryOperator};

use super::context::Ctx;
use super::name_table::NameTable;

/// The flavor of a `.ds` `number` local: ES double (`f64`) or pure integer
/// (`i64`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NumberFlavor {
    F64,
    I64,
}

impl NumberFlavor {
    /// Flavor of a numeric literal value: integer (`i64`) iff finite,
    /// zero-fraction, and `|v| < 2^53` (the ES exact-integer range, so the
    /// `i64` round-trips exactly). `-0`, `NaN`, and `±Inf` are `F64`.
    pub(in crate::translator) fn literal(value: f64) -> Self {
        if value.is_finite() && value.fract() == 0.0 && value.abs() < (1u64 << 53) as f64 {
            Self::I64
        } else {
            Self::F64
        }
    }

    /// Infectious combine: any `F64` operand forces `F64` (ES coercion — one
    /// double operand makes the whole arithmetic double).
    pub(in crate::translator) fn combine(self, other: Self) -> Self {
        match (self, other) {
            (Self::F64, _) | (_, Self::F64) => Self::F64,
            (Self::I64, Self::I64) => Self::I64,
        }
    }
}

/// The flavor of an expression at translate time — synthesized bottom-up from
/// literal integer-ness and each identifier's inferred flavor. Used to decide
/// whether a sub-expression needs an `as f64` cast at an operator site.
/// Unknown / non-number expressions are `F64` (the safe default).
pub(in crate::translator) fn expr_flavor(expr: &Expression, ctx: &Ctx<'_>) -> NumberFlavor {
    match expr {
        Expression::NumericLiteral(n) => NumberFlavor::literal(n.value),
        Expression::Identifier(id) => ctx.local_flavor_for(id),
        Expression::UnaryExpression(u) => unary_flavor(u.operator, &u.argument, ctx),
        Expression::BinaryExpression(b) => binary_flavor(b.operator, &b.left, &b.right, ctx),
        Expression::ParenthesizedExpression(p) => expr_flavor(&p.expression, ctx),
        _ => NumberFlavor::F64,
    }
}

/// Per-operator flavor rule for a `BinaryExpression` (translate-time, with
/// `Ctx` so identifiers resolve to their inferred flavor).
fn binary_flavor(
    op: BinaryOperator,
    left: &Expression,
    right: &Expression,
    ctx: &Ctx<'_>,
) -> NumberFlavor {
    match op {
        // ES division and `**` are always floating-point.
        BinaryOperator::Division | BinaryOperator::Exponential => NumberFlavor::F64,
        // Bitwise ops emit through `bitwise_expr`, which casts the result back
        // to `f64` (a `.ds` `number`) — so the expression's flavor is `F64`,
        // matching the emit. (Phase 2 may promote operands to `i64`.)
        BinaryOperator::BitwiseAnd
        | BinaryOperator::BitwiseOR
        | BinaryOperator::BitwiseXOR
        | BinaryOperator::ShiftLeft
        | BinaryOperator::ShiftRight
        | BinaryOperator::ShiftRightZeroFill => NumberFlavor::F64,
        // Arithmetic propagates: integer iff both operands integral.
        _ => expr_flavor(left, ctx).combine(expr_flavor(right, ctx)),
    }
}

fn unary_flavor(op: UnaryOperator, arg: &Expression, ctx: &Ctx<'_>) -> NumberFlavor {
    match op {
        // `~x` emits through the bitwise path, which casts back to `f64`.
        UnaryOperator::BitwiseNot => NumberFlavor::F64,
        UnaryOperator::UnaryNegation | UnaryOperator::UnaryPlus => {
            // `-0` (negation of literal 0) must stay f64: `Object.is(-0, 0)` is
            // false in ES, and an i64 would erase the sign.
            if let Expression::NumericLiteral(n) = arg {
                if op == UnaryOperator::UnaryNegation && n.value == 0.0 {
                    return NumberFlavor::F64;
                }
                return NumberFlavor::literal(n.value);
            }
            expr_flavor(arg, ctx)
        }
        _ => NumberFlavor::F64,
    }
}

/// Infer every local's number flavor in a function body. Returns only locals
/// with a signal; absent names default to `F64` at query time.
pub(in crate::translator) fn infer(
    stmts: &[Statement],
    names: &NameTable,
) -> HashMap<String, NumberFlavor> {
    let mut force_f64: HashSet<String> = HashSet::new();
    let mut allow_i64: HashSet<String> = HashSet::new();
    for s in stmts {
        walk_stmt(s, names, &mut force_f64, &mut allow_i64);
    }
    allow_i64
        .iter()
        .map(|name| {
            (
                name.clone(),
                if force_f64.contains(name) {
                    NumberFlavor::F64
                } else {
                    NumberFlavor::I64
                },
            )
        })
        .collect()
}

/// Structural flavor (no `Ctx`): used during inference. An identifier takes
/// its flavor from what this pass has collected so far — a counter already in
/// `allow_i64` (from its initializer / `i++`) lets `sum += i` keep `sum`
/// integral. Single-pass approximation (sound because declarations precede
/// uses), not a full fixpoint.
fn structural_flavor(
    expr: &Expression,
    names: &NameTable,
    allow_i64: &HashSet<String>,
    force_f64: &HashSet<String>,
) -> NumberFlavor {
    match expr {
        Expression::NumericLiteral(n) => NumberFlavor::literal(n.value),
        Expression::Identifier(id) => {
            let n = names.of_reference(id).to_string();
            if allow_i64.contains(&n) && !force_f64.contains(&n) {
                NumberFlavor::I64
            } else {
                NumberFlavor::F64
            }
        }
        Expression::UnaryExpression(u) => match u.operator {
            // `~x` emits via `bitwise_expr` which casts back to `f64`; keep
            // the binding `f64` to match (Phase 2 may promote the operand).
            UnaryOperator::BitwiseNot => NumberFlavor::F64,
            UnaryOperator::UnaryNegation | UnaryOperator::UnaryPlus => {
                if let Expression::NumericLiteral(n) = &u.argument {
                    if u.operator == UnaryOperator::UnaryNegation && n.value == 0.0 {
                        return NumberFlavor::F64;
                    }
                    return NumberFlavor::literal(n.value);
                }
                structural_flavor(&u.argument, names, allow_i64, force_f64)
            }
            _ => NumberFlavor::F64,
        },
        Expression::BinaryExpression(b) => match b.operator {
            BinaryOperator::Division | BinaryOperator::Exponential => NumberFlavor::F64,
            // Bitwise ops emit via `bitwise_expr` which casts back to `f64`
            // (Phase 2 may promote); keep the binding `f64` to match the emit.
            BinaryOperator::BitwiseAnd
            | BinaryOperator::BitwiseOR
            | BinaryOperator::BitwiseXOR
            | BinaryOperator::ShiftLeft
            | BinaryOperator::ShiftRight
            | BinaryOperator::ShiftRightZeroFill => NumberFlavor::F64,
            _ => structural_flavor(&b.left, names, allow_i64, force_f64)
                .combine(structural_flavor(&b.right, names, allow_i64, force_f64)),
        },
        Expression::ParenthesizedExpression(p) => {
            structural_flavor(&p.expression, names, allow_i64, force_f64)
        }
        _ => NumberFlavor::F64,
    }
}

/// Fold an expression's structural flavor into the target variable's signals.
fn record(
    name: &str,
    expr: &Expression,
    names: &NameTable,
    force_f64: &mut HashSet<String>,
    allow_i64: &mut HashSet<String>,
) {
    match structural_flavor(expr, names, allow_i64, force_f64) {
        NumberFlavor::I64 => {
            allow_i64.insert(name.to_string());
        }
        NumberFlavor::F64 => {
            force_f64.insert(name.to_string());
        }
    }
}

fn walk_stmt(
    stmt: &Statement,
    names: &NameTable,
    force_f64: &mut HashSet<String>,
    allow_i64: &mut HashSet<String>,
) {
    match stmt {
        Statement::BlockStatement(b) => {
            for s in &b.body {
                walk_stmt(s, names, force_f64, allow_i64);
            }
        }
        Statement::ExpressionStatement(es) => {
            walk_expr(&es.expression, names, force_f64, allow_i64)
        }
        Statement::VariableDeclaration(v) => {
            for d in &v.declarations {
                declare_local(
                    &d.id,
                    d.type_annotation.as_deref(),
                    d.init.as_ref(),
                    names,
                    force_f64,
                    allow_i64,
                );
            }
        }
        Statement::IfStatement(if_stmt) => {
            walk_expr(&if_stmt.test, names, force_f64, allow_i64);
            walk_stmt(&if_stmt.consequent, names, force_f64, allow_i64);
            if let Some(alt) = &if_stmt.alternate {
                walk_stmt(alt, names, force_f64, allow_i64);
            }
        }
        Statement::WhileStatement(w) => {
            walk_expr(&w.test, names, force_f64, allow_i64);
            walk_stmt(&w.body, names, force_f64, allow_i64);
        }
        Statement::DoWhileStatement(dw) => {
            walk_stmt(&dw.body, names, force_f64, allow_i64);
            walk_expr(&dw.test, names, force_f64, allow_i64);
        }
        Statement::ForStatement(f) => {
            if let Some(ForStatementInit::VariableDeclaration(v)) = &f.init {
                for d in &v.declarations {
                    declare_local(
                        &d.id,
                        d.type_annotation.as_deref(),
                        d.init.as_ref(),
                        names,
                        force_f64,
                        allow_i64,
                    );
                }
            }
            if let Some(test) = &f.test {
                walk_expr(test, names, force_f64, allow_i64);
            }
            if let Some(update) = &f.update {
                walk_expr(update, names, force_f64, allow_i64);
            }
            walk_stmt(&f.body, names, force_f64, allow_i64);
        }
        Statement::ForOfStatement(fo) => walk_stmt(&fo.body, names, force_f64, allow_i64),
        Statement::ForInStatement(fi) => walk_stmt(&fi.body, names, force_f64, allow_i64),
        Statement::SwitchStatement(sw) => {
            walk_expr(&sw.discriminant, names, force_f64, allow_i64);
            for c in &sw.cases {
                for s in &c.consequent {
                    walk_stmt(s, names, force_f64, allow_i64);
                }
            }
        }
        Statement::ReturnStatement(r) => {
            if let Some(arg) = &r.argument {
                walk_expr(arg, names, force_f64, allow_i64);
            }
        }
        Statement::ThrowStatement(t) => walk_expr(&t.argument, names, force_f64, allow_i64),
        _ => {}
    }
}

/// Record a `let`/`const`/`var` binding's flavor signals from its annotation
/// and initializer.
fn declare_local(
    id: &oxc_ast::ast::BindingPattern,
    type_annotation: Option<&oxc_ast::ast::TSTypeAnnotation>,
    init: Option<&Expression>,
    names: &NameTable,
    force_f64: &mut HashSet<String>,
    allow_i64: &mut HashSet<String>,
) {
    let name = names.of_pattern(id).to_string();
    // A `: number` annotation forces f64 (R1: `let x: number = 5; x = 0.5`).
    if let Some(ta) = type_annotation {
        if matches!(ta.type_annotation, TSType::TSNumberKeyword(_)) {
            force_f64.insert(name.clone());
        }
    }
    if let Some(init) = init {
        record(&name, init, names, force_f64, allow_i64);
        walk_expr(init, names, force_f64, allow_i64);
    }
}

fn walk_expr(
    expr: &Expression,
    names: &NameTable,
    force_f64: &mut HashSet<String>,
    allow_i64: &mut HashSet<String>,
) {
    match expr {
        Expression::AssignmentExpression(asg) => {
            if let AssignmentTarget::AssignmentTargetIdentifier(id) = &asg.left {
                let name = names.of_reference(id).to_string();
                match asg.operator {
                    // `v = expr`: the RHS flavor is v's flavor.
                    AssignmentOperator::Assign => {
                        record(&name, &asg.right, names, force_f64, allow_i64);
                    }
                    // `v += int` keeps v integral; `v += 0.5` forces f64.
                    // Bitwise-compound (`&=`) yields an integer.
                    AssignmentOperator::Addition
                    | AssignmentOperator::Subtraction
                    | AssignmentOperator::Multiplication
                    | AssignmentOperator::Remainder => {
                        record(&name, &asg.right, names, force_f64, allow_i64);
                    }
                    // Bitwise compound emits via `bitwise_expr` which casts
                    // back to `f64`; force the target `f64` so the result
                    // matches (Phase 2 may promote the operand to `i64`).
                    AssignmentOperator::BitwiseAnd
                    | AssignmentOperator::BitwiseOR
                    | AssignmentOperator::BitwiseXOR
                    | AssignmentOperator::ShiftLeft
                    | AssignmentOperator::ShiftRight
                    | AssignmentOperator::ShiftRightZeroFill => {
                        force_f64.insert(name);
                    }
                    // `/=` and `**=` always produce a double.
                    AssignmentOperator::Division | AssignmentOperator::Exponential => {
                        force_f64.insert(name);
                    }
                    _ => {}
                }
            }
            walk_expr(&asg.right, names, force_f64, allow_i64);
        }
        Expression::UpdateExpression(u) => {
            if let oxc_ast::ast::SimpleAssignmentTarget::AssignmentTargetIdentifier(id) =
                &u.argument
            {
                // `v++` / `v--`: the +1/-1 step is integral.
                allow_i64.insert(names.of_reference(id).to_string());
            }
        }
        Expression::BinaryExpression(b) => {
            walk_expr(&b.left, names, force_f64, allow_i64);
            walk_expr(&b.right, names, force_f64, allow_i64);
        }
        Expression::LogicalExpression(l) => {
            walk_expr(&l.left, names, force_f64, allow_i64);
            walk_expr(&l.right, names, force_f64, allow_i64);
        }
        Expression::UnaryExpression(u) => walk_expr(&u.argument, names, force_f64, allow_i64),
        Expression::ConditionalExpression(c) => {
            walk_expr(&c.test, names, force_f64, allow_i64);
            walk_expr(&c.consequent, names, force_f64, allow_i64);
            walk_expr(&c.alternate, names, force_f64, allow_i64);
        }
        Expression::CallExpression(c) => {
            for arg in &c.arguments {
                if let Some(e) = arg.as_expression() {
                    walk_expr(e, names, force_f64, allow_i64);
                }
            }
            match &c.callee {
                Expression::StaticMemberExpression(sm) => {
                    walk_expr(&sm.object, names, force_f64, allow_i64)
                }
                Expression::ComputedMemberExpression(cm) => {
                    walk_expr(&cm.object, names, force_f64, allow_i64);
                    walk_expr(&cm.expression, names, force_f64, allow_i64);
                }
                _ => {}
            }
        }
        Expression::ArrayExpression(arr) => {
            for el in &arr.elements {
                if let Some(e) = el.as_expression() {
                    walk_expr(e, names, force_f64, allow_i64);
                }
            }
        }
        Expression::ObjectExpression(o) => {
            for p in &o.properties {
                if let oxc_ast::ast::ObjectPropertyKind::ObjectProperty(op) = p {
                    walk_expr(&op.value, names, force_f64, allow_i64);
                }
            }
        }
        Expression::TemplateLiteral(t) => {
            for e in &t.expressions {
                walk_expr(e, names, force_f64, allow_i64);
            }
        }
        Expression::ParenthesizedExpression(p) => {
            walk_expr(&p.expression, names, force_f64, allow_i64)
        }
        Expression::TSNonNullExpression(nn) => {
            walk_expr(&nn.expression, names, force_f64, allow_i64)
        }
        Expression::StaticMemberExpression(sm) => {
            walk_expr(&sm.object, names, force_f64, allow_i64)
        }
        Expression::ComputedMemberExpression(cm) => {
            walk_expr(&cm.object, names, force_f64, allow_i64);
            walk_expr(&cm.expression, names, force_f64, allow_i64);
        }
        Expression::SequenceExpression(s) => {
            for e in &s.expressions {
                walk_expr(e, names, force_f64, allow_i64);
            }
        }
        _ => {}
    }
}
