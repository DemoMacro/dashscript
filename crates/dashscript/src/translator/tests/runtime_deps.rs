//! `translate_with_deps` returns the same Rust as `translate`, plus a
//! runtime-dependency report. A source with no number→string formatting keeps
//! an empty dep set, so `ds build` links nothing extra.
use super::super::{RuntimeDep, RuntimeDeps, Translator};

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
        !deps.needs_ryu_js(),
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
        deps.needs_ryu_js(),
        "needs_ryu_js must flag for a numeric console.log, got deps: {deps:?}"
    );
}

#[test]
fn numeric_local_and_unary_route_through_helper() {
    // A `number` local inferred from its initializer, and a unary `-0`, route
    // through the helper — not just literals. `-0` must print "0" in ES, where
    // Rust's `Display` would print "-0".
    let src = "function main(): void { const x = 1e21; const z = -0; console.log(x, z); }";
    let (rust, deps) = Translator::new()
        .translate_with_deps(src)
        .expect("translate_with_deps");
    assert!(
        rust.contains("__ds::number_to_string"),
        "numeric local/unary should route through the helper, got:\n{rust}"
    );
    assert!(
        deps.needs_ryu_js(),
        "needs_ryu_js must flag, got deps: {deps:?}"
    );
}

#[test]
fn helper_module_present_only_when_needed() {
    // A ryu_js-flagged dep set exposes the `__ds` helper module; a plain one does not.
    let with = RuntimeDeps::empty().with(RuntimeDep::RyuJs);
    let without = RuntimeDeps::empty();
    assert!(
        with.helper_module()
            .is_some_and(|s| s.contains("number_to_string")),
        "ryu_js dep exposes the helper"
    );
    assert!(without.helper_module().is_none(), "no dep → no helper");
}

#[test]
fn array_helper_module_exposes_array_set_without_ryu_js() {
    // `ArrayHelper` alone exposes `array_set` but pulls no `ryu_js` (the helper
    // module is assembled from whichever slices a dep set flagged, not a single
    // blob) — so a `.ds` source that only does `xs[i] = v` links no number-
    // formatting crate.
    let deps = RuntimeDeps::empty().with(RuntimeDep::ArrayHelper);
    let helper = deps.helper_module().expect("array flag exposes helper");
    assert!(helper.contains("pub fn array_set"), "got:\n{helper}");
    assert!(
        !helper.contains("ryu_js"),
        "no ryu_js slice: got:\n{helper}"
    );
}

#[test]
fn apply_to_cargo_toml_inserts_into_dependencies_section() {
    let mut toml = String::from("[package]\nname = \"x\"\n\n[dependencies]\nserde = \"1.0\"\n");
    let deps = RuntimeDeps::empty().with(RuntimeDep::RyuJs);
    deps.apply_to_cargo_toml(&mut toml);
    assert!(toml.contains("ryu-js = \"1.0\""), "got:\n{toml}");
    // Idempotent: a second pass must not duplicate the line.
    deps.apply_to_cargo_toml(&mut toml);
    assert_eq!(toml.matches("ryu-js").count(), 1, "got:\n{toml}");
}

#[test]
fn apply_to_cargo_toml_creates_section_when_absent() {
    let mut toml = String::from("[package]\nname = \"x\"\n");
    let deps = RuntimeDeps::empty().with(RuntimeDep::RyuJs);
    deps.apply_to_cargo_toml(&mut toml);
    assert!(
        toml.contains("[dependencies]\nryu-js = \"1.0\""),
        "got:\n{toml}"
    );
}

#[test]
fn apply_to_cargo_toml_noop_when_not_needed() {
    // A file with no number→string emit point must not pull ryu_js into Cargo.toml.
    let mut toml = String::from("[package]\nname = \"x\"\n");
    let deps = RuntimeDeps::empty();
    deps.apply_to_cargo_toml(&mut toml);
    assert!(!toml.contains("ryu-js"), "got:\n{toml}");
}

#[test]
fn dynamic_reflection_routes_through_engine() {
    // `Object.defineProperty` is ES reflection the static translator cannot
    // lower; the whole program runs under the embedded QuickJS engine instead.
    // The body is never lowered, so an anonymous `{}` receiver is fine — the
    // engine path short-circuits before `translate_statement`.
    let src = "function main(): void {\n  const o = {};\n  Object.defineProperty(o, \"x\", { value: 1 });\n  console.log(o.x);\n}";
    let (rust, deps) = Translator::new()
        .translate_with_deps(src)
        .expect("translate_with_deps");
    assert!(
        deps.needs_engine(),
        "defineProperty should flip needs_engine, got deps: {deps:?}"
    );
    assert!(
        rust.contains("__ds_engine::run"),
        "engine fixture should lower to __ds_engine::run, got:\n{rust}"
    );
    assert!(
        !deps.needs_ryu_js(),
        "engine path emits no __ds::number_to_string"
    );
}

