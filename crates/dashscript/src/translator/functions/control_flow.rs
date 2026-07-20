//! Control-flow translation: `if`/`while`/`do-while`/`for…of`/`for…in`/C-style
//! `for`, plus the truthiness and Option-narrowing helpers they share.

use oxc_ast::ast::{
    ArrayExpression, DoWhileStatement, Expression, ForInStatement, ForOfStatement, ForStatement,
    ForStatementLeft, IfStatement, Statement, WhileStatement,
};
use oxc_syntax::operator::UnaryOperator;
use quote::format_ident;
use syn::{parse_quote, Block, Expr, Ident, Path, Stmt, Type};

use super::super::analysis;
use super::super::context::{is_option_path, Ctx, Locals, Narrow};
use super::super::name_table::NameTable;
use super::super::registry::TypeRegistry;
use super::super::{expressions, types};
use super::{translate_stmt, translate_variable_declaration};

pub(super) fn translate_if(
    stmt: &IfStatement,
    locals: &mut Locals,
    registry: &TypeRegistry,
    narrow: &Narrow,
    return_path: Option<&Path>,
    names: &NameTable<'_>,
) -> Stmt {
    // `if (opt)` where `opt: Option<T>`, `T: Copy`, and `opt` is never mutated
    // → `if let Some(opt) = opt`. The bound copy leaves `opt` usable after the
    // branch (no move); `opt!`/`opt` inside read the inner value, so the
    // unwrap-after-is_some pattern is avoided.
    if let Some((name, ident_expr)) = option_truthiness_target(&stmt.test, locals, names) {
        let child = narrow.with_option_some(name.clone());
        let then_block = statement_block(
            &stmt.consequent,
            locals,
            registry,
            &child,
            return_path,
            names,
        );
        // Bind the inner value only if the branch reads it; else discard it so
        // no `unused_variables` lint fires.
        let bind = if analysis::references(&stmt.consequent, &name, names) {
            format_ident!("{}", name)
        } else {
            format_ident!("_")
        };
        return match &stmt.alternate {
            Some(alt) => {
                let else_block = statement_block(alt, locals, registry, narrow, return_path, names);
                parse_quote!(if let Some(#bind) = #ident_expr #then_block else #else_block)
            }
            None => parse_quote!(if let Some(#bind) = #ident_expr #then_block),
        };
    }
    let cond = condition_expr(&stmt.test, locals, registry, narrow, names);
    let then_block = statement_block(
        &stmt.consequent,
        locals,
        registry,
        narrow,
        return_path,
        names,
    );
    match &stmt.alternate {
        Some(alt) => {
            let else_block = statement_block(alt, locals, registry, narrow, return_path, names);
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
fn option_truthiness_target(
    test: &Expression,
    locals: &Locals,
    names: &NameTable<'_>,
) -> Option<(String, Expr)> {
    let Expression::Identifier(id) = test else {
        return None;
    };
    let name = names.of_reference(id).to_string();
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
    names: &NameTable<'_>,
) -> Stmt {
    let cond = condition_expr(&stmt.test, locals, registry, narrow, names);
    let body = statement_block(&stmt.body, locals, registry, narrow, return_path, names);
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
    names: &NameTable<'_>,
) -> Stmt {
    let body = statement_block(&stmt.body, locals, registry, narrow, return_path, names);
    let test = condition_expr(&stmt.test, locals, registry, narrow, names);
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
    names: &NameTable<'_>,
) -> Expr {
    if let Some(expr) = truthiness(test, false, locals, names) {
        return expr;
    }
    if let Expression::UnaryExpression(un) = test {
        if matches!(un.operator, UnaryOperator::LogicalNot) {
            if let Some(expr) = truthiness(&un.argument, true, locals, names) {
                return expr;
            }
        }
    }
    expressions::translate_expr(test, &Ctx::new(locals, registry, narrow, names))
}

/// The Rust boolean form of an ES truthiness test. A numeric literal folds to
/// its compile-time truthiness (nonzero and non-NaN); a bare identifier of a
/// number (`f64`/integer), collection (`Vec`/`String`), or `Option` type maps
/// to the matching runtime check. `negated` selects the falsy side (`== 0`/
/// `is_nan`/`is_empty`/`is_none`) vs the truthy side. Anything else returns
/// `None` (the caller treats the expression as already boolean).
fn truthiness(
    expr: &Expression,
    negated: bool,
    locals: &Locals,
    names: &NameTable<'_>,
) -> Option<Expr> {
    // A numeric literal's ES truthiness is known at translate time, so a
    // `while (1)` / `do { … } while (0)` folds to a Rust `bool` literal
    // instead of emitting `!(1_f64)` (E0600: `!` on f64).
    if let Expression::NumericLiteral(n) = expr {
        let v = n.value;
        let truthy = v != 0.0 && !v.is_nan();
        let b = if negated { !truthy } else { truthy };
        return Some(if b {
            parse_quote!(true)
        } else {
            parse_quote!(false)
        });
    }
    let Expression::Identifier(id) = expr else {
        return None;
    };
    let ident = names.of_reference(id);
    let last = locals
        .get(&ident.to_string())?
        .segments
        .last()?
        .ident
        .to_string();
    match last.as_str() {
        // ES `Boolean(f64)`: nonzero and non-NaN. NaN is falsy (`!NaN === true`),
        // so the negated form ORs the two falsy cases.
        "f64" => Some(if negated {
            parse_quote!(#ident == 0.0 || #ident.is_nan())
        } else {
            parse_quote!(#ident != 0.0 && !#ident.is_nan())
        }),
        // Integer scalars have no NaN; truthiness is simply != 0.
        "i64" | "i32" | "usize" | "u64" | "u32" | "u16" | "u8" | "i16" | "i8" => Some(if negated {
            parse_quote!(#ident == 0)
        } else {
            parse_quote!(#ident != 0)
        }),
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
    names: &NameTable<'_>,
) -> Vec<Stmt> {
    let Some(pat) = for_of_binding(&stmt.left, names) else {
        return vec![];
    };
    // Record the loop variable's type so receiver-typed methods route
    // correctly inside the body — a `for (let re of [/pat/]) re.test(s)` needs
    // `re` typed as `regress::Regex`, or `.test` wouldn't lower to `.find`.
    // Only a homogeneous inline array literal carries an element type; a
    // non-literal iterable leaves the binding untyped (uses fall through).
    // A non-Copy element (Regex/String) iterates by reference (`for re in &…`,
    // `re: &T`); a Copy one (f64/bool) destructures (`for &v in &…`, `v: T`) —
    // moving a non-Copy out of a shared borrow is E0507.
    let arr_ty = match &stmt.right {
        Expression::ArrayExpression(arr) => for_of_element_type(arr),
        _ => None,
    };
    let is_copy = arr_ty
        .as_ref()
        .and_then(super::path_of)
        .and_then(|p| p.segments.last().map(|s| s.ident.to_string()))
        .is_some_and(|last| matches!(last.as_str(), "f64" | "bool"));
    if let Some(ty) = arr_ty.as_ref() {
        if let Some(path) = super::path_of(ty) {
            locals.insert(pat.to_string(), path);
        }
    }
    // Translate the iterable before the body — `Ctx` borrows `locals`
    // immutably while `statement_block` borrows it mutably, so they can't overlap.
    let slice = match &stmt.right {
        Expression::ArrayExpression(arr) => {
            expressions::array_slice_expr(arr, &Ctx::new(&*locals, registry, narrow, names))
        }
        _ => None,
    };
    let body = statement_block(&stmt.body, locals, registry, narrow, return_path, names);
    // A non-Copy element (Regex/String) iterates by reference (`for re in &…`,
    // `re: &T`); everything else — a Copy element (f64/bool) or an untyped
    // iterable (a `number[]` local) — destructures (`for &v in &…`, `v: T`),
    // since moving a non-Copy out of a shared borrow is E0507.
    let iterates_by_ref = !is_copy
        && arr_ty
            .as_ref()
            .and_then(super::path_of)
            .and_then(|p| p.segments.last().map(|s| s.ident.to_string()))
            .is_some_and(|last| matches!(last.as_str(), "Regex" | "String"));
    if let Some(slice) = slice {
        // A spread-free inline array literal iterates as a borrowed slice
        // `&[…]` (idiomatic; avoids clippy::useless_vec).
        if iterates_by_ref {
            return vec![parse_quote!(for #pat in #slice #body)];
        }
        return vec![parse_quote!(for &#pat in #slice #body)];
    }
    let iter =
        expressions::translate_expr(&stmt.right, &Ctx::new(&*locals, registry, narrow, names));
    if iterates_by_ref {
        vec![parse_quote!(for #pat in &#iter #body)]
    } else {
        vec![parse_quote!(for &#pat in &#iter #body)]
    }
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
    names: &NameTable<'_>,
) -> Vec<Stmt> {
    let Some(pat) = for_of_binding(&stmt.left, names) else {
        return vec![];
    };
    let iter =
        expressions::translate_expr(&stmt.right, &Ctx::new(&*locals, registry, narrow, names));
    let body = statement_block(&stmt.body, locals, registry, narrow, return_path, names);
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
    names: &NameTable<'_>,
) -> Vec<Stmt> {
    // JS `var` is function-scoped: `for (var i = …; …)` must not wrap the loop
    // in a block — the binding is shared with sibling loops in the same
    // function (a later `for (i = …; …)` reuses it). `let`/`const` stay
    // block-scoped (keep the wrapper, matching Rust's block semantics).
    let is_var = stmt.init.as_ref().is_some_and(|i| i.is_var_declaration());
    let init: Vec<Stmt> = match &stmt.init {
        Some(oxc_ast::ast::ForStatementInit::VariableDeclaration(decl)) => {
            translate_variable_declaration(decl, locals, registry, narrow, names)
        }
        // `for (i = -5; …)` — an assignment init reuses an outer (var) binding;
        // emit the assignment as a statement. The catch-all dropped it, losing
        // the reassignment and looping on the prior value.
        Some(oxc_ast::ast::ForStatementInit::AssignmentExpression(a)) => {
            let e = expressions::assignment_expr(a, &Ctx::new(&*locals, registry, narrow, names));
            vec![parse_quote!(#e;)]
        }
        _ => Vec::new(),
    };
    let test = stmt
        .test
        .as_ref()
        .map(|t| condition_expr(t, locals, registry, narrow, names))
        .unwrap_or_else(|| parse_quote!(true));
    let body = translate_stmt(&stmt.body, locals, registry, narrow, return_path, names);
    let update: Option<Stmt> = stmt.update.as_ref().map(|u| {
        let e = expressions::translate_expr(u, &Ctx::new(&*locals, registry, narrow, names));
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
fn for_of_binding(left: &ForStatementLeft, names: &NameTable<'_>) -> Option<Ident> {
    let ForStatementLeft::VariableDeclaration(decl) = left else {
        return None;
    };
    let d = decl.declarations.first()?;
    Some(names.of_pattern(&d.id))
}

/// The element type of a homogeneous inline array literal — `[/pat/]` →
/// `regress::Regex`, `[1, 2]` → `f64`, `["a"]` → `String`. Used by
/// [`super::translate_for_of`] to type the loop variable so receiver-typed
/// methods (`.test` on a regex, …) dispatch inside the body. A mixed, empty,
/// or spread array yields `None` (the binding stays untyped).
fn for_of_element_type(arr: &ArrayExpression) -> Option<Type> {
    let elems: Vec<&Expression> = arr
        .elements
        .iter()
        .filter_map(|e| e.as_expression())
        .collect();
    if elems.is_empty() {
        return None;
    }
    if elems
        .iter()
        .all(|e| matches!(e, Expression::RegExpLiteral(_)))
    {
        Some(parse_quote!(regress::Regex))
    } else if elems
        .iter()
        .all(|e| matches!(e, Expression::NumericLiteral(_)))
    {
        Some(parse_quote!(f64))
    } else if elems
        .iter()
        .all(|e| matches!(e, Expression::StringLiteral(_)))
    {
        Some(parse_quote!(String))
    } else {
        None
    }
}

/// Turn any statement into a `{ … }` block (used by if/while/for bodies).
fn statement_block(
    stmt: &Statement,
    locals: &mut Locals,
    registry: &TypeRegistry,
    narrow: &Narrow,
    return_path: Option<&Path>,
    names: &NameTable<'_>,
) -> Block {
    let stmts: Vec<Stmt> = translate_stmt(stmt, locals, registry, narrow, return_path, names);
    parse_quote!({ #(#stmts)* })
}
