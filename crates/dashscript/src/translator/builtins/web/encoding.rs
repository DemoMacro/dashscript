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
//! `TextDecoder.decode` instance-method dispatch (`text_decoder_method`).

use oxc_ast::ast::{Argument, StaticMemberExpression};
use syn::{parse_quote, Expr, Type};

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
