//! Two facts about a function body, gathered in a single walk:
//!
//! 1. **Mutations** — which locals are assigned / updated / mutated via a
//!    mutator-method receiver, so a `.ds` `let` becomes `let mut` only when
//!    the binding actually changes.
//! 2. **Use counts** — how often each local is *read*. A non-`Copy` local read
//!    more than once cannot be moved on its first read (a later read would see
//!    a moved value), so it must be cloned when passed by value to a user
//!    function. See `clone_owned_local`.
//!
//! Both passes must be complete: a missed mutation drops `mut` from a binding
//! that is then assigned (hard compile error), and a missed read under-counts
//! a local so it moves instead of clones (also a hard error). Every statement
//! and expression kind the translator supports is walked; unfamiliar kinds
//! fall through without recording.

use std::collections::{HashMap, HashSet};

use oxc_ast::ast::{
    AssignmentTarget, Expression, ForStatementInit, IdentifierReference, ObjectPropertyKind,
    SimpleAssignmentTarget, Statement,
};

use super::bindings;

/// Method names whose receiver they mutate, so the receiver needs `let mut`.
const MUTATORS: &[&str] = &[
    "push", "pop", "shift", "unshift", "sort", "reverse", "fill", "splice", "copyWithin",
];

/// The body facts: the set of mutated bindings and per-local read counts.
#[derive(Default)]
pub(super) struct Analysis {
    pub mutated: HashSet<String>,
    pub use_counts: HashMap<String, u32>,
}

/// Walk a function body once, recording mutations and read counts.
pub(super) fn analyze(stmts: &[Statement]) -> Analysis {
    let mut a = Analysis::default();
    for s in stmts {
        walk_stmt(s, &mut a);
    }
    a
}

/// Whether `stmt` reads the local `needle` (snake-cased) anywhere — as a bare
/// identifier or a non-null assertion `needle!`. Used to decide whether an
/// `if (opt)` narrowing binds the inner value (`Some(x)`) or discards it
/// (`Some(_)`), so an unused binding never triggers `unused_variables`.
pub(super) fn references(stmt: &Statement, needle: &str) -> bool {
    let mut a = Analysis::default();
    walk_stmt(stmt, &mut a);
    a.use_counts.contains_key(needle)
}

fn walk_stmt(stmt: &Statement, a: &mut Analysis) {
    match stmt {
        Statement::BlockStatement(b) => {
            for s in &b.body {
                walk_stmt(s, a);
            }
        }
        Statement::ExpressionStatement(es) => walk_expr(&es.expression, a),
        Statement::VariableDeclaration(v) => {
            for d in &v.declarations {
                if let Some(init) = &d.init {
                    walk_expr(init, a);
                }
            }
        }
        Statement::IfStatement(if_stmt) => {
            walk_expr(&if_stmt.test, a);
            walk_stmt(&if_stmt.consequent, a);
            if let Some(alt) = &if_stmt.alternate {
                walk_stmt(alt, a);
            }
        }
        Statement::WhileStatement(w) => {
            walk_expr(&w.test, a);
            walk_stmt(&w.body, a);
        }
        Statement::DoWhileStatement(dw) => {
            walk_stmt(&dw.body, a);
            walk_expr(&dw.test, a);
        }
        Statement::ForStatement(f) => {
            if let Some(ForStatementInit::VariableDeclaration(v)) = &f.init {
                for d in &v.declarations {
                    if let Some(i) = &d.init {
                        walk_expr(i, a);
                    }
                }
            }
            if let Some(test) = &f.test {
                walk_expr(test, a);
            }
            if let Some(update) = &f.update {
                walk_expr(update, a);
            }
            walk_stmt(&f.body, a);
        }
        Statement::ForOfStatement(fo) => walk_stmt(&fo.body, a),
        Statement::ForInStatement(fi) => walk_stmt(&fi.body, a),
        Statement::SwitchStatement(sw) => {
            walk_expr(&sw.discriminant, a);
            for c in &sw.cases {
                for s in &c.consequent {
                    walk_stmt(s, a);
                }
            }
        }
        Statement::ReturnStatement(r) => {
            if let Some(arg) = &r.argument {
                walk_expr(arg, a);
            }
        }
        Statement::ThrowStatement(t) => walk_expr(&t.argument, a),
        _ => {}
    }
}

