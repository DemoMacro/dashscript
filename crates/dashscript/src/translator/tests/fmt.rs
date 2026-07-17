// End-to-end tests for `Translator::format` (oxc_codegen pretty-print).
use super::super::Translator;

#[test]
fn format_pretty_prints_a_function() {
    let src = "function f(x:number):number{return x+1;}";
    let out = Translator::new().format(src).expect("should format");
    assert!(
        out.contains("function f(x: number): number {"),
        "got: {out}"
    );
    assert!(out.contains("return x + 1;"));
}

#[test]
fn format_rejects_a_syntax_error() {
    let src = "function broken(x number) { return x; }";
    assert!(Translator::new().format(src).is_err());
}
