//! WHATWG `Blob` API — `new Blob(parts, options?)` constructor + instance
//! methods/properties (FileAPI, a WinterTC Web API). The constructor flattens
//! the `parts` (a `string`/`BufferSource`/`Blob` sequence) into one byte
//! buffer and builds a `crate::__ds::DsBlob`; `blob_method` dispatches the
//! instance methods (`slice`/`text`/`arrayBuffer`/`bytes`) on the receiver's
//! resolved type; `blob_property` dispatches the `size`/`type` accessors. ES
//! coerces a string part via its UTF-8 bytes; a `number` part routes through
//! `number_to_string` first (the precise ES `ToString`).

use oxc_ast::ast::{
    Argument, ArrayExpression, Expression, ObjectPropertyKind, PropertyKey, StaticMemberExpression,
};
use syn::{parse_quote, Expr, Type};

use super::super::super::context::Ctx;
use super::super::super::expressions::{is_blob_local, translate_expr};

/// The Rust type a WHATWG `Blob` constructor builds, if `name` is `Blob`:
/// `crate::__ds::DsBlob`. `None` otherwise (the `new` lowering falls through
/// to the generic `Foo::new` path and surfaces at `cargo check`).
pub(in crate::translator) fn blob_ctor_type(name: &str) -> Option<Type> {
    match name {
        "Blob" => Some(parse_quote!(crate::__ds::DsBlob)),
        _ => None,
    }
}

