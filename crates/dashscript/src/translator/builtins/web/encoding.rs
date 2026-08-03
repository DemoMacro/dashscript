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

use oxc_ast::ast::{Argument, Expression, StaticMemberExpression};
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
/// `decoder.decode(bytes)`. The ES `decode` second arg `{ stream }` controls
/// an instance buffer that carries an incomplete multi-byte sequence across
/// calls; that streaming state is not modeled, so the option is dropped and
/// each call decodes an independent buffer — matching every fixture that does
/// not split a multi-byte sequence across calls. A non-`TextDecoder` receiver
/// or an unmapped name falls through to a plain call (`cargo check` rejects it
/// honestly if the shapes do not line up).
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
    // equivalent to `decoder.decode(new Uint8Array())`. The streaming instance
    // buffer is not modeled, so this decodes an empty buffer.
    let bytes = match args.first() {
        Some(arg) => translate_argument(arg, ctx),
        None => parse_quote!(::std::vec::Vec::<u8>::new()),
    };
    Some(parse_quote!(#recv.decode(#bytes)))
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
    if sm.property.name.as_str() != "encode" {
        return None;
    }
    if !is_text_encoder_recv(&sm.object, ctx) {
        return None;
    }
    let is_default = match args.first() {
        None => true,
        Some(Argument::Identifier(id)) => id.name.as_str() == "undefined",
        _ => false,
    };
    if !is_default {
        return None;
    }
    let recv = translate_expr(&sm.object, ctx);
    Some(parse_quote!(#recv.encode(::std::string::String::new())))
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
