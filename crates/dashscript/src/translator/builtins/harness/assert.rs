//! The test262 `assert` harness. `assert.sameValue`/`notSameValue` lower to a
//! Rust SameValue check (`__ds::assert_same_value`) that panics a
//! `Test262Error` on mismatch — the conformance harness then reads the
//! `Test262Error:` prefix to mark the fixture `partial`. `assert.throws` lowers
//! to `__ds::assert_throws` (a `catch_unwind` + error-class check). `assert` is
//! a host object injected by the test262 conformance harness (not an ES
//! built-in), so this file lives alongside the ES built-ins for dispatch
//! symmetry.
//!
//! Only the scalar forms lower statically; the reflection helpers
//! (`compareArray`/`verifyProperty`/…) have no static Rust form, so `classify`
//! routes them to the engine, where the test262 harness (`assert.js`/
//! `propertyHelper.js`) runs natively under QuickJS.

use oxc_ast::ast::{Argument, Expression};
use proc_macro2::Span;
use syn::{parse_quote, Expr};

use super::super::super::context::Ctx;
use super::super::super::expressions::translate_argument;

/// `assert.sameValue(a, b)` / `assert.notSameValue(a, b)` → a `__ds::assert_*`
/// call (SameValue check; panics `Test262Error` on failure); `assert.throws`
/// → `__ds::assert_throws` (see [`assert_throws_expr`]). Returns `None` for any
/// other member — `classify` routes `compareArray`/… to the engine before
/// dispatch reaches here, so an unmapped name surfaces honestly.
pub(in crate::translator) fn assert_method(
    name: &str,
    args: &[Argument],
    ctx: &Ctx<'_>,
) -> Option<Expr> {
    // `assert.throws(Ctor, () => …)` takes no sameValue operands — dispatch it
    // before the a/b extraction below. `classify` has already guaranteed a
    // zero-param arrow callback and an Identifier constructor.
    if name == "throws" {
        return assert_throws_expr(args, ctx);
    }
    let a = super::assert_operand(args.first()?, ctx);
    let b = super::assert_operand(args.get(1)?, ctx);
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

/// `assert.throws(Ctor, () => body)` → `__ds::assert_throws("Ctor", || body)`.
/// `Ctor` is the expected ES error class — an Identifier (`RangeError`/…) —
/// emitted as its name literal; the zero-param arrow lowers to a `FnOnce() ->
/// R` closure (arrow params are dropped by `arrow_expr`). The helper
/// catch_unwinds the closure and matches the panic's `DsError` class against
/// the literal. Returns `None` if arg0 is not an Identifier (degraded by
/// `classify` before dispatch, so this is a defensive fallback).
fn assert_throws_expr(args: &[Argument], ctx: &Ctx<'_>) -> Option<Expr> {
    let ctor = match args.first()?.as_expression()? {
        Expression::Identifier(id) => syn::LitStr::new(id.name.as_str(), Span::call_site()),
        _ => return None,
    };
    let f = translate_argument(args.get(1)?, ctx);
    Some(parse_quote!(crate::__ds::assert_throws(#ctor, #f)))
}

/// Bare `assert(mustBeTrue[, message])` → `__ds::assert_same_value(&cond, &true)`.
/// test262's `assert(mustBeTrue)` passes iff `mustBeTrue === true` (strict, per
/// assert.js), so it is exactly `assert.sameValue(mustBeTrue, true)`. The optional
/// `message` is dropped — the conformance verdict keys off the `Test262Error:`
/// prefix only, never the text. Keeping this off the engine matters where the
/// engine cannot parse the source's ES2025 regex (`(?s:…)`, dup-names): regress
/// parses them, QuickJS-NG does not. Returns `None` only if there is no
/// condition operand (a malformed `assert()`).
pub(in crate::translator) fn assert_call(args: &[Argument], ctx: &Ctx<'_>) -> Option<Expr> {
    let cond = translate_argument(args.first()?, ctx);
    Some(parse_quote!(crate::__ds::assert_same_value(&(#cond), &true)))
}
