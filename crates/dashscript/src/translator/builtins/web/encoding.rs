//! WHATWG Encoding API — `new TextEncoder()` / `new TextDecoder(…)` (a
//! WinterTC Web API). `TextEncoder` is UTF-8 only and stateless (the Encoding
//! API guarantees no other encode encoding); `TextDecoder` resolves a `label`
//! to an `encoding_rs::Encoding` at construction and carries `fatal`/
//! `ignoreBOM` flags — so it is NOT a stateless unit, but every field is
//! `Copy` (`&'static Encoding`, `bool`, `&'static str`), so a module-global
//! singleton is still sound behind a `OnceLock`. The structs and the
//! `encoding_rs` dep are injected by the `Encoding` runtime dep
//! (`__ds::TextEncoder` / `__ds::TextDecoder`). This module carries the
//! constructor name → type mapping (`encoding_ctor_type`) and the
//! `TextEncoder.encode` / `TextDecoder.decode` instance-method dispatch
//! (`text_encoder_method` / `text_decoder_method`).

use oxc_ast::ast::{Argument, Expression, ObjectPropertyKind, PropertyKey, StaticMemberExpression};
use syn::{parse_quote, Expr, Type};

use super::super::super::bindings;
use super::super::super::context::Ctx;
use super::super::super::expressions::{is_text_decoder_local, translate_argument, translate_expr};

/// The Rust type a WHATWG Encoding API constructor builds, if `name` is one:
/// `TextEncoder` → `crate::__ds::TextEncoder`, `TextDecoder` →
/// `crate::__ds::TextDecoder`. `None` for any other name.
pub(in crate::translator) fn encoding_ctor_type(name: &str) -> Option<Type> {
    match name {
        "TextEncoder" => Some(parse_quote!(crate::__ds::TextEncoder)),
        "TextDecoder" => Some(parse_quote!(crate::__ds::TextDecoder)),
        _ => None,
    }
}

