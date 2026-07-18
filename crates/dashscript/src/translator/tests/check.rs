// End-to-end tests for `Translator::check` — the translatability layer.
use super::super::Translator;

#[test]
fn check_passes_a_translatable_file() {
    let src = "function f(x: number): number { return x + 1; }";
    assert!(Translator::new().check(src).is_empty());
}

#[test]
fn check_passes_a_basic_class() {
    // A field-only class is translatable (struct + fn new).
    let diags = Translator::new().check("class C { x: number; }");
    assert!(diags.is_empty());
}

#[test]
fn check_flags_unsupported_import() {
    // A namespace import (`import * as ns`) is not mapped yet — only named and
    // default imports lower to a `use`.
    let diags = Translator::new().check("import * as ns from \"m\";");
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("import"));
}

#[test]
fn check_flags_a_syntax_error() {
    // Missing `:` — oxc_parser surfaces a syntax diagnostic.
    let diags = Translator::new().check("function f(x number) { return x; }");
    assert!(!diags.is_empty());
}

// Low-compatibility constructs — ECMAScript reflection/dynamic features with no
// Rust mapping. `check` flags them as `unsupported` (one diagnostic) rather
// than letting the translator lower them to broken Rust that fails `cargo
// check` (which would read as `partial` in the matrix). See `unsupported_pattern`.

#[test]
fn check_flags_instanceof() {
    let diags = Translator::new().check("function f(): boolean { return a instanceof B; }");
    assert!(
        diags.iter().any(|d| d.message.contains("instanceof")),
        "{diags:?}"
    );
}

#[test]
fn check_flags_symbol_call() {
    let diags = Translator::new().check("function f(): void { const s = Symbol(); }");
    assert!(diags.iter().any(|d| d.message.contains("Symbol")));
}

#[test]
fn check_flags_new_proxy() {
    let diags = Translator::new().check("function f(): void { const p = new Proxy({}, {}); }");
    assert!(diags.iter().any(|d| d.message.contains("Proxy")));
}

#[test]
fn check_flags_reflect_namespace() {
    let diags = Translator::new().check("function f(): boolean { return Reflect.has({}, \"x\"); }");
    assert!(diags.iter().any(|d| d.message.contains("Reflect")));
}

#[test]
fn check_flags_object_define_property() {
    let diags =
        Translator::new().check("function f(): void { Object.defineProperty({}, \"x\", {}); }");
    assert!(diags
        .iter()
        .any(|d| d.message.contains("Object.defineProperty")));
}

#[test]
fn check_flags_object_create() {
    let diags = Translator::new().check("function f(): void { Object.create(null); }");
    assert!(diags.iter().any(|d| d.message.contains("Object.create")));
}

#[test]
fn check_flags_has_own_property() {
    let diags =
        Translator::new().check("function f(): boolean { return {}.hasOwnProperty(\"x\"); }");
    assert!(diags.iter().any(|d| d.message.contains("hasOwnProperty")));
}

#[test]
fn check_flags_constructor_reflection() {
    let diags = Translator::new().check("function f(): unknown { return (1).constructor; }");
    assert!(diags.iter().any(|d| d.message.contains("constructor")));
}

#[test]
fn check_flags_arguments_object() {
    let diags = Translator::new().check("function f(): unknown { return arguments[0]; }");
    assert!(diags.iter().any(|d| d.message.contains("arguments")));
}

#[test]
fn check_flags_delete_operator() {
    let diags = Translator::new().check("function f(): void { delete o.x; }");
    assert!(diags.iter().any(|d| d.message.contains("delete")));
}

#[test]
fn check_flags_bigint_literal() {
    let diags = Translator::new().check("function f(): void { const n = 1n; }");
    assert!(diags.iter().any(|d| d.message.contains("BigInt")));
}

#[test]
fn check_flags_low_compat_nested_in_callback() {
    // A construct buried inside a callback body is still surfaced — the walk
    // recurses through every expression kind the translator itself walks.
    let diags =
        Translator::new().check("function f(): void { xs.forEach((x) => x instanceof B); }");
    assert!(diags.iter().any(|d| d.message.contains("instanceof")));
}

#[test]
fn check_does_not_flag_typeof_symbol() {
    // `typeof` has its own mapping (a global constructor → "function"), so its
    // operand is not walked — `typeof Symbol` stays supported.
    let diags = Translator::new().check("function f(): void { console.log(typeof Symbol); }");
    assert!(diags.is_empty(), "{diags:?}");
}

#[test]
fn check_does_not_flag_object_keys() {
    // `Object.keys` (and values/entries/is/freeze/…) is mapped — it must not
    // trip the reflection rule (only the named reflection surface is flagged).
    let diags = Translator::new().check("function f(): void { Object.keys({ a: 1 }); }");
    assert!(diags.is_empty(), "{diags:?}");
}

#[test]
fn check_does_not_flag_supported_code() {
    // A plain supported body has no low-compat construct — the walk adds nothing.
    let diags = Translator::new().check(
        "function f(): void { const xs: number[] = [1, 2]; console.log(Math.round(xs[0])); }",
    );
    assert!(diags.is_empty(), "{diags:?}");
}

#[test]
fn check_flags_reflection_in_function_expression() {
    // A reflection call inside an IIFE body `(function () { … })()` is still
    // surfaced — the walk recurses function-expression bodies, not just arrows.
    let diags = Translator::new()
        .check("function f(): void { (function () { Object.defineProperty({}, \"x\", {}); })(); }");
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("Object.defineProperty")),
        "{diags:?}"
    );
}

#[test]
fn check_flags_reflection_in_try_catch() {
    // A construct in the try body or the catch handler (`e.constructor`) is
    // surfaced — the walk recurses both the try block and the catch body.
    let diags = Translator::new().check(
        "function f(): void { try { Object.create(null); } catch (e) { console.log(e.constructor); } }",
    );
    assert!(
        diags.iter().any(|d| d.message.contains("Object.create")),
        "{diags:?}"
    );
    assert!(
        diags.iter().any(|d| d.message.contains("constructor")),
        "{diags:?}"
    );
}
