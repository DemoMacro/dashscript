//! WebCrypto API — `crypto.randomUUID()` / `crypto.getRandomValues(buf)`
//! (Tier 1), `crypto.subtle.digest(algo, data)` (the one-shot hash), and the
//! HMAC key-bearing subset `crypto.subtle.importKey/sign/verify` (WinterTC Web
//! API, W3C WebCrypto). The receiver is the global `crypto` object; WinterTC's
//! `self` is the global-object alias (`self === globalThis`), so both `crypto.*`
//! and `self.crypto.*` lower to the same `__ds::crypto_*` helper. `randomUUID`
//! returns an RFC 4122 v4 UUID string; `getRandomValues` fills a `Uint8Array`
//! byte buffer; `subtle.digest` is the async hash backed by the RustCrypto
//! `sha1`/`sha2` crates; `subtle.importKey` (raw format) builds a `DsCryptoKey`,
//! and `subtle.sign`/`.verify` are the async HMAC backed by `hmac` (pure-Rust —
//! never degraded). The remaining `SubtleCrypto` methods (`encrypt`/`decrypt`/
//! `generateKey`/`deriveBits`) need a wider key model and are not yet mapped.

use oxc_ast::ast::{Argument, Expression, ObjectPropertyKind, PropertyKey, StaticMemberExpression};
use syn::{parse_quote, Expr};

use super::super::super::bindings::snake;
use super::super::super::context::Ctx;
use super::super::super::expressions::translate_argument;
use super::super::es_to_string_arg;
use super::blob::{blob_part_to_bytes, collect_blob_parts, expr_to_string};

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
    // `crypto.subtle.importKey(format, keyData, algorithm, extractable, usages)`
    // — the HMAC subset (format `"raw"`; pkcs8/spki are not statically modeled).
    // The `algorithm` object's `name`+`hash` are extracted at translate time; the
    // raw `keyData` bytes and `extractable` flag build a `DsCryptoKey`. `usages`
    // is not enforced (lowered empty). ES returns `Promise<CryptoKey>`; the call
    // site's `await` drives the future, and `callee_return_path` records the
    // `DsCryptoKey` type so a later `sign`/`verify` passes the key through.
    if sm.property.name.as_str() == "importKey" {
        if let Expression::StaticMemberExpression(inner) = &sm.object {
            if inner.property.name.as_str() == "subtle"
                && is_crypto_receiver(&inner.object).is_some()
            {
                let key = digest_data_arg(args.get(1)?, ctx);
                let (algorithm, hash) = import_key_algorithm(args.get(2)?, ctx)?;
                let extractable = bool_argument(args.get(3), ctx);
                let usages: Expr = parse_quote!(::std::vec![]);
                return Some(parse_quote!(
                    crate::__ds::crypto_subtle_import_key(#algorithm, #hash, #key, #extractable, #usages)
                ));
            }
        }
    }
    // `crypto.subtle.sign(algo, key, data)` — the HMAC subset. The ES `algo` arg
    // is carried by the key (`key.algorithm`, verified `"HMAC"`); `data` is a
    // `BufferSource` (reuses the digest coercion). ES returns `Promise<ArrayBuffer>`.
    if sm.property.name.as_str() == "sign" {
        if let Expression::StaticMemberExpression(inner) = &sm.object {
            if inner.property.name.as_str() == "subtle"
                && is_crypto_receiver(&inner.object).is_some()
            {
                let key = translate_argument(args.get(1)?, ctx);
                let data = digest_data_arg(args.get(2)?, ctx);
                return Some(parse_quote!(crate::__ds::crypto_subtle_sign(&#key, #data)));
            }
        }
    }
    // `crypto.subtle.verify(algo, key, signature, data)` — the HMAC subset. ES
    // returns `Promise<boolean>`.
    if sm.property.name.as_str() == "verify" {
        if let Expression::StaticMemberExpression(inner) = &sm.object {
            if inner.property.name.as_str() == "subtle"
                && is_crypto_receiver(&inner.object).is_some()
            {
                let key = translate_argument(args.get(1)?, ctx);
                let signature = digest_data_arg(args.get(2)?, ctx);
                let data = digest_data_arg(args.get(3)?, ctx);
                return Some(parse_quote!(
                    crate::__ds::crypto_subtle_verify(&#key, #signature, #data)
                ));
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

/// Whether `expr` is `crypto.subtle` (or `self.crypto.subtle`) — the receiver of
/// the two-level `crypto.subtle.<method>` chains. Used by `callee_return_path` to
/// record the `DsCryptoKey` return type of `crypto.subtle.importKey(…)` (so a
/// later `sign`/`verify` passes the key local through as a `DsCryptoKey` arg).
pub(in crate::translator) fn is_crypto_subtle_member(expr: &Expression) -> bool {
    matches!(expr, Expression::StaticMemberExpression(sm)
        if sm.property.name.as_str() == "subtle"
        && is_crypto_receiver(&sm.object).is_some())
}

/// `crypto.subtle.importKey(…)`'s `algorithm` argument — the ES
/// `{ name: "HMAC", hash: "SHA-256" }` object — lowered to the `(name, hash)`
/// string pair the runtime ctor takes. `hash` may be a string (`"SHA-256"`) or
/// `{ name: "SHA-256" }`; either lowers to its string value. `None` for a
/// non-object `algorithm` (the importKey arm falls through, surfacing honestly
/// at `cargo check`).
fn import_key_algorithm(arg: &Argument, ctx: &Ctx<'_>) -> Option<(Expr, Expr)> {
    let Expression::ObjectExpression(obj) = arg.as_expression()? else {
        return None;
    };
    let mut name = parse_quote!(::std::string::String::new());
    let mut hash = parse_quote!(::std::string::String::new());
    for kind in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(p) = kind else {
            continue;
        };
        let key = match &p.key {
            PropertyKey::StaticIdentifier(id) => id.name.as_str(),
            PropertyKey::StringLiteral(s) => s.value.as_str(),
            _ => continue,
        };
        match key {
            "name" => name = algo_string_value(&p.value, ctx),
            "hash" => hash = algo_string_value(&p.value, ctx),
            _ => {}
        }
    }
    Some((name, hash))
}

/// Extract a string-valued algorithm field. ES WebCrypto uses string literals
/// (`"HMAC"`, `"SHA-256"`), but the `hash` field may also be
/// `{ name: "SHA-256" }`; either form lowers to its string value.
fn algo_string_value(expr: &Expression, ctx: &Ctx<'_>) -> Expr {
    if let Expression::ObjectExpression(obj) = expr {
        for kind in &obj.properties {
            let ObjectPropertyKind::ObjectProperty(p) = kind else {
                continue;
            };
            let is_name = matches!(&p.key, PropertyKey::StaticIdentifier(id) if id.name.as_str() == "name")
                || matches!(
                    &p.key,
                    PropertyKey::StringLiteral(s) if s.value.as_str() == "name"
                );
            if is_name {
                return expr_to_string(&p.value, ctx);
            }
        }
    }
    expr_to_string(expr, ctx)
}

/// A `boolean` argument (the `extractable` flag of `importKey`). A `true`/
/// `false` literal lowers to the Rust literal; an absent or non-literal arg
/// defaults to `false` (the common WinterTC shape passes a literal).
fn bool_argument(arg: Option<&Argument>, _ctx: &Ctx<'_>) -> Expr {
    match arg.and_then(Argument::as_expression) {
        Some(Expression::BooleanLiteral(b)) => {
            if b.value {
                parse_quote!(true)
            } else {
                parse_quote!(false)
            }
        }
        _ => parse_quote!(false),
    }
}
