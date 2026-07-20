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
fn regex_exec_in_loop_routes_to_engine() {
    // `re.exec(s)` inside a loop body — regress is stateless, so the loop
    // would re-find the same match every iteration (an infinite loop). The
    // engine (rquickjs) advances `lastIndex` like ES, so a looped exec routes
    // there rather than hanging on the regress path.
    let src = "function main(): void {\n  const re = /a/g;\n  const s = \"banana\";\n  var n = 0;\n  do {\n    const m = re.exec(s);\n    if (m !== null) { n = n + 1; } else { break; }\n  } while (1);\n  console.log(n);\n}";
    let (_rust, deps) = Translator::new()
        .translate_with_deps(src)
        .expect("translate_with_deps");
    assert!(
        deps.needs_engine(),
        "looped .exec should flip needs_engine, got deps: {deps:?}"
    );
}

#[test]
fn regex_exec_in_loop_condition_routes_to_engine() {
    // `re.exec(s)` in the loop *condition* (`while (re.exec(s) !== null)`) is
    // looped just like one in the body — regress would re-find the same match
    // every test (an infinite loop). The condition is walked with IN_LOOP set
    // so it routes to the engine too.
    let src = "function main(): void {\n  const re = /\\w/g;\n  const s = \"abc\";\n  var k = 0;\n  while (re.exec(s) !== null) { k = k + 1; }\n  console.log(k);\n}";
    let (_rust, deps) = Translator::new()
        .translate_with_deps(src)
        .expect("translate_with_deps");
    assert!(
        deps.needs_engine(),
        "looped .exec in the condition should flip needs_engine, got deps: {deps:?}"
    );
}

#[test]
fn regex_exec_once_outside_loop_stays_on_regress() {
    // `/pat/.exec(s)` once, outside any loop, is a single `find` — regress
    // handles it, so the engine dep must not flip.
    let src =
        "function main(): void {\n  const m = /a/.exec(\"abc\");\n  console.log(m !== null);\n}";
    let (_rust, deps) = Translator::new()
        .translate_with_deps(src)
        .expect("translate_with_deps");
    assert!(
        !deps.needs_engine(),
        "single .exec outside a loop must not pull the engine, got deps: {deps:?}"
    );
}

#[test]
fn array_indexof_non_number_needle_routes_to_engine() {
    // `xs.indexOf(true)` — ES SameValueZero distinguishes `true` from `1`, but
    // DashScript's Vec<f64> search assumes a numeric needle, so a boolean needle
    // would be a type error (E0277/E0308). The fixture routes to the engine,
    // whose element comparison matches ES.
    let src =
        "function main(): void {\n  const xs = [0, 1, 2];\n  console.log(xs.indexOf(true));\n}";
    let (_rust, deps) = Translator::new()
        .translate_with_deps(src)
        .expect("translate_with_deps");
    assert!(
        deps.needs_engine(),
        "indexOf with a non-number needle should flip needs_engine, got deps: {deps:?}"
    );
}

#[test]
fn array_indexof_numeric_needle_stays_mapped() {
    // `.indexOf(<number>)` stays on the mapped Vec<f64> path — no engine dep.
    let src = "function main(): void {\n  const xs = [0, 1, 2];\n  console.log(xs.indexOf(1));\n}";
    let (_rust, deps) = Translator::new()
        .translate_with_deps(src)
        .expect("translate_with_deps");
    assert!(
        !deps.needs_engine(),
        "indexOf with a numeric needle must not pull the engine, got deps: {deps:?}"
    );
}

#[test]
fn regex_lastindex_access_routes_to_engine() {
    // `<re>.lastIndex` read or write — regress is stateless (no lastIndex
    // field → E0609), so route to the engine, whose regex carries the cursor.
    let src = "function main(): void {\n  const re = /a/g;\n  re.lastIndex = 2;\n  console.log(re.lastIndex);\n}";
    let (_rust, deps) = Translator::new()
        .translate_with_deps(src)
        .expect("translate_with_deps");
    assert!(
        deps.needs_engine(),
        "lastIndex access should flip needs_engine, got deps: {deps:?}"
    );
}

