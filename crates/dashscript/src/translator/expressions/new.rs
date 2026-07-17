//! `new Foo(args)` → `Foo::new(args)`.
use oxc_ast::ast::{Expression, NewExpression};
use syn::{parse_quote, Expr, Ident};

use super::super::bindings;
use super::super::context::Ctx;
use super::translate_argument;

/// `new Foo(args)` → `Foo::new(args)`. Only an identifier callee (a user class)
/// maps; `new foo.Bar()` or `new (factory())()` fall back to `todo!()`.
pub(super) fn new_expr(n: &NewExpression, ctx: &Ctx<'_>) -> Expr {
    let Some(name) = class_name(&n.callee) else {
        return parse_quote!(::core::todo!());
    };
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
