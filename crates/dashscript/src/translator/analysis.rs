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
//!
//! Keys are the per-symbol Rust name from [`super::name_table::NameTable`] (not
//! the lossy `snake(name)` fold), so two bindings that collapse to one
//! snake-name (`N` and `n`) stay distinct — a mutation of `n` never leaks onto
//! `N`'s `let mut` decision.

use std::collections::{HashMap, HashSet};

use oxc_ast::ast::{
    AssignmentTarget, Expression, ForStatementInit, IdentifierReference, ObjectPropertyKind,
    SimpleAssignmentTarget, Statement,
};

use super::name_table::NameTable;

/// Method names whose receiver they mutate, so the receiver needs `let mut`.
const MUTATORS: &[&str] = &[
    "push",
    "pop",
    "shift",
    "unshift",
    "sort",
    "reverse",
    "fill",
    "splice",
    "copyWithin",
    // ES Map/Set mutators — the receiver takes `&mut self`.
    "set",
    "add",
    "delete",
    "clear",
];

/// The body facts: the set of mutated bindings and per-local read counts.
#[derive(Default)]
pub(super) struct Analysis {
    pub mutated: HashSet<String>,
    pub use_counts: HashMap<String, u32>,
    /// True when the body assigns/updates a member of `this` (e.g. `this.x = 1`
    /// or `this.n++`) — the enclosing method needs `&mut self`.
    pub mutates_this: bool,
}

/// Walk a function body once, recording mutations and read counts.
pub(super) fn analyze(stmts: &[Statement], names: &NameTable) -> Analysis {
    let mut a = Analysis::default();
    for s in stmts {
        walk_stmt(s, names, &mut a);
    }
    a
}

/// Whether `stmt` reads the local `needle` (its per-symbol Rust name) anywhere
/// — as a bare identifier or a non-null assertion `needle!`. Used to decide
/// whether an `if (opt)` narrowing binds the inner value (`Some(x)`) or
/// discards it (`Some(_)`), so an unused binding never triggers
/// `unused_variables`.
pub(super) fn references(stmt: &Statement, needle: &str, names: &NameTable) -> bool {
    let mut a = Analysis::default();
    walk_stmt(stmt, names, &mut a);
    a.use_counts.contains_key(needle)
}

fn walk_stmt(stmt: &Statement, names: &NameTable, a: &mut Analysis) {
    match stmt {
        Statement::BlockStatement(b) => {
            for s in &b.body {
                walk_stmt(s, names, a);
            }
        }
        Statement::ExpressionStatement(es) => walk_expr(&es.expression, names, a),
        Statement::VariableDeclaration(v) => {
            for d in &v.declarations {
                if let Some(init) = &d.init {
                    walk_expr(init, names, a);
                }
            }
        }
        Statement::IfStatement(if_stmt) => {
            walk_expr(&if_stmt.test, names, a);
            walk_stmt(&if_stmt.consequent, names, a);
            if let Some(alt) = &if_stmt.alternate {
                walk_stmt(alt, names, a);
            }
        }
        Statement::WhileStatement(w) => {
            walk_expr(&w.test, names, a);
            walk_stmt(&w.body, names, a);
        }
        Statement::DoWhileStatement(dw) => {
            walk_stmt(&dw.body, names, a);
            walk_expr(&dw.test, names, a);
        }
        Statement::ForStatement(f) => {
            if let Some(ForStatementInit::VariableDeclaration(v)) = &f.init {
                for d in &v.declarations {
                    if let Some(i) = &d.init {
                        walk_expr(i, names, a);
                    }
                }
            }
            if let Some(test) = &f.test {
                walk_expr(test, names, a);
            }
            if let Some(update) = &f.update {
                walk_expr(update, names, a);
            }
            walk_stmt(&f.body, names, a);
        }
        Statement::ForOfStatement(fo) => walk_stmt(&fo.body, names, a),
        Statement::ForInStatement(fi) => walk_stmt(&fi.body, names, a),
        Statement::SwitchStatement(sw) => {
            walk_expr(&sw.discriminant, names, a);
            for c in &sw.cases {
                for s in &c.consequent {
                    walk_stmt(s, names, a);
                }
            }
        }
        Statement::ReturnStatement(r) => {
            if let Some(arg) = &r.argument {
                walk_expr(arg, names, a);
            }
        }
        Statement::ThrowStatement(t) => walk_expr(&t.argument, names, a),
        _ => {}
    }
}

