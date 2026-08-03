//! WPT testharness builtin mappings — `test()`/`assert_equals`/…. See
//! `builtins/harness/testharness.rs`. These verify the static lowering
//! (`__ds::wpt_*`) end-to-end through `Translator::translate`/`check`, the way
//! `tests/console.rs` verifies `console.*`. WinterTC is pure-Rust, so these must
//! lower statically — never to the engine.

use super::super::Translator;

#[test]
fn test_call_with_arrow_body_lowers_to_wpt_assert() {
    // `test(() => { assert_equals(1, 1); }, "trivial")` — the arrow body runs
    // immediately (the `test` arm emits `(fn)()`), and `assert_equals` lowers
    // to `__ds::wpt_assert_equals`. The `test(...)` is a top-level executable
    // statement → implicit `fn main`.
    let src = "test(() => { assert_equals(1, 1); }, \"trivial\");";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("wpt_assert_equals"),
        "test(arrow) should lower assert_equals to wpt_assert_equals: {rust}"
    );
    assert!(
        !rust.contains("todo!"),
        "no todo! in testharness lowering: {rust}"
    );
}

#[test]
fn assert_true_lowers_to_wpt_equals_true() {
    // `assert_true(b)` → `wpt_assert_equals(&b, &true)` — strict boolean
    // SameValue (WPT requires `actual === true`).
    let src = "function f(b: boolean): void { assert_true(b); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("wpt_assert_equals"), "got: {rust}");
    assert!(
        rust.contains("&true"),
        "assert_true should compare against &true: {rust}"
    );
}

#[test]
fn assert_unreached_lowers_to_wpt_panic() {
    let src = "function f(): void { assert_unreached(); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("wpt_assert_unreached"),
        "assert_unreached should lower to wpt_assert_unreached: {rust}"
    );
}

#[test]
fn assert_throws_dom_lowers_to_wpt_assert_throws() {
    let src = "function f(): void { assert_throws_dom(\"NetworkError\", () => { throw 1; }); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("wpt_assert_throws"),
        "assert_throws_dom should lower to wpt_assert_throws: {rust}"
    );
}

#[test]
fn check_passes_test_call_and_assert_equals() {
    // `test`/`assert_equals` are Mapped — check produces no diagnostics.
    let diags = Translator::new().check("test(() => { assert_equals(1, 1); }, \"x\");");
    assert!(diags.is_empty(), "test/assert_equals flagged: {diags:?}");
}

#[test]
fn check_rejects_async_test() {
    // `async_test` has no static lowering (needs tokio) and no engine fallback
    // (WinterTC is static-only) — check flags it `unsupported`.
    let diags = Translator::new().check("async_test(() => {});");
    assert!(
        diags.iter().any(|d| d.message.contains("async_test")),
        "async_test should be flagged unsupported: {diags:?}"
    );
}

#[test]
fn assert_array_equals_lowers_to_wpt_helper() {
    // `assert_array_equals(actual, expected)` → `wpt_assert_array_equals(&a,
    // &b)` — length + per-element SameValue. The operands deref from
    // `&Vec<T>` (`a: number[]` → `Vec<f64>`).
    let src = "function f(a: number[], b: number[]): void { assert_array_equals(a, b); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("wpt_assert_array_equals"),
        "assert_array_equals should lower to wpt_assert_array_equals: {rust}"
    );
}

#[test]
fn check_passes_assert_array_equals() {
    // `assert_array_equals` is now Mapped (moved from the composite-rejected
    // set) — check produces no diagnostics.
    let diags = Translator::new()
        .check("function f(a: number[], b: number[]): void { assert_array_equals(a, b); }");
    assert!(diags.is_empty(), "assert_array_equals flagged: {diags:?}");
}
