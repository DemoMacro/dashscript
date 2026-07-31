//! The test262 `assert` harness. `assert.sameValue`/`notSameValue` lower to a
//! Rust SameValue check (`__ds::assert_same_value`) that panics a
//! `Test262Error` on mismatch — the conformance harness then reads the
//! `Test262Error:` prefix to mark the fixture `partial`. `assert` is a host
//! object injected by the test262 conformance harness (not an ES built-in),
//! so this file lives alongside the ES built-ins for dispatch symmetry.
//!
//! Only the scalar forms lower statically. `assert.throws` and the reflection
//! helpers (`compareArray`/`verifyProperty`/…) have no static Rust form, so
//! `classify` routes them to the engine, where the test262 harness
//! (`assert.js`/`propertyHelper.js`) runs natively under QuickJS.

use oxc_ast::ast::Argument;
use syn::{parse_quote, Expr};

use super::super::context::Ctx;
use super::super::expressions::translate_argument;

/// `assert.sameValue(a, b)` / `assert.notSameValue(a, b)` → a `__ds::assert_*`
/// call (SameValue check; panics `Test262Error` on failure). Returns `None`
/// for any other member — `classify` routes `throws`/`compareArray`/… to the
/// engine before dispatch reaches here, so an unmapped name surfaces honestly.
pub(in crate::translator) fn assert_method(
    name: &str,
    args: &[Argument],
    ctx: &Ctx<'_>,
) -> Option<Expr> {
    let a = translate_argument(args.first()?, ctx);
    let b = translate_argument(args.get(1)?, ctx);
    Some(match name {
        // SameValue (Object.is): `===` plus distinct +0/-0 and NaN===NaN —
        // see `ASSERT_HELPER`. Both operands lower to the same Rust type (TS
        // `assert.sameValue` is same-typed), so the generic helper picks the
        // matching `DsSameValue` impl by inference.
        "sameValue" => parse_quote!(crate::__ds::assert_same_value(&(#a), &(#b))),
        "notSameValue" => parse_quote!(crate::__ds::assert_not_same_value(&(#a), &(#b))),
        _ => return None,
    })
}
