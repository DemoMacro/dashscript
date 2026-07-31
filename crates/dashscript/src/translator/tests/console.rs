use super::super::Translator;

#[test]
fn translates_multi_arg_console_log() {
    let src = "function f(): void { console.log(\"x\", 1, true); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("\"x {} {}\""), "got:\n{rust}");
    assert!(!rust.contains("todo!"), "got:\n{rust}");
}

#[test]
fn console_log_container_routes_through_inspect() {
    // A `console.log` of a `Vec`/`HashMap` (no Rust `Display`) routes through
    // `__ds::inspect` (Node's console.log inspect format) instead of `{}`
    // Display, which would not compile. Regression for the CONTRIBUTING.md
    // "never a bare Vec/struct" limit being lifted.
    let src = "function f(): void {\n  const xs: string[] = [\"a\", \"b\"];\n  console.log(xs);\n}";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("__ds::inspect"),
        "container console.log should inspect: {rust}"
    );
}

#[test]
fn console_log_primitive_identifier_stays_display() {
    // A `console.log` of a scalar identifier (`number`/`string`/`boolean`)
    // keeps the `{}` Display path — no inspect needed, matches Node verbatim.
    let src = "function f(): void {\n  const n: number = 3;\n  console.log(n);\n}";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        !rust.contains("__ds::inspect"),
        "scalar console.log should not inspect: {rust}"
    );
}

#[test]
fn translates_console_warn_to_eprintln() {
    let src = "function f(): void { console.warn(\"careful\"); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("eprintln!("), "got:\n{rust}");
    assert!(
        rust.contains("\"careful\"") && !rust.contains("to_string()"),
        "literal folds into format string, got:\n{rust}"
    );
}
