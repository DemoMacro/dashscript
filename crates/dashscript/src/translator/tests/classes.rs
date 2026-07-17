// Class / `new` / `this` translation.
use super::super::Translator;

#[test]
fn this_outside_method_is_compile_error() {
    // `this` has no receiver at module scope or in a free function, so it lowers
    // to a `compile_error!` (the generated Rust still parses; it fails loudly).
    let src = "function f() { return this; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("compile_error"),
        "this outside method: {rust}"
    );
}

#[test]
fn translates_field_only_class() {
    let src = "class C { x: number; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("derive(Clone)"), "struct derive: {rust}");
    assert!(rust.contains("pub x: f64"), "pub field: {rust}");
}

#[test]
fn translates_class_with_default_initializer() {
    let src = "class C { x: number = 5; }";
    let rust = Translator::new().translate(src).expect("should translate");
    // `x = 5` fills the field initializer inside `fn new()`.
    assert!(rust.contains("x: 5.0"), "default init: {rust}");
}

#[test]
fn translates_new_expression() {
    let src = "class C { x: number = 1; }\nfunction f(): C { return new C(); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("C::new()"), "new C(): {rust}");
}

#[test]
fn translates_exported_class() {
    let src = "export class C { x: number; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("pub struct C"), "pub struct: {rust}");
}
