//! WebCrypto API — `crypto.randomUUID()` / `crypto.getRandomValues(buf)`
//! (Tier 1) and `crypto.subtle.digest(algo, data)` (the one-shot hash, a
//! WinterTC Web API, W3C WebCrypto). The receiver is the global `crypto`
//! object; WinterTC's `self` is the global-object alias (`self === globalThis`),
//! so both `crypto.*` and `self.crypto.*` lower to the same `__ds::crypto_*`
//! helper. `randomUUID` returns an RFC 4122 v4 UUID string; `getRandomValues`
//! fills a `Uint8Array` byte buffer; `subtle.digest` is the async hash backed
//! by the RustCrypto `sha1`/`sha2` crates (pure-Rust — never degraded). The
//! key-bearing `SubtleCrypto` methods (`encrypt`/`sign`/`verify`/`generateKey`/
//! `importKey`/`deriveBits`) need a `CryptoKey` value model and are not yet
//! mapped.

use oxc_ast::ast::{Argument, Expression, StaticMemberExpression};
use syn::{parse_quote, Expr};

use super::super::super::bindings::snake;
use super::super::super::context::Ctx;
use super::super::super::expressions::translate_argument;
use super::super::es_to_string_arg;
use super::blob::{blob_part_to_bytes, collect_blob_parts};

/// `crypto.<method>(…)` / `crypto.subtle.<method>(…)` → the matching
/// `__ds::crypto_*` helper. `None` unless the callee roots at the global
/// `crypto` object (the bare identifier, or via the WinterTC `self` global-object
/// alias) and the method is one of the mapped forms; any other receiver/name
/// falls through to a plain method call (cargo check rejects it honestly). The
/// two-level `crypto.subtle.digest(algo, data)` chain is detected before the
/// Tier-1 `crypto_receiver` guard (its receiver is `crypto.subtle`, not `crypto`).
pub(in crate::translator) fn crypto_method(
    sm: &StaticMemberExpression,
    args: &[Argument],
    ctx: &Ctx<'_>,
) -> Option<Expr> {
    // `crypto.subtle.digest(algo, data)` — the two-level chain. The receiver
    // (`sm.object`) is itself `crypto.subtle`, rooted at the global `crypto`.
    // ES `digest` returns a `Promise<ArrayBuffer>`; the emit is the bare async
    // fn call, and the `await` at the call site drives the future (the async
    // entry gate flips `fn main` to `#[tokio::main]`). Intercepted before the
    // Tier-1 guard (which rejects `crypto.subtle` as a non-`crypto` receiver).
    if sm.property.name.as_str() == "digest" {
        if let Expression::StaticMemberExpression(inner) = &sm.object {
            if inner.property.name.as_str() == "subtle"
                && is_crypto_receiver(&inner.object).is_some()
            {
                let algo = es_to_string_arg(args.first()?, ctx);
                let data = digest_data_arg(args.get(1)?, ctx);
                return Some(parse_quote!(crate::__ds::crypto_subtle_digest(#algo, #data)));
            }
        }
    }
    // `crypto.randomUUID()` / `crypto.getRandomValues(buf)` (Tier 1).
    is_crypto_receiver(&sm.object)?;
    Some(match sm.property.name.as_str() {
        // `crypto.randomUUID()` → `__ds::crypto_random_uuid()`.
        "randomUUID" => parse_quote!(crate::__ds::crypto_random_uuid()),
        // `crypto.getRandomValues(buf)` → `__ds::crypto_get_random_values(buf)`.
        // The argument must lower to a `Vec<u8>` (a `Uint8Array`); the helper
        // consumes and returns it filled (ES returns the same typed array).
        "getRandomValues" => {
            let arg = translate_argument(args.first()?, ctx);
            parse_quote!(crate::__ds::crypto_get_random_values(#arg))
        }
        _ => return None,
    })
}

/// Coerce the `data` argument of `crypto.subtle.digest(algo, data)` to a
/// `Vec<u8>` expression. ES accepts a `BufferSource` (an `ArrayBuffer` or an
/// `ArrayBufferView` like `Uint8Array`); the common shapes lower:
/// - a `new Uint8Array(…)` expression → already a `Vec<u8>`, passed through;
/// - a `Vec<u8>` local (a `Uint8Array` binding) → passed through;
/// - an array literal `[…]` → the `Blob` parts collector (string/number/blob
///   elements flattened to bytes);
/// - a string / any other expression → UTF-8 / `ToString` bytes (the `Blob`
///   single-part coercion, reused via `blob_part_to_bytes`).
fn digest_data_arg(arg: &Argument, ctx: &Ctx<'_>) -> Expr {
    let Some(e) = arg.as_expression() else {
        return translate_argument(arg, ctx);
    };
    match e {
        Expression::ArrayExpression(arr) => collect_blob_parts(arr, ctx),
        Expression::NewExpression(_) => translate_argument(arg, ctx),
        Expression::Identifier(id) => {
            let name = snake(&id.name).to_string();
            let is_vec = ctx
                .local_type(&name)
                .is_some_and(|p| p.segments.last().is_some_and(|s| s.ident == "Vec"));
            if is_vec {
                translate_argument(arg, ctx)
            } else {
                blob_part_to_bytes(e, ctx)
            }
        }
        _ => blob_part_to_bytes(e, ctx),
    }
}

/// Whether `expr` is the global `crypto` object: the bare identifier, or
/// `self.crypto` (the WinterTC `self` global-object alias).
fn is_crypto_receiver(expr: &Expression) -> Option<()> {
    match expr {
        Expression::Identifier(id) if id.name.as_str() == "crypto" => Some(()),
        Expression::StaticMemberExpression(sm) if sm.property.name.as_str() == "crypto" => {
            matches!(&sm.object, Expression::Identifier(id) if id.name.as_str() == "self")
                .then_some(())
        }
        _ => None,
    }
}
