//! `new Foo(args)` → `Foo::new(args)`.
use oxc_ast::ast::{Expression, NewExpression};
use syn::{parse_quote, Expr, Ident};

use super::super::bindings;
use super::super::builtins;
use super::super::context::Ctx;
use super::array_elem_arg;

/// `new Foo(args)` → `Foo::new(args)`. Only an identifier callee (a user class)
/// maps; `new foo.Bar()` or `new (factory())()` fall back to `todo!()`.
///
/// `new Map()` / `new Set()` are special-cased to empty Rust collections — the
/// no-arg form only; `new Map(entries)` needs a `(K, V)` pair iterable (not yet
/// supported), so it falls through to `Map::new(…)` and surfaces as a `cargo
/// check` error honestly.
pub(super) fn new_expr(n: &NewExpression, ctx: &Ctx<'_>) -> Expr {
    // `new RegExp("pat"[, flags])` — the ES RegExp constructor, lowered to the
    // same `__ds::regex` helper as `/pat/` literals. Intercepted before the
    // generic `Foo::new` lowering, which would emit `RegExp::new` (E0425).
    if let Expression::Identifier(id) = &n.callee {
        if id.name.as_str() == "RegExp" {
            if let Some(e) = builtins::reg_exp_constructor(&n.arguments, ctx) {
                return e;
            }
        }
    }
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
    // A class field typed `number` is `f64`, so the synthesized `new` takes
    // `f64` parameters — a flavor-promoted `i64` argument (`new Point3D(i, …)`
    // where `i` is an `i64` counter) is site-cast via `array_elem_arg`.
    let args: Vec<Expr> = n.arguments.iter().map(|a| array_elem_arg(a, ctx)).collect();
    parse_quote!(#name::new(#(#args),*))
}

/// The class type name when `callee` is a plain identifier (`Foo`).
fn class_name(callee: &Expression) -> Option<Ident> {
    let Expression::Identifier(id) = callee else {
        return None;
    };
    Some(bindings::type_ident(&id.name))
}
