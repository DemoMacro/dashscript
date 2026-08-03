use super::super::Translator;

#[test]
fn translates_optional_chain_to_as_ref_map() {
    let src = "interface V { x: number } function f(): void { const v: V | null = null; const x = v?.x; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("v.as_ref().map(|__c| __c.x)"), "got:\n{rust}");
}

#[test]
fn translates_optional_chain_coalesce_to_unwrap_or() {
    let src = "interface V { x: number } function f(): number { const v: V | null = null; return v?.x ?? -1; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("unwrap_or_else(|| -1_f64)"), "got:\n{rust}");
    assert!(rust.contains("__c.x"), "got:\n{rust}");
}

#[test]
fn option_field_access_unwraps_keyword_binding_via_raw_ident() {
    // A binding whose snake name is a Rust keyword (`ref` → `r#ref`) that is an
    // `Option<T>` parameter: a field access on it unwraps via
    // `r#ref.as_ref().unwrap()`. Previously this panicked — `Ident::new`
    // rejected the `r#` prefix after stringifying the raw ident to rebuild it.
    let src = "function f(ref: string | undefined): number { return ref.length; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("r#ref.as_ref().unwrap"),
        "keyword option binding not unwrapped via raw ident:\n{rust}"
    );
}

#[test]
fn translates_optional_chain_on_optional_field_uses_and_then() {
    // `a` is optional (`?:` → Option<bool>), so `opts?.a ?? false` must use
    // `and_then` (which flattens) rather than `map` (which would nest
    // Option<Option<bool>> and mistype at the `??`).
    let src = "interface Opts { a?: boolean } function f(opts?: Opts): boolean { return opts?.a ?? false; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("and_then(|__c| __c.a.clone())"),
        "expected and_then flatten, got:\n{rust}"
    );
    assert!(
        !rust.contains(".map(|__c| __c.a)"),
        "should not use map, got:\n{rust}"
    );
}

#[test]
fn translates_some_wrapping() {
    let src = "function main(): void { let x: number | null = 5; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("Option<f64>"), "got:\n{rust}");
    assert!(rust.contains("Some(5_f64)"), "got:\n{rust}");
}

#[test]
fn translates_non_null_assertion() {
    let src = "function f(x: number | null): void { console.log(x!); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("x: Option<f64>"), "got:\n{rust}");
    assert!(rust.contains("x.unwrap()"), "got:\n{rust}");
}

#[test]
fn narrows_option_truthiness_branch_binding() {
    // The branch reads `m!`, so the inner value binds and `m!` needs no unwrap.
    let src = "function f(): void { let m: number | null = 1; if (m) { console.log(m!); } }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("if let Some(m) = m"), "got:\n{rust}");
    assert!(!rust.contains(".unwrap()"), "got:\n{rust}");
}

#[test]
fn non_copy_option_truthiness_keeps_is_some() {
    // `Option<String>` inner is not Copy: narrowing would move out of it.
    let src = "function f(): void { let m: string | null = \"a\"; if (m) { console.log(1); } }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("m.is_some()"), "got:\n{rust}");
}

#[test]
fn mutated_option_truthiness_keeps_is_some() {
    // `m` is reassigned: an `if let` binding cannot be reassigned.
    let src = "function f(): void { let m: number | null = 1; if (m) { m = 2; } }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("m.is_some()"), "got:\n{rust}");
}

#[test]
fn translates_null_equality_to_is_none() {
    let src = "function f(m: number | null): boolean { return m === null; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("m.is_none()"), "got:\n{rust}");
}

#[test]
fn translates_null_inequality_to_is_some() {
    let src = "function f(m: number | null): boolean { return m !== null; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("m.is_some()"), "got:\n{rust}");
}

#[test]
fn value_type_null_inequality_folds_to_true() {
    // A value of a non-nullable type (a `Map`/struct) can never be null or
    // undefined, so `m != null` folds to `true` — the WPT harness' common
    // `assert_true(params != null)` constructor check.
    let src = "function f(): boolean { var m = new Map<string, number>(); return m != null; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        !rust.contains("None") && rust.contains("true"),
        "a non-nullable value `!= null` should fold to `true`, not compare against None: {rust}"
    );
}

