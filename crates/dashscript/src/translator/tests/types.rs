use super::super::Translator;
use quote::ToTokens;

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
fn collect_function_signatures_captures_generic_function_return_type() {
    // A generic function's signature (type params + return type) is collected
    // for cross-file sharing: a module-global factory singleton reads its type
    // from a callee defined in another file.
    let src = "function make<T>(x: T): Wrapper<T> { return x; }";
    let sigs = Translator::new()
        .collect_function_signatures(src)
        .expect("should collect");
    let make = sigs.get("make").expect("make signature collected");
    assert_eq!(make.type_params, vec!["T".to_string()]);
    let ret = make
        .return_type
        .as_ref()
        .expect("return type present")
        .to_token_stream()
        .to_string();
    assert!(ret.contains("Wrapper"), "got: {ret}");
    assert!(ret.contains("T"), "got: {ret}");
}

#[test]
fn collect_function_signatures_captures_const_arrow_return_type() {
    // A const arrow (`const factory = <T>(x): Box<T> => ..`) carries the same
    // signature — the module-global singleton's type comes from the arrow it
    // assigns to.
    let src = "const factory = <T>(x: T): Box<T> => x;";
    let sigs = Translator::new()
        .collect_function_signatures(src)
        .expect("should collect");
    let f = sigs.get("factory").expect("factory signature collected");
    assert_eq!(f.type_params, vec!["T".to_string()]);
    let ret = f
        .return_type
        .as_ref()
        .expect("return type present")
        .to_token_stream()
        .to_string();
    assert!(ret.contains("Box"), "got: {ret}");
    assert!(ret.contains("T"), "got: {ret}");
}

#[test]
fn factory_singleton_infers_generic_return_type() {
    // A module-global factory singleton (`const p = make<T>(...)`) with no
    // annotation infers its OnceLock type from the callee's signature,
    // instantiating the generic param with the call's type argument
    // (`make<TFile>` ← `make<number>` → `Wrapper<f64>`).
    let src = "function make<T>(x: T): Wrapper<T> { return x; }\
               const p = make<number>(42);\
               function useP(): Wrapper<number> { return p; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("static P_CELL: ::std::sync::OnceLock<Wrapper<f64>>"),
        "got:\n{rust}"
    );
    assert!(
        rust.contains("fn p() -> &'static Wrapper<f64>"),
        "got:\n{rust}"
    );
}

#[test]
fn cross_package_factory_singleton_prefixes_return_type() {
    // A factory whose signature was collected from another package carries
    // `source_crate`; its return type is prefixed `crate::<pkg>::…` on the
    // consumer's OnceLock, since the consumer imports only the factory, not its
    // return type (`createPacker` is imported, `Packer` is not).
    use super::super::FnSignature;
    use std::collections::HashMap;
    let mut sigs = HashMap::new();
    sigs.insert(
        "make".to_string(),
        FnSignature {
            type_params: vec!["T".to_string()],
            return_type: Some(syn::parse_quote!(Wrapper<T>)),
            source_crate: Some("member_crate".to_string()),
        },
    );
    let src = "const p = make<number>(42);\
               function useP(): Wrapper<number> { return p; }";
    let rust = Translator::new()
        .with_extra_function_signatures(sigs)
        .translate(src)
        .expect("should translate");
    assert!(
        rust.contains("static P_CELL: ::std::sync::OnceLock<crate::member_crate::Wrapper<f64>>"),
        "got:\n{rust}"
    );
    assert!(
        rust.contains("fn p() -> &'static crate::member_crate::Wrapper<f64>"),
        "got:\n{rust}"
    );
}

#[test]
fn method_call_singleton_infers_builtin_return_type() {
    // A module-global constant whose initializer is a method call with a known
    // builtin return type (`arr.join("")` → String) lowers to a OnceLock<String>
    // without an explicit annotation, mirroring the factory-call path.
    let src = "const arr: string[] = [\"a\", \"b\"];\
               const joined = arr.join(\"\");\
               function f(): string { return joined; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("static JOINED_CELL: ::std::sync::OnceLock<String>"),
        "got:\n{rust}"
    );
}

#[test]
fn set_literal_singleton_infers_hashset_type() {
    // A module-global `new Set([literal array])` with no annotation lowers to a
    // OnceLock<HashSet<String>> — element type inferred from the first literal,
    // matching the `.to_string()` translation each element gets — and the
    // initializer is `HashSet::from([...])`, mirroring the factory/method-call
    // singleton paths.
    let src = "const EXT = new Set([\"jpg\", \"png\"]);\
               function contains(value: string): boolean { return EXT.has(value); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains(
            "static EXT_CELL: ::std::sync::OnceLock<::std::collections::HashSet<String>>"
        ),
        "got:\n{rust}"
    );
    assert!(
        rust.contains("::std::collections::HashSet::from(["),
        "got:\n{rust}"
    );
}

