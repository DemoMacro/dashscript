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
fn translates_mutated_var_is_let_mut() {
    // JS `var` is reassignable (unlike `const`), so `var n = …; n = …` needs
    // `let mut n` — same as `let`. test262 leans heavily on `for (var i …)`.
    let src = "function main(): void { var n = 0; n = 5; console.log(n); }";
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
fn index_clone_for_multi_use_non_copy_element() {
    // `let row = nested[i]` would move the inner Vec out of `nested`; `nested`
    // is read again (the loop bound), so the non-Copy element is cloned at the
    // index site.
    let src = "function f(): void { const nested: number[][] = [[1, 2], [3, 4]]; for (let i = 0; i < nested.length; i++) { const row = nested[i]; console.log(row.length); } }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("nested[i as usize].clone()"),
        "a reused Vec-of-Vec index is cloned, got:\n{rust}"
    );
}

#[test]
fn index_no_clone_for_copy_element() {
    // A scalar element (f64) copies on index — no clone even when reused.
    let src = "function f(): void { const xs: number[] = [1, 2, 3]; for (let i = 0; i < xs.length; i++) { const x = xs[i]; console.log(x); } }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        !rust.contains(".clone()"),
        "a Copy element is not cloned, got:\n{rust}"
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

#[test]
fn member_mutated_array_param_is_ref_mut() {
    // `c[i] = v` mutates a member of `c`, not a rebind — ES reference
    // semantics, so the parameter is `&mut Vec` and the caller sees the change.
    let src = "function f(c: number[]): void { c[0] = 1; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("c: &mut Vec<f64>"), "got:\n{rust}");
    assert!(
        !rust.contains("mut c:"),
        "a non-rebound param is not `mut`, got:\n{rust}"
    );
}

#[test]
fn reassigned_array_param_stays_owned_mut() {
    // A rebind (`c = …`) does not propagate to the caller, so the parameter
    // stays owned `mut c` — reassign wins over member-mutation.
    let src = "function f(c: number[]): void { c = [9]; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("mut c:"), "got:\n{rust}");
    assert!(
        !rust.contains("c: &mut"),
        "a rebound param is not a reference, got:\n{rust}"
    );
}

#[test]
fn member_mutated_array_local_is_let_mut() {
    // A local Vec whose element is assigned still needs `let mut` (it owns the
    // value); only parameters become `&mut`.
    let src = "function main(): void { let c: number[] = [1]; c[0] = 2; console.log(c[0]); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("let mut c:"), "got:\n{rust}");
}

#[test]
fn ref_param_call_site_borrows_in_place() {
    // A reference parameter is borrowed in place at the call site (`&mut a`),
    // not cloned — the callee's mutation is then visible to the caller. The
    // local also needs `let mut` since it is borrowed `&mut`.
    let src = "function fill(c: number[]): void { c[0] = 1; }\nfunction main(): void { let a: number[] = [0]; fill(a); console.log(a[0]); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("fill(&mut a)"), "got:\n{rust}");
    assert!(
        !rust.contains("fill(a.clone())"),
        "a ref-param arg is borrowed, not cloned, got:\n{rust}"
    );
    assert!(
        rust.contains("let mut a:"),
        "a local borrowed `&mut` needs `let mut`, got:\n{rust}"
    );
}

#[test]
fn ref_param_array_set_reborrows_without_mut_prefix() {
    // Inside the callee, `c[i] = v` on a reference parameter reborrows —
    // `array_set(c, …)` (no leading `&mut`), since `c` is already a `&mut Vec`.
    let src = "function f(c: number[]): void { c[0] = 1; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("array_set(c,"), "got:\n{rust}");
    assert!(
        !rust.contains("array_set(&mut c,"),
        "a ref-param target reborrows, got:\n{rust}"
    );
}
