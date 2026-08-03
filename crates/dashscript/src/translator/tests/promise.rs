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