#[test]
fn object_literal_singleton_infers_hashmap_type() {
    // A module-global object literal with no annotation whose properties share
    // one scalar kind lowers to a OnceLock<HashMap<String, V>> — V inferred from
    // the uniform property value kind (`{ flag: true, on: true }` → bool) — and
    // the initializer is `HashMap::from([...])`, the anonymous-object mapping.
    let src = "const OPTS = { flag: true, on: true };\
               function read(): boolean { const alias = OPTS; return true; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains(
            "static OPTS_CELL: ::std::sync::OnceLock<::std::collections::HashMap<String, bool>>"
        ),
        "got:\n{rust}"
    );
    assert!(
        rust.contains("::std::collections::HashMap::from(["),
        "got:\n{rust}"
    );
}

#[test]
fn string_concat_singleton_infers_string_type() {
    // A module-global `+` chain with a string-literal leaf lowers to a
    // OnceLock<String> — the init translates to `format!(...)` (Rust's `+` does
    // not apply to `String`), so the cell holds a `String`.
    let src = "const NS = \"http://x\";\
               const XML = '<a>' + NS + '</a>';\
               function read(): string { const ref = XML; return ref; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("static XML_CELL: ::std::sync::OnceLock<String>"),
        "got:\n{rust}"
    );
}

#[test]
fn typed_assertion_singleton_uses_assertion_type() {
    // A module-global `const X = expr as T` whose only type clue is the
    // assertion lowers to a OnceLock<T> — the assertion's type feeds the cell,
    // and the init translates as the inner `expr` (the assertion is stripped).
    let src = "const X = { a: 1 } as Record<string, number>;\
               function read(): number { const ref = X; return 0; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains(
            "static X_CELL: ::std::sync::OnceLock<::std::collections::HashMap<String, f64>>"
        ),
        "got:\n{rust}"
    );
}

#[test]
fn map_ctor_singleton_infers_hashmap_type() {
    // A module-global `new Map<K, V>()` (no entries) lowers to a
    // OnceLock<HashMap<K, V>> — K, V from the type args, init `HashMap::new()`.
    let src = "const CACHE = new Map<string, number>();\
               function read(): number { const ref = CACHE; return 0; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains(
            "static CACHE_CELL: ::std::sync::OnceLock<::std::collections::HashMap<String, f64>>"
        ),
        "got:\n{rust}"
    );
}

#[test]
fn const_arrow_lowers_to_fn_item() {
    // A module-level const arrow (`const f = () => …`) is a declaration that
    // lowers to a `fn` item, not an executable statement — so it never lands in
    // the rejected module-file executable set.
    let src = "const greet = (name: string): string => \"hi \" + name;";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("fn greet("), "got:\n{rust}");
}

#[test]
fn export_const_literal_lowers_to_pub_const() {
    // `export const X = <literal>` (Number/Bool/String) is a const-expr literal
    // → a `pub const` item, not a dropped executable statement (an arrow
    // initializer is already a `fn` item). String and boolean literals map to
    // `&'static str` and `bool`.
    let src = "export const LEVEL = 1;\
               export const NAME = \"x\";\
               export const FLAG = true;";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("pub const level: f64 = 1_f64;"),
        "got:\n{rust}"
    );
    assert!(
        rust.contains("pub const name: &'static str = \"x\";"),
        "got:\n{rust}"
    );
    assert!(
        rust.contains("pub const flag: bool = true;"),
        "got:\n{rust}"
    );
}

#[test]
fn interface_extends_keyword_field_name() {
    // A parent interface field named `type` (a Rust keyword) flattens into the
    // child struct as `r#type` (raw ident), not a panic on `Ident::new`.
    let src = "interface A { type: string } interface B extends A {}";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("r#type"), "got:\n{rust}");
}

#[test]
fn interface_keyword_name_becomes_raw_ident() {
    // A TS type name that lands on a Rust reserved keyword (`macro`) emits a
    // raw ident so `struct r#macro {}` parses, instead of panicking on
    // `format_ident!("macro")` (a bare reserved keyword is not a valid item name).
    let src = "interface macro { x: number }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("struct r#macro"), "got:\n{rust}");
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
fn translates_scalar_union_to_enum() {
    // `string | number | boolean | undefined` — the XML-attribute / JSON-value
    // shape — becomes tuple+unit variants wrapping the scalar Rust types.
    let src = "type AttrVal = string | number | boolean | undefined;";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("enum AttrVal"), "got:\n{rust}");
    assert!(rust.contains("Str(String)"), "got:\n{rust}");
    assert!(rust.contains("Num(f64)"), "got:\n{rust}");
    assert!(rust.contains("Bool(bool)"), "got:\n{rust}");
    assert!(rust.contains("Undef"), "got:\n{rust}");
}

