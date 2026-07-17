// Class / `new` / `this` translation.
use super::super::Translator;

#[test]
fn this_outside_method_is_compile_error() {
    // `this` has no receiver at module scope or in a free function, so it lowers
    // to a `compile_error!` (the generated Rust still parses; it fails loudly).
    let src = "function f() { return this; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("compile_error"),
        "this outside method: {rust}"
    );
}
