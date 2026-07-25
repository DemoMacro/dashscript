// End-to-end tests for module declarations (export/import).
use super::super::Translator;

#[test]
fn exports_function_as_pub() {
    let rust = Translator::new()
        .translate("export function foo(): number { return 1; }")
        .expect("should translate");
    assert!(rust.contains("pub fn foo"), "got: {rust}");
}

#[test]
fn exports_interface_as_pub_struct() {
    let rust = Translator::new()
        .translate("export interface P { x: number; }")
        .expect("should translate");
    assert!(rust.contains("pub struct P"), "got: {rust}");
}

#[test]
fn exports_type_alias_as_pub_type() {
    let rust = Translator::new()
        .translate("export type Id = number;")
        .expect("should translate");
    assert!(rust.contains("pub type Id"), "got: {rust}");
}

#[test]
fn import_emits_use() {
    let rust = Translator::new()
        .translate("import { foo } from \"./other\";")
        .expect("should translate");
    assert!(rust.contains("use other::foo"), "got: {rust}");
}

#[test]
fn import_groups_multiple_names() {
    let rust = Translator::new()
        .translate("import { foo, bar } from \"./other\";")
        .expect("should translate");
    assert!(rust.contains("use other::{foo, bar}"), "got: {rust}");
}

#[test]
fn import_cargo_emits_use() {
    // `cargo:serde` is a Cargo crate (the `cargo:` family marker, aligned
    // with Deno's `npm:`/`jsr:`); it lowers to `use serde::foo`.
    let rust = Translator::new()
        .translate("import { foo } from \"cargo:serde\";")
        .expect("should translate");
    assert!(rust.contains("use serde::foo"), "got: {rust}");
}

#[test]
fn import_cargo_hyphen_to_underscore() {
    // A crate name may contain a hyphen, but a `use` path / module ident may
    // not — `cargo:cfg-if` becomes `use cfg_if::x`.
    let rust = Translator::new()
        .translate("import { x } from \"cargo:cfg-if\";")
        .expect("should translate");
    assert!(rust.contains("use cfg_if::x"), "got: {rust}");
    assert!(!rust.contains("cfg-if"), "hyphen leaked: {rust}");
}

#[test]
fn collect_skips_cargo_import() {
    // A `cargo:` import is a crate, not a local `.ts` file — it must not be
    // collected for module assembly (only relative imports are).
    let imports = Translator::new().imports("import { foo } from \"cargo:serde\";");
    assert!(imports.is_empty(), "cargo import collected: {imports:?}");
}

#[test]
fn bare_import_is_unsupported() {
    // A bare specifier (`lodash`) has no resolver — DashScript supports only
    // `cargo:` and relative imports. The translator emits no `use`, and
    // `check` flags it as unsupported.
    let rust = Translator::new()
        .translate("import { x } from \"lodash\";")
        .expect("should translate");
    assert!(
        !rust.contains("use lodash"),
        "bare import emitted a use: {rust}"
    );
    let diags = Translator::new().check("import { x } from \"lodash\";");
    assert!(!diags.is_empty(), "bare import not flagged by check");
}

#[test]
fn collect_local_imports() {
    let imports = Translator::new().imports("import { foo, bar } from \"./other\";");
    assert_eq!(imports.len(), 1);
    assert_eq!(imports[0].module, "other");
    assert_eq!(imports[0].source, "./other");
}

#[test]
fn import_keeps_type_name_pascalcase() {
    // A type binding (uppercase) is kept as-is so it matches the PascalCase
    // struct/type the module exports; a value binding is snake_cased.
    let rust = Translator::new()
        .translate("import { add, Point } from \"./other\";")
        .expect("should translate");
    assert!(rust.contains("use other::{add, Point}"), "got: {rust}");
}

#[test]
fn import_default_value_emits_use() {
    // A default import (`import foo`) lowers like a named one: Rust crates have
    // no default export, so the local name names the crate item directly.
    let rust = Translator::new()
        .translate("import foo from \"cargo:serde\";")
        .expect("should translate");
    assert!(rust.contains("use serde::foo"), "got: {rust}");
}

#[test]
fn import_default_type_keeps_pascalcase() {
    // A default import naming a type keeps PascalCase, like a named type import.
    let rust = Translator::new()
        .translate("import Foo from \"cargo:serde\";")
        .expect("should translate");
    assert!(rust.contains("use serde::Foo"), "got: {rust}");
}

#[test]
fn declarations_list_local_bindings() {
    let decls = Translator::new().declarations(
        "function foo() {}\ninterface Bar {}\ntype Baz = number\nimport { qux } from \"./other\";",
    );
    let names: Vec<&str> = decls.iter().map(|d| d.name.as_str()).collect();
    assert!(names.contains(&"foo"), "missing foo: {names:?}");
    assert!(names.contains(&"Bar"), "missing Bar: {names:?}");
    assert!(names.contains(&"Baz"), "missing Baz: {names:?}");
    assert!(names.contains(&"qux"), "missing qux: {names:?}");
}

#[test]
fn has_main_detects_function_main() {
    assert!(Translator::new().has_main("function main() { console.log(1); }"));
}

#[test]
fn has_main_detects_export_function_main() {
    assert!(Translator::new().has_main("export function main(): number { return 0; }"));
}

#[test]
fn has_main_false_when_absent() {
    assert!(!Translator::new().has_main("function helper() {}"));
}

#[test]
fn has_main_ignores_main_loop_helper() {
    // `main_loop` is a common helper name; a substring scan of `fn main` in the
    // translated Rust would trip on it. AST-level matching only counts an
    // identifier literally named `main`.
    assert!(!Translator::new().has_main("function main_loop() {}"));
}

#[test]
fn has_main_ignores_string_literal() {
    // A `"fn main"` string literal must not count as declaring `main` — the
    // reason `has_main` walks the AST rather than scanning the source text.
    assert!(!Translator::new().has_main("const s = \"fn main\";"));
}

#[test]
fn top_level_function_main_renames_to_ds_main() {
    // Pure-TS execution semantics: `function main` is an ordinary declaration,
    // not the cargo entry. Rename the root scope's `main` to `__ds_main` so the
    // implicit `fn main` the translator emits cannot collide with it. The
    // rename lives in `NameTable`, so every call site follows automatically.
    let rust = Translator::new()
        .translate("function main(): void { console.log(1); }")
        .expect("should translate");
    assert!(rust.contains("fn __ds_main"), "got: {rust}");
}
