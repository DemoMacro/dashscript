//! Records which locals a function body mutates, so a `.ds` `let` becomes Rust
//! `let mut` only when the binding is actually changed. Mutations come from:
//! assignment targets (`x =`, `x.y =`, `x[i] =`, and every compound/logical
//! assign), `++`/`--`, and mutator-method receivers (`x.push`, `x.sort`, …).
//!
//! The pass must be complete: a missed mutation would drop `mut` from a binding
//! that is then assigned, which is a hard compile error (worse than an
//! `unused_mut` warning). It walks every statement and expression kind the
//! translator supports; unfamiliar kinds fall through without recording.

use std::collections::HashSet;

use oxc_ast::ast::{
    AssignmentTarget, Expression, ForStatementInit, ObjectPropertyKind, SimpleAssignmentTarget,
    Statement,
};

use super::bindings;

/// Method names whose receiver they mutate, so the receiver needs `let mut`.
const MUTATORS: &[&str] = &[
    "push", "pop", "shift", "unshift", "sort", "reverse", "fill", "splice", "copyWithin",
];

/// The set of mutated local names (snake-cased) reachable from a body.
pub(super) fn collect_mutations(stmts: &[Statement]) -> HashSet<String> {
    let mut set = HashSet::new();
    for s in stmts {
        walk_stmt(s, &mut set);
    }
    set
}

fn walk_stmt(stmt: &Statement, set: &mut HashSet<String>) {
    match stmt {
        Statement::BlockStatement(b) => {
            for s in &b.body {
                walk_stmt(s, set);
            }
        }
        Statement::ExpressionStatement(es) => walk_expr(&es.expression, set),
        Statement::VariableDeclaration(v) => {
            for d in &v.declarations {
                if let Some(init) = &d.init {
                    walk_expr(init, set);
                }
            }
        }
        Statement::IfStatement(if_stmt) => {
            walk_expr(&if_stmt.test, set);
            walk_stmt(&if_stmt.consequent, set);
            if let Some(alt) = &if_stmt.alternate {
                walk_stmt(alt, set);
            }
        }
        Statement::WhileStatement(w) => {
            walk_expr(&w.test, set);
            walk_stmt(&w.body, set);
        }
        Statement::DoWhileStatement(dw) => {
            walk_stmt(&dw.body, set);
            walk_expr(&dw.test, set);
        }
        Statement::ForStatement(f) => {
            if let Some(ForStatementInit::VariableDeclaration(v)) = &f.init {
                for d in &v.declarations {
                    if let Some(i) = &d.init {
                        walk_expr(i, set);
                    }
                }
            }
            if let Some(test) = &f.test {
                walk_expr(test, set);
            }
            if let Some(update) = &f.update {
                walk_expr(update, set);
            }
            walk_stmt(&f.body, set);
        }
        Statement::ForOfStatement(fo) => walk_stmt(&fo.body, set),
        Statement::ForInStatement(fi) => walk_stmt(&fi.body, set),
        Statement::SwitchStatement(sw) => {
            walk_expr(&sw.discriminant, set);
            for c in &sw.cases {
                for s in &c.consequent {
                    walk_stmt(s, set);
                }
            }
        }
        Statement::ReturnStatement(r) => {
            if let Some(arg) = &r.argument {
                walk_expr(arg, set);
            }
        }
        Statement::ThrowStatement(t) => walk_expr(&t.argument, set),
        _ => {}
    }
}

fn walk_expr(expr: &Expression, set: &mut HashSet<String>) {
    match expr {
        Expression::AssignmentExpression(a) => {
            record_target(&a.left, set);
            walk_expr(&a.right, set);
        }
        Expression::UpdateExpression(u) => record_simple(&u.argument, set),
        Expression::CallExpression(c) => {
            walk_callee(&c.callee, set);
            for arg in &c.arguments {
                if let Some(e) = arg.as_expression() {
                    walk_expr(e, set);
                }
            }
        }
        Expression::BinaryExpression(b) => {
            walk_expr(&b.left, set);
            walk_expr(&b.right, set);
        }
        Expression::LogicalExpression(l) => {
            walk_expr(&l.left, set);
            walk_expr(&l.right, set);
        }
        Expression::UnaryExpression(u) => walk_expr(&u.argument, set),
        Expression::ConditionalExpression(c) => {
            walk_expr(&c.test, set);
            walk_expr(&c.consequent, set);
            walk_expr(&c.alternate, set);
        }
        Expression::ArrayExpression(a) => {
            for el in &a.elements {
                if let Some(e) = el.as_expression() {
                    walk_expr(e, set);
                }
            }
        }
        Expression::ObjectExpression(o) => {
            for p in &o.properties {
                if let ObjectPropertyKind::ObjectProperty(op) = p {
                    walk_expr(&op.value, set);
                }
            }
        }
        Expression::TemplateLiteral(t) => {
            for e in &t.expressions {
                walk_expr(e, set);
            }
        }
        Expression::ParenthesizedExpression(p) => walk_expr(&p.expression, set),
        Expression::StaticMemberExpression(sm) => walk_expr(&sm.object, set),
        Expression::ComputedMemberExpression(cm) => {
            walk_expr(&cm.object, set);
            walk_expr(&cm.expression, set);
        }
        Expression::SequenceExpression(s) => {
            for e in &s.expressions {
                walk_expr(e, set);
            }
        }
        _ => {}
    }
}

/// A call's callee: a mutator method `x.push(…)` records `x`; otherwise the
/// callee's sub-expressions are walked (e.g. `a.b.c()` walks `a.b`).
fn walk_callee(callee: &Expression, set: &mut HashSet<String>) {
    if let Expression::StaticMemberExpression(sm) = callee {
        if MUTATORS.contains(&sm.property.name.as_str()) {
            record_root(&sm.object, set);
            return;
        }
        walk_expr(&sm.object, set);
        return;
    }
    walk_expr(callee, set);
}

fn record_target(left: &AssignmentTarget, set: &mut HashSet<String>) {
    match left {
        AssignmentTarget::AssignmentTargetIdentifier(id) => {
            set.insert(bindings::snake(&id.name).to_string());
        }
        AssignmentTarget::StaticMemberExpression(sm) => record_root(&sm.object, set),
        AssignmentTarget::ComputedMemberExpression(cm) => record_root(&cm.object, set),
        _ => {}
    }
}

fn record_simple(target: &SimpleAssignmentTarget, set: &mut HashSet<String>) {
    if let SimpleAssignmentTarget::AssignmentTargetIdentifier(id) = target {
        set.insert(bindings::snake(&id.name).to_string());
    }
}

fn record_root(expr: &Expression, set: &mut HashSet<String>) {
    if let Some(name) = root_ident(expr) {
        set.insert(name);
    }
}

/// The leftmost identifier of a member chain (`a.b.c` → `a`), snake-cased.
fn root_ident(expr: &Expression) -> Option<String> {
    match expr {
        Expression::Identifier(id) => Some(bindings::snake(&id.name).to_string()),
        Expression::StaticMemberExpression(sm) => root_ident(&sm.object),
        Expression::ComputedMemberExpression(cm) => root_ident(&cm.object),
        Expression::ParenthesizedExpression(p) => root_ident(&p.expression),
        _ => None,
    }
}
