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