fn walk_expr(expr: &Expression, a: &mut Analysis) {
    match expr {
        // A bare identifier is a read of that local (counted), except inside
        // an assignment/update LHS, where mutation/recording is handled there.
        Expression::Identifier(id) => count_read(id, a),
        Expression::AssignmentExpression(asg) => {
            record_target(&asg.left, a);
            walk_expr(&asg.right, a);
        }
        Expression::UpdateExpression(u) => record_simple(&u.argument, a),
        Expression::CallExpression(c) => {
            walk_callee(&c.callee, a);
            for arg in &c.arguments {
                if let Some(e) = arg.as_expression() {
                    walk_expr(e, a);
                }
            }
        }
        Expression::BinaryExpression(b) => {
            walk_expr(&b.left, a);
            walk_expr(&b.right, a);
        }
        Expression::LogicalExpression(l) => {
            walk_expr(&l.left, a);
            walk_expr(&l.right, a);
        }
        Expression::UnaryExpression(u) => walk_expr(&u.argument, a),
        Expression::ConditionalExpression(c) => {
            walk_expr(&c.test, a);
            walk_expr(&c.consequent, a);
            walk_expr(&c.alternate, a);
        }
        Expression::ArrayExpression(arr) => {
            for el in &arr.elements {
                if let Some(e) = el.as_expression() {
                    walk_expr(e, a);
                }
            }
        }
        Expression::ObjectExpression(o) => {
            for p in &o.properties {
                if let ObjectPropertyKind::ObjectProperty(op) = p {
                    walk_expr(&op.value, a);
                }
            }
        }
        Expression::TemplateLiteral(t) => {
            for e in &t.expressions {
                walk_expr(e, a);
            }
        }
        Expression::ParenthesizedExpression(p) => walk_expr(&p.expression, a),
        // `x!` reads `x` — a non-null assertion dereferences the Option value.
        Expression::TSNonNullExpression(nn) => walk_expr(&nn.expression, a),
        Expression::StaticMemberExpression(sm) => walk_expr(&sm.object, a),
        Expression::ComputedMemberExpression(cm) => {
            walk_expr(&cm.object, a);
            walk_expr(&cm.expression, a);
        }
        Expression::SequenceExpression(s) => {
            for e in &s.expressions {
                walk_expr(e, a);
            }
        }
        _ => {}
    }
}

/// A call's callee: a mutator method `x.push(…)` mutates *and* reads `x`
/// (`&mut self`); otherwise the callee's sub-expressions are walked normally
/// (e.g. `a.b.c()` reads `a.b`).
fn walk_callee(callee: &Expression, a: &mut Analysis) {
    if let Expression::StaticMemberExpression(sm) = callee {
        if MUTATORS.contains(&sm.property.name.as_str()) {
            record_root(&sm.object, a);
            return;
        }
        walk_expr(&sm.object, a);
        return;
    }
    walk_expr(callee, a);
}

/// An assignment LHS. A plain `x = …` rebinds `x` (mutated, not read); a
/// member target `x.y = …` / `x[i] = …` takes `&mut x` (mutated *and* read).
fn record_target(left: &AssignmentTarget, a: &mut Analysis) {
    match left {
        AssignmentTarget::AssignmentTargetIdentifier(id) => {
            record_mutation(&id.name, a);
        }
        AssignmentTarget::StaticMemberExpression(sm) => record_root(&sm.object, a),
        AssignmentTarget::ComputedMemberExpression(cm) => {
            record_root(&cm.object, a);
            walk_expr(&cm.expression, a);
        }
        _ => {}
    }
}

/// An update (`x++` / `x--`) takes `&mut x`: mutated *and* read.
fn record_simple(target: &SimpleAssignmentTarget, a: &mut Analysis) {
    if let SimpleAssignmentTarget::AssignmentTargetIdentifier(id) = target {
        record_mutation(&id.name, a);
        count_name(&id.name, a);
    }
}

/// The leftmost identifier of a member chain (`a.b.c` → `a`): mutated and read
/// (the chain borrows it through `&mut`).
fn record_root(expr: &Expression, a: &mut Analysis) {
    if let Some(name) = root_ident(expr) {
        a.mutated.insert(name.clone());
        *a.use_counts.entry(name).or_default() += 1;
    }
}

fn record_mutation(name: &str, a: &mut Analysis) {
    a.mutated.insert(bindings::snake(name).to_string());
}

fn count_name(name: &str, a: &mut Analysis) {
    *a.use_counts.entry(bindings::snake(name).to_string()).or_default() += 1;
}

/// A read of a bare identifier local (`undefined` is not a value local).
fn count_read(id: &IdentifierReference, a: &mut Analysis) {
    if id.name.as_str() == "undefined" {
        return;
    }
    count_name(&id.name, a);
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