/// `new Blob(parts?, options?)` → `DsBlob::new(bytes, type_)`. The `parts`
/// sequence is flattened to a `Vec<u8>` (each `string` → UTF-8 bytes, each
/// `number` → `number_to_string` then bytes, each `Uint8Array`/`Blob` local →
/// its bytes); `options.type` supplies the MIME (default `""`). A non-sequence
/// `parts` panics the `TypeError` ES throws (the WPT verdict reads the panic
/// prefix). `endings` is read but applied as `transparent` (the static path
/// does not model platform newlines); the `native`-endings fixtures are
/// reflection/accessor-shaped and stay out of scope.
pub(in crate::translator) fn blob_ctor(args: &[Argument], ctx: &Ctx<'_>) -> Expr {
    let bytes = match args.first().and_then(Argument::as_expression) {
        None => parse_quote!(::std::vec::Vec::new()),
        Some(Expression::ArrayExpression(arr)) => collect_blob_parts(arr, ctx),
        Some(_) => {
            parse_quote!({ ::core::panic!("TypeError: Blob construct: parts must be a sequence") })
        }
    };
    let type_ = blob_type_arg(args.get(1).and_then(Argument::as_expression), ctx);
    parse_quote!(crate::__ds::DsBlob::new(#bytes, #type_))
}

/// A `Blob` instance method, dispatched on the receiver's resolved type.
/// Returns `None` for a non-`DsBlob` receiver or an unmapped name, so the call
/// falls through to a plain method call (cargo check rejects it honestly).
/// `slice(start, end, contentType)` is synchronous (returns a new `DsBlob`);
/// `text()`/`arrayBuffer()`/`bytes()` are async — ES returns a `Promise`, so
/// the emit is the async fn call and `await blob.text()` adds the `.await`.
pub(in crate::translator) fn blob_method(
    sm: &StaticMemberExpression,
    args: &[Argument],
    ctx: &Ctx<'_>,
) -> Option<Expr> {
    if !is_blob_local(&sm.object, ctx) {
        return None;
    }
    let name = sm.property.name.as_str();
    let obj = translate_expr(&sm.object, ctx);
    Some(match name {
        "slice" => {
            let start = arg_to_f64_option(args.first(), ctx);
            let end = arg_to_f64_option(args.get(1), ctx);
            let ct = arg_to_string_option(args.get(2), ctx);
            parse_quote!(#obj.slice(#start, #end, #ct))
        }
        "text" if args.is_empty() => parse_quote!(#obj.text()),
        "arrayBuffer" if args.is_empty() => parse_quote!(#obj.array_buffer()),
        "bytes" if args.is_empty() => parse_quote!(#obj.bytes()),
        _ => return None,
    })
}

/// Collect a `Blob` parts sequence into a `Vec<u8>` expression: each element
/// becomes a `Vec<u8>` (string → UTF-8 bytes, `number` → `number_to_string`
/// then bytes, a `Vec<u8>` local → itself, a `DsBlob` local → its bytes, any
/// other expression → `ToString` then bytes), and the parts concatenate. A
/// spread/`holes` element is skipped (a non-static part has no lowering here).
pub(super) fn collect_blob_parts(arr: &ArrayExpression, ctx: &Ctx<'_>) -> Expr {
    let mut parts: Vec<Expr> = Vec::new();
    for el in &arr.elements {
        let Some(e) = el.as_expression() else {
            continue;
        };
        parts.push(blob_part_to_bytes(e, ctx));
    }
    parse_quote!({
        let __p: ::std::vec::Vec<::std::vec::Vec<u8>> = ::std::vec![#(#parts),*];
        __p.concat()
    })
}

/// Lower one `BlobPart` to a `Vec<u8>` expression. A `string` literal is its
/// UTF-8 bytes; a `number` is `number_to_string(…)` then bytes (ES `ToString`);
/// an identifier resolves via `Ctx.local_type` — a `DsBlob` donates its bytes,
/// a `Vec` is already bytes, anything else is `ToString`-coerced. Any other
/// expression is `ToString`-coerced (the common fallback for an unannotated
/// binding).
pub(super) fn blob_part_to_bytes(e: &Expression, ctx: &Ctx<'_>) -> Expr {
    match e {
        Expression::StringLiteral(s) => {
            let lit = syn::LitStr::new(s.value.as_str(), proc_macro2::Span::call_site());
            parse_quote!((#lit).as_bytes().to_vec())
        }
        Expression::NumericLiteral(_) => {
            let n = translate_expr(e, ctx);
            parse_quote!(crate::__ds::number_to_string(#n).as_bytes().to_vec())
        }
        Expression::Identifier(id) => {
            use super::super::super::bindings::snake;
            let name = snake(&id.name).to_string();
            let seg = ctx
                .local_type(&name)
                .and_then(|p| p.segments.last().map(|s| s.ident.to_string()));
            match seg.as_deref() {
                Some("DsBlob") => {
                    let v = translate_expr(e, ctx);
                    parse_quote!((#v).bytes.clone())
                }
                Some("Vec") => translate_expr(e, ctx),
                _ => {
                    let v = translate_expr(e, ctx);
                    parse_quote!((#v).to_string().as_bytes().to_vec())
                }
            }
        }
        _ => {
            let v = translate_expr(e, ctx);
            parse_quote!((#v).to_string().as_bytes().to_vec())
        }
    }
}

/// Read `options.type` (the MIME string, default `""`) from the ES options
/// object. A non-object `options` yields `""` (ES `ToString(options)` would
/// rarely carry a usable `type`; the static path does not chase it).
pub(super) fn blob_type_arg(opt: Option<&Expression>, ctx: &Ctx<'_>) -> Expr {
    let Some(Expression::ObjectExpression(obj)) = opt else {
        return parse_quote!(::std::string::String::new());
    };
    for kind in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(p) = kind else {
            continue;
        };
        let is_type = match &p.key {
            PropertyKey::StaticIdentifier(id) => id.name.as_str() == "type",
            PropertyKey::StringLiteral(s) => s.value.as_str() == "type",
            _ => false,
        };
        if is_type {
            return expr_to_string(&p.value, ctx);
        }
    }
    parse_quote!(::std::string::String::new())
}

/// `blob.slice(start, …)` argument → `Option<f64>`. `None` when absent; else
/// `Some(<expr>)` (the ES argument is a number; the receiver's `slice`
/// resolves the index, clamping non-finite values).
fn arg_to_f64_option(arg: Option<&Argument>, ctx: &Ctx<'_>) -> Expr {
    match arg.and_then(Argument::as_expression) {
        Some(e) => {
            let v = translate_expr(e, ctx);
            parse_quote!(::std::option::Option::Some(#v))
        }
        None => parse_quote!(::std::option::Option::None),
    }
}

/// `blob.slice(…, contentType)` argument → `Option<String>`. `None` when
/// absent; else `Some(<to-string>)` via ES `ToString`.
fn arg_to_string_option(arg: Option<&Argument>, ctx: &Ctx<'_>) -> Expr {
    match arg.and_then(Argument::as_expression) {
        Some(e) => {
            let v = expr_to_string(e, ctx);
            parse_quote!(::std::option::Option::Some(#v))
        }
        None => parse_quote!(::std::option::Option::None),
    }
}

/// Coerce an arbitrary `Expression` to a `String`-typed expression via ES
/// `ToString`. A `number` routes through `number_to_string` (the precise ES
/// form); any other expression lowers via `translate_expr` then `.to_string()`.
pub(super) fn expr_to_string(expr: &Expression, ctx: &Ctx<'_>) -> Expr {
    match expr {
        Expression::NumericLiteral(_) => {
            let n = translate_expr(expr, ctx);
            parse_quote!(crate::__ds::number_to_string(#n))
        }
        _ => {
            let e = translate_expr(expr, ctx);
            parse_quote!((#e).to_string())
        }
    }
}
