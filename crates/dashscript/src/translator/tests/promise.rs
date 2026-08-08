//! `Promise.resolve(x)` / `Promise.all([...])` static combinators — the only
//! `Promise` forms with a native Rust lowering (T3 stage 2a). A `Promise<T>`
//! lowers to a boxed single-threaded `DsPromise<T> = Pin<Box<dyn Future<Output
//! = T>>>`; every other `Promise` form (bare value, `new Promise`, `.then`,
//! `.race`, `.allSettled`, …) degrades to the engine.
use super::super::{RuntimeDep, Translator};

#[test]
fn promise_resolve_emits_ds_promise_resolve_and_flags_dep() {
    // `Promise.resolve(x)` → `__ds::ds_promise_resolve(x)`; the
    // `__ds::ds_promise_` marker flags the `Promise` dep, which ships the
    // `DsPromise<T>` type alias + `ds_promise_resolve`/`ds_promise_all` helpers
    // in `__ds.rs` (the `futures` crate — `future::ready`/`future::join_all`).
    let src = "function f(): void { const p = Promise.resolve(42); }";
    let (rust, deps) = Translator::new()
        .translate_with_deps(src)
        .expect("translate_with_deps");
    assert!(
        rust.contains("crate::__ds::ds_promise_resolve"),
        "Promise.resolve → __ds::ds_promise_resolve, got:\n{rust}"
    );
    assert!(
        deps.has(RuntimeDep::Promise),
        "Promise dep must flag, got deps: {deps:?}"
    );
    let helper = deps.helper_module().expect("Promise dep ships a helper");
    assert!(
        helper.contains("type DsPromise") && helper.contains("fn ds_promise_resolve"),
        "Promise dep ships DsPromise + ds_promise_resolve, got helper: {helper:?}"
    );
    // The `futures` crate is appended to Cargo.toml.
    let mut toml = String::from("[dependencies]\n");
    deps.apply_to_cargo_toml(&mut toml);
    assert!(
        toml.contains("futures"),
        "futures crate in Cargo.toml: {toml}"
    );
}

#[test]
fn promise_all_emits_ds_promise_all_vec() {
    // `Promise.all([p1, p2])` → `__ds::ds_promise_all(vec![e1, e2])`; each
    // element lowers to a `DsPromise<T>` (the call site wraps; ES coerces each
    // via `Promise.resolve`). An empty array fulfills with `[]`.
    let src = "function f(): void {\n  const p = Promise.all([Promise.resolve(1), Promise.resolve(2)]);\n}";
    let (rust, deps) = Translator::new()
        .translate_with_deps(src)
        .expect("translate_with_deps");
    assert!(
        rust.contains("crate::__ds::ds_promise_all") && rust.contains("vec!["),
        "Promise.all → __ds::ds_promise_all(vec![…]), got:\n{rust}"
    );
    assert!(
        deps.has(RuntimeDep::Promise),
        "Promise dep must flag, got deps: {deps:?}"
    );
    let helper = deps.helper_module().expect("Promise dep ships a helper");
    assert!(
        helper.contains("fn ds_promise_all") && helper.contains("join_all"),
        "Promise dep ships ds_promise_all (join_all), got helper: {helper:?}"
    );
}

#[test]
fn promise_other_static_methods_stay_degraded() {
    // `Promise.race`/`Promise.allSettled` have no static lowering — `classify`
    // pulls only `resolve`/`all` out of the engine degrade, so the rest stay on
    // the engine path (a bare `Promise`/`new Promise` degrades the same way).
    // Guards against the resolve/all specialization accidentally widening.
    for src in [
        "function f(): void { const p = Promise.race([Promise.resolve(1)]); }",
        "function f(): void { const p = Promise.allSettled([Promise.resolve(1)]); }",
        "function f(): void { const C = Promise; }",
    ] {
        let (_rust, deps) = Translator::new()
            .translate_with_deps(src)
            .unwrap_or_else(|_| panic!("translate src: {src}"));
        assert!(
            deps.needs_engine(),
            "Promise.race/allSettled/bare should stay degraded, src: {src}, deps: {deps:?}"
        );
    }
}

#[test]
fn promise_then_on_resolve_call_lowers_to_ds_promise_then() {
    // `Promise.resolve(x).then(cb)` → `ds_promise_then(ds_promise_resolve(x), cb)`.
    // The receiver is a `Promise.resolve(..)` call (a DsPromise value), so
    // `.then` lowers to the combinator rather than degrading to the engine.
    let src = "function f(): void { const p = Promise.resolve(42).then((x) => x + 1); }";
    let (rust, deps) = Translator::new()
        .translate_with_deps(src)
        .expect("translate_with_deps");
    assert!(
        rust.contains("crate::__ds::ds_promise_then")
            && rust.contains("crate::__ds::ds_promise_resolve"),
        "Promise.resolve(..).then(cb) → ds_promise_then(ds_promise_resolve(..), cb), got:\n{rust}"
    );
    assert!(
        deps.has(RuntimeDep::Promise),
        "Promise dep must flag, got deps: {deps:?}"
    );
    let helper = deps.helper_module().expect("Promise dep ships a helper");
    assert!(
        helper.contains("fn ds_promise_then"),
        "Promise dep ships ds_promise_then, got helper: {helper:?}"
    );
}

#[test]
fn promise_then_on_promise_local_lowers_to_ds_promise_then() {
    // `const p = Promise.resolve(x); p.then(cb)` — the local `p` has resolved
    // type `DsPromise`, so `.then` dispatches on the receiver type rather than
    // falling through to a phantom method binding (E0425).
    let src = "function f(): void {\n  const p = Promise.resolve(42);\n  const q = p.then((x) => x + 1);\n}";
    let (rust, _deps) = Translator::new()
        .translate_with_deps(src)
        .expect("translate_with_deps");
    assert!(
        rust.contains("crate::__ds::ds_promise_then"),
        "p.then(cb) on a DsPromise local → ds_promise_then, got:\n{rust}"
    );
}

#[test]
fn async_fn_degrade_stub_drops_promise_return() {
    // An `async fn` degraded to QuickJS returns a JS `Promise`, which cannot
    // marshal across the serde boundary to Rust's `DsPromise<T>` (a
    // `Pin<Box<dyn Future>>` — not `DeserializeOwned`). The degraded stub is a
    // sync `fn` calling `__ds::engine::call_fn`; it drops the return (the JS
    // Promise resolves inside QuickJS's event loop). Guards against the stub
    // emitting `-> DsPromise<T>` + `from_value::<DsPromise<T>>` (E0277), which
    // the conformance WPT layer hit on every `promise_test` fixture rewrapped
    // to `async function main(): Promise<void>`.
    // A bare `Promise` value forces the engine path for `main`.
    let src = "async function main(): Promise<void> { const C = Promise; }\nmain();\n";
    let (rust, deps) = Translator::new()
        .translate_with_deps(src)
        .expect("translate_with_deps");
    assert!(
        deps.needs_engine(),
        "bare Promise forces the engine path, got deps: {deps:?}"
    );
    assert!(
        !rust.contains("from_value::<crate::__ds::DsPromise"),
        "async degrade stub must not deserialize a DsPromise (E0277), got:\n{rust}"
    );
    assert!(
        !rust.contains("-> crate::__ds::DsPromise"),
        "async degrade stub drops the Promise return, got:\n{rust}"
    );
    assert!(
        rust.contains("async fn __ds_main"),
        "async degrade stub keeps `async fn` so the injected `.await` resolves, got:\n{rust}"
    );
}
