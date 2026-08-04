//! WebCrypto API — `crypto.randomUUID()` / `crypto.getRandomValues(buf)`
//! (Tier 1), `crypto.subtle.digest(algo, data)` (the one-shot hash), the HMAC
//! key-bearing subset `crypto.subtle.importKey/sign/verify`, and the AES-GCM
//! `crypto.subtle.encrypt/decrypt` (WinterTC Web API, W3C WebCrypto). The
//! receiver is the global `crypto` object; WinterTC's `self` is the global-object
//! alias (`self === globalThis`), so both `crypto.*` and `self.crypto.*` lower to
//! the same `__ds::crypto_*` helper. `randomUUID` returns an RFC 4122 v4 UUID
//! string; `getRandomValues` fills a `Uint8Array` byte buffer; `subtle.digest`
//! is the async hash backed by the RustCrypto `sha1`/`sha2` crates;
//! `subtle.importKey` (raw format) builds a `DsCryptoKey`, `subtle.sign`/
//! `.verify` are the async HMAC backed by `hmac`, and `subtle.encrypt`/`.decrypt`
//! are the async AES-GCM backed by `aes-gcm` (pure-Rust — never degraded),
//! `crypto.subtle.generateKey` is the fresh-key factory (random AES/HMAC keys),
//! and `crypto.subtle.deriveBits` is the PBKDF2 key-derivation path (a small
//! HMAC round-XOR loop, the same `hmac` backing as `sign`). The remaining
//! `SubtleCrypto` methods (AES-CBC, `deriveKey`/HKDF, `wrapKey`, `exportKey`)
//! are not yet mapped.

use oxc_ast::ast::{Argument, Expression, ObjectPropertyKind, PropertyKey, StaticMemberExpression};
use syn::{parse_quote, Expr};

