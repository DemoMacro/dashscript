//! Transpile-first probe: an untyped `.js` ESM module lowers to Rust the same
//! way a `.ts` module does. oxc parses `.js` as TypeScript (a superset), and the
//! translator already handles untyped params (default `f64`) and literal type
//! inference — so a pure-JS source that is a TS subset translates without an
//! engine. This is the basis for batch C: an npm `.js` package is transpiled
//! first, and the engine is only a fallback for dynamic JS the table cannot
//! lower (typeof / prototype / eval).

use super::super::{FileRole, Translator};

#[test]
fn untyped_js_number_function_transpiles() {
    // `add(a, b)` has no type annotations. Per `translate_params`, an unannotated
    // param defaults to `f64` (a `number`), so `a + b` is `f64 + f64`. The return
    // type is left to Rust inference (no annotation → no `-> f64`), which is the
    // correct Rust idiom — the transpile-first path works for plain numerics.
    let js = "export function add(a, b) { return a + b; }";
    let (rust, deps) = Translator::new()
        .translate_with_deps_as(js, FileRole::Module)
        .expect("a number-only .js module transpiles");
    assert!(rust.contains("pub fn add(a: f64, b: f64)"), "got: {rust}");
    assert!(
        !deps.needs_engine(),
        "no engine dep for a pure-number module"
    );
}

#[test]
fn untyped_js_uses_literal_type_inference() {
    // An untyped `let s = "hi"` infers `String` from the literal (see
    // `infer_literal_type`), so a string-returning function translates without a
    // type annotation — the transpile-first path works for string literals too.
    let js = "export function greet() { let s = \"hi\"; return s; }";
    let rust = Translator::new()
        .translate_with_deps_as(js, FileRole::Module)
        .expect("a literal-inferred .js module transpiles")
        .0;
    assert!(rust.contains("pub fn greet()"), "got: {rust}");
    assert!(rust.contains("let s: String"), "got: {rust}");
}

#[test]
fn untyped_js_homogeneous_array_transpiles() {
    // `let xs = [1, 2, 3]` infers `Vec<f64>` (homogeneous numeric array), and
    // `xs.length` lowers to `xs.len() as f64` (the array `length` builtin). Both
    // routes work without a `number[]` annotation — the transpile-first path
    // covers array literals and their builtins.
    let js = "export function count() { let xs = [1, 2, 3]; return xs.length; }";
    let rust = Translator::new()
        .translate_with_deps_as(js, FileRole::Module)
        .expect("a homogeneous-array .js module transpiles")
        .0;
    assert!(rust.contains("let xs: Vec<f64>"), "got: {rust}");
    assert!(rust.contains("xs.len() as f64"), "got: {rust}");
}