fn walk_expr(expr: &Expression, names: &NameTable, a: &mut Analysis) {
    match expr {
        // A bare identifier is a read of that local (counted), except inside
        // an assignment/update LHS, where mutation/recording is handled there.
        Expression::Identifier(id) => count_read(id, names, a),
        Expression::AssignmentExpression(asg) => {
            record_target(&asg.left, names, a);
            walk_expr(&asg.right, names, a);
        }
        Expression::UpdateExpression(u) => record_simple(&u.argument, names, a),
        Expression::CallExpression(c) => {
            walk_callee(&c.callee, names, a);
            for arg in &c.arguments {
                if let Some(e) = arg.as_expression() {
                    walk_expr(e, names, a);
                }
            }
        }
        Expression::BinaryExpression(b) => {
            walk_expr(&b.left, names, a);
            walk_expr(&b.right, names, a);
        }
        Expression::LogicalExpression(l) => {
            walk_expr(&l.left, names, a);
            walk_expr(&l.right, names, a);
        }
        Expression::UnaryExpression(u) => walk_expr(&u.argument, names, a),
        Expression::ConditionalExpression(c) => {
            walk_expr(&c.test, names, a);
            walk_expr(&c.consequent, names, a);
            walk_expr(&c.alternate, names, a);
        }
        Expression::ArrayExpression(arr) => {
            for el in &arr.elements {
                if let Some(e) = el.as_expression() {
                    walk_expr(e, names, a);
                }
            }
        }
        Expression::ObjectExpression(o) => {
            for p in &o.properties {
                if let ObjectPropertyKind::ObjectProperty(op) = p {
                    walk_expr(&op.value, names, a);
                }
            }
        }
        Expression::TemplateLiteral(t) => {
            for e in &t.expressions {
                walk_expr(e, names, a);
            }
        }
        Expression::ParenthesizedExpression(p) => walk_expr(&p.expression, names, a),
        // `x!` reads `x` — a non-null assertion dereferences the Option value.
        Expression::TSNonNullExpression(nn) => walk_expr(&nn.expression, names, a),
        Expression::StaticMemberExpression(sm) => walk_expr(&sm.object, names, a),
        Expression::ComputedMemberExpression(cm) => {
            walk_expr(&cm.object, names, a);
            walk_expr(&cm.expression, names, a);
        }
        Expression::SequenceExpression(s) => {
            for e in &s.expressions {
                walk_expr(e, names, a);
            }
        }
        _ => {}
    }
}

/// A call's callee: a mutator method `x.push(…)` mutates *and* reads `x`
/// (`&mut self`); otherwise the callee's sub-expressions are walked normally
/// (e.g. `a.b.c()` reads `a.b`).
fn walk_callee(callee: &Expression, names: &NameTable, a: &mut Analysis) {
    if let Expression::StaticMemberExpression(sm) = callee {
        if MUTATORS.contains(&sm.property.name.as_str()) {
            record_root(&sm.object, names, a);
            return;
        }
        walk_expr(&sm.object, names, a);
        return;
    }
    walk_expr(callee, names, a);
}

/// An assignment LHS. A plain `x = …` rebinds `x` (mutated, not read); a
/// member target `x.y = …` / `x[i] = …` takes `&mut x` (mutated *and* read).
fn record_target(left: &AssignmentTarget, names: &NameTable, a: &mut Analysis) {
    match left {
        AssignmentTarget::AssignmentTargetIdentifier(id) => {
            record_mutation(id, names, a);
        }
        AssignmentTarget::StaticMemberExpression(sm) => {
            if root_is_this(&sm.object) {
                a.mutates_this = true;
            }
            record_root(&sm.object, names, a)
        }
        AssignmentTarget::ComputedMemberExpression(cm) => {
            record_root(&cm.object, names, a);
            walk_expr(&cm.expression, names, a);
        }
        _ => {}
    }
}

/// An update (`x++` / `x--`) takes `&mut x`: mutated *and* read.
fn record_simple(target: &SimpleAssignmentTarget, names: &NameTable, a: &mut Analysis) {
    if let SimpleAssignmentTarget::StaticMemberExpression(sm) = target {
        if root_is_this(&sm.object) {
            a.mutates_this = true;
        }
    }
    if let SimpleAssignmentTarget::AssignmentTargetIdentifier(id) = target {
        record_mutation(id, names, a);
        count_name(id, names, a);
    }
}

/// The leftmost identifier of a member chain (`a.b.c` → `a`): mutated and read
/// (the chain borrows it through `&mut`).
fn record_root(expr: &Expression, names: &NameTable, a: &mut Analysis) {
    if let Some(name) = root_ident(expr, names) {
        a.mutated.insert(name.clone());
        *a.use_counts.entry(name).or_default() += 1;
    }
}

fn record_mutation(id: &IdentifierReference, names: &NameTable, a: &mut Analysis) {
    a.mutated.insert(names.of_reference(id).to_string());
}

fn count_name(id: &IdentifierReference, names: &NameTable, a: &mut Analysis) {
    *a.use_counts
        .entry(names.of_reference(id).to_string())
        .or_default() += 1;
}

/// A read of a bare identifier local (`undefined` is not a value local).
fn count_read(id: &IdentifierReference, names: &NameTable, a: &mut Analysis) {
    if id.name.as_str() == "undefined" {
        return;
    }
    count_name(id, names, a);
}

/// The leftmost identifier of a member chain (`a.b.c` → `a`), as its per-symbol
/// Rust name.
fn root_ident(expr: &Expression, names: &NameTable) -> Option<String> {
    match expr {
        Expression::Identifier(id) => Some(names.of_reference(id).to_string()),
        Expression::StaticMemberExpression(sm) => root_ident(&sm.object, names),
        Expression::ComputedMemberExpression(cm) => root_ident(&cm.object, names),
        Expression::ParenthesizedExpression(p) => root_ident(&p.expression, names),
        _ => None,
    }
}

/// Whether a member chain is rooted at `this` (`this.x.y` → true) — used to
/// detect `this.field = …` / `this.field++` so the method takes `&mut self`.
fn root_is_this(expr: &Expression) -> bool {
    match expr {
        Expression::ThisExpression(_) => true,
        Expression::StaticMemberExpression(sm) => root_is_this(&sm.object),
        Expression::ParenthesizedExpression(p) => root_is_this(&p.expression),
        _ => false,
    }
}
