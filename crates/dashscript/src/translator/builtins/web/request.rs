//! WHATWG `Request` API — `new Request(url, init?)` constructor (FETCH §5.2, a
//! WinterTC Web API). A `Request` is a fetch descriptor: the constructor parses
//! the ES `init` object's `method`/`body`/`headers` via the same `fetch_init`
//! extraction `fetch(url, init)` uses, and builds a `crate::__ds::DsRequest`.
//! `fetch(request)` then unwraps its fields via `ds_fetch_request` (dispatched
//! in `builtins::global`). The `.url`/`.method`/`.headers` read-only accessors
//! dispatch in the member path (ES properties → zero-arg Rust methods, like
//! `Blob.size`); the streaming `.body` (a ReadableStream) and the no-fetch-
//! effect metadata (`.cache`/`.mode`/`.credentials`) are not mapped.

use oxc_ast::ast::{Argument, Expression};
use syn::{parse_quote, Expr, Type};

use super::super::super::context::Ctx;
use super::super::es_to_string_arg;
use super::super::global::fetch_init;

/// The Rust type a WHATWG `Request` constructor builds, if `name` is `Request`:
/// `crate::__ds::DsRequest`. `None` otherwise (the `new` lowering falls through
/// to the generic `Foo::new` path and surfaces at `cargo check`).
pub(in crate::translator) fn request_ctor_type(name: &str) -> Option<Type> {
    match name {
        "Request" => Some(parse_quote!(crate::__ds::DsRequest)),
        _ => None,
    }
}

/// `new Request(url, init?)` → `DsRequest::new(url, method, body, headers)`.
/// The `url` is ES `ToString`-coerced; a plain-object `init` is parsed by
/// `fetch_init` (the same `(method, body, headers)` extraction `fetch(url,
/// init)` uses), so a `Request` and an inline `init` agree; no `init` defaults
/// to GET / no body / no headers. `new Request(otherRequest)` (Request cloning)
/// and any non-object second arg have no static lowering — the `TypeError` ES
/// surfaces is emitted as a panic (the WPT verdict reads the panic prefix),
/// mirroring `form_data_ctor`'s rejection of the `(form)` arg.
pub(in crate::translator) fn request_ctor(args: &[Argument], ctx: &Ctx<'_>) -> Expr {
    // ES `new Request()` (no url) throws a TypeError; surface it as a panic
    // (the WPT verdict reads the panic prefix), the way `form_data_ctor` rejects
    // the `(form)` arg. The `url` is ES `ToString`-coerced.
    let url = match args.first() {
        Some(arg) => es_to_string_arg(arg, ctx),
        None => {
            return parse_quote!({ ::core::panic!("TypeError: Request construct: url required") });
        }
    };
    let (method, body, headers) = match args.get(1).and_then(Argument::as_expression) {
        Some(Expression::ObjectExpression(obj)) => fetch_init(obj, ctx),
        None => (
            parse_quote!("GET".to_string()),
            parse_quote!(::std::option::Option::None),
            parse_quote!(::std::vec![]),
        ),
        // A non-object second arg (Request cloning, or an unparseable init) has
        // no static lowering — diverge with the `TypeError` panic (the triple
        // type is never materialized; the `return` short-circuits).
        Some(_) => {
            return parse_quote!({
                ::core::panic!("TypeError: Request construct: init must be an object")
            });
        }
    };
    parse_quote!(crate::__ds::DsRequest::new(#url, #method, #body, #headers))
}
