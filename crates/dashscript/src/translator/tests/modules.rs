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
fn import_bare_crate_emits_use() {
    // A bare specifier is a crate added via `ds add`; it lowers to a `use`.
    let rust = Translator::new()
        .translate("import { foo } from \"serde\";")
        .expect("should translate");
    assert!(rust.contains("use serde::foo"), "got: {rust}");
}

#[test]
fn import_bare_crate_hyphen_to_underscore() {
    // A crate name may contain a hyphen, but a `use` path / module ident may
    // not — `cfg-if` becomes `cfg_if`.
    let rust = Translator::new()
        .translate("import { x } from \"cfg-if\";")
        .expect("should translate");
    assert!(rust.contains("use cfg_if::x"), "got: {rust}");
    assert!(!rust.contains("cfg-if"), "hyphen leaked: {rust}");
}

#[test]
fn collect_skips_bare_crate_import() {
    // A bare specifier is a crate, not a local `.ds` file — it must not be
    // collected for module assembly (only relative imports are).
    let imports = Translator::new().imports("import { foo } from \"serde\";");
    assert!(imports.is_empty(), "bare import collected: {imports:?}");
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
        .translate("import foo from \"serde\";")
        .expect("should translate");
    assert!(rust.contains("use serde::foo"), "got: {rust}");
}

#[test]
fn import_default_type_keeps_pascalcase() {
    // A default import naming a type keeps PascalCase, like a named type import.
    let rust = Translator::new()
        .translate("import Foo from \"serde\";")
        .expect("should translate");
    assert!(rust.contains("use serde::Foo"), "got: {rust}");
}