#[test]
fn translates_mixed_union_scalar_and_array_to_enum() {
    // `boolean | string[]` — the `alwaysArray` shape — mixes a scalar and an
    // array member; each lowers to its own variant (ts2rust tagged-union model).
    let src = "type AlwaysArray = boolean | string[];";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("enum AlwaysArray"), "got:\n{rust}");
    assert!(rust.contains("Bool(bool)"), "got:\n{rust}");
    assert!(rust.contains("ArrayOfStr(Vec<String>)"), "got:\n{rust}");
}

#[test]
fn translates_mixed_union_with_inline_object_to_enum() {
    // `boolean | { encoding?: string }` — a scalar plus an inline-object
    // member; the object becomes a tuple variant wrapping a `__DsAnon_<hash>`
    // struct (the duplicate pattern), emitted before the enum.
    let src = "type Decl = boolean | { encoding?: string; standalone?: string };";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("enum Decl"), "got:\n{rust}");
    assert!(rust.contains("Bool(bool)"), "got:\n{rust}");
    assert!(
        rust.contains("__DsAnon_"),
        "anon struct emitted, got:\n{rust}"
    );
    assert!(
        rust.contains("EncodingStandalone("),
        "tuple variant, got:\n{rust}"
    );
}

#[test]
fn translates_tuple_to_rust_tuple() {
    let src = "type Pair = [number, string];";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("type Pair = (f64, String)"), "got:\n{rust}");
}

#[test]
fn translates_complex_mixed_union_with_map_and_array_members() {
    // A complex mixed-union shape: inline-object members, a type ref,
    // and an array-of-type-ref — a real library union. Every member
    // lowers to its own variant; objects without a discriminant become tuple
    // variants wrapping a `__DsAnon_<hash>` struct.
    let src = r#"
        type AttrMap = { [key: string]: string | number };
        type Atom = string | number | boolean | null;
        type ArrayMember = { [index: number]: { _meta: AttrMap } };
        export type Node =
          | { _meta: AttrMap }
          | { _text: string }
          | { _meta: AttrMap; _text: string }
          | Atom
          | Atom[]
          | ArrayMember;
    "#;
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("enum Node"),
        "Node should be enum, got:\n{rust}"
    );
    assert!(rust.contains("Meta("), "got:\n{rust}");
    assert!(rust.contains("Text("), "got:\n{rust}");
    assert!(rust.contains("Atom(Atom)"), "got:\n{rust}");
    assert!(rust.contains("ArrayOfAtom"), "got:\n{rust}");
}

#[test]
fn translates_tuple_rest_to_head_and_vec_tail() {
    // `[T, ...T[]]` (NonEmptyArray) → `(T, Vec<T>)` — a fixed head then a Vec tail.
    let src = "type NonEmpty = [number, ...number[]];";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("type NonEmpty = (f64, Vec<f64>)"),
        "got:\n{rust}"
    );
}

#[test]
fn translates_tuple_rest_readonly_array_reference() {
    // `...ReadonlyArray<T>` spells the rest array shape the reference way; the
    // element unwraps the same as `...T[]`.
    let src = "type NonEmpty = [number, ...ReadonlyArray<number>];";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("type NonEmpty = (f64, Vec<f64>)"),
        "got:\n{rust}"
    );
}

