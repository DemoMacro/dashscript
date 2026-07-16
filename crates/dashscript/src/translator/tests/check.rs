// End-to-end tests for `Translator::check` — the translatability layer.
use super::super::Translator;

#[test]
fn check_passes_a_translatable_file() {
    let src = "function f(x: number): number { return x + 1; }";
    assert!(Translator::new().check(src).is_empty());
}

#[test]
fn check_flags_unsupported_class() {
    let diags = Translator::new().check("class C { x: number; }");
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("classes"));
}

#[test]
fn check_flags_unsupported_import() {
    let diags = Translator::new().check("import { x } from \"m\";");
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("import"));
}

#[test]
fn check_flags_a_syntax_error() {
    // Missing `:` — oxc_parser surfaces a syntax diagnostic.
    let diags = Translator::new().check("function f(x number) { return x; }");
    assert!(!diags.is_empty());
}
