use super::super::Translator;

#[test]
fn translates_object_destructure_default_to_unwrap_or() {
    let src = "interface User { name?: string; } function f(u: User): void { const { name = \"anon\" } = u; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains(".unwrap_or(\"anon\".to_string())"),
        "got:\n{rust}"
    );
}

#[test]
fn translates_discriminated_union_switch_destructure() {
    let src = "type Shape = { kind: \"circle\"; radius: number } | { kind: \"square\"; side: number }; function area(s: Shape): number { switch (s.kind) { case \"circle\": return s.radius * s.radius; case \"square\": return s.side * s.side; } }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("match s"), "got:\n{rust}");
    assert!(rust.contains("Shape::Circle { radius }"), "got:\n{rust}");
    assert!(rust.contains("Shape::Square { side }"), "got:\n{rust}");
    // narrowing: each `s.radius` reads as the destructured `radius` binding.
    assert!(rust.contains("radius * radius"), "got:\n{rust}");
}

#[test]
fn translates_object_destructuring_to_struct_pattern() {
    let src = "interface V { x: number; y: number; } function f(v: V): number { const { x, y } = v; return x + y; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("let V { x, y, .. } = v;"), "got:\n{rust}");
}

#[test]
fn translates_array_destructuring_to_indexed_lets() {
    let src = "function f(): void { const xs: number[] = [1, 2]; const [a, b] = xs; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("let a = xs[0];"), "got:\n{rust}");
    assert!(rust.contains("let b = xs[1];"), "got:\n{rust}");
}

#[test]
fn translates_array_destructure_rest_to_slice() {
    let src = "function f(): void { const xs: number[] = [1, 2, 3]; const [a, ...rest] = xs; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("let rest = xs[1..].to_vec()"), "got:\n{rust}");
}

#[test]
fn translates_object_spread_to_struct_update() {
    let src = "interface Vector { x: number; y: number; } function f(v: Vector): Vector { return { ...v, y: 9 }; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("Vector { y: 9_f64, ..v }"), "got:\n{rust}");
}

#[test]
fn translates_array_spread_to_slice_concat() {
    let src = "function f(): void { const xs: number[] = [1, 2]; const ys = [...xs, 3]; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("[xs.as_slice(), &[3_f64][..]].concat()"),
        "got:\n{rust}"
    );
}

#[test]
fn translates_array_destructure_skips_holes() {
    let src = "function f(xs: number[]): void { const [a, , c] = xs; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("xs[0]"), "got:\n{rust}");
    assert!(rust.contains("xs[2]"), "got:\n{rust}");
    assert!(!rust.contains("xs[1]"), "hole must be skipped");
}

#[test]
fn translates_object_destructure_rename() {
    let src =
        "interface Vector { x: number; } function f(v: Vector): void { const { x: renamed } = v; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("x: renamed"), "got:\n{rust}");
}

#[test]
fn translates_object_destructure_compound_init_to_field_access() {
    // init is a CallExpression (not a plain identifier), so `expr_type_path`
    // can't resolve a struct name → the field-access fallback must bind each
    // field via a temp so `done`/`value` enter scope (regression: E0425, the
    // old `let _ = makeResult();` dropped every binding).
    let src = "interface ReadResult { done: boolean; value: number; } function makeResult(): ReadResult { return { done: false, value: 42 }; } function f(): number { const { done, value } = makeResult(); return done ? 0 : value; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("let __ds_tmp = make_result();"),
        "temp binding keeps init single-eval, got:\n{rust}"
    );
    assert!(
        rust.contains("let done = __ds_tmp.done;"),
        "field-access binding enters scope, got:\n{rust}"
    );
    assert!(
        rust.contains("let value = __ds_tmp.value;"),
        "field-access binding enters scope, got:\n{rust}"
    );
}

#[test]
fn translates_object_destructure_compound_init_mutable() {
    // `let` destructure on a compound init → `mut` field-access binding.
    let src = "interface Cell { v: number; } function makeCell(): Cell { return { v: 1 }; } function f(): void { let { v } = makeCell(); v = v + 1; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("let mut v = __ds_tmp.v;"),
        "mutable field-access binding, got:\n{rust}"
    );
}

#[test]
fn translates_closure_param_object_destructure_to_field_bindings() {
    // `({ value, done }) => { … }`: a destructuring parameter binds to a
    // synthesized `__ds_arg0`, and its sub-bindings are extracted at the body
    // top so `value`/`done` enter scope. Regression: E0425 — `binding_name`
    // folded an `ObjectPattern` to `_`, so the body's `value`/`done` were
    // never defined (the lipfuzz `.then(({ value, done }) => …)` shape).
    let src = "function g(): void { const f = ({ value, done }: { value: number; done: boolean }) => { const x = value; const y = done; }; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("__ds_arg0"),
        "destructuring param binds to a synthesized name, got:\n{rust}"
    );
    assert!(
        rust.contains("let value = __ds_arg0.value;"),
        "prelude extracts field into scope, got:\n{rust}"
    );
    assert!(
        rust.contains("let done = __ds_arg0.done;"),
        "prelude extracts field into scope, got:\n{rust}"
    );
}

#[test]
fn translates_closure_param_array_destructure_to_index_bindings() {
    // `([a, b]) => { … }`: same synthesized-`__ds_argN` mechanism, indexed
    // element access (`let a = __ds_arg0[0];`).
    let src =
        "function g(): void { const f = ([a, b]: number[]) => { const x = a; const y = b; }; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("let a = __ds_arg0[0];"),
        "prelude indexes element into scope, got:\n{rust}"
    );
    assert!(
        rust.contains("let b = __ds_arg0[1];"),
        "prelude indexes element into scope, got:\n{rust}"
    );
}

#[test]
fn translates_closure_param_destructure_expression_body_to_block() {
    // `({ value }) => value`: an expression body has no block for the prelude,
    // so it wraps — `{ let value = __ds_arg0.value; value }` (the trailing
    // expression is the closure's return value).
    let src = "function g(): void { const f = ({ value }: { value: number }) => value; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("let value = __ds_arg0.value;"),
        "expression-body destructure still extracts the field, got:\n{rust}"
    );
}