#[test]
fn translates_function_type_to_fn_pointer() {
    // `(a: number, b: string) => boolean` → `fn(f64, String) -> bool` (a fn pointer).
    let src = "type Pred = (a: number, b: string) => boolean;";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("type Pred = fn(f64, String) -> bool"),
        "got:\n{rust}"
    );
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
fn translates_record_union_member_to_hashmap_variant() {
    // A `Record<K, V>` union member lowers to `Record(HashMap<K, V>)` — the
    // type arguments resolve, rather than emitting the bare `Record` name.
    let src = "type Bag = string | Record<string, number>;";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("Record(::std::collections::HashMap<String, f64>)"),
        "got:\n{rust}"
    );
    assert!(!rust.contains("Record(Record)"), "got:\n{rust}");
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
fn boxes_record_literal_values_into_union_enum() {
    // A top-level `Record<K, scalar-union>` literal (the implicit-`main` shape a
    // script uses) boxes each value into the matching variant of the generated
    // enum, so the map matches a `HashMap<K, Enum>` parameter type. Mirrors the
    // `attrs({ id, name, hidden })` XML-attribute shape that motivated scalar
    // unions.
    let src = "const r: Record<string, string | number | boolean | undefined> = { id: 1, name: \"foo\", ok: true, hidden: undefined };";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("::Num(1_f64)"),
        "number value boxes into Num: got:\n{rust}"
    );
    assert!(
        rust.contains("::Str(\"foo\".to_string())"),
        "string value boxes into Str: got:\n{rust}"
    );
    assert!(
        rust.contains("::Bool(true)"),
        "boolean value boxes into Bool: got:\n{rust}"
    );
    assert!(
        rust.contains("::Undef"),
        "undefined value boxes into Undef: got:\n{rust}"
    );
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
fn translates_object_freeze_degrades_to_engine() {
    // Object.freeze/seal/preventExtensions mutate [[Extensible]] state — a
    // Record carries no runtime freeze flag, so the static no-op emit would
    // mis-report `isExtensible`. The function degrades to the engine, whose
    // `call_fn` stub keeps the JS body (Object.freeze verbatim) for QuickJS to
    // run with real ES extensibility tracking.
    let src = "function f(m: Record<string, number>): Record<string, number> { Object.freeze(m); Object.seal(m); return Object.preventExtensions(m); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("call_fn"), "degrades to engine: {rust}");
    assert!(rust.contains("Object.freeze"), "JS body verbatim: {rust}");
}

#[test]
fn translates_object_isfrozen_degrades_to_engine() {
    // Object.isFrozen/isSealed/isExtensible query [[Extensible]] state — the
    // same untracked state as freeze, so the function degrades to the engine
    // rather than emit the hardcoded false/true a freeze-then-query fixture
    // would mis-report.
    let src = "function f(m: Record<string, number>): boolean { return Object.isFrozen(m) && Object.isSealed(m) && Object.isExtensible(m); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("call_fn"), "degrades to engine: {rust}");
    assert!(rust.contains("Object.isFrozen"), "JS body verbatim: {rust}");
}

#[test]
fn translates_generic_type_alias_keeps_param() {
    // `type NonEmptyArray<T> = [T, ...T[]]` → `type NonEmptyArray<T> = (T,
    // Vec<T>)` — the `<T>` is kept so the body's `T` resolves instead of
    // dangling (E0425). A non-generic alias is unchanged.
    let src = "type NonEmptyArray<T> = [T, ...T[]];";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("type NonEmptyArray<T> = (T, Vec<T>)"),
        "generic param lost: {rust}"
    );
}

#[test]
fn resolves_return_type_typeof_query_in_param() {
    // `ReturnType<typeof fn>` (a TS utility type) resolves to the named
    // function's declared return type, so a parameter typed with it gets the
    // function's return shape rather than `_` (which would surface as E0308).
    let src = "\
interface Options { indent: number; }
function normalizeOptions(o: Options): Options { return o; }
function writeElement(opts: ReturnType<typeof normalizeOptions>): void {}
";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("fn write_element(opts: Options)"),
        "ReturnType<typeof fn> not resolved: {rust}"
    );
}

#[test]
fn unmappable_field_type_becomes_serde_value_in_struct() {
    // A struct field whose TS type has no Rust lowering (`unknown`) must not
    // emit `_` (E0121 in a signature) — the data-position overlay replaces it
    // with the universal marshal type, preserving the struct.
    let src = "interface O { x: unknown; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("pub x: ::serde_json::Value"), "got:\n{rust}");
    assert!(!rust.contains("pub x: _"), "got:\n{rust}");
}

#[test]
fn unmappable_generic_alias_drops_unused_param() {
    // A conditional type alias lowers to serde_json::Value; its generic param
    // is then unused and must be dropped (E0392), not carried as `<T>`.
    let src = "type NonNullable<T> = T extends null | undefined ? never : T;";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("type NonNullable = ::serde_json::Value"),
        "got:\n{rust}"
    );
    assert!(!rust.contains("NonNullable<"), "got:\n{rust}");
}

#[test]
fn record_of_unknown_member_preserves_structure() {
    // `Record<string, unknown>` as a union member keeps the HashMap wrapper and
    // replaces only the unmappable value leaf — `HashMap<String, Value>`, not a
    // flat `Value` and not `HashMap<String, _>`.
    let src = "type R = Record<string, unknown> | string;";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("HashMap<String, ::serde_json::Value>"),
        "got:\n{rust}"
    );
    assert!(!rust.contains("HashMap<String, _>"), "got:\n{rust}");
}
