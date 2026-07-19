use super::super::Translator;

#[test]
fn translates_a_typed_function_returning_a_string() {
    let src = "function greet(name: string): string { return \"Hello\"; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("fn greet(name: String) -> String"),
        "got:\n{rust}"
    );
    assert!(rust.contains("\"Hello\".to_string()"), "got:\n{rust}");
}

#[test]
fn reports_parse_diagnostics() {
    assert!(Translator::new().translate("function (").is_err());
}

#[test]
fn translates_interface_to_struct() {
    let src = "interface Point { x: number; y: number; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("struct Point"), "got:\n{rust}");
    assert!(rust.contains("pub x: f64"), "got:\n{rust}");
    assert!(rust.contains("pub y: f64"), "got:\n{rust}");
}

#[test]
fn translates_optional_field_to_option_and_fills_none() {
    let src =
            "interface V { x: number; y?: number; } function f(): void { const v: V = { x: 1 }; console.log(v.x); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("pub y: Option<f64>"), "got:\n{rust}");
    assert!(rust.contains("V { x: 1_f64, y: None }"), "got:\n{rust}");
}

#[test]
fn translates_optional_field_supplied_wraps_some() {
    let src =
            "interface V { x: number; y?: number; } function f(): void { const v: V = { x: 1, y: 2 }; console.log(v.x); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("V { x: 1_f64, y: Some(2_f64) }"),
        "got:\n{rust}"
    );
}

#[test]
fn translates_generic_function_params() {
    let src = "function id<T>(x: T): T { return x; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("fn id<T>(x: T) -> T"), "got:\n{rust}");
}

#[test]
fn translates_default_param_to_option_unwrap_or_and_call_none() {
    let src = "function greet(name: string, greeting: string = \"hello\"): string { return greeting + \" \" + name; } function f(): string { return greet(\"world\"); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("greeting: Option<String>"), "got:\n{rust}");
    assert!(
        rust.contains("let greeting = greeting.unwrap_or(\"hello\".to_string());"),
        "got:\n{rust}"
    );
    assert!(
        rust.contains("greet(\"world\".to_string(), None)"),
        "got:\n{rust}"
    );
}

#[test]
fn translates_default_param_supplied_wraps_some() {
    let src = "function greet(name: string, greeting: string = \"hi\"): string { return greeting + name; } function f(): string { return greet(\"world\", \"hey\"); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("greet(\"world\".to_string(), Some(\"hey\".to_string()))"),
        "got:\n{rust}"
    );
}

#[test]
fn translates_array_type_to_vec() {
    let src = "interface Box { items: number[]; ids: Array<string>; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("Vec<f64>"), "got:\n{rust}");
    assert!(rust.contains("Vec<String>"), "got:\n{rust}");
}

#[test]
fn translates_locals_object_literal_and_field_access() {
    let src =
            "interface Point { x: number } function main(): void { const p: Point = { x: 1 }; console.log(p.x); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("Point { x: 1_f64 }"), "got:\n{rust}");
    assert!(rust.contains("p.x"), "got:\n{rust}");
}

#[test]
fn translates_nullable_to_option() {
    let src = "function main(): void { let x: number | null = null; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("Option<f64>"), "got:\n{rust}");
    assert!(rust.contains("= None"), "got:\n{rust}");
}

#[test]
fn translates_nullable_return_type() {
    let src = "function f(): number | null { return null; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("-> Option<f64>"), "got:\n{rust}");
    assert!(
        rust.contains("None") && !rust.contains("return"),
        "null -> trailing None, got:\n{rust}"
    );
}

#[test]
fn translates_string_union_to_enum() {
    let src = "type Status = \"pending\" | \"active\" | \"done\";";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("enum Status"), "got:\n{rust}");
    assert!(rust.contains("Pending"), "got:\n{rust}");
    assert!(rust.contains("Active"), "got:\n{rust}");
    assert!(rust.contains("Done"), "got:\n{rust}");
}

#[test]
fn translates_enum_variant_construction() {
    let src =
        "type Status = \"pending\" | \"done\"; function f(): void { let s: Status = \"done\"; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("Status::Done"), "got:\n{rust}");
}

#[test]
fn translates_object_keys_to_hashmap_keys() {
    let src = "function f(m: Record<string, number>): number { return Object.keys(m).length; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains(".keys().map(|k| k.to_string()).collect"),
        "got:\n{rust}"
    );
}

#[test]
fn translates_object_values_to_hashmap_values() {
    let src = "function f(m: Record<string, number>): number { return Object.values(m).length; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".values().cloned().collect"), "got:\n{rust}");
}

#[test]
fn translates_type_union_to_tagged_enum() {
    let src = "interface Circle { radius: number } interface Square { side: number } type Shape = Circle | Square;";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("enum Shape"), "got:\n{rust}");
    assert!(rust.contains("Circle(Circle)"), "got:\n{rust}");
    assert!(rust.contains("Square(Square)"), "got:\n{rust}");
}

#[test]
fn translates_discriminated_union_to_field_variants() {
    let src =
        "type Shape = { kind: \"circle\"; radius: number } | { kind: \"square\"; side: number };";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("enum Shape"), "got:\n{rust}");
    assert!(rust.contains("Circle { radius: f64 }"), "got:\n{rust}");
    assert!(rust.contains("Square { side: f64 }"), "got:\n{rust}");
}

