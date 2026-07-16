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
fn import_bare_specifier_is_dropped() {
    // Bare (crate) specifiers are not local modules — not mapped by the
    // translator yet (crate imports arrive through `ds add` + manifest.json).
    let rust = Translator::new()
        .translate("import { foo } from \"react\";")
        .expect("should translate");
    assert!(!rust.contains("use react"), "got: {rust}");
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
