//! Control-flow translation: `if`/`while`/`do-while`/`for…of`/`for…in`/C-style
//! `for`, plus the truthiness and Option-narrowing helpers they share.

use oxc_ast::ast::{
    ArrayExpression, BindingPattern, ChainElement, DoWhileStatement, Expression, ForInStatement,
    ForOfStatement, ForStatement, ForStatementLeft, IfStatement, Statement, StaticMemberExpression,
    WhileStatement,
};
use oxc_syntax::operator::LogicalOperator;
use quote::format_ident;
use syn::{parse_quote, Block, Expr, Ident, Path, Stmt, Type};

use super::super::analysis;
use super::super::context::{is_option_path, Ctx, Locals, Narrow};
use super::super::name_table::NameTable;
use super::super::registry::TypeRegistry;
use super::super::{bindings, expressions, types};
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
    let cond = expressions::condition_expr(&stmt.test, &Ctx::new(locals, registry, narrow, names));
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
    let cond = expressions::condition_expr(&stmt.test, &Ctx::new(locals, registry, narrow, names));
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
    let test = expressions::condition_expr(&stmt.test, &Ctx::new(locals, registry, narrow, names));
    parse_quote!(loop {
        #body
        if !(#test) {
            break;
        }
    })
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
    // `for (const [k, v] of Object.entries(record))` →
    // `for (k, v) in record.clone().into_iter()`. The destructured bindings are
    // typed to the record's HashMap K/V so a union-variant check on `v` (`v
    // !== undefined`) lowers to `matches!`. Values iterate owned (cloned),
    // mirroring the `Vec<(K, V)>` ES `Object.entries` allocates anyway — `v` is
    // non-Copy (a union enum carrying a String), so `record.iter()`'s `&v`
    // would leave every `v` use site a `&V` mismatch.
    if let Some((k, v)) = for_of_array_pattern_bindings(&stmt.left) {
        if let Some(arg) = object_entries_receiver(&stmt.right) {
            if let Some((key_ty, val_ty)) = record_kv_types(arg, locals) {
                locals.insert(k.to_string(), key_ty);
                locals.insert(v.to_string(), val_ty);
            }
            let ctx = Ctx::new(&*locals, registry, narrow, names);
            let record = expressions::translate_argument(arg, &ctx);
            let body = statement_block(&stmt.body, locals, registry, narrow, return_path, names);
            return vec![parse_quote!(for (#k, #v) in #record.clone().into_iter() #body)];
        }
    }
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
        other => iterable_element_type(other, locals, registry),
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
    // `for (const v of x ?? [])` over a non-Copy `Option<Vec<T>>` (e.g.
    // `parent.elements` where `elements?: Element[]`) → `for v in
    // x.iter().flatten().cloned()` (v: T owned). The `?? []` is redundant in
    // Rust (None iterates empty), and this borrows the Option field in place —
    // dodging the E0507 of `.unwrap_or_else` on a borrowed field — and yields
    // owned values, so a later `result.push(v)` / `return v` needs no reference
    // special-casing beyond the usual clone-on-extra-use.
    if !is_copy {
        if let Some(iter) = option_vec_flatten_iter(&stmt.right, locals, registry, narrow, names) {
            return vec![parse_quote!(for #pat in #iter #body)];
        }
    }
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
/// as owned `String`s (the `.ts` `Record` is a `HashMap<String, …>`). A struct
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
/// `.ts` `number` is `f64`, and `Range<f64>` isn't iterable in Rust, so a
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
        .map(|t| expressions::condition_expr(t, &Ctx::new(locals, registry, narrow, names)))
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

/// `for (const [k, v] of …)` — the two-element ArrayPattern's binding idents,
/// when the left is exactly `[id, id]`. `None` for any other shape (a single
/// binding, a different count, a nested pattern).
fn for_of_array_pattern_bindings(left: &ForStatementLeft) -> Option<(Ident, Ident)> {
    let ForStatementLeft::VariableDeclaration(decl) = left else {
        return None;
    };
    let d = decl.declarations.first()?;
    let BindingPattern::ArrayPattern(arr) = &d.id else {
        return None;
    };
    let key = binding_ident(arr.elements.first()?.as_ref()?)?;
    let val = binding_ident(arr.elements.get(1)?.as_ref()?)?;
    Some((key, val))
}

fn binding_ident(pat: &BindingPattern) -> Option<Ident> {
    let BindingPattern::BindingIdentifier(id) = pat else {
        return None;
    };
    Some(super::super::bindings::snake(id.name.as_str()))
}

/// The `(K, V)` type paths of a `Record`/`HashMap` argument, so a
/// `for (const [k, v] of Object.entries(record))` destructure can register each
/// binding's type — `v` must be typed as the value union for a `v !== undefined`
/// check to lower to `matches!`. `None` when the argument is not a `HashMap`
/// local (or its type parameters are not plain paths).
fn record_kv_types(arg: &oxc_ast::ast::Argument<'_>, locals: &Locals) -> Option<(Path, Path)> {
    let oxc_ast::ast::Argument::Identifier(id) = arg else {
        return None;
    };
    let name = super::super::bindings::snake(id.name.as_str()).to_string();
    let path = locals.get(&name)?;
    let seg = path.segments.last()?;
    if seg.ident != "HashMap" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(args) = &seg.arguments else {
        return None;
    };
    let mut it = args.args.iter().filter_map(|g| match g {
        syn::GenericArgument::Type(t) => types::type_path(t).cloned(),
        _ => None,
    });
    Some((it.next()?, it.next()?))
}

/// `Object.entries(record)` → the `record` argument, when `right` is exactly
/// that call (`Object` / `entries` / one arg). `None` for anything else, so the
/// generic for-of path handles ordinary iterables.
fn object_entries_receiver<'a, 'b>(
    right: &'b Expression<'a>,
) -> Option<&'b oxc_ast::ast::Argument<'a>> {
    let Expression::CallExpression(call) = right else {
        return None;
    };
    let Expression::StaticMemberExpression(m) = &call.callee else {
        return None;
    };
    if m.property.name.as_str() != "entries" {
        return None;
    }
    let Expression::Identifier(id) = &m.object else {
        return None;
    };
    if id.name.as_str() != "Object" {
        return None;
    }
    call.arguments.first()
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

/// The element type of a non-literal iterable expression — `parent.elements
/// ?? []` iterates `Element`s when `elements: Option<Vec<Element>>`. Handles a
/// nullish/fallback chain (the element type rides on the non-fallback side)
/// and a struct field whose type is `Vec<T>` / `Option<Vec<T>>` → `T`. Returns
/// `None` for anything else so the binding stays untyped.
fn iterable_element_type(
    expr: &Expression,
    locals: &Locals,
    registry: &TypeRegistry,
) -> Option<Type> {
    match expr {
        // `x ?? []` / `x || []` — the right side is an empty fallback; the
        // element type rides on the left (e.g. `parent.elements ?? []`).
        Expression::LogicalExpression(l)
            if matches!(l.operator, LogicalOperator::Coalesce | LogicalOperator::Or) =>
        {
            iterable_element_type(&l.left, locals, registry)
        }
        Expression::StaticMemberExpression(sm) => member_iter_element_type(sm, locals, registry),
        // A bare local iterable `for (const x of arr)` where `arr: Vec<T>` (or
        // `Option<Vec<T>>`) → the element type `T`, typing the loop variable so
        // a receiver-typed body call (`Temporal.X.from(x)` → `from_utf8`)
        // routes correctly. Without this the binding stays untyped and the body
        // sees a spurious `TypeError` instead of parsing the string.
        Expression::Identifier(id) => {
            let path = locals.get(&bindings::snake(id.name.as_str()).to_string())?;
            let ty: Type = parse_quote!(#path);
            vec_element_type(&ty)
        }
        Expression::ChainExpression(c) => match &c.expression {
            ChainElement::StaticMemberExpression(sm) => {
                member_iter_element_type(sm, locals, registry)
            }
            _ => None,
        },
        _ => None,
    }
}

/// `obj.field` where `obj` is a known struct local (or `Option<Struct>`) and
/// `field` is `Vec<T>` / `Option<Vec<T>>` → the element type `T`.
fn member_iter_element_type(
    sm: &StaticMemberExpression,
    locals: &Locals,
    registry: &TypeRegistry,
) -> Option<Type> {
    member_field_type(sm, locals, registry).and_then(|(ty, _)| vec_element_type(ty))
}

/// The translated type of `obj.field` plus whether it is an optional `?:`
/// field — the registry stores the field type *without* the `Option<…>`
/// wrapper the emitted struct adds for an optional field, carrying that on
/// `InterfaceField.optional` instead. Shared lookup behind
/// [`member_iter_element_type`] (wants the element `T`) and
/// [`option_vec_flatten_iter`] (needs `optional` to pick `.iter().flatten()`
/// vs `.iter()`).
fn member_field_type<'a>(
    sm: &StaticMemberExpression,
    locals: &Locals,
    registry: &'a TypeRegistry,
) -> Option<(&'a Type, bool)> {
    let Expression::Identifier(obj_id) = &sm.object else {
        return None;
    };
    let obj_path = locals.get(&bindings::snake(obj_id.name.as_str()).to_string())?;
    let struct_name = struct_name_of(obj_path)?;
    let field = bindings::snake(sm.property.name.as_str()).to_string();
    let f = registry
        .interface_own_fields
        .get(&struct_name)?
        .iter()
        .find(|f| bindings::snake(&f.name) == field)?;
    Some((&f.ty, f.optional))
}

/// `for (const v of x ?? [])` / `x || []` where `x.field` is a non-Copy
/// `Vec<T>` (e.g. `parent.elements` with `elements?: Element[]`) → an owned
/// iterator yielding `T`, so the loop binds `v: T` by value. An optional field
/// is emitted as `Option<Vec<T>>`, so `.iter().flatten()` walks the Vec behind
/// the Option; a bare `Vec<T>` uses `.iter()` directly. Returns `None` for
/// anything else so the generic `for &v in &…unwrap_or_else(…)` path handles
/// the rest (and fails loudly via cargo check where it cannot).
fn option_vec_flatten_iter(
    expr: &Expression,
    locals: &Locals,
    registry: &TypeRegistry,
    narrow: &Narrow,
    names: &NameTable<'_>,
) -> Option<Expr> {
    let Expression::LogicalExpression(l) = expr else {
        return None;
    };
    if !matches!(l.operator, LogicalOperator::Coalesce | LogicalOperator::Or) {
        return None;
    }
    is_empty_array_expr(&l.right)?;
    let (field_ty, optional) = iter_field_type(&l.left, locals, registry)?;
    let elem = vec_element_type(field_ty)?;
    if is_copy_type(&elem) {
        return None;
    }
    let left = expressions::translate_expr(&l.left, &Ctx::new(locals, registry, narrow, names));
    Some(if optional {
        parse_quote!(#left.iter().flatten().cloned())
    } else {
        parse_quote!(#left.iter().cloned())
    })
}

/// The translated field type of a member/chain iterable (`obj.field` or
/// `obj?.field`) plus its `optional` flag, or `None` for any other shape.
fn iter_field_type<'a>(
    expr: &'a Expression,
    locals: &Locals,
    registry: &'a TypeRegistry,
) -> Option<(&'a Type, bool)> {
    match expr {
        Expression::StaticMemberExpression(sm) => member_field_type(sm, locals, registry),
        Expression::ChainExpression(c) => match &c.expression {
            ChainElement::StaticMemberExpression(sm) => member_field_type(sm, locals, registry),
            _ => None,
        },
        _ => None,
    }
}

/// `[]` (an empty array fallback) — matches the right side of `x ?? []`.
fn is_empty_array_expr(expr: &Expression) -> Option<()> {
    match expr {
        Expression::ArrayExpression(a) if a.elements.is_empty() => Some(()),
        _ => None,
    }
}

/// True for a Copy scalar type (`f64`/`bool`) — those use the `for &v` path.
fn is_copy_type(ty: &Type) -> bool {
    let Type::Path(tp) = ty else {
        return false;
    };
    tp.path
        .segments
        .last()
        .is_some_and(|s| s.ident == "f64" || s.ident == "bool")
}

/// `Vec<T>` → `T` (after any `Option<…>` wrapper is stripped). `None` for a
/// non-`Vec` type.
fn vec_element_type(ty: &Type) -> Option<Type> {
    let inner = strip_option(ty);
    let Type::Path(tp) = inner else {
        return None;
    };
    let seg = tp.path.segments.last()?;
    if seg.ident != "Vec" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(args) = &seg.arguments else {
        return None;
    };
    args.args.iter().find_map(|g| match g {
        syn::GenericArgument::Type(t) => Some(t.clone()),
        _ => None,
    })
}

/// `Option<T>` → `T`; any other type passes through unchanged.
fn strip_option(ty: &Type) -> &Type {
    let Type::Path(tp) = ty else {
        return ty;
    };
    let Some(seg) = tp.path.segments.last() else {
        return ty;
    };
    if seg.ident != "Option" {
        return ty;
    }
    let syn::PathArguments::AngleBracketed(args) = &seg.arguments else {
        return ty;
    };
    args.args
        .iter()
        .find_map(|g| match g {
            syn::GenericArgument::Type(t) => Some(t),
            _ => None,
        })
        .unwrap_or(ty)
}

/// The struct name a path denotes, stripping an `Option<…>` wrapper —
/// `Element` → `Element`, `Option<Element>` → `Element`. `None` when the path
/// is not a plain struct or `Option<struct>`.
fn struct_name_of(path: &Path) -> Option<String> {
    let seg = path.segments.last()?;
    if seg.ident == "Option" {
        let syn::PathArguments::AngleBracketed(args) = &seg.arguments else {
            return None;
        };
        args.args.iter().find_map(|g| match g {
            syn::GenericArgument::Type(Type::Path(t)) => {
                t.path.segments.last().map(|s| s.ident.to_string())
            }
            _ => None,
        })
    } else {
        Some(seg.ident.to_string())
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
