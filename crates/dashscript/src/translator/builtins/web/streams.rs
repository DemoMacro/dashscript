//! WHATWG `ReadableStream` API (a WinterTC Web API) — the readable side of the
//! Streams standard. The push-source baseline: `new ReadableStream({ start(c) {
//! c.enqueue(…); c.close() } })` + `stream.getReader()` + `await reader.read()`
//! → `{ done, value }`. `controller.enqueue(v)`/`.close()` map to the
//! `DsReadableStreamController`; `reader.read()` polls the shared chunk queue
//! via a `DsReadResult`. Backed by the `Arc<Mutex<…>>` state machine injected
//! by the `Streams` runtime dep (mirroring `DsResolver`); pure `std`, never
//! degraded. `pull`/`cancel`/`tee`/BYOB are out of scope (honest partials).

use oxc_ast::ast::{
    Argument, Expression, ObjectExpression, ObjectPropertyKind, PropertyKey, StaticMemberExpression,
};
use syn::{parse_quote, Expr, Pat, Type};

use super::super::super::bindings;
use super::super::super::context::Ctx;
use super::super::super::expressions::{
    body_block_with_param_type, translate_argument, translate_expr,
};

/// The Rust type a `ReadableStream` constructor builds: `ReadableStream` →
/// `crate::__ds::DsReadableStream`. `None` for any other name (the `new`
/// lowering falls through to the generic `Foo::new` path).
pub(in crate::translator) fn streams_ctor_type(name: &str) -> Option<Type> {
    match name {
        "ReadableStream" => Some(parse_quote!(crate::__ds::DsReadableStream)),
        _ => None,
    }
}

/// `new ReadableStream(...)` — the constructor emit. `new ReadableStream()` (no
/// underlying source) → `empty_closed` (a stream whose first `read()` is
/// `{ done: true }`); `new ReadableStream({ start(controller) { … } })` →
/// `from_start(|controller| { … })` (the controller param is registered as
/// `DsReadableStreamController` so `controller.enqueue(v)`/`.close()` dispatch).
/// Any other shape (a non-object source, a source without `start`, or a
/// `start` that is not a function) has no static form → `empty_closed` (the
/// stream exists but produces nothing — an honest partial, not an E0433).
pub(in crate::translator) fn readable_stream_ctor(args: &[Argument], ctx: &Ctx<'_>) -> Expr {
    let Some(start) = args
        .first()
        .and_then(|a| a.as_expression())
        .and_then(|e| match e {
            Expression::ObjectExpression(o) => Some(o),
            _ => None,
        })
        .and_then(|o| start_value(o))
    else {
        return parse_quote!(crate::__ds::DsReadableStream::<()>::empty_closed());
    };
    let (pat, body) = start_closure(start, ctx);
    parse_quote!(crate::__ds::DsReadableStream::from_start(|#pat| #body))
}

/// The `start` method's value if it is a function/arrow; `None` otherwise. The
/// value may be a `FunctionExpression` (`start(c) { … }`) or an
/// `ArrowFunctionExpression` (`start: (c) => { … }`); a non-function `start`
/// has no static form.
fn start_value<'a>(obj: &'a ObjectExpression<'a>) -> Option<&'a Expression<'a>> {
    for kind in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(p) = kind else {
            continue;
        };
        let name = match &p.key {
            PropertyKey::StaticIdentifier(id) => id.name.as_str(),
            PropertyKey::StringLiteral(s) => s.value.as_str(),
            _ => continue,
        };
        if name != "start" {
            continue;
        }
        return match &p.value {
            Expression::FunctionExpression(_) | Expression::ArrowFunctionExpression(_) => {
                Some(&p.value)
            }
            _ => None,
        };
    }
    None
}

/// Build the `|controller| { body }` pair for the `start` callback: the
/// controller pat (registered as `DsReadableStreamController` so its methods
/// dispatch) and the translated body block. A `start` with no parameter still
/// lowers with a `|_|` (the closure must take one arg); an expression-body
/// arrow has no statement list, so it falls back to `|_| {}` (an honest
/// no-op `start`).
fn start_closure(start: &Expression, ctx: &Ctx<'_>) -> (Pat, syn::Block) {
    let (params, body) = match start {
        Expression::FunctionExpression(f) => match f.body.as_deref() {
            Some(b) => (&f.params, b),
            None => return (parse_quote!(_), parse_quote!({})),
        },
        Expression::ArrowFunctionExpression(a) => (&a.params, a.body.as_ref()),
        _ => return (parse_quote!(_), parse_quote!({})),
    };
    let (pat, override_) = match params.items.first() {
        Some(fp) => {
            let ident = bindings::binding_name(&fp.pattern);
            let name = ident.to_string();
            (
                parse_quote!(#ident),
                Some((name, parse_quote!(crate::__ds::DsReadableStreamController))),
            )
        }
        None => (parse_quote!(_), None),
    };
    let block = body_block_with_param_type(params, body, ctx, override_);
    (pat, block)
}

/// A `ReadableStream` / reader / controller instance method, dispatched on the
/// receiver's resolved type. Returns `None` for an unmapped receiver or name,
/// so the call falls through to a plain method call (cargo check rejects it
/// honestly). Covers `stream.getReader()`, `reader.read()`, and
/// `controller.enqueue(v)`/`.close()` — the push-source read loop.
pub(in crate::translator) fn streams_method(
    sm: &StaticMemberExpression,
    args: &[Argument],
    ctx: &Ctx<'_>,
) -> Option<Expr> {
    let obj = translate_expr(&sm.object, ctx);
    let name = sm.property.name.as_str();
    // `stream.getReader()` on a `DsReadableStream` local.
    if is_stream_local(&sm.object, ctx, "DsReadableStream") {
        return match name {
            "getReader" if args.is_empty() => Some(parse_quote!(#obj.get_reader())),
            _ => None,
        };
    }
    // `reader.read()` on a `DsReadableStreamDefaultReader` local.
    if is_stream_local(&sm.object, ctx, "DsReadableStreamDefaultReader") {
        return match name {
            "read" if args.is_empty() => Some(parse_quote!(#obj.read())),
            _ => None,
        };
    }
    // `controller.enqueue(v)` / `.close()` on a `DsReadableStreamController`
    // local (the `start(controller)` callback's param).
    if is_stream_local(&sm.object, ctx, "DsReadableStreamController") {
        return match name {
            "enqueue" => {
                let v = translate_argument(args.first()?, ctx);
                Some(parse_quote!({ #obj.enqueue(#v); }))
            }
            "close" if args.is_empty() => Some(parse_quote!({ #obj.close(); })),
            _ => None,
        };
    }
    None
}

/// True when `expr` is a local whose recorded type's last segment is `type_`
/// (a `DsReadableStream` / `DsReadableStreamDefaultReader` /
/// `DsReadableStreamController` binding). The controller's type is registered
/// by [`super::super::super::expressions::body_block_with_param_type`] (the
/// `start` callback's param); the stream/reader types by `register_declarator`.
fn is_stream_local(expr: &Expression, ctx: &Ctx<'_>, type_: &str) -> bool {
    let Expression::Identifier(id) = expr else {
        return false;
    };
    let name = bindings::snake(&id.name).to_string();
    ctx.local_type(&name)
        .is_some_and(|p| p.segments.last().is_some_and(|s| s.ident == type_))
}