#[test]
fn translates_discriminated_union_variant_construction() {
    let src = "type Shape = { kind: \"circle\"; radius: number } | { kind: \"square\"; side: number }; function f(): void { const s: Shape = { kind: \"circle\", radius: 3 }; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("Shape::Circle { radius: 3_f64 }"),
        "got:\n{rust}"
    );
}

#[test]
fn translates_return_object_literal_to_struct_init() {
    let src = "interface V { x: number; y: number; } function f(): V { return { x: 1, y: 2 }; }";
    let rust = Translator::new().translate(src).expect("should translate");
    // `return { … }` borrows the struct name from the return-type annotation.
    assert!(rust.contains("V { x: 1_f64, y: 2_f64 }"), "got:\n{rust}");
}

#[test]
fn translates_object_literal_argument_to_struct_init() {
    let src = "interface V { x: number; y: number; } function g(v: V): number { return v.x; } function f(): number { return g({ x: 1, y: 2 }); }";
    let rust = Translator::new().translate(src).expect("should translate");
    // `f({ x, y })` borrows the struct name from the callee's parameter type.
    assert!(rust.contains("g(V { x: 1_f64, y: 2_f64 })"), "got:\n{rust}");
}

#[test]
fn translates_record_computed_key_to_hashmap_entry() {
    let src = "function f(k: string): void { const m: Record<string, number> = { [k]: 1 }; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("(k, 1_f64)"), "got:\n{rust}");
}

#[test]
fn escapes_rust_keyword_variable_to_raw_ident() {
    let src = "function f(): number { const type = 5; return type; }";
    let rust = Translator::new().translate(src).expect("should translate");
    // `const type = 5` now infers f64 and annotates the binding (a bare `5`
    // would leave the local as an ambiguous {float}); `type` still escapes to
    // `r#type`.
    assert!(rust.contains("let r#type: i64 = 5_i64"), "got:\n{rust}");
    assert!(
        !rust.contains("return"),
        "trailing r#type, no return, got:\n{rust}"
    );
}

#[test]
fn translates_record_to_hashmap_literal() {
    let src = "function f(): void { const m: Record<string, number> = { a: 1, b: 2 }; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("HashMap<String, f64>"), "got:\n{rust}");
    assert!(rust.contains("HashMap::from"), "got:\n{rust}");
    assert!(rust.contains("\"a\".to_string()"), "got:\n{rust}");
}

#[test]
fn translates_hashmap_index_to_get() {
    let src =
        "function f(): number { const m: Record<string, number> = { a: 1 }; return m[\"a\"]; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains(".get(\"a\").copied().unwrap()"),
        "got:\n{rust}"
    );
}

#[test]
fn translates_hashmap_index_assign_to_insert() {
    let src = "function f(): void { let m: Record<string, number> = { a: 1 }; m[\"b\"] = 2; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains(".insert(\"b\".to_string(), 2_f64)"),
        "got:\n{rust}"
    );
}

#[test]
fn unwraps_type_assertion_as_expression() {
    let src = "function f(x: number): number { return x as number; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        !rust.contains("return"),
        "x as number unwraps to trailing x, got:\n{rust}"
    );
}

#[test]
fn translates_object_is_nan_equal() {
    let src = "function f(a: number, b: number): boolean { return Object.is(a, b); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("is_nan()"), "got:\n{rust}");
}

#[test]
fn translates_object_has_own_to_contains_key() {
    let src = "function f(m: Record<string, number>): boolean { return Object.hasOwn(m, \"a\"); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".contains_key(\"a\")"), "got:\n{rust}");
}

#[test]
fn translates_object_get_own_property_names_to_keys() {
    let src = "function f(m: Record<string, number>): number { return Object.getOwnPropertyNames(m).length; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".keys().map("), "got:\n{rust}");
}

#[test]
fn translates_object_assign_to_extend() {
    let src = "function f(a: Record<string, number>, b: Record<string, number>): Record<string, number> { return Object.assign(a, b); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".extend("), "got:\n{rust}");
}

#[test]
fn translates_object_from_entries_to_collect() {
    let src = "function f(m: Record<string, number>): Record<string, number> { return Object.fromEntries(Object.entries(m)); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("collect::<::std::collections::HashMap<String, f64>>()"),
        "got:\n{rust}"
    );
}

#[test]
fn translates_object_freeze_to_passthrough() {
    let src = "function f(m: Record<string, number>): Record<string, number> { Object.freeze(m); Object.seal(m); return Object.preventExtensions(m); }";
    let rust = Translator::new().translate(src).expect("should translate");
    // freeze/seal/preventExtensions are no-ops returning the value unchanged.
    assert!(
        !rust.contains("freeze") && !rust.contains("seal") && !rust.contains("preventExtensions"),
        "got:\n{rust}"
    );
}

#[test]
fn translates_object_is_frozen_to_false() {
    let src = "function f(m: Record<string, number>): boolean { return Object.isFrozen(m) && Object.isSealed(m) && Object.isExtensible(m); }";
    let rust = Translator::new().translate(src).expect("should translate");
    // A Record is never frozen/sealed (false), always extensible (true).
    assert!(
        rust.contains("false") && rust.contains("true"),
        "got:\n{rust}"
    );
}