/// `decoder.decode(bytes[, options])` on a `TextDecoder` local →
/// `decoder.decode(bytes, stream)`. The ES `decode` second arg `{ stream }`
/// controls an instance buffer that carries an incomplete multi-byte sequence
/// across calls: a literal `stream: true` lowers to the runtime `stream` flag
/// (buffer the trailing sequence; more input coming); anything else (absent,
/// `stream: false`, or a non-literal value) is a flush. A non-`TextDecoder`
/// receiver or an unmapped name falls through to a plain call (`cargo check`
/// rejects it honestly if the shapes do not line up).
pub(in crate::translator) fn text_decoder_method(
    sm: &StaticMemberExpression,
    args: &[Argument],
    ctx: &Ctx<'_>,
) -> Option<Expr> {
    if sm.property.name.as_str() != "decode" {
        return None;
    }
    if !is_text_decoder_local(&sm.object, ctx) {
        return None;
    }
    let recv = translate_expr(&sm.object, ctx);
    // `decoder.decode()` with no args is the ES "flush the stream" call —
    // equivalent to `decoder.decode(new Uint8Array())`, i.e. an empty buffer
    // with `stream: false` (flush any pending bytes from a prior `stream: true`
    // call as replacement).
    let bytes = match args.first() {
        Some(arg) => translate_argument(arg, ctx),
        None => parse_quote!(::std::vec::Vec::<u8>::new()),
    };
    let stream = decode_stream_flag(args);
    Some(parse_quote!(#recv.decode(#bytes, #stream)))
}

/// The ES `decode(bytes, { stream })` second arg's literal `stream` value, as a
/// `bool` expr. Only a BooleanLiteral `stream` field lowers statically (the
/// common fixture shape); an absent field, a `false` literal, or a non-literal
/// value defaults to `false` (a flush), matching `decode_options`.
fn decode_stream_flag(args: &[Argument]) -> Expr {
    let value = match args.get(1) {
        Some(Argument::ObjectExpression(obj)) => obj.properties.iter().find_map(|kind| {
            let ObjectPropertyKind::ObjectProperty(p) = kind else {
                return None;
            };
            let name = match &p.key {
                PropertyKey::StaticIdentifier(id) => id.name.as_str(),
                PropertyKey::StringLiteral(s) => s.value.as_str(),
                _ => return None,
            };
            (name == "stream")
                .then_some(&p.value)
                .and_then(|v| match v {
                    Expression::BooleanLiteral(b) => Some(b.value),
                    _ => None,
                })
        }),
        _ => None,
    };
    match value {
        Some(b) => {
            let lit = syn::LitBool::new(b, proc_macro2::Span::call_site());
            parse_quote!(#lit)
        }
        None => parse_quote!(false),
    }
}

/// `encoder.encode()` / `encoder.encode(undefined)` on a `TextEncoder` (a
/// `new TextEncoder()` local or an inline `new TextEncoder()` expression) →
/// `encoder.encode(String::new())`. The ES signature is `encode(input = "")`:
/// both a missing argument and an explicit `undefined` trigger the default
/// (JS default-parameter semantics, not `String(undefined)`), and `""` UTF-8
/// encodes to an empty byte sequence. A supplied value falls through to a
/// plain call — the `String` argument is already handled by the generic path.
pub(in crate::translator) fn text_encoder_method(
    sm: &StaticMemberExpression,
    args: &[Argument],
    ctx: &Ctx<'_>,
) -> Option<Expr> {
    if !is_text_encoder_recv(&sm.object, ctx) {
        return None;
    }
    let recv = translate_expr(&sm.object, ctx);
    match sm.property.name.as_str() {
        // `encoder.encode()` / `encoder.encode(undefined)` → `encode(String::new())`.
        // The ES signature is `encode(input = "")`: both a missing argument and
        // an explicit `undefined` trigger the default (JS default-parameter
        // semantics, not `String(undefined)`). A supplied value falls through to
        // a plain call — the `String` argument is already handled by the generic path.
        "encode" => {
            let is_default = match args.first() {
                None => true,
                Some(Argument::Identifier(id)) => id.name.as_str() == "undefined",
                _ => false,
            };
            if !is_default {
                return None;
            }
            Some(parse_quote!(#recv.encode(::std::string::String::new())))
        }
        // `encoder.encodeInto(src, dst)` → `encode_into(&src, &mut dst)`. The ES
        // destination is a `Uint8Array` (`Vec<u8>`); `encodeInto` writes in
        // place, so it must borrow the destination binding as `&mut` (a clone
        // would drop the writes on the floor — ES `dst` is reference-semantic).
        // A non-identifier destination (an inline expression) has no binding to
        // borrow, so it falls through. Returns `{ read, written }`.
        "encodeInto" => {
            let src = translate_argument(args.first()?, ctx);
            let dst: Expr = match args.get(1)? {
                Argument::Identifier(id) => {
                    let name = bindings::snake(&id.name);
                    parse_quote!(&mut #name)
                }
                _ => return None,
            };
            Some(parse_quote!(#recv.encode_into(&#src, #dst)))
        }
        _ => None,
    }
}

/// True when `expr` is a `TextEncoder` receiver — either a local whose type
/// is `crate::__ds::TextEncoder` (a `let e = new TextEncoder()` binding) or an
/// inline `new TextEncoder()` expression. The inline form is the common WPT
/// shape (`assert_array_equals(new TextEncoder().encode(), [])`), so unlike
/// [`is_text_decoder_local`] the check is not identifier-only.
fn is_text_encoder_recv(expr: &Expression, ctx: &Ctx<'_>) -> bool {
    // Unwrap one layer of parentheses — `(new TextEncoder()).encode()` reaches
    // the member object as a `ParenthesizedExpression(NewExpression)`.
    let inner = match expr {
        Expression::ParenthesizedExpression(p) => &p.expression,
        other => other,
    };
    match inner {
        Expression::Identifier(id) => {
            let name = bindings::snake(&id.name).to_string();
            ctx.local_type(&name)
                .is_some_and(|p| p.segments.last().is_some_and(|s| s.ident == "TextEncoder"))
        }
        Expression::NewExpression(n) => {
            matches!(&n.callee, Expression::Identifier(id) if id.name.as_str() == "TextEncoder")
        }
        _ => false,
    }
}
