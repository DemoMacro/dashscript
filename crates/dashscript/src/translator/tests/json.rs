use super::super::Translator;

#[test]
fn translates_json_parse_to_serde_json_from_str() {
    // `JSON.parse(s)` inlines `serde_json::from_str::<Value>` (no `__ds`
    // helper); a malformed string falls back to `Value::Null` (ES would throw
    // `SyntaxError` — a per-call `catch` is out of scope for the inline path).
    let src = "function f(s: string): void { const v = JSON.parse(s); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("serde_json::from_str::<serde_json::Value>"),
        "parse: {rust}"
    );
    assert!(
        rust.contains("serde_json::Value::Null"),
        "null fallback: {rust}"
    );
}

#[test]
fn translates_json_stringify_to_serde_json_to_string() {
    // `JSON.stringify(x)` inlines `serde_json::to_string`; a serialize error
    // (a non-`Serialize` receiver) falls back to `"null"`, matching ES's
    // `undefined`/unserializable result.
    let src = "function f(v: number): string { return JSON.stringify(v); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("serde_json::to_string"), "stringify: {rust}");
    assert!(rust.contains("\"null\""), "null fallback: {rust}");
}

#[test]
fn json_calls_flag_needs_serde_json_not_ryu_js() {
    // The emitted `serde_json::` prefix flags `needs_serde_json`; JSON alone
    // pulls in no `ryu_js` (no `__ds` helper module — the calls are direct).
    let src = "function f(s: string): void { console.log(JSON.stringify(JSON.parse(s))); }";
    let (_rust, deps) = Translator::new()
        .translate_with_deps(src)
        .expect("translate_with_deps");
    assert!(
        deps.needs_serde_json,
        "serde_json dep must flag, got: {deps:?}"
    );
    assert!(!deps.needs_ryu_js, "JSON alone pulls no ryu_js: {deps:?}");
}
