//! Control-flow translation: `if`/`while`/`do-while`/`for…of`/`for…in`/C-style
//! `for`, plus the truthiness and Option-narrowing helpers they share.

use oxc_ast::ast::{
    DoWhileStatement, Expression, ForInStatement, ForOfStatement, ForStatement, ForStatementLeft,
    IfStatement, Statement, WhileStatement,
};
use oxc_syntax::operator::UnaryOperator;
use quote::format_ident;
use syn::{parse_quote, Block, Expr, Ident, Path, Stmt};

use super::super::analysis;
use super::super::context::{is_option_path, Ctx, Locals, Narrow};
use super::super::registry::TypeRegistry;
use super::super::{bindings, expressions, types};
use super::{translate_stmt, translate_variable_declaration};

pub(super) fn translate_if(
    stmt: &IfStatement,
    locals: &mut Locals,
    registry: &TypeRegistry,
    narrow: &Narrow,
    return_path: Option<&Path>,
) -> Stmt {
    // `if (opt)` where `opt: Option<T>`, `T: Copy`, and `opt` is never mutated
    // → `if let Some(opt) = opt`. The bound copy leaves `opt` usable after the
    // branch (no move); `opt!`/`opt` inside read the inner value, so the
    // unwrap-after-is_some pattern is avoided.
    if let Some((name, ident_expr)) = option_truthiness_target(&stmt.test, locals) {
        let child = narrow.with_option_some(name.clone());
        let then_block = statement_block(&stmt.consequent, locals, registry, &child, return_path);
        // Bind the inner value only if the branch reads it; else discard it so
        // no `unused_variables` lint fires.
        let bind = if analysis::references(&stmt.consequent, &name) {
            format_ident!("{}", name)
        } else {
            format_ident!("_")
        };
        return match &stmt.alternate {
            Some(alt) => {
                let else_block = statement_block(alt, locals, registry, narrow, return_path);
                parse_quote!(if let Some(#bind) = #ident_expr #then_block else #else_block)
            }
            None => parse_quote!(if let Some(#bind) = #ident_expr #then_block),
        };
    }
    let cond = condition_expr(&stmt.test, locals, registry, narrow);
    let then_block = statement_block(&stmt.consequent, locals, registry, narrow, return_path);
    match &stmt.alternate {
        Some(alt) => {
            let else_block = statement_block(alt, locals, registry, narrow, return_path);
            parse_quote!(if #cond #then_block else #else_block)
        }
        None => parse_quote!(if #cond #then_block),
    }
}

/// The target of an `if (opt)` test that can narrow soundly: a bare identifier
/// of `Option<T>` where `T: Copy` and the binding is never mutated. Returns its
/// snake-cased name and a bare-identifier expression. A non-`Copy` inner type
/// is left alone (the value would move out of the Option); so is a mutated
/// binding (an `if let` binding cannot be reassigned).
fn option_truthiness_target(test: &Expression, locals: &Locals) -> Option<(String, Expr)> {
    let Expression::Identifier(id) = test else {
        return None;
    };
    let name = bindings::snake(&id.name).to_string();
    let path = locals.get(&name)?;
    if !is_option_path(path) || !types::is_copy_path(path) {
        return None;
    }
    if locals.mutated.contains(&name) {
        return None;
    }
    let ident = format_ident!("{}", name);
    Some((name, parse_quote!(#ident)))
}

pub(super) fn translate_while(
    stmt: &WhileStatement,
    locals: &mut Locals,
    registry: &TypeRegistry,
    narrow: &Narrow,
    return_path: Option<&Path>,
) -> Stmt {
    let cond = condition_expr(&stmt.test, locals, registry, narrow);
    let body = statement_block(&stmt.body, locals, registry, narrow, return_path);
    parse_quote!(while #cond #body)
}

/// `do { body } while (test)` → `loop { body; if !(test) { break; } }` — Rust
/// has no do-while, so the body runs once then the test gates each repeat.
pub(super) fn translate_do_while(
    stmt: &DoWhileStatement,
    locals: &mut Locals,
    registry: &TypeRegistry,
    narrow: &Narrow,
    return_path: Option<&Path>,
) -> Stmt {
    let body = statement_block(&stmt.body, locals, registry, narrow, return_path);
    let test = condition_expr(&stmt.test, locals, registry, narrow);
    parse_quote!(loop {
        #body
        if !(#test) {
            break;
        }
    })
}

/// Translate an `if`/`while` test. A bare identifier of a `Vec`/`String` type
/// maps to an emptiness check, and an `Option` maps to `is_some`; negation flips
/// to `is_empty`/`is_none`. Anything else translates as a plain boolean expr.
fn condition_expr(
    test: &Expression,
    locals: &Locals,
    registry: &TypeRegistry,
    narrow: &Narrow,
) -> Expr {
    if let Some(expr) = truthiness(test, false, locals) {
        return expr;
    }
    if let Expression::UnaryExpression(un) = test {
        if matches!(un.operator, UnaryOperator::LogicalNot) {
            if let Some(expr) = truthiness(&un.argument, true, locals) {
                return expr;
            }
        }
    }
    expressions::translate_expr(test, &Ctx::new(locals, registry, narrow))
}

/// If `expr` is a bare identifier of a collection (`Vec`/`String`) or `Option`
/// type, return its Rust boolean form. `negated` selects the falsy side
/// (`is_empty`/`is_none`) vs the truthy side (`!is_empty`/`is_some`).
fn truthiness(expr: &Expression, negated: bool, locals: &Locals) -> Option<Expr> {
    let Expression::Identifier(id) = expr else {
        return None;
    };
    let ident = bindings::snake(&id.name);
    let last = locals
        .get(&ident.to_string())?
        .segments
        .last()?
        .ident
        .to_string();
    match last.as_str() {
        "Vec" | "String" => Some(if negated {
            parse_quote!(#ident.is_empty())
        } else {
            parse_quote!(!#ident.is_empty())
        }),
        "Option" => Some(if negated {
            parse_quote!(#ident.is_none())
        } else {
            parse_quote!(#ident.is_some())
        }),
        _ => None,
    }
}

/// `for (const v of xs)` → `for &v in &xs { … }`.
///
/// The `&v` pattern destructures the borrow so `v` is an owned `f64` (Copy),
/// avoiding a `&f64`/`f64` mismatch on comparisons inside the body. This works
/// for Copy elements (DashScript `number`/`boolean`); iterating owned values
/// out of a `Vec<String>` is unsupported yet.
pub(super) fn translate_for_of(
    stmt: &ForOfStatement,
    locals: &mut Locals,
    registry: &TypeRegistry,
    narrow: &Narrow,
    return_path: Option<&Path>,
) -> Vec<Stmt> {
    let Some(pat) = for_of_binding(&stmt.left) else {
        return vec![];
    };
    // Translate the iterable before the body — `Ctx` borrows `locals`
    // immutably while `statement_block` borrows it mutably, so they can't overlap.
    let slice = match &stmt.right {
        Expression::ArrayExpression(arr) => {
            expressions::array_slice_expr(arr, &Ctx::new(&*locals, registry, narrow))
        }
        _ => None,
    };
    let body = statement_block(&stmt.body, locals, registry, narrow, return_path);
    if let Some(slice) = slice {
        // A spread-free inline array literal iterates as a borrowed slice
        // `&[…]` (idiomatic; avoids clippy::useless_vec).
        return vec![parse_quote!(for &#pat in #slice #body)];
    }
    let iter = expressions::translate_expr(&stmt.right, &Ctx::new(&*locals, registry, narrow));
    vec![parse_quote!(for &#pat in &#iter #body)]
}

/// `for (const k in m)` → `for k in m.keys().cloned()` — iterates a map's keys
/// as owned `String`s (the `.ds` `Record` is a `HashMap<String, …>`). A struct
/// source has no keys iterator, so only a `Record`/`HashMap` is supported.
pub(super) fn translate_for_in(
    stmt: &ForInStatement,
    locals: &mut Locals,
    registry: &TypeRegistry,
    narrow: &Narrow,
    return_path: Option<&Path>,
) -> Vec<Stmt> {
    let Some(pat) = for_of_binding(&stmt.left) else {
        return vec![];
    };
    let iter = expressions::translate_expr(&stmt.right, &Ctx::new(&*locals, registry, narrow));
    let body = statement_block(&stmt.body, locals, registry, narrow, return_path);
    vec![parse_quote!(for #pat in #iter.keys().cloned() #body)]
}

/// `for (init; test; update) body` → `{ init; while test { body; update; } }`.
///
/// `.ds` `number` is `f64`, and `Range<f64>` isn't iterable in Rust, so a
/// C-style loop decomposes into a `while` (not `for i in 0..n`). It is wrapped
/// in a block so the loop's own bindings (e.g. `i`) don't collide across loops.
/// A `continue` inside the body skips the `update` step — a known limitation;
/// use a `while` if the update must run every iteration.
pub(super) fn translate_for(
    stmt: &ForStatement,
    locals: &mut Locals,
    registry: &TypeRegistry,
    narrow: &Narrow,
    return_path: Option<&Path>,
) -> Vec<Stmt> {
    // JS `var` is function-scoped: `for (var i = …; …)` must not wrap the loop
    // in a block — the binding is shared with sibling loops in the same
    // function (a later `for (i = …; …)` reuses it). `let`/`const` stay
    // block-scoped (keep the wrapper, matching Rust's block semantics).
    let is_var = stmt.init.as_ref().is_some_and(|i| i.is_var_declaration());
    let init: Vec<Stmt> = match &stmt.init {
        Some(oxc_ast::ast::ForStatementInit::VariableDeclaration(decl)) => {
            translate_variable_declaration(decl, locals, registry, narrow)
        }
        // `for (i = -5; …)` — an assignment init reuses an outer (var) binding;
        // emit the assignment as a statement. The catch-all dropped it, losing
        // the reassignment and looping on the prior value.
        Some(oxc_ast::ast::ForStatementInit::AssignmentExpression(a)) => {
            let e = expressions::assignment_expr(a, &Ctx::new(&*locals, registry, narrow));
            vec![parse_quote!(#e;)]
        }
        _ => Vec::new(),
    };
    let test = stmt
        .test
        .as_ref()
        .map(|t| condition_expr(t, locals, registry, narrow))
        .unwrap_or_else(|| parse_quote!(true));
    let body = translate_stmt(&stmt.body, locals, registry, narrow, return_path);
    let update: Option<Stmt> = stmt.update.as_ref().map(|u| {
        let e = expressions::translate_expr(u, &Ctx::new(&*locals, registry, narrow));
        parse_quote!(#e;)
    });
    let while_loop: Stmt = parse_quote!(while #test {
        #(#body)*
        #update
    });
    if is_var {
        // flat: the var bindings live in the enclosing function scope
        let mut out = init;
        out.push(while_loop);
        out
    } else {
        vec![parse_quote!({
            #(#init)*
            #while_loop
        })]
    }
}

/// Binding name from `for (const v of …)`; other left forms are unmapped.
fn for_of_binding(left: &ForStatementLeft) -> Option<Ident> {
    let ForStatementLeft::VariableDeclaration(decl) = left else {
        return None;
    };
    let d = decl.declarations.first()?;
    Some(bindings::binding_name(&d.id))
}

/// Turn any statement into a `{ … }` block (used by if/while/for bodies).
fn statement_block(
    stmt: &Statement,
    locals: &mut Locals,
    registry: &TypeRegistry,
    narrow: &Narrow,
    return_path: Option<&Path>,
) -> Block {
    let stmts: Vec<Stmt> = translate_stmt(stmt, locals, registry, narrow, return_path);
    parse_quote!({ #(#stmts)* })
}
