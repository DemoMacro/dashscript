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
    assert!(rust.contains("x: 5_f64"), "default init: {rust}");
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

#[test]
fn translates_constructor_with_params() {
    let src = "class P { x: number; y: number; constructor(x: number, y: number) { this.x = x; this.y = y; } }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("fn new(x: f64, y: f64)"),
        "ctor params: {rust}"
    );
    // `this.x = x` folds into the struct literal via the __ds_self block.
    assert!(rust.contains("__ds_self"), "ctor block: {rust}");
}

#[test]
fn translates_method_reads_this() {
    let src =
        "class C { x: number; constructor(x: number) { this.x = x; } value(): number { return this.x; } }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("fn value(&self)"), "&self method: {rust}");
    assert!(rust.contains("self.x"), "this.x -> self.x: {rust}");
}

#[test]
fn translates_method_mutates_this_to_mut_self() {
    let src =
        "class C { n: number; constructor() { this.n = 0; } inc(): void { this.n = this.n + 1; } }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("fn inc(&mut self)"), "&mut self: {rust}");
}

#[test]
fn mut_self_method_call_marks_receiver_let_mut() {
    // The call-site analogue of `translates_method_mutates_this_to_mut_self`:
    // a local that calls a project `&mut self` method must itself be `let mut`.
    let src = "class C { n: number; constructor() { this.n = 0; } inc(): void { this.n = this.n + 1; } }\nfunction f(): void { let c = new C(); c.inc(); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("let mut c"),
        "receiver of a `&mut self` call must be `let mut`: {rust}",
    );
}

#[test]
fn translates_new_with_arguments() {
    let src = "class P { x: number; constructor(x: number) { this.x = x; } }\nfunction f(): number { let p = new P(5); return p.x; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("P::new(5_f64)"), "new P(5): {rust}");
}

#[test]
fn flags_class_inheritance() {
    let rust = Translator::new()
        .translate("class C extends B { x: number; }")
        .expect("should translate");
    assert!(rust.contains("inheritance"), "extends diag: {rust}");
}

#[test]
fn flags_static_field() {
    let rust = Translator::new()
        .translate("class C { static x: number; }")
        .expect("should translate");
    assert!(rust.contains("`static`"), "static diag: {rust}");
}

#[test]
fn flags_get_accessor() {
    let rust = Translator::new()
        .translate("class C { get val(): number { return 1; } }")
        .expect("should translate");
    assert!(rust.contains("accessors"), "get diag: {rust}");
}

#[test]
fn flags_private_field() {
    let rust = Translator::new()
        .translate("class C { #x: number; }")
        .expect("should translate");
    assert!(rust.contains("private"), "private diag: {rust}");
}

#[test]
fn flags_abstract_class() {
    let rust = Translator::new()
        .translate("abstract class C { x: number; }")
        .expect("should translate");
    assert!(rust.contains("abstract"), "abstract diag: {rust}");
}