#[test]
fn regex_test_nonstring_var_routes_to_engine() {
    // `var x = 1.01; re.test(x)` — ES coerces the number argument via
    // ToString, but regress takes `&str` (the translator emits `x.as_str()`,
    // E0599). Route to the engine, whose ToString matches ES.
    let src = "function main(): void {\n  var x = 1.01;\n  const re = /1/;\n  console.log(re.test(x));\n}";
    let (_rust, deps) = Translator::new()
        .translate_with_deps(src)
        .expect("translate_with_deps");
    assert!(
        deps.needs_engine(),
        ".test on a number-bound var should flip needs_engine, got deps: {deps:?}"
    );
}

#[test]
fn regex_test_nonstring_literal_arg_routes_to_engine() {
    // `re.test(true)` — a non-string literal argument needs ES ToString.
    let src = "function main(): void {\n  const re = /t/;\n  console.log(re.test(true));\n}";
    let (_rust, deps) = Translator::new()
        .translate_with_deps(src)
        .expect("translate_with_deps");
    assert!(
        deps.needs_engine(),
        ".test on a boolean literal should flip needs_engine, got deps: {deps:?}"
    );
}

#[test]
fn regex_test_null_literal_routes_to_engine() {
    // `re.test(null)` — null coerces to "null" via ES ToString (not a string
    // the static `as_str` lowering can produce).
    let src = "function main(): void {\n  const re = /ll/;\n  console.log(re.test(null));\n}";
    let (_rust, deps) = Translator::new()
        .translate_with_deps(src)
        .expect("translate_with_deps");
    assert!(
        deps.needs_engine(),
        ".test on null should flip needs_engine, got deps: {deps:?}"
    );
}

#[test]
fn regex_test_void_zero_routes_to_engine() {
    // `re.test(void 0)` — `void 0` is `undefined` → ToString "undefined".
    let src = "function main(): void {\n  const re = /e/;\n  console.log(re.test(void 0));\n}";
    let (_rust, deps) = Translator::new()
        .translate_with_deps(src)
        .expect("translate_with_deps");
    assert!(
        deps.needs_engine(),
        ".test on `void 0` should flip needs_engine, got deps: {deps:?}"
    );
}

#[test]
fn regex_test_string_var_stays_on_regress() {
    // `var s = "abc"; re.test(s)` — a string-bound variable must NOT route to
    // the engine (regress handles it). Guards against over-broad detection.
    let src = "function main(): void {\n  var s = \"abc\";\n  const re = /a/;\n  console.log(re.test(s));\n}";
    let (_rust, deps) = Translator::new()
        .translate_with_deps(src)
        .expect("translate_with_deps");
    assert!(
        !deps.needs_engine(),
        ".test on a string-bound var must stay on regress, got deps: {deps:?}"
    );
}

