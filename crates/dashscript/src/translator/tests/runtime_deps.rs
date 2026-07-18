//! `translate_with_deps` returns the same Rust as `translate`, plus a
//! runtime-dependency report. A source with no number→string formatting keeps
//! an empty dep set, so `ds build` links nothing extra.
use super::super::Translator;

#[test]
fn with_deps_matches_translate() {
    // A string-only source never formats an f64, so it pulls in no `ryu_js`.
    let src = "function main(): void { console.log(\"hi\"); }";
    let plain = Translator::new().translate(src).expect("translate");
    let (with_deps, deps) = Translator::new()
        .translate_with_deps(src)
        .expect("translate_with_deps");
    assert_eq!(plain, with_deps);
    assert!(
        !deps.needs_ryu_js,
        "a string-only source pulls in no ryu_js"
    );
}

#[test]
fn numeric_console_log_routes_through_helper_and_flags_dep() {
    // `console.log(1e21)` must route the literal through `__ds::number_to_string`
    // (ryu_js), not Rust's `f64` `Display`, and flag the file as needing `ryu_js`.
    let src = "function main(): void { console.log(1e21); }";
    let (rust, deps) = Translator::new()
        .translate_with_deps(src)
        .expect("translate_with_deps");
    assert!(
        rust.contains("__ds::number_to_string"),
        "numeric literal should route through the helper, got:\n{rust}"
    );
    assert!(
        deps.needs_ryu_js,
        "needs_ryu_js must flag for a numeric console.log, got deps: {deps:?}"
    );
}
