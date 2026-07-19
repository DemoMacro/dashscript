//! `new Foo(args)` → `Foo::new(args)`.
use oxc_ast::ast::{Expression, NewExpression};
use syn::{parse_quote, Expr, Ident};

use super::super::bindings;
use super::super::context::Ctx;
use super::translate_argument;

/// `new Foo(args)` → `Foo::new(args)`. Only an identifier callee (a user class)
/// maps; `new foo.Bar()` or `new (factory())()` fall back to `todo!()`.
///
/// `new Map()` / `new Set()` are special-cased to empty Rust collections — the
/// no-arg form only; `new Map(entries)` needs a `(K, V)` pair iterable (not yet
/// supported), so it falls through to `Map::new(…)` and surfaces as a `cargo
/// check` error honestly.
pub(super) fn new_expr(n: &NewExpression, ctx: &Ctx<'_>) -> Expr {
    let Some(name) = class_name(&n.callee) else {
        return parse_quote!(::core::todo!());
    };
    if n.arguments.is_empty() {
        match name.to_string().as_str() {
            "Map" => return parse_quote!(::std::collections::HashMap::new()),
            "Set" => return parse_quote!(::std::collections::HashSet::new()),
            _ => {}
        }
    }
    let args: Vec<Expr> = n
        .arguments
        .iter()
        .map(|a| translate_argument(a, ctx))
        .collect();
    parse_quote!(#name::new(#(#args),*))
}

/// The class type name when `callee` is a plain identifier (`Foo`).
fn class_name(callee: &Expression) -> Option<Ident> {
    let Expression::Identifier(id) = callee else {
        return None;
    };
    Some(bindings::type_ident(&id.name))
}
