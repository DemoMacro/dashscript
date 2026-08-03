//! WebCrypto API — `crypto.randomUUID()` (a WinterTC Web API, W3C WebCrypto).
//! Returns an RFC 4122 version-4 UUID string. The receiver is the global
//! `crypto` object; WinterTC's `self` is the global-object alias
//! (`self === globalThis`), so both `crypto.randomUUID()` and
//! `self.crypto.randomUUID()` lower to the same `__ds::crypto_random_uuid()`
//! helper (injected by the `Crypto` runtime dep — `uuid::Uuid::new_v4`,
//! pure-Rust). Only `randomUUID` is mapped today; `getRandomValues`/`subtle`
//! are not Tier 1.

use oxc_ast::ast::{Expression, StaticMemberExpression};
use syn::{parse_quote, Expr};

/// `crypto.randomUUID()` → `__ds::crypto_random_uuid()`. Returns `None` unless
/// the callee is `<crypto>.randomUUID` (the bare global, or via the WinterTC
/// `self` global-object alias), so any other receiver/name falls through to a
/// plain method call (cargo check rejects it honestly).
pub(in crate::translator) fn crypto_method(sm: &StaticMemberExpression) -> Option<Expr> {
    if sm.property.name.as_str() != "randomUUID" {
        return None;
    }
    is_crypto_receiver(&sm.object)?;
    Some(parse_quote!(crate::__ds::crypto_random_uuid()))
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
