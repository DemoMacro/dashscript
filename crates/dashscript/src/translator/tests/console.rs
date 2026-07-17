use super::super::Translator;

#[test]
fn translates_multi_arg_console_log() {
    let src = "function f(): void { console.log(\"x\", 1, true); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("\"x {} {}\""), "got:\n{rust}");
    assert!(!rust.contains("todo!"), "got:\n{rust}");
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
