//! WebCrypto API — `crypto.randomUUID()` / `crypto.getRandomValues(buf)`
//! (WinterTC Web API, W3C WebCrypto). `randomUUID` returns an RFC 4122 v4 UUID
//! string; `getRandomValues(buf)` fills a `Uint8Array` byte buffer with
//! cryptographically-strong random bytes. The receiver is the global `crypto`
//! object; WinterTC's `self` is the global-object alias (`self === globalThis`),
//! so both `crypto.randomUUID()` and `self.crypto.randomUUID()` lower to the
//! same `__ds::crypto_*` helper (injected by the `Crypto` runtime dep —
//! `uuid::Uuid::new_v4` for the UUID, `getrandom` for the bytes, pure-Rust).
//! `subtle` (SubtleCrypto) is async, not Tier 1.

use oxc_ast::ast::{Argument, Expression, StaticMemberExpression};
use syn::{parse_quote, Expr};

use super::super::super::context::Ctx;
use super::super::super::expressions::translate_argument;

/// `crypto.randomUUID()` / `crypto.getRandomValues(buf)` → the matching
/// `__ds::crypto_*` helper. `None` unless the callee is the global `crypto`
/// object (the bare identifier, or via the WinterTC `self` global-object alias)
/// and the method is one of the mapped WinterTC Tier-1 forms; any other
/// receiver/name falls through to a plain method call (cargo check rejects it
/// honestly). `subtle` (SubtleCrypto) is not Tier 1 — async, unmapped.
pub(in crate::translator) fn crypto_method(
    sm: &StaticMemberExpression,
    args: &[Argument],
    ctx: &Ctx<'_>,
) -> Option<Expr> {
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
