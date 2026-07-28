//! TypeScript `enum` → Rust `mod` of typed `const` members.

use super::super::Translator;

#[test]
fn translates_numeric_enum_to_mod_of_consts() {
    let src = "enum Color { Red, Green, Blue } function f(): void { console.log(Color.Red); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("mod Color"), "got:\n{rust}");
    assert!(rust.contains("pub const Red: i64 = 0"), "got:\n{rust}");
    assert!(rust.contains("pub const Green: i64 = 1"), "got:\n{rust}");
    assert!(rust.contains("pub const Blue: i64 = 2"), "got:\n{rust}");
    assert!(rust.contains("Color::Red"), "got:\n{rust}");
}

#[test]
fn numeric_enum_auto_increments_from_explicit_value() {
    let src = "enum E { A = 10, B, C = 20 }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("pub const A: i64 = 10"), "got:\n{rust}");
    assert!(rust.contains("pub const B: i64 = 11"), "got:\n{rust}");
    assert!(rust.contains("pub const C: i64 = 20"), "got:\n{rust}");
}

#[test]
fn translates_string_enum_to_str_consts() {
    let src = "enum Status { Active = \"active\", Inactive = \"inactive\" }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("pub const Active: &'static str = \"active\""),
        "got:\n{rust}"
    );
    assert!(
        rust.contains("pub const Inactive: &'static str = \"inactive\""),
        "got:\n{rust}"
    );
}

#[test]
fn enum_member_access_in_expression() {
    let src = "enum Dir { North, South } function f(): number { return Dir.North + Dir.South; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("Dir::North"), "got:\n{rust}");
    assert!(rust.contains("Dir::South"), "got:\n{rust}");
}

#[test]
fn enum_with_non_literal_initializer_emits_nothing() {
    // `1 << 2` is not a literal — DashScript does not constant-evaluate (oxc
    // pre-computes these in `Scoping`), so the enum stays untranslated and
    // `check` flags it unsupported.
    let src = "enum Flags { A = 1 << 2 }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(!rust.contains("mod Flags"), "got:\n{rust}");
}

#[test]
fn exported_enum_emits_pub_mod() {
    let src = "export enum Color { Red, Green }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("pub mod Color"), "got:\n{rust}");
}
