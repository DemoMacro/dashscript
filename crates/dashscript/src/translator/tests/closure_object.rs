// Closure-object factory (e.g. `bidi`) → `struct` + `impl` + factory `fn`.

use super::super::Translator;

#[test]
fn translates_bidi_factory_to_struct_impl() {
    let src = r#"
function bidi<K extends string, V extends string>(forward: Record<K, V>) {
  return {
    to: (key: string): string => (forward as Record<string, string>)[key] ?? key,
    forward,
  };
}
"#;
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("struct __DsBidi"),
        "struct synthesized: {rust}"
    );
    assert!(
        rust.contains("pub forward: ::std::collections::HashMap<String, String>"),
        "generic param specialized to HashMap<String, String>: {rust}"
    );
    assert!(
        rust.contains("fn bidi(forward: ::std::collections::HashMap<String, String>) -> __DsBidi"),
        "factory fn returns the struct: {rust}"
    );
    assert!(rust.contains("impl __DsBidi"), "impl synthesized: {rust}");
    assert!(
        rust.contains("fn to(&self"),
        "arrow becomes &self method: {rust}"
    );
    assert!(
        rust.contains("forward.get(") && !rust.contains("forward["),
        "captured field indexes via HashMap .get, not array index: {rust}"
    );
}

#[test]
fn does_not_recognize_plain_record_return() {
    // Returns a plain Record (no closure methods) — not a closure-object
    // factory, so it lowers as an ordinary function (no synthesized struct).
    let src = "function f(m: Record<string, string>): Record<string, string> { return m; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        !rust.contains("__DsF"),
        "plain record is not a factory: {rust}"
    );
}

#[test]
fn does_not_recognize_untyped_arrow_param() {
    // An untyped arrow param `(key) => …` leaves the method param type
    // indeterminate (E0121) — the conservative shape match rejects it.
    let src = r#"
function f(forward: Record<string, string>) {
  return { to: (key) => key, forward };
}
"#;
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        !rust.contains("__DsF"),
        "untyped arrow param rejected: {rust}"
    );
}
