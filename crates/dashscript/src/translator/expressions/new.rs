//! `new Foo(args)` → `Foo::new(args)`.
use oxc_ast::ast::{Argument, Expression, NewExpression};
use syn::{parse_quote, Expr, Ident};

use super::super::bindings;
use super::super::builtins;
use super::super::context::Ctx;
use super::super::types;
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
        // `new Worker(handler)` — a Web Worker isolate (Direction D, D1): spawns a
        // thread running `handler` for each message received. Lowered before
        // the generic `Foo::new` path (which would emit `Worker::new` — E0425,
        // `Worker` is the runtime type, not a user class). File-based
        // `new Worker('./w.ts')` (worker-entry translation + build-time dep
        // scan) is a later batch reusing this runtime.
        if id.name.as_str() == "Worker" {
            if let Some(arg) = n.arguments.first() {
                let handler = array_elem_arg(arg, ctx);
                return worker_ctor(arg, handler);
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

/// `new Worker(handler)` constructor selection (Direction D).
///
/// - D1 one-way: a 1-arg handler `(msg) => { … }` → `Worker::new`.
/// - D2 bidirectional: a 2-arg handler `(msg, reply) => { reply.send(v); }` →
///   `Worker::new_with_reply`, so the worker can reply and main reads it via
///   `recv`.
///
/// The first param's type annotation is threaded through as a turbofish
/// `new_with_reply::<A, _>`: the worker deserializes each incoming message to
/// `A`, but the closure body alone may not pin `A` (e.g. `reply.send(msg * 2)`
/// — the generic `send` does not anchor `msg`'s type), so the `: number`
/// annotation is the anchor. An untyped 2-arg handler falls back to
/// `new_with_reply` and surfaces at `cargo check` if `A` stays ambiguous. Only
/// an inline arrow's arity is inspected; a named-function handler (an
/// identifier) defaults to one-way.
fn worker_ctor(arg: &Argument, handler: Expr) -> Expr {
    let Argument::ArrowFunctionExpression(a) = arg else {
        return parse_quote!(crate::__ds::Worker::new(#handler));
    };
    if a.params.items.len() < 2 {
        return parse_quote!(crate::__ds::Worker::new(#handler));
    }
    let msg_ty = a
        .params
        .items
        .first()
        .and_then(|p| p.type_annotation.as_deref())
        .map(|ta| types::translate_type(&ta.type_annotation));
    match msg_ty {
        Some(ty) => parse_quote!(crate::__ds::Worker::new_with_reply::<#ty, _>(#handler)),
        None => parse_quote!(crate::__ds::Worker::new_with_reply(#handler)),
    }
}

/// The class type name when `callee` is a plain identifier (`Foo`).
fn class_name(callee: &Expression) -> Option<Ident> {
    let Expression::Identifier(id) = callee else {
        return None;
    };
    Some(bindings::type_ident(&id.name))
}
