//! WHATWG `Response` API — `new Response(body?, init?)` constructor (FETCH
//! §5.3, a WinterTC Web API). A `Response` is the fetch receive-side surface:
//! the constructor builds a synthetic `crate::__ds::DsResponse` (no network) from
//! a `body` (a `string`/`Blob`/`Uint8Array` flattened to bytes via the Blob
//! single-part coercion) and an `init` object whose `status` (default `200`),
//! `statusText` (default `""`), and `headers` (a `(name, value)` list, the same
//! plain-object extraction `fetch_init` uses) it reads. `fetch(…)` returns the
//! same `DsResponse` shape (its body drained eagerly by `DsResponse::from_reqwest`),
//! so a constructed and a fetched `Response` share one read surface —
//! `.status`/`.statusText`/`.ok`/`.headers` (member accessors) and
//! `await .text()`/`.json()`/`.arrayBuffer()` (the body-consuming methods on
//! `DsResponse`). `Response.error()`/`.redirect()`/`.json()` statics and the
//! streaming `.body` are not mapped.

use oxc_ast::ast::{Argument, Expression, ObjectExpression, ObjectPropertyKind};
use proc_macro2::Span;
use syn::{parse_quote, Expr, LitStr, Type};

use super::super::super::context::Ctx;
use super::super::super::expressions::translate_expr;
use super::super::global::init_key_name;
use super::blob::blob_part_to_bytes;

/// The Rust type a WHATWG `Response` constructor builds, if `name` is
/// `Response`: `crate::__ds::DsResponse`. `None` otherwise (the `new` lowering
/// falls through to the generic `Foo::new` path and surfaces at `cargo check`).
pub(in crate::translator) fn response_ctor_type(name: &str) -> Option<Type> {
    match name {
        "Response" => Some(parse_quote!(crate::__ds::DsResponse)),
        _ => None,
    }
}

/// `new Response(body?, init?)` → `DsResponse::new(body, status, statusText,
/// headers)`. The `body` is a single ES `BodyInit` (a `string`, a `Blob`, or a
/// `Uint8Array`) flattened to a byte vector via the Blob single-part coercion;
/// an absent body is an empty vector (ES `new Response()` has a null body). A
/// plain-object `init` supplies `status` (default `200`), `statusText` (default
/// `""`), and `headers` (a `(name, value)` list); an absent or non-object `init`
/// takes the defaults. The `status` number is cast to `u16` (the `DsResponse`
/// field type).
pub(in crate::translator) fn response_ctor(args: &[Argument], ctx: &Ctx<'_>) -> Expr {
    // `body`: a single `BodyInit` flattened to bytes; absent → empty (the ES
    // null body). The common shapes (string / Blob local / Uint8Array local)
    // lower via `blob_part_to_bytes`; a non-static body form surfaces honestly.
    let body = match args.first().and_then(Argument::as_expression) {
        Some(e) => blob_part_to_bytes(e, ctx),
        None => parse_quote!(::std::vec![]),
    };
    let (status, status_text, headers) = match args.get(1).and_then(Argument::as_expression) {
        Some(Expression::ObjectExpression(obj)) => response_init(obj, ctx),
        None | Some(_) => default_response_init(),
    };
    parse_quote!(crate::__ds::DsResponse::new(#body, #status, #status_text, #headers))
}

/// `new Response(body, init)`'s `init` object — lowered to the
/// `(status, statusText, headers)` triple `DsResponse::new` takes. `status`
/// (a number) is cast to `u16`; `statusText` lowers as given (an ES string);
/// `headers` (a plain object literal `{name: value}`) lowers to a
/// `(String, String)` list the way `fetch_init` extracts it. Absent fields take
/// the ES defaults (`200` / `""` / empty).
fn response_init(obj: &ObjectExpression<'_>, ctx: &Ctx<'_>) -> (Expr, Expr, Expr) {
    let mut status: Option<Expr> = None;
    let mut status_text: Option<Expr> = None;
    let mut header_pairs: Vec<Expr> = Vec::new();
    for prop in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(op) = prop else {
            continue;
        };
        let Some(name) = init_key_name(&op.key) else {
            continue;
        };
        match name.as_str() {
            "status" => {
                let s = translate_expr(&op.value, ctx);
                status = Some(parse_quote!((#s) as u16));
            }
            "statusText" => status_text = Some(translate_expr(&op.value, ctx)),
            "headers" => {
                if let Expression::ObjectExpression(ho) = &op.value {
                    for hp in &ho.properties {
                        let ObjectPropertyKind::ObjectProperty(hop) = hp else {
                            continue;
                        };
                        let Some(k) = init_key_name(&hop.key) else {
                            continue;
                        };
                        let v = translate_expr(&hop.value, ctx);
                        let k_lit = LitStr::new(&k, Span::call_site());
                        header_pairs.push(parse_quote!((#k_lit.to_string(), (#v).to_string())));
                    }
                }
            }
            _ => {}
        }
    }
    let status = status.unwrap_or_else(|| parse_quote!(200_u16));
    let status_text = status_text.unwrap_or_else(|| parse_quote!(::std::string::String::new()));
    let headers = parse_quote!(::std::vec![#(#header_pairs),*]);
    (status, status_text, headers)
}

/// The ES defaults for a `Response` `init` absent or non-object: status `200`,
/// empty status text, no headers.
fn default_response_init() -> (Expr, Expr, Expr) {
    (
        parse_quote!(200_u16),
        parse_quote!(::std::string::String::new()),
        parse_quote!(::std::vec![]),
    )
}
