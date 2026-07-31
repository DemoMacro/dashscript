//! `new Foo(args)` → `Foo::new(args)`.
use oxc_ast::ast::{Argument, Expression, NewExpression};
use syn::{parse_quote, Expr, Ident};

use super::super::bindings;
use super::super::builtins;
use super::super::context::Ctx;
use super::super::types;
use super::{array_elem_arg, array_owned_expr};

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
        // `new Uint8Array(n)` / `Uint8ClampedArray(n)` / `Int8Array(n)` — an ES
        // typed array of `u8` elements (a crypto byte buffer). Two constructor
        // forms lower: a numeric length `new Uint8Array(n)` → `vec![0_u8; n as
        // usize]` (n zeroed u8s), and `new Uint8Array([1, 2, 3])` → a copy with
        // each element cast to u8 (the typed-array-from-array case). An empty
        // `new Uint8Array()` is an empty vec. The element type is `u8` for these
        // three; other typed arrays (`Int32Array`, …) and a non-array iterable
        // arg are later work — they fall through to the generic `Foo::new(…)`
        // path and surface at `cargo check` honestly.
        if matches!(
            id.name.as_str(),
            "Uint8Array" | "Uint8ClampedArray" | "Int8Array"
        ) {
            if n.arguments.is_empty() {
                return parse_quote!(::std::vec::Vec::<u8>::new());
            }
            if n.arguments.len() == 1 {
                let arg = &n.arguments[0];
                let elem = array_elem_arg(arg, ctx);
                if matches!(arg.as_expression(), Some(Expression::ArrayExpression(_))) {
                    // `new Uint8Array([1, 2, 3])` — copy from a literal array.
                    return parse_quote!(
                        (#elem).into_iter().map(|x| x as u8).collect::<::std::vec::Vec<u8>>()
                    );
                }
                // `new Uint8Array(n)` — n zeroed u8 elements.
                return parse_quote!(::std::vec![0_u8; (#elem) as usize]);
            }
        }
        // `new TextEncoder()` / `new TextDecoder()` — the WHATWG Encoding API
        // (a WinterTC Web API). Stateless global constructors; `.encode`/
        // `.decode` map to the `__ds::TextEncoder`/`__ds::TextDecoder` impls
        // (UTF-8). Intercepted before the generic `Foo::new` path, which would
        // emit `TextEncoder::new` (E0425 — no such Rust type). The `Encoding`
        // runtime dep is flagged by the `__ds::TextEncoder` marker probe, which
        // injects both struct defs into `__ds.rs`.
        if matches!(id.name.as_str(), "TextEncoder" | "TextDecoder") {
            let name = bindings::type_ident(&id.name);
            return parse_quote!(crate::__ds::#name::new());
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
    // `new Set([a, b, …])` → `HashSet::from([a, b, …])` — a literal initial set
    // of scalar values, the common module-constant case. ES Set accepts any
    // iterable; a spread or a non-array arg falls through to the generic
    // `Set::new(…)` path (a `cargo check` error honestly), matching `new Map()`.
    if name.to_string().as_str() == "Set" {
        if let Some(e) = set_from_array_arg(&n.arguments, ctx) {
            return e;
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

/// `new Set([a, b, …])` → `HashSet::from([a, b, …])` — a literal initial set of
/// scalar values, the common module-constant case. `None` unless the sole arg
/// is a spread-free array literal, so anything else falls through to the
/// generic `Set::new(…)` path.
fn set_from_array_arg(args: &[Argument], ctx: &Ctx<'_>) -> Option<Expr> {
    if args.len() != 1 {
        return None;
    }
    let Expression::ArrayExpression(arr) = args[0].as_expression()? else {
        return None;
    };
    let arr_expr = array_owned_expr(arr, ctx)?;
    Some(parse_quote!(::std::collections::HashSet::from(#arr_expr)))
}

/// The class type name when `callee` is a plain identifier (`Foo`).
fn class_name(callee: &Expression) -> Option<Ident> {
    let Expression::Identifier(id) = callee else {
        return None;
    };
    Some(bindings::type_ident(&id.name))
}
