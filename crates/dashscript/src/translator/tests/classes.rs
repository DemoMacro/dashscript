// Class / `new` / `this` translation.
use super::super::Translator;

#[test]
fn this_outside_method_routes_to_engine() {
    // `this` has no receiver at module scope or in a free function — no static
    // lowering. Rather than emit `compile_error!` (which would break `ds build`
    // in production — the conformance harness' cargo-check-fail fallback is
    // harness-only), the function degrades to the engine: the emit carries the
    // JS source verbatim for the engine to run.
    let src = "function f() { return this; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        !rust.contains("compile_error"),
        "`this` outside a method must route to the engine, not emit compile_error: {rust}"
    );
    assert!(
        rust.contains("return this;"),
        "the engine must receive the original `this` source verbatim: {rust}"
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
fn get_accessor_lowers_to_zero_arg_method() {
    // A `get` accessor has no Rust property analogue, so it lowers as a zero-arg
    // method: `get val()` → `pub fn val(&self) -> f64` (a property read rewrites
    // to a call at the call site).
    let rust = Translator::new()
        .translate("class C { get val(): number { return 1; } }")
        .expect("should translate");
    assert!(
        !rust.contains("compile_error"),
        "get accessor lowered: {rust}"
    );
    assert!(
        rust.contains("fn val(&self)"),
        "get -> fn val(&self): {rust}"
    );
}

#[test]
fn getter_property_read_rewrites_to_method_call() {
    // `obj.val` where `val` is a `get` accessor of obj's class rewrites to
    // `obj.val()` — a getter lowers to a zero-arg method, so a property read
    // is a call, not a field access. The receiver's class is read off its
    // declared parameter type, then checked against the class-getter table.
    let src = "class C { n = 1; get val(): number { return this.n; } }\nfunction f(c: C): number { return c.val; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("c.val()"), "getter read -> c.val(): {rust}");
}

#[test]
fn flags_private_field() {
    let rust = Translator::new()
        .translate("class C { #x: number; }")
        .expect("should translate");
    assert!(rust.contains("private"), "private diag: {rust}");
}

#[test]
fn ts_private_modifier_lowers_to_pub() {
    // A TS `private`/`protected` modifier is access control only; Rust struct
    // fields / impl methods are all `pub`, so it lowers as a normal member (no
    // `compile_error!`). Only a `#private` identifier stays unsupported.
    let rust = Translator::new()
        .translate("class C { private x: number = 0; protected y: number = 0; }")
        .expect("should translate");
    assert!(
        !rust.contains("compile_error"),
        "ts private lowered: {rust}"
    );
    assert!(rust.contains("pub x"), "private field -> pub: {rust}");
    assert!(rust.contains("pub y"), "protected field -> pub: {rust}");
}

#[test]
fn infers_initializer_only_field_type() {
    // An initializer-only field (no annotation) infers its type from the
    // initializer: `new Map<string, number>()` → `HashMap<String, f64>`,
    // `new WeakMap<Uint8Array, string>()` → `HashMap<Vec<u8>, String>` (a
    // `WeakMap` uses the same strong-collection backing — no GC-precise weak
    // refs).
    let rust = Translator::new()
        .translate(
            "class C { m = new Map<string, number>(); w = new WeakMap<Uint8Array, string>(); }",
        )
        .expect("should translate");
    assert!(
        rust.contains("HashMap<String, f64>"),
        "map field inferred: {rust}"
    );
    assert!(
        rust.contains("HashMap<Vec<u8>, String>"),
        "weakmap field inferred: {rust}"
    );
}

#[test]
fn collection_method_dispatches_on_this_field() {
    // Inside a method, a `this.<field>` receiver resolves its type from the
    // class's instance fields, so a `Map`/`Set` method dispatches on it the
    // same way it does on a local: `this.m.set(k, v)` → `self.m.insert(k, v)`,
    // `this.m.has(k)` → `self.m.contains_key(&k)`.
    let src = "class C { m = new Map<string, number>(); add(k: string, v: number): void { this.m.set(k, v); } has(k: string): boolean { return this.m.has(k); } }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("self.m.insert"),
        "this.m.set -> insert: {rust}"
    );
    assert!(
        rust.contains("self.m.contains_key(&"),
        "this.m.has -> contains_key: {rust}"
    );
}

#[test]
fn prefix_increment_this_field_yields_number() {
    // `++this.counter` inside a template literal routes through
    // `number_to_string` (an ES update expression always yields a number), and
    // lowers to a block that mutates the field and returns the new value — not
    // a `todo!()` and not the `()` of a bare `+=` statement.
    let src = "class C { counter = 0; inc(): string { return `n${++this.counter}`; } }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("number_to_string"),
        "++this.counter -> number_to_string: {rust}"
    );
    assert!(
        rust.contains("self.counter += 1_f64"),
        "++this.counter mutates the field: {rust}"
    );
    assert!(
        !rust.contains("todo!()"),
        "no todo!() for ++this.counter: {rust}"
    );
}

#[test]
fn flags_abstract_class() {
    let rust = Translator::new()
        .translate("abstract class C { x: number; }")
        .expect("should translate");
    assert!(rust.contains("abstract"), "abstract diag: {rust}");
}

#[test]
fn generic_class_lowers_generic_struct_and_impl() {
    // `class C<T>` → `struct C<T>` + `impl<T: Clone> C<T>`. The Clone bound is
    // added on the impl because the class derives Clone and its methods clone
    // field values; `T extends X` itself has no Rust trait analogue (X is a
    // struct, not a trait).
    let src = "class C<T> { items: number[] = []; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("struct C<T>"), "generic struct: {rust}");
    assert!(
        rust.contains("impl<T: Clone> C<T>"),
        "generic impl + Clone bound: {rust}"
    );
}
