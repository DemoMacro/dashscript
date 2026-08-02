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

pub(in crate::translator) use assert::{assert_call, assert_method};
pub(in crate::translator) use testharness::testharness_function;