use super::super::super::bindings::snake;
use super::super::super::context::Ctx;
use super::super::super::expressions::{translate_argument, translate_expr};
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
    // `crypto.subtle.generateKey(algorithm, extractable, usages)` — the factory
    // for a fresh `DsCryptoKey` (AES-GCM/AES-CBC/HMAC). The ES `algorithm`
    // object's `name`+`length` (AES) or `name`+`hash`+`length` (HMAC) are
    // extracted at translate time; the helper fills `length/8` cryptographically
    // random bytes. ES returns `Promise<CryptoKey>`; the call site's `await`
    // drives the future, and `callee_return_path` records the `DsCryptoKey` type
    // (mirroring `importKey`, so a later `sign`/`encrypt` passes the key through).
    if sm.property.name.as_str() == "generateKey" {
        if let Expression::StaticMemberExpression(inner) = &sm.object {
            if inner.property.name.as_str() == "subtle"
                && is_crypto_receiver(&inner.object).is_some()
            {
                let (algorithm, hash, length) = generate_key_algorithm(args.first()?, ctx)?;
                let extractable = bool_argument(args.get(1), ctx);
                let usages: Expr = parse_quote!(::std::vec![]);
                return Some(parse_quote!(
                    crate::__ds::crypto_subtle_generate_key(
                        #algorithm, #hash, #length, #extractable, #usages
                    )
                ));
            }
        }
    }
    // `crypto.subtle.deriveBits(algo, key, length)` — the PBKDF2 subset. The ES
    // `algo` object's `name`/`salt`/`iterations`/`hash` are extracted at translate
    // time; `key` is the password `DsCryptoKey` (imported raw); `length` is the
    // output length in bits. ES returns `Promise<ArrayBuffer>`; the call site's
    // `await` drives the future, and `callee_return_path` records the `Vec<u8>`
    // return.
    if sm.property.name.as_str() == "deriveBits" {
        if let Expression::StaticMemberExpression(inner) = &sm.object {
            if inner.property.name.as_str() == "subtle"
                && is_crypto_receiver(&inner.object).is_some()
            {
                let (algorithm, hash, salt, iterations) =
                    derive_bits_algorithm(args.first()?, ctx)?;
                let key = translate_argument(args.get(1)?, ctx);
                let length = length_value(args.get(2)?.as_expression()?);
                return Some(parse_quote!(
                    crate::__ds::crypto_subtle_derive_bits(
                        #algorithm, #hash, #salt, #iterations, &#key, #length
                    )
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
    // `crypto.subtle.encrypt(algo, key, data)` — the AES-GCM subset. The ES
    // `algo` object's `name` (`"AES-GCM"`) and `iv` (the nonce, a `Uint8Array`)
    // are extracted at translate time; `data` is a `BufferSource` (reuses the
    // digest coercion). ES returns `Promise<ArrayBuffer>`; the call site's
    // `await` drives the future, and `callee_return_path` records the `Vec<u8>`
    // return so a later `decrypt`/`assert_equals` passes the ciphertext through.
    if sm.property.name.as_str() == "encrypt" {
        if let Expression::StaticMemberExpression(inner) = &sm.object {
            if inner.property.name.as_str() == "subtle"
                && is_crypto_receiver(&inner.object).is_some()
            {
                let (algorithm, iv) = encrypt_algorithm(args.first()?, ctx)?;
                let key = translate_argument(args.get(1)?, ctx);
                let data = digest_data_arg(args.get(2)?, ctx);
                return Some(parse_quote!(
                    crate::__ds::crypto_subtle_encrypt(#algorithm, &#iv, &#key, #data)
                ));
            }
        }
    }
    // `crypto.subtle.decrypt(algo, key, data)` — the AES-GCM subset (the inverse
    // of `encrypt`): `data` is `ciphertext || tag`. ES returns
    // `Promise<ArrayBuffer>`.
    if sm.property.name.as_str() == "decrypt" {
        if let Expression::StaticMemberExpression(inner) = &sm.object {
            if inner.property.name.as_str() == "subtle"
                && is_crypto_receiver(&inner.object).is_some()
            {
                let (algorithm, iv) = encrypt_algorithm(args.first()?, ctx)?;
                let key = translate_argument(args.get(1)?, ctx);
                let data = digest_data_arg(args.get(2)?, ctx);
                return Some(parse_quote!(
                    crate::__ds::crypto_subtle_decrypt(#algorithm, &#iv, &#key, #data)
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
    match arg.as_expression() {
        Some(e) => digest_data_expr(e, ctx),
        None => translate_argument(arg, ctx),
    }
}

/// The `&Expression` form of [`digest_data_arg`] — the per-shape `BufferSource`
/// → `Vec<u8>` coercion, factored so `encrypt`/`decrypt`'s `iv` field (a
/// `Uint8Array` nonce) reuses it. See [`digest_data_arg`] for the per-shape
/// lowering.
fn digest_data_expr(e: &Expression, ctx: &Ctx<'_>) -> Expr {
    match e {
        Expression::ArrayExpression(arr) => collect_blob_parts(arr, ctx),
        Expression::NewExpression(_) => translate_expr(e, ctx),
        Expression::Identifier(id) => {
            let name = snake(&id.name).to_string();
            let is_vec = ctx
                .local_type(&name)
                .is_some_and(|p| p.segments.last().is_some_and(|s| s.ident == "Vec"));
            if is_vec {
                translate_expr(e, ctx)
            } else {
                blob_part_to_bytes(e, ctx)
            }
        }
        _ => blob_part_to_bytes(e, ctx),
    }
}

/// `crypto.subtle.encrypt(algo, key, data)` / `.decrypt(…)`'s `algorithm`
/// argument — the ES `{ name: "AES-GCM", iv: <Uint8Array> }` object — lowered to
/// the `(name, iv)` pair the runtime helpers take. `iv` is a `BufferSource`
/// coerced via the same path as `digest`'s `data`; an absent `iv` is an empty
/// vector (ES requires it, so a missing `iv` surfaces honestly at runtime).
/// `None` for a non-object `algorithm` (the arm falls through).
fn encrypt_algorithm(arg: &Argument, ctx: &Ctx<'_>) -> Option<(Expr, Expr)> {
    let Expression::ObjectExpression(obj) = arg.as_expression()? else {
        return None;
    };
    let mut name = parse_quote!(::std::string::String::new());
    let mut iv: Option<Expr> = None;
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
            "iv" => iv = Some(digest_data_expr(&p.value, ctx)),
            _ => {}
        }
    }
    let iv = iv.unwrap_or_else(|| parse_quote!(::std::vec![]));
    Some((name, iv))
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

/// `crypto.subtle.generateKey(…)`'s `algorithm` argument — the ES
/// `{ name: "AES-GCM", length: 256 }` (AES) or
/// `{ name: "HMAC", hash: "SHA-256", length: 256 }` (HMAC) object — lowered to
/// the `(name, hash, length)` triple the runtime ctor takes. `name`/`hash` reuse
/// the string extraction (the `hash` field may be a string or `{ name: "SHA-256"
/// }`); `length` is a numeric literal (128/192/256) read as a `usize` (absent →
/// `0`, the helper's "use the hash-block default" sentinel for HMAC). `None` for
/// a non-object `algorithm` (the arm falls through).
fn generate_key_algorithm(arg: &Argument, ctx: &Ctx<'_>) -> Option<(Expr, Expr, Expr)> {
    let Expression::ObjectExpression(obj) = arg.as_expression()? else {
        return None;
    };
    let mut name = parse_quote!(::std::string::String::new());
    let mut hash = parse_quote!(::std::string::String::new());
    let mut length: Option<Expr> = None;
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
            "length" => length = Some(length_value(&p.value)),
            _ => {}
        }
    }
    let length = length.unwrap_or_else(|| parse_quote!(0usize));
    Some((name, hash, length))
}

/// Read a `length` algorithm field (the AES/HMAC key length in bits) as a `usize`
/// expression. A numeric literal (`256`) lowers to a `usize` literal matching the
/// helper's `length` param; any other shape lowers to `0` (the default sentinel).
fn length_value(expr: &Expression) -> Expr {
    if let Expression::NumericLiteral(n) = expr {
        let v = n.value as usize;
        return parse_quote!(#v);
    }
    parse_quote!(0usize)
}

/// `crypto.subtle.deriveBits(…)`'s `algorithm` argument — the ES
/// `{ name: "PBKDF2", salt: <Uint8Array>, iterations: 100000, hash: "SHA-256" }`
/// object — lowered to the `(name, hash, salt, iterations)` quadruple the
/// runtime helper takes. `salt` is a `BufferSource` coerced via the digest path;
/// `iterations` is a numeric literal read as a `u32`. `None` for a non-object
/// `algorithm` (the arm falls through).
fn derive_bits_algorithm(arg: &Argument, ctx: &Ctx<'_>) -> Option<(Expr, Expr, Expr, Expr)> {
    let Expression::ObjectExpression(obj) = arg.as_expression()? else {
        return None;
    };
    let mut name = parse_quote!(::std::string::String::new());
    let mut hash = parse_quote!(::std::string::String::new());
    let mut salt: Option<Expr> = None;
    let mut iterations: Option<Expr> = None;
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
            "salt" => salt = Some(digest_data_expr(&p.value, ctx)),
            "iterations" => iterations = Some(iterations_value(&p.value)),
            _ => {}
        }
    }
    let salt = salt.unwrap_or_else(|| parse_quote!(::std::vec![]));
    let iterations = iterations.unwrap_or_else(|| parse_quote!(0u32));
    Some((name, hash, salt, iterations))
}

/// Read an `iterations` algorithm field (the PBKDF2 round count) as a `u32`
/// expression. A numeric literal lowers to a `u32` literal; any other shape
/// lowers to `0`.
fn iterations_value(expr: &Expression) -> Expr {
    if let Expression::NumericLiteral(n) = expr {
        let v = n.value as u32;
        return parse_quote!(#v);
    }
    parse_quote!(0u32)
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
