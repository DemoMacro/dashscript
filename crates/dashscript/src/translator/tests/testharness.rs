//! WPT testharness builtin mappings — `test()`/`assert_equals`/…. See
//! `builtins/harness/testharness.rs`. These verify the static lowering
//! (`__ds::wpt_*`) end-to-end through `Translator::translate`/`check`, the way
//! `tests/console.rs` verifies `console.*`. The mapped set lowers statically;
//! composite/async forms with no static lowering fall back to the engine.

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
    // `async_test` has no static lowering (needs tokio) — check flags it so the
    // fixture falls back to the engine.
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

#[test]
fn assert_approx_equals_lowers_to_wpt_helper() {
    // `assert_approx_equals(actual, expected, epsilon)` →
    // `wpt_assert_approx_equals((a) as f64, (b) as f64, (eps) as f64)` — pass
    // iff `|actual - expected| <= epsilon`. Each operand casts to `f64` so an
    // `i64`-flavor local type-checks.
    let src =
        "function f(a: number, b: number, eps: number): void { assert_approx_equals(a, b, eps); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("wpt_assert_approx_equals"),
        "assert_approx_equals should lower to wpt_assert_approx_equals: {rust}"
    );
    assert!(
        rust.contains(" as f64"),
        "operands should cast to f64: {rust}"
    );
}

#[test]
fn check_passes_assert_approx_equals() {
    // `assert_approx_equals` is Mapped (numeric approximation) — check produces
    // no diagnostics.
    let diags = Translator::new().check(
        "function f(a: number, b: number, eps: number): void { assert_approx_equals(a, b, eps); }",
    );
    assert!(diags.is_empty(), "assert_approx_equals flagged: {diags:?}");
}

#[test]
fn promise_test_lowers_to_async_await() {
    // `promise_test(async () => { assert_equals(1, 1); }, "n")` — the async
    // callback's body lowers to `async move { … }`, awaited via
    // `wpt_promise_test`. The top-level `.await` makes the entry's `main` async
    // under `#[tokio::main]` (Stage 1 wired async main; Stage 2 wires the
    // promise_test lowering). The callback's name arg is dropped (the verdict
    // keys off the `AssertionError:` prefix).
    let src = "promise_test(async () => { assert_equals(1, 1); }, \"basic\");";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("wpt_promise_test"),
        "promise_test should lower to wpt_promise_test: {rust}"
    );
    assert!(
        rust.contains("async move"),
        "promise_test callback body should wrap in async move: {rust}"
    );
    assert!(
        rust.contains(".await"),
        "promise_test should .await the future: {rust}"
    );
    assert!(
        rust.contains("#[tokio::main"),
        "top-level await should make main async under tokio: {rust}"
    );
    assert!(
        rust.contains("async fn main"),
        "main should be async: {rust}"
    );
    assert!(
        !rust.contains("todo!"),
        "no todo! in promise_test lowering: {rust}"
    );
}

#[test]
fn check_passes_promise_test() {
    // `promise_test` is now Mapped (Stage 2 moved it from the async-rejected
    // set, now that tokio ships on the static path) — check produces no
    // diagnostics.
    let diags =
        Translator::new().check("promise_test(async () => { assert_equals(1, 1); }, \"x\");");
    assert!(diags.is_empty(), "promise_test flagged: {diags:?}");
}

#[test]
fn promise_test_named_callback_lowers() {
    // `promise_test(namedFn, name)` — a reference to an async function
    // declaration. The fn item is called `()` to yield its `Future` (a Rust
    // async fn item is `fn() -> impl Future`); the top-level `.await` makes
    // the entry's `main` async under `#[tokio::main]`. The inline-async form
    // (slice 2a) covers `promise_test(async () => …)`; this covers the named
    // reference form (slice 2c).
    let src = "async function runTest(): Promise<void> { console.log(\"x\"); }\npromise_test(runTest, \"named\");";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("wpt_promise_test"),
        "named callback should lower to wpt_promise_test: {rust}"
    );
    assert!(
        rust.contains("(run_test)()"),
        "named callback should call the fn item: {rust}"
    );
    assert!(rust.contains(".await"), "should await: {rust}");
    assert!(
        rust.contains("async fn main"),
        "main should be async: {rust}"
    );
    assert!(!rust.contains("todo!"), "no todo!: {rust}");
}

#[test]
fn promise_test_non_async_function_callback_lowers() {
    // `promise_test(function () { return promise }, name)` — a NON-async
    // callback returning a promise (a common WPT idiom: WPT awaits the
    // returned promise). The callback lowers to a closure, called `()` to
    // yield its return value, and `.await`ed inside `async move { … }` so the
    // result is `Output = ()` (matching `wpt_promise_test`). Covers the 167
    // fetch/webcryptoapi/fileapi fixtures that use this shape.
    let src = "promise_test(function() { return Promise.resolve(1); }, \"nonasync\");";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("wpt_promise_test"),
        "non-async callback should lower to wpt_promise_test: {rust}"
    );
    assert!(
        rust.contains("async move") && rust.contains(".await"),
        "non-async callback should wrap in async move {{ f().await }}: {rust}"
    );
    assert!(
        rust.contains("ds_promise_resolve"),
        "callback body (Promise.resolve) lowers: {rust}"
    );
    assert!(
        rust.contains("async fn main"),
        "main should be async: {rust}"
    );
}

#[test]
fn check_passes_promise_test_non_async_function_callback() {
    // A non-async function callback is Mapped (the emit wraps it) — check
    // produces no diagnostics.
    let src = "promise_test(function() { return Promise.resolve(1); }, \"n\");";
    let diags = Translator::new().check(src);
    assert!(
        diags.is_empty(),
        "non-async promise_test flagged: {diags:?}"
    );
}

#[test]
fn check_passes_promise_test_named_callback() {
    // A named async-function reference is Mapped (slice 2c) — check produces
    // no diagnostics.
    let src = "async function runTest(): Promise<void> { console.log(\"x\"); }\npromise_test(runTest, \"n\");";
    let diags = Translator::new().check(src);
    assert!(diags.is_empty(), "named promise_test flagged: {diags:?}");
}
