use super::super::Translator;

#[test]
fn translates_unmutated_let_is_plain_let() {
    let src = "function main(): void { let n: number = 0; console.log(n); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("let n:"), "got:\n{rust}");
    assert!(!rust.contains("let mut n"), "got:\n{rust}");
}

#[test]
fn translates_mutated_let_is_let_mut() {
    let src = "function main(): void { let n: number = 0; n = 5; console.log(n); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("let mut n"), "got:\n{rust}");
}

#[test]
fn translates_mutated_let_by_compound_assign_is_let_mut() {
    let src = "function main(): void { let n: number = 0; n += 5; console.log(n); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("let mut n"), "got:\n{rust}");
}

#[test]
fn translates_mutated_vec_by_method_is_let_mut() {
    let src =
        "function main(): void { let xs: number[] = [1]; xs.push(2); console.log(xs.length); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("let mut xs"), "got:\n{rust}");
}

#[test]
fn translates_for_in_to_keys_cloned() {
    let src =
        "function f(m: Record<string, number>): void { for (const k in m) { console.log(k); } }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("for k in m.keys().cloned()"), "got:\n{rust}");
}

#[test]
fn single_use_moves_without_clone() {
    let src = "interface V { x: number } function consume(v: V): void {} function f(v: V): void { consume(v); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        !rust.contains(".clone()"),
        "a single use moves (last use), got:\n{rust}"
    );
}

#[test]
fn multi_use_clones_non_copy_local() {
    let src = "interface V { x: number } function consume(v: V): void {} function f(v: V): void { consume(v); consume(v); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("consume(v.clone())"),
        "a reused non-Copy local is cloned, got:\n{rust}"
    );
}

#[test]
fn multi_use_with_field_read_clones_call_arg() {
    // `v` is read twice (call + field); the call must clone so `v.x` works.
    let src = "interface V { x: number } function consume(v: V): void {} function f(v: V): void { consume(v); console.log(v.x); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("consume(v.clone())"),
        "call clones when v is read again, got:\n{rust}"
    );
}

#[test]
fn scalar_multi_use_not_cloned() {
    let src = "function consume(n: number): void {} function f(n: number): void { consume(n); consume(n); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(!rust.contains(".clone()"), "a scalar is Copy, got:\n{rust}");
}

#[test]
fn option_of_scalar_multi_use_not_cloned() {
    let src = "function consume(o: number | null): void {} function f(o: number | null): void { consume(o); consume(o); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        !rust.contains(".clone()"),
        "Option<f64> is Copy, got:\n{rust}"
    );
}

#[test]
fn struct_and_enum_derive_clone() {
    let src = "interface V { x: number } type K = \"a\" | \"b\";";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.matches("#[derive(Clone)]").count() >= 2,
        "both struct and enum derive Clone, got:\n{rust}"
    );
}
