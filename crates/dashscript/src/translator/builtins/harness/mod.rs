//! Conformance test-harness mappings — the test262 `assert.sameValue`/
//! `assert.throws` API ([`assert`]) and the WPT (web-platform-tests)
//! `test()`/`assert_equals()` API ([`testharness`]). These are *not* ES
//! built-ins, Web APIs, or Node modules — they are the host-defined test
//! harness each conformance suite injects, and the only reason DashScript maps
//! them is so conformance fixtures lower on the static path (the verdict is
//! assert-driven: a failure panics `Test262Error`/`AssertionError`, the
//! conformance runner reads the prefix). Both lower to `__ds::assert_*` /
//! `__ds::wpt_*` Rust helpers and share the `ASSERT_HELPER` slice.
//!
//! This is the fourth, orthogonal layer of `builtins/` — ES built-in / Web API
//! / Node module / **test harness**. Deno's `ext/` has no analogue because Deno
//! does not run test262/WPT fixtures; the harness layer exists here only to
//! keep the conformance oracle on the static path (WinterTC is pure-Rust, no
//! degradation — so the WPT harness must lower statically, not fall back to the
//! engine the way a test262 helper sometimes does).

mod assert;
mod testharness;

use oxc_ast::ast::Argument;
use syn::{parse_quote, Expr};

use super::super::context::Ctx;
use super::super::expressions::translate_argument;

pub(in crate::translator) use assert::{assert_call, assert_method};
pub(in crate::translator) use testharness::testharness_function;

/// Translate an `assert.sameValue` / WPT `assert_equals` operand, mapping a
/// `null` (or the `undefined` global) to a concrete `Option::<()>::None` so the
/// two-param `…_assert_equals<A, B>` helper can infer `B`. A bare `None` leaves
/// `B = Option<_>` — `A` and `B` are independent type params, so the `&None`
/// operand carries no type information (E0282). `Option<()>: DsSameValue`
/// projects to `DsCmp::Undefined`, matching any `Option`'s own `None`, so
/// SameValue against an absent value holds the way ES `assert_equals(x, null)`
/// does when `x` is null — `params.get("missing")` returns `Option::None`,
/// which compares `Undefined`-to-`Undefined` against this `null` operand.
/// `null` and `undefined` both project to `Undefined` (DashScript's unified
/// nullable model), so an explicit `null` compares against an `Option`'s `None`
/// regardless of which JS nullish the harness wrote.
pub(in crate::translator) fn assert_operand(arg: &Argument, ctx: &Ctx<'_>) -> Expr {
    match arg {
        Argument::NullLiteral(_) => parse_quote!(::core::option::Option::<()>::None),
        Argument::Identifier(id) if id.name.as_str() == "undefined" => {
            parse_quote!(::core::option::Option::<()>::None)
        }
        _ => translate_argument(arg, ctx),
    }
}