#[test]
fn regex_test_func_result_var_stays_on_regress() {
    // `var s = " abc ".trim(); re.test(s)` — a function-call initializer may
    // yield a string, so the var is not flagged; the engine is not pulled
    // (regress lowers `.test`, and the String arg satisfies `as_str`).
    let src = "function main(): void {\n  var s = \" abc \".trim();\n  const re = /a/;\n  console.log(re.test(s));\n}";
    let (_rust, deps) = Translator::new()
        .translate_with_deps(src)
        .expect("translate_with_deps");
    assert!(
        !deps.needs_engine(),
        ".test on a func-result var must stay on regress, got deps: {deps:?}"
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

#[test]
fn replace_regex_emits_regex_replace() {
    // `s.replace(/pat/, repl)` (non-global) — `$` patterns expanded.
    let src = "function main(): void {\n  console.log(\"hello world\".replace(/(\\w+) (\\w+)/, \"$2 $1\"));\n  console.log(\"abc\".replace(/b/, \"[$&]\"));\n}";
    let (rust, deps) = Translator::new()
        .translate_with_deps(src)
        .expect("translate_with_deps");
    assert!(deps.needs_regress(), "replace regex flags needs_regress");
    assert!(
        rust.contains("regex_replace"),
        "replace regex emits regex_replace, got:\n{rust}"
    );
}

#[test]
fn split_regex_emits_regex_split() {
    // `s.split(/pat/[, limit])` → regex_split; a string separator stays on
    // the str `split` path.
    let src = "function main(): void {\n  console.log(\"a1b2c\".split(/\\d/).length);\n  console.log(\"a1b2c\".split(/\\d/, 2).length);\n  console.log(\"a,b\".split(\",\").length);\n}";
    let (rust, deps) = Translator::new()
        .translate_with_deps(src)
        .expect("translate_with_deps");
    assert!(deps.needs_regress(), "split regex flags needs_regress");
    assert!(
        rust.contains("regex_split"),
        "split regex emits regex_split, got:\n{rust}"
    );
    assert!(
        rust.contains(".split(\",\")"),
        "string-arg split stays on str path, got:\n{rust}"
    );
}

#[test]
fn regexp_call_constructor_emits_regex() {
    // `RegExp("pat", "g")` (no `new`) → `__ds::regex`, same as a `/pat/` literal.
    // The runtime-string pattern is ToString'd; flags pass through verbatim.
    let src = "function main(): void {\n  const r = RegExp(\"\\\\d+\", \"g\");\n  console.log(r.test(\"abc123\"));\n}";
    let (rust, deps) = Translator::new()
        .translate_with_deps(src)
        .expect("translate_with_deps");
    assert!(
        deps.needs_regress(),
        "RegExp() flags needs_regress, got deps: {deps:?}"
    );
    assert!(
        rust.contains("__ds::regex(") && rust.contains("\"g\""),
        "RegExp() emits __ds::regex with flags, got:\n{rust}"
    );
    assert!(
        rust.contains(".find("),
        "RegExp() local infers Regex so .test lowers to .find, got:\n{rust}"
    );
}

#[test]
fn new_regexp_constructor_emits_regex() {
    // `new RegExp(/pat/)` copies the literal's pattern; `new RegExp(var)` takes
    // a runtime pattern. Both lower to `__ds::regex`, not `RegExp::new`.
    let src = "function main(): void {\n  const r1 = new RegExp(/[a-z]+/);\n  const pat = \"x\";\n  const r2 = new RegExp(pat);\n  console.log(r1.test(\"hi\"));\n  console.log(r2.test(\"ax\"));\n}";
    let (rust, deps) = Translator::new()
        .translate_with_deps(src)
        .expect("translate_with_deps");
    assert!(
        deps.needs_regress(),
        "new RegExp flags needs_regress, got deps: {deps:?}"
    );
    assert!(
        !rust.contains("RegExp::new"),
        "new RegExp must not emit RegExp::new (E0425), got:\n{rust}"
    );
    assert!(
        rust.matches("__ds::regex(").count() >= 2,
        "two new RegExp() calls emit two __ds::regex, got:\n{rust}"
    );
}

#[test]
fn reg_exp_escape_emits_inline_metachar_escape() {
    // `RegExp.escape(s)` (TC39 Stage 3) — inline backslash-escape of
    // metacharacters; no runtime dep (a pure std char loop).
    let src = "function main(): void {\n  console.log(RegExp.escape(\"a.b*c\"));\n}";
    let (rust, deps) = Translator::new()
        .translate_with_deps(src)
        .expect("translate_with_deps");
    assert!(
        !deps.needs_regress(),
        "RegExp.escape pulls no regress dep, got deps: {deps:?}"
    );
    assert!(
        rust.contains("push('\\\\')"),
        "RegExp.escape emits backslash-escape loop, got:\n{rust}"
    );
}

#[test]
fn regex_local_exec_emits_ds_match_from() {
    // `let r = /pat/; r.exec(s)` — the variable receiver reuses the already-
    // compiled `Regex` (`.find` + `ds_match_from`), not `regex_match` (which
    // needs the source pattern the variable has lost).
    let src = "function main(): void {\n  const r = /(\\w+)/;\n  const m = r.exec(\"hi\");\n  console.log(m[0]);\n}";
    let (rust, deps) = Translator::new()
        .translate_with_deps(src)
        .expect("translate_with_deps");
    assert!(deps.needs_regress(), "regex local exec flags needs_regress");
    assert!(
        rust.contains("ds_match_from"),
        "variable .exec lowers to ds_match_from, got:\n{rust}"
    );
}

#[test]
fn regex_local_exec_result_infers_option_ds_match() {
    // `let r = /pat/; const m = r.exec(s); m !== null` — `m` infers
    // `Option<DsMatch>` (the receiver is a regex local, not just a literal),
    // so `m !== null` lowers to `is_some()` (not a plain `!= None`, which would
    // be E0369), and `m.index` reaches the DsMatch field, not Option's missing
    // `index`.
    let src = "function main(): void {\n  const r = /(\\w+)/;\n  const m = r.exec(\"hi\");\n  console.log(m !== null);\n  console.log(m.index);\n}";
    let (rust, _deps) = Translator::new()
        .translate_with_deps(src)
        .expect("translate_with_deps");
    assert!(
        rust.contains(".is_some()"),
        "m !== null lowers to is_some, got:\n{rust}"
    );
    assert!(
        !rust.contains("!= None") && !rust.contains("!= ::core::option::Option::None"),
        "m !== null must not emit a plain != None (E0369), got:\n{rust}"
    );
}

#[test]
fn console_log_exec_routes_to_fmt_option_match() {
    // Option<DsMatch> has no Display, so console.log(/pat/.exec(s)) routes the
    // arg through __ds::fmt_option_match (Node's match-array inspect form).
    let src = "function main(): void {\n  console.log(/a/.exec(\"a\"));\n}";
    let (rust, deps) = Translator::new()
        .translate_with_deps(src)
        .expect("translate_with_deps");
    assert!(deps.needs_regress(), "regex exec flags needs_regress");
    assert!(
        rust.contains("fmt_option_match"),
        "console.log(exec) routes to fmt_option_match, got: {rust}"
    );
}

#[test]
fn console_log_match_local_routes_to_fmt_option_match() {
    // console.log(m) where m is Option<DsMatch> routes through fmt_option_match
    // too (the local path, not just the inline .exec call).
    let src = "function main(): void {\n  const m = /a/.exec(\"a\");\n  console.log(m);\n}";
    let (rust, _deps) = Translator::new()
        .translate_with_deps(src)
        .expect("translate_with_deps");
    assert!(
        rust.contains("fmt_option_match"),
        "console.log(m) on Option<DsMatch> routes to fmt_option_match, got: {rust}"
    );
}

#[test]
fn console_log_string_match_routes_to_fmt_option_match() {
    // console.log("s".match(/pat/)) — a non-global .match is Option<DsMatch>, so
    // it routes through fmt_option_match too (the .match path, not just .exec).
    let src = "function main(): void {\n  console.log(\"abc\".match(/a/));\n}";
    let (rust, deps) = Translator::new()
        .translate_with_deps(src)
        .expect("translate_with_deps");
    assert!(deps.needs_regress(), "regex match flags needs_regress");
    assert!(
        rust.contains("fmt_option_match"),
        "console.log(str.match) routes to fmt_option_match, got: {rust}"
    );
}

#[test]
fn temporal_plain_date_from_routes_through_temporal_rs() {
    // `Temporal.PlainDate.from(s)` → `temporal_rs::PlainDate::from_utf8` (the
    // inherent constructor — no FromStr trait import). Flags `needs_temporal`;
    // `.toString()` reuses the Display-based `to_string` mapping.
    let src = "function main(): void {\n  const d = Temporal.PlainDate.from(\"2024-01-01\");\n  console.log(d.toString());\n}";
    let (rust, deps) = Translator::new()
        .translate_with_deps(src)
        .expect("translate_with_deps");
    assert!(
        deps.needs_temporal(),
        "Temporal.PlainDate.from flags needs_temporal, got deps: {deps:?}"
    );
    assert!(
        rust.contains("temporal_rs::PlainDate::from_utf8"),
        "from routes through temporal_rs, got:\n{rust}"
    );
}

#[test]
fn temporal_plain_date_accessors_route_to_methods() {
    // `d.year`/`d.month`/`d.day` on a `Temporal.PlainDate` local → the matching
    // `temporal_rs::PlainDate` accessor method (ES calendar fields are
    // properties; Rust accessors are methods — numeric ones cast to `f64`).
    let src = "function main(): void {\n  const d = Temporal.PlainDate.from(\"2024-03-15\");\n  console.log(d.year);\n  console.log(d.month);\n  console.log(d.day);\n  console.log(d.inLeapYear);\n}";
    let (rust, deps) = Translator::new()
        .translate_with_deps(src)
        .expect("translate_with_deps");
    assert!(
        deps.needs_temporal(),
        "flags needs_temporal, got deps: {deps:?}"
    );
    assert!(
        rust.contains(".year()") && rust.contains(".month()") && rust.contains(".day()"),
        "calendar accessors route to methods, got:\n{rust}"
    );
    assert!(
        rust.contains(".in_leap_year()"),
        "inLeapYear routes to in_leap_year, got:\n{rust}"
    );
    assert!(
        rust.contains("as f64"),
        "numeric accessors cast to f64, got:\n{rust}"
    );
}

#[test]
fn temporal_plain_date_compare_emits_ordering_match() {
    // `Temporal.PlainDate.compare(a, b)` → -1/0/1 (Temporal.CompareResult) via
    // `compare_iso` + an `Ordering` match; args are bound so a `&` borrow works
    // for both locals and inline `from(…)` calls.
    let src = "function main(): void {\n  const a = Temporal.PlainDate.from(\"2024-01-01\");\n  const b = Temporal.PlainDate.from(\"2024-12-31\");\n  console.log(Temporal.PlainDate.compare(a, b));\n  console.log(Temporal.PlainDate.compare(a, a));\n  console.log(Temporal.PlainDate.compare(b, a));\n}";
    let (rust, deps) = Translator::new()
        .translate_with_deps(src)
        .expect("translate_with_deps");
    assert!(
        deps.needs_temporal(),
        "flags needs_temporal, got deps: {deps:?}"
    );
    assert!(
        rust.contains("compare_iso"),
        "compare routes to compare_iso, got:\n{rust}"
    );
    assert!(
        rust.contains("Ordering::Less") && rust.contains("Ordering::Greater"),
        "compare lowers an Ordering match, got:\n{rust}"
    );
}

#[test]
fn regex_literal_flags_and_source_are_static() {
    // `/abc/gi.flags` / `.source` / `.global` / `.ignoreCase` → bare literals
    // (the flags are known at translate time), not a runtime `Regex` field —
    // so a `.ds` source that only reads static regex properties links no
    // `regress` dep and never constructs a `Regex`.
    let src = "function main(): void {\n  console.log(/abc/gi.flags);\n  console.log(/abc/gi.global);\n  console.log(/abc/gi.ignoreCase);\n  console.log(/abc/gi.multiline);\n  console.log(/abc/gi.source);\n  console.log(/(?:)/.source);\n}";
    let (rust, deps) = Translator::new()
        .translate_with_deps(src)
        .expect("translate_with_deps");
    assert!(
        !deps.needs_regress(),
        "static regex properties pull no regress dep, got deps: {deps:?}"
    );
    assert!(
        rust.contains("\"gi\""),
        ".flags lowers to the ES-order flag string, got:\n{rust}"
    );
    assert!(
        rust.contains("\"abc\""),
        ".source lowers to the pattern literal, got:\n{rust}"
    );
    assert!(
        rust.contains("\"(?:)\""),
        "an empty pattern's source is ES's (?:), got:\n{rust}"
    );
    assert!(
        !rust.contains(".flags") && !rust.contains("__ds::regex"),
        ".flags/.source must not survive as a field/Regex, got:\n{rust}"
    );
}

#[test]
fn for_of_regex_array_test_routes_through_find() {
    // `for (let re of [/^.$/s]) re.test(s)` — the loop variable infers
    // `regress::Regex`, so `.test` lowers to `.find(…).is_some()` (without the
    // type, `.test` would be an unmapped method on `Regex` → E0599).
    let src = "function main(): void {\n  for (let re of [/^.$/s]) {\n    console.log(re.test(\"a\"));\n  }\n}";
    let (rust, deps) = Translator::new()
        .translate_with_deps(src)
        .expect("translate_with_deps");
    assert!(
        deps.needs_regress(),
        "for-of regex array flags needs_regress, got deps: {deps:?}"
    );
    assert!(
        rust.contains(".find("),
        "for-of regex .test lowers to .find, got:\n{rust}"
    );
}

#[test]
fn check_rejects_match_result_property_assignment() {
    // `["a"].index = 2` (the test262 s15.10.2.13 idiom of stamping match-result
    // fields onto a plain Array) is dynamic property mutation → `check` flags
    // it unsupported rather than letting it mis-compile into a `Vec` field.
    let src = "function main(): void {\n  var a = [\"a\"];\n  a.index = 2;\n  a.input = \"x\";\n}";
    let diags = Translator::new().check(src);
    assert!(
        diags
            .iter()
            .any(|d| format!("{d}").contains("match-result property")),
        "index/input assignment should be unsupported, got: {diags:?}"
    );
}

#[test]
fn function_expression_callback_routes_to_engine() {
    // `[1].find(function (kValue) { … })` — a `function` expression as a call
    // argument (a callback) has no static lowering (`translate_expr` maps a
    // `FunctionExpression` to `todo!()`), so the call site is `!` (never) and
    // fails `cargo check` (E0618 "expected function, found `!`"). The engine
    // (rquickjs) runs the callback verbatim. (An arrow callback stays mapped.)
    let src = "function main(): void {\n  const r = [1, 2, 3].find(function (kValue) { return kValue > 1; });\n  console.log(r);\n}";
    let (_rust, deps) = Translator::new()
        .translate_with_deps(src)
        .expect("translate_with_deps");
    assert!(
        deps.needs_engine(),
        "function-expression callback should flip needs_engine, got deps: {deps:?}"
    );
}

#[test]
fn iife_routes_to_engine() {
    // `(function () { … })()` — an IIFE's callee is a `function` expression,
    // which has no static lowering, so the whole program routes to the engine.
    let src = "function main(): void {\n  (function () { console.log(1); })();\n}";
    let (_rust, deps) = Translator::new()
        .translate_with_deps(src)
        .expect("translate_with_deps");
    assert!(
        deps.needs_engine(),
        "an IIFE should flip needs_engine, got deps: {deps:?}"
    );
}

#[test]
fn arrow_callback_stays_mapped() {
    // `[1,2,3].find((x) => x > 1)` — an arrow callback is statically lowered
    // (a Rust closure), so the program must NOT pull the engine.
    let src =
        "function main(): void {\n  const r = [1, 2, 3].find((x) => x > 1);\n  console.log(r);\n}";
    let (_rust, deps) = Translator::new()
        .translate_with_deps(src)
        .expect("translate_with_deps");
    assert!(
        !deps.needs_engine(),
        "an arrow callback must not pull the engine, got deps: {deps:?}"
    );
}