#[test]
fn value_type_null_equality_folds_to_false() {
    let src = "function f(): boolean { const m = new Map<string, number>(); return m === null; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        !rust.contains("None") && rust.contains("false"),
        "a non-nullable value `=== null` should fold to `false`, not compare against None: {rust}"
    );
}

#[test]
fn url_search_params_null_inequality_folds_to_true() {
    // WPT urlsearchparams-get shape: an unannotated `var params = new
    // URLSearchParams("a=b")` records `DsUrlSearchParams`, so a harness
    // `params != null` constructor check folds to `true` (the value can never
    // be null/undefined) instead of E0369 (`DsUrlSearchParams != None`).
    let src =
        "function f(): boolean { var params = new URLSearchParams('a=b'); return params != null; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        !rust.contains("None") && rust.contains("true"),
        "a DsUrlSearchParams `!= null` should fold to `true`: {rust}"
    );
}

#[test]
fn url_search_params_ctor_coerces_number_init_to_string() {
    // ES `ToString` coerces a numeric `URLSearchParams` init before parsing
    // (`new URLSearchParams(0)` → `from_query("0")`), matching the instance
    // methods' `es_to_string_arg` — otherwise the ctor fails `from_query`'s
    // `AsRef<str>` (E0277 `f64: AsRef<str>`). `null`/`undefined` likewise.
    let src = "function f(): void { var params = new URLSearchParams(0); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("number_to_string"),
        "a numeric URLSearchParams init should ToString-coerce via number_to_string: {rust}"
    );
}

#[test]
fn translates_nullish_coalescing_to_unwrap_or_else() {
    let src = "function f(m: number | null): number { return m ?? 0; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("m.unwrap_or_else(|| 0_f64)"), "got:\n{rust}");
}

#[test]
fn translates_logical_or_value_returns_left_when_truthy() {
    let src = "function f(s: string): string { return s || \"default\"; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("let __l = "), "got:\n{rust}");
    assert!(rust.contains("!__l.is_empty()"), "got:\n{rust}");
}

#[test]
fn translates_logical_or_bool_short_circuits() {
    let src = "function f(a: boolean, b: boolean): boolean { return a || b; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("a || b"), "got:\n{rust}");
    assert!(
        !rust.contains("__l"),
        "bool should short-circuit, not block"
    );
}

#[test]
fn translates_logical_nullish_assign() {
    let src = "function f(x: number | null): void { x ??= 5; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("is_none()"), "got:\n{rust}");
    assert!(rust.contains("Some("), "got:\n{rust}");
}

#[test]
fn translates_logical_or_assign() {
    let src = "function f(x: number): void { x ||= 5; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("if !"), "got:\n{rust}");
}

#[test]
fn translates_member_access_truthiness_via_ds_truthy() {
    // `if (o.indent)` where `indent` is a string field — the translator has no
    // type checker (a `let opts = f(…)` binding is `_`-typed), so it emits
    // `__ds::truthy(&o.indent)` and the Rust compiler picks the `String` impl.
    // A bare `if o.indent` would be E0308 (expected bool, found String).
    let src = "interface Opts { indent: string } function f(o: Opts): void { if (o.indent) { console.log(1); } }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("__ds::truthy(&"), "got:\n{rust}");
}

#[test]
fn url_ctor_emits_dsurl_parse() {
    // `new URL(str)` → `DsUrl::parse(str)`; `JSON.stringify(url)` routes through
    // the generic `serde_json::to_string`, which needs `DsUrl: Serialize` (the
    // href string). The WPT url-tojson fixture is exactly this shape.
    let src = "function f(): void { const a = new URL(\"https://example.com/\"); console.log(JSON.stringify(a)); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("DsUrl::parse(\"https://example.com/\""),
        "new URL(str) should emit DsUrl::parse(str): {rust}"
    );
    assert!(
        rust.contains("serde_json::to_string(&"),
        "JSON.stringify(url) should emit serde_json::to_string: {rust}"
    );
}