#[test]
fn plain_source_stays_on_static_rust_path() {
    // No reflection → the static Rust lowering; no engine dep.
    let src = "function main(): void { console.log(1 + 2); }";
    let (rust, deps) = Translator::new()
        .translate_with_deps(src)
        .expect("translate_with_deps");
    assert!(!deps.needs_engine(), "plain source pulls no engine");
    assert!(
        !rust.contains("__ds_engine::run"),
        "plain source must not lower to engine, got:\n{rust}"
    );
}

#[test]
fn regex_literal_test_flags_regress_dep() {
    // `/pat/i.test(s)` lowers to a `regress::Regex` (not the engine), so the
    // file flags `needs_regress` and emits `__ds::regex` — no rquickjs.
    let src = "function main(): void {\n  console.log(/\\d+/i.test(\"abc123\"));\n}";
    let (rust, deps) = Translator::new()
        .translate_with_deps(src)
        .expect("translate_with_deps");
    assert!(
        deps.needs_regress(),
        "regex literal flags needs_regress, got deps: {deps:?}"
    );
    assert!(
        !deps.needs_engine(),
        "regex literal must not pull the engine, got deps: {deps:?}"
    );
    assert!(
        rust.contains("__ds::regex"),
        "regex literal emits __ds::regex, got:\n{rust}"
    );
}

#[test]
fn regex_local_test_uses_regress() {
    // `let r = /pat/; r.test(s)` — the local infers `regress::Regex`, so
    // `.test` dispatches to the regress `find` method, not the engine.
    let src = "function main(): void {\n  const r = /[a-z]+/g;\n  console.log(r.test(\"hi\"));\n}";
    let (rust, deps) = Translator::new()
        .translate_with_deps(src)
        .expect("translate_with_deps");
    assert!(deps.needs_regress(), "regex local flags needs_regress");
    assert!(
        rust.contains(".find("),
        "regex local .test lowers to .find, got:\n{rust}"
    );
}

#[test]
fn match_emits_ds_match_accessor() {
    // `const m = s.match(/pat/); m[0]; m.index` — the local infers
    // `Option<DsMatch>`, so `m[0]` lowers to the captures accessor and
    // `m.index` to the field (not `Option::len` / `Option::Index`).
    let src = "function main(): void {\n  const m = \"hello world\".match(/(\\w+) (\\w+)/);\n  console.log(m[0]);\n  console.log(m.index);\n  console.log(m.input);\n  console.log(m.length);\n}";
    let (rust, deps) = Translator::new()
        .translate_with_deps(src)
        .expect("translate_with_deps");
    assert!(deps.needs_regress(), "match flags needs_regress");
    assert!(
        rust.contains("regex_match"),
        "match emits regex_match, got:\n{rust}"
    );
    assert!(
        rust.contains("DsMatch"),
        "match records DsMatch type, got:\n{rust}"
    );
    assert!(
        rust.contains(".captures."),
        "m[i]/m.length route through captures, got:\n{rust}"
    );
}

#[test]
fn exec_emits_ds_match_accessor() {
    // `/pat/.exec(s)` mirrors `s.match(/pat/)` — the receiver is the regex,
    // the arg is the string. Lowers to `regex_match` and infers DsMatch.
    let src = "function main(): void {\n  const m = /(\\w+) (\\w+)/.exec(\"hello world\");\n  console.log(m[0]);\n  console.log(m.index);\n}";
    let (rust, deps) = Translator::new()
        .translate_with_deps(src)
        .expect("translate_with_deps");
    assert!(deps.needs_regress(), "exec flags needs_regress");
    assert!(
        rust.contains("regex_match"),
        "exec emits regex_match, got:\n{rust}"
    );
    assert!(
        rust.contains("DsMatch"),
        "exec records DsMatch type, got:\n{rust}"
    );
}

#[test]
fn search_emits_regex_search() {
    // `s.search(/pat/)` → the byte index of the first match, or -1.
    let src = "function main(): void {\n  console.log(\"hello world\".search(/world/));\n  console.log(\"hello\".search(/xyz/));\n}";
    let (rust, deps) = Translator::new()
        .translate_with_deps(src)
        .expect("translate_with_deps");
    assert!(deps.needs_regress(), "search flags needs_regress");
    assert!(
        rust.contains("regex_search"),
        "search emits regex_search, got:\n{rust}"
    );
}
