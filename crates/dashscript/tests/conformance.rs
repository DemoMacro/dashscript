//! Conformance / support-matrix harness for DashScript.
//!
//! Three data sources merged into one feature list:
//! - `tests-fixtures.json` — auto-extracted from `translator/tests/*.rs` by
//!   `scripts/extract-tests.mjs` (**zero hand-written fixtures**). Each entry is
//!   a verified-translatable `.ts` snippet; the runner cargo-checks it
//!   informationally (`translator/tests` only asserts the translated Rust
//!   *contains* a substring — it never compiles). No `expect`, so the run
//!   reports the current state without asserting it.
//! - `test262.json` — auto-extracted from tc39 test262 by
//!   `scripts/extract-test262.mjs`. The conformance layer: each fixture's body
//!   is wrapped verbatim in `function main(): void { … }` (asserts kept as-is),
//!   and the verdict is **assert-driven** — no Node oracle. The static path
//!   (`Translator::check` → `cargo build` → run the probe). A degrading fixture
//!   (`needs_engine`) takes the same compile path: the emitted binary embeds a
//!   `__ds_engine` QuickJS that runs the body with the test262 assert family
//!   registered as a production builtin (Javy register pattern). Verdict:
//!   exit 0 = every assert held = `supported`, a thrown `Test262Error` =
//!   `partial`, a `ReferenceError` (a host global DashScript does not ship) /
//!   build failure / timeout = `unsupported`.
//! - `correctness.json` — hand-written correctness cases (the *only* hand-written
//!   fixtures). Each carries `expect` + `expect_output`; the runner cargo-runs
//!   the emitted program and compares stdout. Asserted (regression guard).
//!
//! Support judgment for any *run* feature runs the full three-layer chain:
//! `Translator::check` (translatability) → `Translator::translate` + `cargo
//! check` (the emitted Rust must compile — translatability alone is not enough).
//! Result: `supported` | `partial` (translates but won't compile) |
//! `unsupported` (`check` flags it).
//!
//! Output: `matrix.md` (human) + `matrix.json` (machine) beside this file.
//!
//! Run: `cargo test -p dashscript --test conformance`.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicBool, Ordering},
};

use dashscript::{FileRole, RuntimeDeps, Translator};
use serde::{Deserialize, Serialize};
use tempfile::TempDir;

const TESTS_JSON: &str = include_str!("conformance/data/tests-fixtures.json");
const CORRECTNESS_JSON: &str = include_str!("conformance/data/correctness.json");
// test262 data is per-category under `data/test262/<cat>.json`, discovered at
// runtime (see `conformance_matrix`) — not a single compiled-in blob.

/// A minimal binary manifest — conformance fixtures exercise built-in APIs only
/// (no crate dependencies), and `cargo check` does not require `main`, so a bare
/// declaration compiles. `cargo run` (the correctness layer) does require `main`,
/// which correctness fixtures provide.
const MANIFEST: &str =
    "[package]\nname = \"probe\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n[dependencies]\n";

/// Ceiling on a single fixture's run. A hanging fixture (catastrophic regexp
/// backtracking, an infinite loop) is killed instead of stalling the whole
/// matrix. 30s sits between mutants.rs's 20s floor and nextest's 60s default.
const PROBE_TIMEOUT_SECS: u64 = 30;

/// test262 `features:` the ds toolchain does not ship — a fixture exercising
/// one has no ds support (neither the static translator nor the engine covers
/// it), so it is honestly `unsupported` without running anything. Currently
/// empty: the ds side maps Temporal via the `temporal-rs` crate, and the engine
/// (QuickJS) inherits full ECMAScript semantics. Add a feature here only when
/// both the static path and the engine lack it.
const UNSHIPPED_FEATURES: &[&str] = &[] as &[&str];

#[derive(Debug, Deserialize)]
struct FeatureFile {
    features: Vec<RawFeature>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawFeature {
    id: String,
    category: String,
    #[serde(default)]
    fixture: String,
    expect: Option<String>,
    expect_output: Option<String>,
    #[serde(default)]
    note: String,
    /// test262 `features:` frontmatter (e.g. `["Temporal"]`) — drives the
    /// unshipped-feature short-circuit in `run_test262`.
    #[serde(default)]
    features: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct Outcome {
    id: String,
    /// Which data source this outcome came from — drives the per-file matrix
    /// output (`test262` → one file per category; `translator-tests` /
    /// `correctness` → one file each).
    layer: String,
    category: String,
    status: &'static str,
    detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    expect: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    correct: Option<bool>,
    note: String,
}

#[test]
fn conformance_matrix() {
    let tests: FeatureFile = serde_json::from_str(TESTS_JSON).expect("parse tests-fixtures.json");
    let correct: FeatureFile =
        serde_json::from_str(CORRECTNESS_JSON).expect("parse correctness.json");
    // test262 lives per-category under `data/test262/<cat>.json`, discovered at
    // runtime so a new category file is picked up with no Rust edit. The layer
    // is opt-in: `DASH_TEST262_CATEGORIES=math,number` runs only those builtins;
    // unset → test262 skipped (correctness + translator-tests always run, so a
    // bare `cargo test` stays fast). A category can be large (Object is ~1.5k
    // fixtures) — `DASH_TEST262=<n>` caps each category at n fixtures.
    let test262_dir = conformance_dir().join("data").join("test262");
    let cats: Vec<String> = match std::env::var("DASH_TEST262_CATEGORIES") {
        // `=all` discovers every `data/test262/<cat>.json` at runtime, so a full
        // run is one short env var — not a hand-maintained comma list, and never
        // a bare `"all"` treated as a category name that silently runs nothing.
        Ok(s) if s.trim().eq_ignore_ascii_case("all") => discover_categories(&test262_dir),
        Ok(s) => s
            .split(',')
            .map(|c| c.trim().to_lowercase())
            .filter(|c| !c.is_empty())
            .collect(),
        Err(_) => Vec::new(),
    };
    let limit = match std::env::var("DASH_TEST262") {
        Ok(v) if v == "all" || v == "0" => usize::MAX,
        Ok(v) => v.parse().unwrap_or(usize::MAX),
        Err(_) => usize::MAX,
    };
    let mut test262_features: Vec<RawFeature> = Vec::new();
    for cat in &cats {
        let path = test262_dir.join(format!("{cat}.json"));
        let json = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => {
                eprintln!(
                    "conformance: {} not found — run \
                     `node scripts/extract-test262.mjs --category {cat}`",
                    path.display()
                );
                continue;
            }
        };
        let file: FeatureFile = match serde_json::from_str(&json) {
            Ok(f) => f,
            Err(e) => panic!("parse {}: {e}", path.display()),
        };
        test262_features.extend(file.features.into_iter().take(limit));
    }
    // WinterTC (Ecma TC55) lives per-dir under `data/wpt/<dir>.json`, discovered
    // at runtime so a new dir file is picked up with no Rust edit. The layer is
    // opt-in: `DASH_WPT_CATEGORIES=url,encoding` runs only those WinterTC dirs;
    // unset → wpt skipped (so a bare `cargo test` stays fast). WinterTC is
    // static-first with per-function engine degrade (same model as test262): a
    // Web API not yet mapped statically falls back to the engine path with Web
    // API builtins registered (degraded behavior == static behavior — same Rust
    // impl). `DASH_WPT=<n>` caps each dir.
    let wpt_dir = conformance_dir().join("data").join("wpt");
    let wpt_cats: Vec<String> = match std::env::var("DASH_WPT_CATEGORIES") {
        // `=all` discovers every `data/wpt/<dir>.json` at runtime (see above).
        Ok(s) if s.trim().eq_ignore_ascii_case("all") => discover_categories(&wpt_dir),
        Ok(s) => s
            .split(',')
            .map(|c| c.trim().to_lowercase())
            .filter(|c| !c.is_empty())
            .collect(),
        Err(_) => Vec::new(),
    };
    let wpt_limit = match std::env::var("DASH_WPT") {
        Ok(v) if v == "all" || v == "0" => usize::MAX,
        Ok(v) => v.parse().unwrap_or(usize::MAX),
        Err(_) => usize::MAX,
    };
    let mut wpt_features: Vec<RawFeature> = Vec::new();
    for cat in &wpt_cats {
        let path = wpt_dir.join(format!("{cat}.json"));
        let json = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => {
                eprintln!(
                    "conformance: {} not found — run \
                     `node scripts/extract-wpt.mjs --dirs {cat}`",
                    path.display()
                );
                continue;
            }
        };
        let file: FeatureFile = match serde_json::from_str(&json) {
            Ok(f) => f,
            Err(e) => panic!("parse {}: {e}", path.display()),
        };
        wpt_features.extend(file.features.into_iter().take(wpt_limit));
    }
    // Each raw paired with its layer — drives the per-file matrix output
    // (`test262`/`wpt` → one file per category; the other two → one file each).
    let raws: Vec<(RawFeature, &'static str)> = tests
        .features
        .into_iter()
        .map(|r| (r, "translator-tests"))
        .chain(test262_features.into_iter().map(|r| (r, "test262")))
        .chain(wpt_features.into_iter().map(|r| (r, "wpt")))
        .chain(correct.features.into_iter().map(|r| (r, "correctness")))
        .collect();

    // N independent probe projects, each with its own `target/`, run in
    // parallel. cargo's incremental build is keyed per-target-dir, so a single
    // shared `target/` forces the whole matrix to serialize on one linker lock
    // — every tiny `main.rs` revision re-links under it. Splitting the fixtures
    // across workers gives cargo N independent `target/`s to compile into
    // concurrently; each worker pays a one-time std compile, amortized across
    // its share.
    let n_workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .clamp(1, 8);
    let n_workers = std::env::var("DASH_CONF_WORKERS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(n_workers);
    let tmp = TempDir::new().expect("tempdir");
    let workers: Vec<(PathBuf, PathBuf)> = (0..n_workers)
        .map(|i| {
            let root = tmp.path().join(format!("w{i}"));
            let project = root.join("probe");
            let target_dir = root.join("target");
            fs::create_dir_all(project.join("src")).expect("create probe src");
            (project, target_dir)
        })
        .collect();

    // Populate `~/.cargo` once (serially) with every runtime dep a fixture might
    // inject, so the N parallel workers don't race the crates-io registry
    // update lock on their first `cargo` call. On success the workers then build
    // offline (`CARGO_NET_OFFLINE=true`), eliminating the registry-lock race
    // entirely — without this the losers fail "unable to update registry
    // `crates-io`" and the fixture is mis-recorded as `partial`.
    warm_cargo_cache(&workers[0].0);

    // Split the fixtures into `n_workers` contiguous chunks — one per worker
    // thread. Each thread runs its chunk sequentially against its own
    // project/target pair, so the parallelism is across workers (N simultaneous
    // cargo invocations), not within one.
    let chunk_size = raws.len().div_ceil(n_workers).max(1);
    let handles: Vec<_> = raws
        .chunks(chunk_size)
        .enumerate()
        .map(|(i, chunk)| {
            let (project, target_dir) = workers[i].clone();
            let chunk: Vec<(RawFeature, &'static str)> = chunk.to_vec();
            std::thread::spawn(move || {
                chunk
                    .into_iter()
                    .map(|(raw, layer)| run_fixture(&raw, layer, &project, &target_dir))
                    .collect::<Vec<Outcome>>()
            })
        })
        .collect();
    let mut outcomes: Vec<Outcome> = Vec::new();
    for h in handles {
        outcomes.extend(h.join().expect("worker thread"));
    }
    // Stable order regardless of which worker handled which fixture, so the
    // per-slice matrix tables are deterministic.
    outcomes.sort_by(|a, b| a.id.cmp(&b.id));

    write_matrix_split(&outcomes);

    // Regression guard: every declared `expect` must match the observed status.
    // Today only `correctness.json` declares `expect`; translator-tests are
    // informational (recorded, not asserted).
    let mismatches: Vec<&Outcome> = outcomes
        .iter()
        .filter(|o| o.expect.as_ref().is_some_and(|e| e.as_str() != o.status))
        .collect();
    if mismatches.is_empty() {
        return;
    }
    let report = mismatches
        .iter()
        .map(|o| {
            format!(
                "  - {}: expected {:?}, got {} — {}",
                o.id, o.expect, o.status, o.detail
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    panic!(
        "{} conformance expectation(s) not met:\n{}",
        mismatches.len(),
        report
    );
}

/// WinterTC smoke: a minimal WPT fixture (no Web API — just the testharness
/// builtin) must run the full static path end-to-end and pass. Proves the
/// `test`/`assert_equals` → `__ds::wpt_*` lowering closes the loop through
/// `run_wpt` (translate → check → cargo → run → exit 0), independent of any
/// Web API mapping — so a Tier-1 API's matrix can drop to zero without hiding
/// a harness regression. The fixture is inline (not from `data/wpt/`) so it
/// cannot be regressed by re-extraction.
#[test]
fn wpt_testharness_compiles_and_runs() {
    let raw = RawFeature {
        id: "wpt.smoke".into(),
        category: "smoke".into(),
        fixture: "test(function () { assert_equals(1, 1); }, \"trivial\");\n".into(),
        expect: None,
        expect_output: None,
        note: String::new(),
        features: Vec::new(),
    };
    let tmp = TempDir::new().expect("tempdir");
    let project = tmp.path().join("probe");
    let target_dir = tmp.path().join("target");
    fs::create_dir_all(project.join("src")).expect("probe src");
    let (status, detail) = run_wpt(&raw, &project, &target_dir);
    assert_eq!(
        status, "supported",
        "testharness smoke should be supported: {detail}"
    );
}

/// `new EventTarget()` / `new Event(type, init)` / `addEventListener`/
/// `removeEventListener`/`dispatchEvent`/`preventDefault` end-to-end on the
/// static path — the WinterTC DOM Events API core loop. A named-function
/// listener (`function listener(evt) { evt.preventDefault(); }`) whose `evt`
/// parameter is inferred as `&DsEvent` (the per-body scan in `analysis.rs`),
/// wrapped in a discard-return adapter so it satisfies `Box<dyn FnMut(&DsEvent)>`;
/// `dispatchEvent` returns `false` after `preventDefault` on a cancelable event,
/// `true` once the listener is removed. Inline fixture (not from `data/wpt/`)
/// so it cannot be regressed by re-extraction.
#[test]
fn wpt_eventtarget_compiles_and_runs() {
    let raw = RawFeature {
        id: "wpt.eventtarget_smoke".into(),
        category: "smoke".into(),
        fixture: "function listener(evt) { evt.preventDefault(); }\n\
                  test(() => {\n\
                  \x20 const et = new EventTarget();\n\
                  \x20 et.addEventListener(\"x\", listener, false);\n\
                  \x20 const event = new Event(\"x\", { cancelable: true });\n\
                  \x20 assert_false(et.dispatchEvent(event));\n\
                  \x20 et.removeEventListener(\"x\", listener);\n\
                  \x20 assert_true(et.dispatchEvent(new Event(\"x\", { cancelable: true })));\n\
                  }, \"add-remove-listener\");\n"
            .into(),
        expect: None,
        expect_output: None,
        note: String::new(),
        features: Vec::new(),
    };
    let tmp = TempDir::new().expect("tempdir");
    let project = tmp.path().join("probe");
    let target_dir = tmp.path().join("target");
    fs::create_dir_all(project.join("src")).expect("probe src");
    let (status, detail) = run_wpt(&raw, &project, &target_dir);
    assert_eq!(
        status, "supported",
        "EventTarget smoke should be supported: {detail}"
    );
}

/// WinterTC `self` / `globalThis` is the global scope, which is itself an
/// `EventTarget` — `self.addEventListener(…)` / `globalThis.removeEventListener(…)`
/// route to the shared `__ds::wpt_self()` target rather than a phantom binding.
/// Mirrors the WPT `dom/events/eventtarget-removeeventlistener` shape (a
/// `globalThis.removeEventListener(…)` no-op returning `undefined`).
#[test]
fn wpt_eventtarget_self_global_this_compiles_and_runs() {
    let raw = RawFeature {
        id: "wpt.eventtarget_self_smoke".into(),
        category: "smoke".into(),
        fixture: "function main(): void {\n\
                  \x20 test(function() {\n\
                  \x20   assert_equals(globalThis.removeEventListener(\"x\", null, false), undefined);\n\
                  \x20   assert_equals(self.removeEventListener(\"x\", null), undefined);\n\
                  \x20 }, \"removing a null event listener should succeed\");\n\
                  }\nmain();\n"
            .into(),
        expect: None,
        expect_output: None,
        note: String::new(),
        features: Vec::new(),
    };
    let tmp = TempDir::new().expect("tempdir");
    let project = tmp.path().join("probe");
    let target_dir = tmp.path().join("target");
    fs::create_dir_all(project.join("src")).expect("probe src");
    let (status, detail) = run_wpt(&raw, &project, &target_dir);
    assert_eq!(
        status, "supported",
        "self/globalThis EventTarget smoke should be supported: {detail}"
    );
}

/// `new CustomEvent(type, init)` + property reads end-to-end on the static
/// path — the WinterTC DOM `CustomEvent` interface. Mirrors the WPT
/// `event-constructors` CustomEvent region: `detail` carries through, the
/// `cancelable` flag reads back, and an unknown init field (`sweet`) reads as
/// `undefined` (it is dropped on the static path — `DsCustomEvent` has no such
/// field). Inline fixture (not from `data/wpt/`) so it cannot be regressed by
/// re-extraction.
#[test]
fn wpt_custom_event_compiles_and_runs() {
    let raw = RawFeature {
        id: "wpt.custom_event_smoke".into(),
        category: "smoke".into(),
        fixture: "test(() => {\n\
                  \x20 const ev = new CustomEvent(\"$\", {detail: 54, sweet: \"x\", sweet2: \"x\", cancelable: true});\n\
                  \x20 assert_equals(ev.type, \"$\");\n\
                  \x20 assert_equals(ev.bubbles, false);\n\
                  \x20 assert_equals(ev.cancelable, true);\n\
                  \x20 assert_equals(ev.detail, 54);\n\
                  });\n"
            .into(),
        expect: None,
        expect_output: None,
        note: String::new(),
        features: Vec::new(),
    };
    let tmp = TempDir::new().expect("tempdir");
    let project = tmp.path().join("probe");
    let target_dir = tmp.path().join("target");
    fs::create_dir_all(project.join("src")).expect("probe src");
    let (status, detail) = run_wpt(&raw, &project, &target_dir);
    assert_eq!(
        status, "supported",
        "CustomEvent smoke should be supported: {detail}"
    );
}

/// `for (const entry of url.searchParams)` — the WHATWG URLSearchParams
/// iterable contract: each yielded entry is a `[name, value]` pair, and the
/// iterator is **live** (a mid-iteration `url.search = …` mutation is visible
/// to later steps). Covers the `IntoIterator for &DsUrlSearchParams` helper
/// (yields owned `Vec<String>` pairs) plus the for-of lowering's value-pattern
/// branch for an untyped Web-collection iterable. Mirrors the
/// `urlsearchparams-foreach` WPT fixture's "For-of Check".
#[test]
fn wpt_url_search_params_iter_compiles_and_runs() {
    let raw = RawFeature {
        id: "wpt.url_search_params_iter_smoke".into(),
        category: "smoke".into(),
        fixture: "test(() => {\n\
                  \x20 const u = new URL(\"http://a.b/c?a=1&b=2&c=3\");\n\
                  \x20 const c = [];\n\
                  \x20 for (const entry of u.searchParams) { c.push(entry); }\n\
                  \x20 assert_array_equals(c[0], [\"a\", \"1\"]);\n\
                  \x20 assert_array_equals(c[1], [\"b\", \"2\"]);\n\
                  \x20 assert_array_equals(c[2], [\"c\", \"3\"]);\n\
                  });\n"
            .into(),
        expect: None,
        expect_output: None,
        note: String::new(),
        features: Vec::new(),
    };
    let tmp = TempDir::new().expect("tempdir");
    let project = tmp.path().join("probe");
    let target_dir = tmp.path().join("target");
    fs::create_dir_all(project.join("src")).expect("probe src");
    let (status, detail) = run_wpt(&raw, &project, &target_dir);
    assert_eq!(
        status, "supported",
        "URLSearchParams for-of should be supported: {detail}"
    );
}

/// `new AbortController()` + `controller.signal` (both a binding and a chained
/// read) + `signal.aborted` (before/after) + `controller.abort()` end-to-end on
/// the static path — the WinterTC WHATWG DOM Abort API core. Covers the two
/// receiver shapes (a `DsAbortSignal` Identifier local, and a chained
/// `controller.signal`) and the abort semantics (the shared flag flips). The
/// `signal.addEventListener("abort", cb)` EventTarget-inheritance path is
/// emitted by `abort_method` (it routes to the embedded `DsEventTarget`); a
/// listener whose body calls a `DsEvent`-unique method (`preventDefault`, …) is
/// inferred as `&DsEvent` by `analysis.rs`, the same path EventTarget uses.
/// Inline fixture (not from `data/wpt/`) so it cannot be regressed by
/// re-extraction.
#[test]
fn wpt_abort_controller_compiles_and_runs() {
    let raw = RawFeature {
        id: "wpt.abort_smoke".into(),
        category: "smoke".into(),
        fixture: "test(() => {\n\
                  \x20 const controller = new AbortController();\n\
                  \x20 const signal = controller.signal;\n\
                  \x20 assert_false(signal.aborted);\n\
                  \x20 controller.abort();\n\
                  \x20 assert_true(signal.aborted);\n\
                  \x20 assert_true(controller.signal.aborted);\n\
                  }, \"abort-controller-core\");\n"
            .into(),
        expect: None,
        expect_output: None,
        note: String::new(),
        features: Vec::new(),
    };
    let tmp = TempDir::new().expect("tempdir");
    let project = tmp.path().join("probe");
    let target_dir = tmp.path().join("target");
    fs::create_dir_all(project.join("src")).expect("probe src");
    let (status, detail) = run_wpt(&raw, &project, &target_dir);
    assert_eq!(
        status, "supported",
        "AbortController smoke should be supported: {detail}"
    );
}

/// `new Headers()` / `.append` / `.set` / `.get` / `.has` / `.delete` + a
/// Record init end-to-end on the static path — the WinterTC WHATWG FETCH
/// `Headers` API core. Covers the semantics a naive `HashMap<String, String>`
/// gets wrong: names are case-insensitive (`append("Content-Type", …)` then
/// `append("content-type", …)` combine, read back via either casing), `append`
/// joins duplicate-name values with `", "` (ES `get` order), `set` overwrites,
/// and a Record `{ "X-Test": "v" }` init lowercases the key. Inline fixture
/// (not from `data/wpt/`) so it cannot be regressed by re-extraction.
#[test]
fn wpt_headers_compiles_and_runs() {
    let raw = RawFeature {
        id: "wpt.headers_smoke".into(),
        category: "smoke".into(),
        fixture: "test(() => {\n\
                  \x20 const h = new Headers();\n\
                  \x20 h.append(\"Content-Type\", \"text/plain\");\n\
                  \x20 h.append(\"content-type\", \"application/json\");\n\
                  \x20 assert_equals(h.get(\"Content-Type\"), \"text/plain, application/json\");\n\
                  \x20 h.set(\"CONTENT-TYPE\", \"text/html\");\n\
                  \x20 assert_equals(h.get(\"content-type\"), \"text/html\");\n\
                  \x20 assert_true(h.has(\"Content-Type\"));\n\
                  \x20 h.delete(\"content-type\");\n\
                  \x20 assert_false(h.has(\"Content-Type\"));\n\
                  \x20 const h2 = new Headers({ \"X-Test\": \"value\" });\n\
                  \x20 assert_equals(h2.get(\"x-test\"), \"value\");\n\
                  }, \"headers-core\");\n"
            .into(),
        expect: None,
        expect_output: None,
        note: String::new(),
        features: Vec::new(),
    };
    let tmp = TempDir::new().expect("tempdir");
    let project = tmp.path().join("probe");
    let target_dir = tmp.path().join("target");
    fs::create_dir_all(project.join("src")).expect("probe src");
    let (status, detail) = run_wpt(&raw, &project, &target_dir);
    assert_eq!(
        status, "supported",
        "Headers smoke should be supported: {detail}"
    );
}

/// WHATWG `Blob` (FileAPI, a WinterTC Web API) end-to-end — synchronous core:
/// `new Blob(parts, options?)` flattens the parts to bytes; `blob.size`/
/// `blob.type` are zero-arg accessors, `blob.slice(start, end)` returns a new
/// `Blob`, and `blob instanceof Blob` folds to `true` (the ctor is in
/// `MAPPED_CTOR_RUST_TYPE`). The async `text()`/`arrayBuffer()`/`bytes()` ride
/// the same dispatch under `.await` (pinned by `wpt_blob_text_compiles_and_runs`).
#[test]
fn wpt_blob_compiles_and_runs() {
    let raw = RawFeature {
        id: "wpt.blob_smoke".into(),
        category: "smoke".into(),
        fixture: "test(() => {\n\
                  \x20 const b = new Blob([\"hello\", \" world\"]);\n\
                  \x20 assert_equals(b.size, 11);\n\
                  \x20 assert_equals(b.type, \"\");\n\
                  \x20 const t = new Blob([\"x\"], { type: \"text/plain\" });\n\
                  \x20 assert_equals(t.type, \"text/plain\");\n\
                  \x20 const s = b.slice(0, 5);\n\
                  \x20 assert_equals(s.size, 5);\n\
                  \x20 assert_true(b instanceof Blob);\n\
                  }, \"blob-core\");\n"
            .into(),
        expect: None,
        expect_output: None,
        note: String::new(),
        features: Vec::new(),
    };
    let tmp = TempDir::new().expect("tempdir");
    let project = tmp.path().join("probe");
    let target_dir = tmp.path().join("target");
    fs::create_dir_all(project.join("src")).expect("probe src");
    let (status, detail) = run_wpt(&raw, &project, &target_dir);
    assert_eq!(
        status, "supported",
        "Blob smoke should be supported: {detail}"
    );
}

/// `await blob.text()` end-to-end — the async Blob methods lower to a
/// `pub async fn` call whose `.await` flips the entry to `#[tokio::main]`.
/// Verifies the full async chain: translate → cargo build (tokio) → the future
/// awaits and the UTF-8 text assert holds (exit 0). A sibling to the sync
/// smoke above; a failure means the async lowering or the `DsBlob::text` emit
/// regressed.
#[test]
fn wpt_blob_text_compiles_and_runs() {
    let raw = RawFeature {
        id: "wpt.blob_text_smoke".into(),
        category: "smoke".into(),
        fixture: "promise_test(async () => {\n\
                  \x20 const b = new Blob([\"hello\"]);\n\
                  \x20 const t = await b.text();\n\
                  \x20 assert_equals(t, \"hello\");\n\
                  }, \"blob-async\");\n"
            .into(),
        expect: None,
        expect_output: None,
        note: String::new(),
        features: Vec::new(),
    };
    let tmp = TempDir::new().expect("tempdir");
    let project = tmp.path().join("probe");
    let target_dir = tmp.path().join("target");
    fs::create_dir_all(project.join("src")).expect("probe src");
    let (status, detail) = run_wpt(&raw, &project, &target_dir);
    assert_eq!(
        status, "supported",
        "Blob async smoke should be supported: {detail}"
    );
}

/// `new File(bits, name, options?)` end-to-end — the WHATWG `File` API (a
/// WinterTC Web API). A `File` is a `Blob` with a `name`/`lastModified`: the
/// ctor flattens `bits`, `file.name`/`.lastModified` are the `File`-only
/// accessors, the inherited `file.size`/`.type` ride the `Blob` accessors
/// (`is_blob_local` is widened to accept a `DsFile`), `file.slice(…)` returns a
/// `Blob`, and `instanceof Blob`/`instanceof File` fold to `true`/`true` (the
/// `Blob` ctor has the `File` subtype special-case). `lastModified` defaults to
/// `Date.now()`, so the assert is `> 0` (a number), not an exact value.
#[test]
fn wpt_file_compiles_and_runs() {
    let raw = RawFeature {
        id: "wpt.file_smoke".into(),
        category: "smoke".into(),
        fixture: "test(() => {\n\
                  \x20 const f = new File([\"hello\", \" world\"], \"greeting.txt\", { type: \"text/plain\" });\n\
                  \x20 assert_equals(f.name, \"greeting.txt\");\n\
                  \x20 assert_equals(f.size, 11);\n\
                  \x20 assert_equals(f.type, \"text/plain\");\n\
                  \x20 assert_true(f.lastModified > 0);\n\
                  \x20 const s = f.slice(0, 5);\n\
                  \x20 assert_equals(s.size, 5);\n\
                  \x20 assert_true(f instanceof Blob);\n\
                  \x20 assert_true(f instanceof File);\n\
                  }, \"file-core\");\n"
            .into(),
        expect: None,
        expect_output: None,
        note: String::new(),
        features: Vec::new(),
    };
    let tmp = TempDir::new().expect("tempdir");
    let project = tmp.path().join("probe");
    let target_dir = tmp.path().join("target");
    fs::create_dir_all(project.join("src")).expect("probe src");
    let (status, detail) = run_wpt(&raw, &project, &target_dir);
    assert_eq!(
        status, "supported",
        "File smoke should be supported: {detail}"
    );
}

/// `await file.text()` end-to-end — the inherited async `Blob` method on a
/// `File` receiver. `file.text()` dispatches through `blob_method` (which keys
/// off `is_blob_local`, widened to accept a `DsFile`), and the `.await` flips
/// the entry to `#[tokio::main]`. A failure means the `DsFile` delegation or
/// the async lowering on a `File` receiver regressed.
#[test]
fn wpt_file_text_compiles_and_runs() {
    let raw = RawFeature {
        id: "wpt.file_text_smoke".into(),
        category: "smoke".into(),
        fixture: "promise_test(async () => {\n\
                  \x20 const f = new File([\"hello\"], \"g.txt\");\n\
                  \x20 const t = await f.text();\n\
                  \x20 assert_equals(t, \"hello\");\n\
                  }, \"file-async\");\n"
            .into(),
        expect: None,
        expect_output: None,
        note: String::new(),
        features: Vec::new(),
    };
    let tmp = TempDir::new().expect("tempdir");
    let project = tmp.path().join("probe");
    let target_dir = tmp.path().join("target");
    fs::create_dir_all(project.join("src")).expect("probe src");
    let (status, detail) = run_wpt(&raw, &project, &target_dir);
    assert_eq!(
        status, "supported",
        "File async smoke should be supported: {detail}"
    );
}

/// `new FormData()` + the void/bool instance methods end-to-end — the WinterTC
/// FETCH `FormData` API. Exercises the `_str` variant (`append`/`set` with a
/// string value), `has`/`delete`, and the `_file` variant (`append` with a
/// `File` value, resolved by `is_file_arg`). A failure means the `DsFormData`
/// helper, the ctor lowering, or the `_str`/`_file` dispatch regressed. The
/// value-returning `get`/`getAll`/`entries` (a `string | File` union result)
/// are out of scope — they need the union-unboxing path.
#[test]
fn wpt_form_data_compiles_and_runs() {
    let raw = RawFeature {
        id: "wpt.form_data_smoke".into(),
        category: "smoke".into(),
        fixture: "test(() => {\n\
                  \x20 const fd = new FormData();\n\
                  \x20 fd.append(\"a\", \"1\");\n\
                  \x20 fd.append(\"b\", \"2\");\n\
                  \x20 assert_equals(fd.has(\"a\"), true);\n\
                  \x20 fd.delete(\"a\");\n\
                  \x20 assert_equals(fd.has(\"a\"), false);\n\
                  \x20 fd.set(\"b\", \"3\");\n\
                  \x20 assert_equals(fd.has(\"b\"), true);\n\
                  \x20 const f = new File([\"x\"], \"f.txt\");\n\
                  \x20 fd.append(\"file\", f);\n\
                  \x20 assert_equals(fd.has(\"file\"), true);\n\
                  }, \"formdata-core\");\n"
            .into(),
        expect: None,
        expect_output: None,
        note: String::new(),
        features: Vec::new(),
    };
    let tmp = TempDir::new().expect("tempdir");
    let project = tmp.path().join("probe");
    let target_dir = tmp.path().join("target");
    fs::create_dir_all(project.join("src")).expect("probe src");
    let (status, detail) = run_wpt(&raw, &project, &target_dir);
    assert_eq!(
        status, "supported",
        "FormData smoke should be supported: {detail}"
    );
}

/// `await crypto.subtle.digest(algo, data)` end-to-end — the WinterTC WebCrypto
/// `SubtleCrypto.digest` one-shot hash. Exercises the two-level `crypto.subtle`
/// member chain, the async lowering (`await` flips the entry to
/// `#[tokio::main]`), and both the `sha2` (SHA-256 → 32 bytes) and `sha1`
/// (SHA-1 → 20 bytes) crates. A failure means the nested-member dispatch, the
/// async helper, or the `sha1`/`sha2` wiring regressed.
#[test]
fn wpt_subtle_digest_compiles_and_runs() {
    let raw = RawFeature {
        id: "wpt.subtle_digest_smoke".into(),
        category: "smoke".into(),
        fixture: "promise_test(async () => {\n\
                  \x20 const a = await crypto.subtle.digest(\"SHA-256\", new Uint8Array([0, 1, 2, 3]));\n\
                  \x20 assert_equals(a.length, 32);\n\
                  \x20 const b = await crypto.subtle.digest(\"SHA-1\", \"abc\");\n\
                  \x20 assert_equals(b.length, 20);\n\
                  }, \"subtle-digest\");\n"
            .into(),
        expect: None,
        expect_output: None,
        note: String::new(),
        features: Vec::new(),
    };
    let tmp = TempDir::new().expect("tempdir");
    let project = tmp.path().join("probe");
    let target_dir = tmp.path().join("target");
    fs::create_dir_all(project.join("src")).expect("probe src");
    let (status, detail) = run_wpt(&raw, &project, &target_dir);
    assert_eq!(
        status, "supported",
        "SubtleCrypto digest smoke should be supported: {detail}"
    );
}

/// `await crypto.subtle.{importKey,sign,verify}(…)` end-to-end — the WinterTC
/// WebCrypto HMAC subset. Exercises the two-level `crypto.subtle` chain for
/// each method, the `importKey → DsCryptoKey` return-type inference (so the
/// later `sign`/`verify` pass the key local through as `&DsCryptoKey`), the
/// `hmac` crate backing (SHA-256 → 32-byte tag), and the constant-time verify.
/// The data arrays are inlined per call (each `sign`/`verify` takes the
/// `Vec<u8>` by value); the key is `&DsCryptoKey`, borrowed across the three
/// calls. A failure means the importKey wiring, the key-bearing dispatch, or
/// the `hmac` dependency regressed.
#[test]
fn wpt_subtle_hmac_compiles_and_runs() {
    let raw = RawFeature {
        id: "wpt.subtle_hmac_smoke".into(),
        category: "smoke".into(),
        fixture: "promise_test(async () => {\n\
                  \x20 const key = await crypto.subtle.importKey(\n\
                  \x20   \"raw\", new Uint8Array([1, 2, 3]), { name: \"HMAC\", hash: \"SHA-256\" }, false, []);\n\
                  \x20 const sig = await crypto.subtle.sign(\"HMAC\", key, new Uint8Array([10, 20, 30]));\n\
                  \x20 assert_equals(sig.length, 32);\n\
                  \x20 const ok = await crypto.subtle.verify(\"HMAC\", key, sig, new Uint8Array([10, 20, 30]));\n\
                  \x20 assert_equals(ok, true);\n\
                  \x20 const sig2 = await crypto.subtle.sign(\"HMAC\", key, new Uint8Array([99, 99, 99]));\n\
                  \x20 const bad = await crypto.subtle.verify(\"HMAC\", key, sig2, new Uint8Array([10, 20, 30]));\n\
                  \x20 assert_equals(bad, false);\n\
                  }, \"subtle-hmac\");\n"
            .into(),
        expect: None,
        expect_output: None,
        note: String::new(),
        features: Vec::new(),
    };
    let tmp = TempDir::new().expect("tempdir");
    let project = tmp.path().join("probe");
    let target_dir = tmp.path().join("target");
    fs::create_dir_all(project.join("src")).expect("probe src");
    let (status, detail) = run_wpt(&raw, &project, &target_dir);
    assert_eq!(
        status, "supported",
        "SubtleCrypto HMAC smoke should be supported: {detail}"
    );
}

/// `await crypto.subtle.{importKey,encrypt,decrypt}(…)` end-to-end — the
/// WinterTC WebCrypto AES-GCM subset. Exercises the `crypto.subtle.encrypt`/
/// `.decrypt` dispatch, the `encrypt → Vec<u8>` return-type inference (so the
/// ciphertext local passes through to `decrypt` as a byte vector), the
/// `encrypt_algorithm` `{name, iv}` extraction, and the `aes-gcm` crate backing
/// (AES-256-GCM, 32-byte raw key). The `iv` is a local reused across the
/// encrypt/decrypt pair (the helper takes it by reference). The assertions lean
/// on AES-GCM's authentication: a 3-byte plaintext encrypts to 19 bytes (3 +
/// 16-byte tag), and decrypting authenticates before returning the 3-byte
/// plaintext — a wrong key/iv/tampered tag panics rather than returns a wrong
/// length, so the length checks prove the round-trip.
#[test]
fn wpt_subtle_aes_gcm_compiles_and_runs() {
    let raw = RawFeature {
        id: "wpt.subtle_aes_gcm_smoke".into(),
        category: "smoke".into(),
        fixture: "promise_test(async () => {\n\
                  \x20 const key = await crypto.subtle.importKey(\n\
                  \x20   \"raw\", new Uint8Array([0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31]),\n\
                  \x20   { name: \"AES-GCM\" }, false, [\"encrypt\", \"decrypt\"]);\n\
                  \x20 const iv = new Uint8Array([0,1,2,3,4,5,6,7,8,9,10,11]);\n\
                  \x20 const ct = await crypto.subtle.encrypt({ name: \"AES-GCM\", iv: iv }, key, new Uint8Array([104, 105, 106]));\n\
                  \x20 assert_equals(ct.length, 19);\n\
                  \x20 const pt = await crypto.subtle.decrypt({ name: \"AES-GCM\", iv: iv }, key, ct);\n\
                  \x20 assert_equals(pt.length, 3);\n\
                  \x20 }, \"subtle-aes-gcm\");\n"
            .into(),
        expect: None,
        expect_output: None,
        note: String::new(),
        features: Vec::new(),
    };
    let tmp = TempDir::new().expect("tempdir");
    let project = tmp.path().join("probe");
    let target_dir = tmp.path().join("target");
    fs::create_dir_all(project.join("src")).expect("probe src");
    let (status, detail) = run_wpt(&raw, &project, &target_dir);
    assert_eq!(
        status, "supported",
        "SubtleCrypto AES-GCM smoke should be supported: {detail}"
    );
}

/// `await crypto.subtle.generateKey(…)` end-to-end — the WinterTC WebCrypto key
/// factory. Exercises the `generateKey` dispatch, the `generateKey → DsCryptoKey`
/// return-type inference (so the key local passes to a later `encrypt`/`sign`),
/// the `generate_key_algorithm` `{name, length}` / `{name, hash, length}` triple
/// extraction, and the `getrandom` key fill. Two paths in one fixture: an
/// AES-GCM-256 key encrypts a 3-byte plaintext (19-byte ciphertext = 3 + 16-byte
/// tag) then decrypts it (3-byte round-trip); an HMAC-SHA-256 key signs a 3-byte
/// message (32-byte tag) and verifies it (`true`). AES-GCM's authentication and
/// HMAC's determinism mean the length/`true` checks prove the keys are usable.
#[test]
fn wpt_subtle_generate_key_compiles_and_runs() {
    let raw = RawFeature {
        id: "wpt.subtle_generate_key_smoke".into(),
        category: "smoke".into(),
        fixture: r#"promise_test(async () => {
    const aesKey = await crypto.subtle.generateKey({ name: "AES-GCM", length: 256 }, false, ["encrypt", "decrypt"]);
    const iv = new Uint8Array([0,1,2,3,4,5,6,7,8,9,10,11]);
    const ct = await crypto.subtle.encrypt({ name: "AES-GCM", iv: iv }, aesKey, new Uint8Array([7,8,9]));
    assert_equals(ct.length, 19);
    const pt = await crypto.subtle.decrypt({ name: "AES-GCM", iv: iv }, aesKey, ct);
    assert_equals(pt.length, 3);

    const hmacKey = await crypto.subtle.generateKey({ name: "HMAC", hash: "SHA-256", length: 256 }, false, ["sign", "verify"]);
    const sig = await crypto.subtle.sign({ name: "HMAC" }, hmacKey, new Uint8Array([1,2,3]));
    assert_equals(sig.length, 32);
    const ok = await crypto.subtle.verify({ name: "HMAC" }, hmacKey, sig, new Uint8Array([1,2,3]));
    assert_equals(ok, true);
}, "subtle-generatekey");
"#
        .into(),
        expect: None,
        expect_output: None,
        note: String::new(),
        features: Vec::new(),
    };
    let tmp = TempDir::new().expect("tempdir");
    let project = tmp.path().join("probe");
    let target_dir = tmp.path().join("target");
    fs::create_dir_all(project.join("src")).expect("probe src");
    let (status, detail) = run_wpt(&raw, &project, &target_dir);
    assert_eq!(
        status, "supported",
        "SubtleCrypto generateKey smoke should be supported: {detail}"
    );
}

/// `await crypto.subtle.deriveBits(…)` end-to-end — the WinterTC WebCrypto
/// PBKDF2 key-derivation path. Exercises the `deriveBits` dispatch, the
/// `deriveBits → Vec<u8>` return-type inference, and the
/// `derive_bits_algorithm` `{name, salt, iterations, hash}` extraction. Uses the
/// RFC 6070 PBKDF2-SHA-1 reference vector (P="password", S="salt", c=1, dkLen=20
/// → `0c60c80f…`) as a deterministic correctness check: `dk[0] == 0x0c` proves
/// the `pbkdf2` crate is fed the password (the `importKey` raw `baseKey`), salt,
/// iteration count, and SHA-1 PRF correctly.
#[test]
fn wpt_subtle_derive_bits_pbkdf2_compiles_and_runs() {
    let raw = RawFeature {
        id: "wpt.subtle_derive_bits_pbkdf2_smoke".into(),
        category: "smoke".into(),
        fixture: r#"promise_test(async () => {
    const baseKey = await crypto.subtle.importKey(
        "raw", new Uint8Array([112,97,115,115,119,111,114,100]),
        { name: "PBKDF2" }, false, ["deriveBits"]);
    const salt = new Uint8Array([115,97,108,116]);
    const dk = await crypto.subtle.deriveBits(
        { name: "PBKDF2", salt: salt, iterations: 1, hash: "SHA-1" }, baseKey, 160);
    assert_equals(dk.length, 20);
    assert_equals(dk[0], 12);
}, "subtle-derivebits-pbkdf2");
"#
        .into(),
        expect: None,
        expect_output: None,
        note: String::new(),
        features: Vec::new(),
    };
    let tmp = TempDir::new().expect("tempdir");
    let project = tmp.path().join("probe");
    let target_dir = tmp.path().join("target");
    fs::create_dir_all(project.join("src")).expect("probe src");
    let (status, detail) = run_wpt(&raw, &project, &target_dir);
    assert_eq!(
        status, "supported",
        "SubtleCrypto PBKDF2 deriveBits smoke should be supported: {detail}"
    );
}

/// `crypto.subtle.exportKey("raw", key)` end-to-end — the WinterTC WEBCRYPTO §5
/// raw symmetric-key export (the inverse of `importKey`). Exercises the
/// importKey → exportKey round-trip: a 16-byte AES-GCM key imported raw, then
/// exported raw, must come back byte-equal (length + first/last bytes). Proves
/// the static path keeps the key bytes through the `DsCryptoKey` without
/// coercion (`callee_return_path` records the `Vec<u8>` return, so the
/// `dk.length`/`dk[0]`/`dk[15]` reads are byte-vector indexing).
#[test]
fn wpt_subtle_export_key_compiles_and_runs() {
    let raw = RawFeature {
        id: "wpt.subtle_export_key_smoke".into(),
        category: "smoke".into(),
        fixture: r#"promise_test(async () => {
    const key = await crypto.subtle.importKey(
        "raw", new Uint8Array([1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16]),
        { name: "AES-GCM" }, true, ["encrypt"]);
    const raw = await crypto.subtle.exportKey("raw", key);
    assert_equals(raw.length, 16);
    assert_equals(raw[0], 1);
    assert_equals(raw[15], 16);
}, "subtle-exportkey");
"#
        .into(),
        expect: None,
        expect_output: None,
        note: String::new(),
        features: Vec::new(),
    };
    let tmp = TempDir::new().expect("tempdir");
    let project = tmp.path().join("probe");
    let target_dir = tmp.path().join("target");
    fs::create_dir_all(project.join("src")).expect("probe src");
    let (status, detail) = run_wpt(&raw, &project, &target_dir);
    assert_eq!(
        status, "supported",
        "SubtleCrypto exportKey smoke should be supported: {detail}"
    );
}

/// `crypto.subtle.deriveKey(…)` end-to-end — the WinterTC WEBCRYPTO §5
/// PBKDF2→AES-GCM derivation-and-import path (an orchestrator over
/// `deriveBits` + the key ctor). Exercises the full importKey(PBKDF2 password)
/// → deriveKey(PBKDF2 c=1 SHA-1 → AES-GCM 256) → exportKey round-trip: the
/// derived AES-256 key is 32 bytes, and its first byte is `0x0c` (the RFC 6070
/// PBKDF2-SHA1 c=1 first byte — proving the derivation actually ran, not just a
/// zero-filled key). Proves `deriveKey` composes the already-mapped
/// `deriveBits` + `DsCryptoKey::new` (DRY — no duplicated PBKDF2 core).
#[test]
fn wpt_subtle_derive_key_compiles_and_runs() {
    let raw = RawFeature {
        id: "wpt.subtle_derive_key_smoke".into(),
        category: "smoke".into(),
        fixture: r#"promise_test(async () => {
    const baseKey = await crypto.subtle.importKey(
        "raw", new Uint8Array([112,97,115,115,119,111,114,100]),
        { name: "PBKDF2" }, false, ["deriveKey"]);
    const salt = new Uint8Array([115,97,108,116]);
    const key = await crypto.subtle.deriveKey(
        { name: "PBKDF2", salt: salt, iterations: 1, hash: "SHA-1" },
        baseKey,
        { name: "AES-GCM", length: 256 },
        true, ["encrypt"]);
    const raw = await crypto.subtle.exportKey("raw", key);
    assert_equals(raw.length, 32);
    assert_equals(raw[0], 12);
}, "subtle-derivekey");
"#
        .into(),
        expect: None,
        expect_output: None,
        note: String::new(),
        features: Vec::new(),
    };
    let tmp = TempDir::new().expect("tempdir");
    let project = tmp.path().join("probe");
    let target_dir = tmp.path().join("target");
    fs::create_dir_all(project.join("src")).expect("probe src");
    let (status, detail) = run_wpt(&raw, &project, &target_dir);
    assert_eq!(
        status, "supported",
        "SubtleCrypto deriveKey smoke should be supported: {detail}"
    );
}

/// `crypto.subtle.deriveBits(…)` with `name: "HKDF"` end-to-end — the WinterTC
/// WEBCRYPTO §5 HKDF (RFC 5869) extract-then-expand path, the second of the two
/// §5 key-derivation functions (PBKDF2 is the other). Exercises the canonical
/// RFC 5869 Test Case 1: SHA-256, IKM = 22 bytes of `0x0b`, salt = `0..12`,
/// info = `0xf0..0xf9`, L = 42 bytes — the first output byte is `0x3c` (60),
/// proving the extract+expand ran correctly (not a zero-filled buffer). Reuses
/// the same `hmac` backing as PBKDF2 — no new crate.
#[test]
fn wpt_subtle_derive_bits_hkdf_compiles_and_runs() {
    let raw = RawFeature {
        id: "wpt.subtle_derive_bits_hkdf_smoke".into(),
        category: "smoke".into(),
        fixture: r#"promise_test(async () => {
    const baseKey = await crypto.subtle.importKey(
        "raw", new Uint8Array([11,11,11,11,11,11,11,11,11,11,11,11,11,11,11,11,11,11,11,11,11,11]),
        { name: "HKDF" }, false, ["deriveBits"]);
    const salt = new Uint8Array([0,1,2,3,4,5,6,7,8,9,10,11,12]);
    const info = new Uint8Array([240,241,242,243,244,245,246,247,248,249]);
    const okm = await crypto.subtle.deriveBits(
        { name: "HKDF", salt: salt, info: info, hash: "SHA-256" }, baseKey, 336);
    assert_equals(okm.length, 42);
    assert_equals(okm[0], 60);
}, "subtle-derivebits-hkdf");
"#
        .into(),
        expect: None,
        expect_output: None,
        note: String::new(),
        features: Vec::new(),
    };
    let tmp = TempDir::new().expect("tempdir");
    let project = tmp.path().join("probe");
    let target_dir = tmp.path().join("target");
    fs::create_dir_all(project.join("src")).expect("probe src");
    let (status, detail) = run_wpt(&raw, &project, &target_dir);
    assert_eq!(
        status, "supported",
        "SubtleCrypto HKDF deriveBits smoke should be supported: {detail}"
    );
}

/// `crypto.subtle.encrypt`/`.decrypt` with `name: "AES-CBC"` end-to-end — the
/// WinterTC WEBCRYPTO §5 unauthenticated block-cipher subset (the companion of
/// AES-GCM). Exercises the canonical NIST SP 800-38A F.2.1 AES-128-CBC vector:
/// the first ciphertext byte is `0x76` (118), proving the CBC core is correct;
/// the PKCS7-padded ciphertext is 32 bytes (16 NIST block + a full padding
/// block — PKCS7 always pads, even on a block boundary); and the
/// encrypt→decrypt round-trip restores the 16-byte plaintext.
#[test]
fn wpt_subtle_aes_cbc_compiles_and_runs() {
    let raw = RawFeature {
        id: "wpt.subtle_aes_cbc_smoke".into(),
        category: "smoke".into(),
        fixture: r#"promise_test(async () => {
    const key = await crypto.subtle.importKey(
        "raw", new Uint8Array([43,126,21,22,40,174,210,166,171,247,21,136,9,207,79,60]),
        { name: "AES-CBC", length: 128 }, true, ["encrypt"]);
    const iv = new Uint8Array([0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15]);
    const pt = new Uint8Array([107,193,190,226,46,64,159,150,233,61,126,17,115,147,23,42]);
    const ct = await crypto.subtle.encrypt({ name: "AES-CBC", iv: iv }, key, pt);
    assert_equals(ct.length, 32);
    assert_equals(ct[0], 118);
    const back = await crypto.subtle.decrypt({ name: "AES-CBC", iv: iv }, key, ct);
    assert_equals(back.length, 16);
    assert_equals(back[0], 107);
}, "subtle-aescbc");
"#
        .into(),
        expect: None,
        expect_output: None,
        note: String::new(),
        features: Vec::new(),
    };
    let tmp = TempDir::new().expect("tempdir");
    let project = tmp.path().join("probe");
    let target_dir = tmp.path().join("target");
    fs::create_dir_all(project.join("src")).expect("probe src");
    let (status, detail) = run_wpt(&raw, &project, &target_dir);
    assert_eq!(
        status, "supported",
        "SubtleCrypto AES-CBC smoke should be supported: {detail}"
    );
}

/// `crypto.subtle.wrapKey`/`unwrapKey` (AES-KW) end-to-end — RFC 3394 §4.1
/// (wrap a 128-bit key under a 128-bit KEK). The wrapped bytes' first block is
/// `1FA68B0A…` (`wrapped[0] == 0x1F == 31`), and the unwrap round-trip recovers
/// the original raw key bytes (verified by re-exporting). Pure-Rust — WinterTC
/// never degrades a Web API.
#[test]
fn wpt_subtle_aes_kw_compiles_and_runs() {
    let raw = RawFeature {
        id: "wpt.subtle_aes_kw_smoke".into(),
        category: "smoke".into(),
        fixture: r#"promise_test(async () => {
    const kek = await crypto.subtle.importKey(
        "raw", new Uint8Array([0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15]),
        { name: "AES-KW", length: 128 }, true, ["wrapKey","unwrapKey"]);
    const key = await crypto.subtle.importKey(
        "raw", new Uint8Array([0,17,34,51,68,85,102,119,136,153,170,187,204,221,238,255]),
        { name: "AES-GCM", length: 128 }, true, ["encrypt"]);
    const wrapped = await crypto.subtle.wrapKey("raw", key, kek, { name: "AES-KW" });
    assert_equals(wrapped.length, 24);
    assert_equals(wrapped[0], 31);
    const unwrapped = await crypto.subtle.unwrapKey(
        "raw", wrapped, kek, { name: "AES-KW" },
        { name: "AES-GCM", length: 128 }, true, ["encrypt"]);
    const exported = await crypto.subtle.exportKey("raw", unwrapped);
    assert_equals(exported.length, 16);
    assert_equals(exported[0], 0);
    assert_equals(exported[15], 255);
}, "subtle-aeskw");
"#
        .into(),
        expect: None,
        expect_output: None,
        note: String::new(),
        features: Vec::new(),
    };
    let tmp = TempDir::new().expect("tempdir");
    let project = tmp.path().join("probe");
    let target_dir = tmp.path().join("target");
    fs::create_dir_all(project.join("src")).expect("probe src");
    let (status, detail) = run_wpt(&raw, &project, &target_dir);
    assert_eq!(
        status, "supported",
        "SubtleCrypto AES-KW smoke should be supported: {detail}"
    );
}

/// `new Request(url, init?)` + the `.method`/`.url` read-only accessors
/// end-to-end — the WinterTC FETCH §5.2 `Request` ctor. Exercises the ctor's
/// `init` parsing (reusing `fetch_init`, the same path as `fetch(url, init)`),
/// the default GET when `init` is absent, and the accessors (`request.method`
/// uppercased, `request.url`). `fetch(request)` itself is network-bound (a real
/// send that panics on a missing WPT server), so the smoke verifies the
/// descriptor's construction + inspection, not the send; the `fetch(request)`
/// dispatch (`ds_fetch_request`) is compile-verified by the type wiring.
#[test]
fn wpt_request_compiles_and_runs() {
    let raw = RawFeature {
        id: "wpt.request_smoke".into(),
        category: "smoke".into(),
        fixture: "test(() => {\n\
                  \x20 const r = new Request(\"https://example.com/a\", { method: \"POST\" });\n\
                  \x20 assert_equals(r.method, \"POST\");\n\
                  \x20 assert_equals(r.url, \"https://example.com/a\");\n\
                  \x20 const g = new Request(\"https://example.com/b\");\n\
                  \x20 assert_equals(g.method, \"GET\");\n\
                  \x20 assert_equals(g.url, \"https://example.com/b\");\n\
                  }, \"request-core\");\n"
            .into(),
        expect: None,
        expect_output: None,
        note: String::new(),
        features: Vec::new(),
    };
    let tmp = TempDir::new().expect("tempdir");
    let project = tmp.path().join("probe");
    let target_dir = tmp.path().join("target");
    fs::create_dir_all(project.join("src")).expect("probe src");
    let (status, detail) = run_wpt(&raw, &project, &target_dir);
    assert_eq!(
        status, "supported",
        "Request smoke should be supported: {detail}"
    );
}

/// `new Response(body?, init?)` end-to-end — the WHATWG `Response` constructor
/// (FETCH §5.3, a WinterTC Web API). A synthetic `DsResponse` is built from a
/// body flattened to bytes and a `status`/`statusText`/`headers` init object,
/// the same surface `fetch(…)` returns: `.status`/`.statusText`/`.ok` (member
/// accessors, synchronous) and `await .text()` (the body-consuming async
/// method). The smoke covers a full-init Response (201/Created/headers), a
/// default-arg Response (`new Response()` → 200/ok), and the async `text()`
/// drain — verifying the constructor, the accessors, and the body method together.
#[test]
fn wpt_response_compiles_and_runs() {
    let raw = RawFeature {
        id: "wpt.response_smoke".into(),
        category: "smoke".into(),
        fixture: "promise_test(async () => {\n\
                  \x20 const r = new Response(\"hello\", { status: 201, statusText: \"Created\", headers: { \"content-type\": \"text/plain\" } });\n\
                  \x20 assert_equals(r.status, 201);\n\
                  \x20 assert_equals(r.statusText, \"Created\");\n\
                  \x20 assert_equals(r.ok, true);\n\
                  \x20 assert_equals(await r.text(), \"hello\");\n\
                  \x20 const d = new Response();\n\
                  \x20 assert_equals(d.status, 200);\n\
                  \x20 assert_equals(d.ok, true);\n\
                  \x20 }, \"response-core\");\n"
            .into(),
        expect: None,
        expect_output: None,
        note: String::new(),
        features: Vec::new(),
    };
    let tmp = TempDir::new().expect("tempdir");
    let project = tmp.path().join("probe");
    let target_dir = tmp.path().join("target");
    fs::create_dir_all(project.join("src")).expect("probe src");
    let (status, detail) = run_wpt(&raw, &project, &target_dir);
    assert_eq!(
        status, "supported",
        "Response smoke should be supported: {detail}"
    );
}

/// `promise_test(async () => { … }, "n")` end-to-end: the async callback lowers
/// to `wpt_promise_test("n", async move { … }).await` under `#[tokio::main]`,
/// so this verifies the full Stage 2 chain — translate → cargo build (pulls
/// tokio + futures) → run (the future awaits, the assert holds, exit 0). A
/// failure here means the async lowering, the tokio wiring, or the helper
/// signature regressed.
#[test]
fn wpt_promise_test_compiles_and_runs() {
    let raw = RawFeature {
        id: "wpt.promise_smoke".into(),
        category: "smoke".into(),
        fixture: "promise_test(async () => { assert_equals(1, 1); }, \"trivial async\");\n".into(),
        expect: None,
        expect_output: None,
        note: String::new(),
        features: Vec::new(),
    };
    let tmp = TempDir::new().expect("tempdir");
    let project = tmp.path().join("probe");
    let target_dir = tmp.path().join("target");
    fs::create_dir_all(project.join("src")).expect("probe src");
    let (status, detail) = run_wpt(&raw, &project, &target_dir);
    assert_eq!(
        status, "supported",
        "promise_test smoke should be supported: {detail}"
    );
}

/// `promise_test(namedFn, name)` — the named-async-function-reference form
/// (slice 2c) — end-to-end on the static path, sibling to the inline-async
/// smoke above. The async callback is a top-level `async function` declaration
/// referenced by name; it must lower to `wpt_promise_test(name, run()).await`
/// and run supported (exit 0).
#[test]
fn wpt_promise_test_named_callback_compiles_and_runs() {
    let raw = RawFeature {
        id: "wpt.promise_named_smoke".into(),
        category: "smoke".into(),
        fixture:
            "async function run(): Promise<void> { assert_equals(1, 1); }\npromise_test(run, \"named\");\n".into(),
        expect: None,
        expect_output: None,
        note: String::new(),
        features: Vec::new(),
    };
    let tmp = TempDir::new().expect("tempdir");
    let project = tmp.path().join("probe");
    let target_dir = tmp.path().join("target");
    fs::create_dir_all(project.join("src")).expect("probe src");
    let (status, detail) = run_wpt(&raw, &project, &target_dir);
    assert_eq!(
        status, "supported",
        "promise_test named-callback smoke should be supported: {detail}"
    );
}

/// `promise_test(function () { return promise }, name)` — the NON-async
/// callback that *returns* a promise (a common WPT idiom: WPT awaits the
/// returned promise) — end-to-end on the static path, sibling to the async
/// smokes above. The callback lowers to a closure, called `()` to yield its
/// `DsPromise<T>`, `.await`ed inside `async move { … }` so the result is
/// `Output = ()` (matching `wpt_promise_test`). Covers the 167
/// fetch/webcryptoapi/fileapi fixtures that use this shape; a failure here
/// means the non-async lowering regressed.
#[test]
fn wpt_promise_test_non_async_function_callback_compiles_and_runs() {
    let raw = RawFeature {
        id: "wpt.promise_nonasync_smoke".into(),
        category: "smoke".into(),
        fixture: "promise_test(function() { return Promise.resolve(1); }, \"nonasync\");\n".into(),
        expect: None,
        expect_output: None,
        note: String::new(),
        features: Vec::new(),
    };
    let tmp = TempDir::new().expect("tempdir");
    let project = tmp.path().join("probe");
    let target_dir = tmp.path().join("target");
    fs::create_dir_all(project.join("src")).expect("probe src");
    let (status, detail) = run_wpt(&raw, &project, &target_dir);
    assert_eq!(
        status, "supported",
        "promise_test non-async-callback smoke should be supported: {detail}"
    );
}

/// `new ReadableStream({ start(c) { c.enqueue(v); c.close(); } })` + reader
/// `read()` — the WHATWG Streams push-source read loop — end-to-end on the
/// static path. The constructor lowers to `DsReadableStream::from_start`; the
/// `start` callback's controller param registers as `DsReadableStreamController`
/// (`.enqueue`/`.close` dispatch); `getReader()` → `get_reader()`; and
/// `await reader.read()` drives the pinned `read()` future under
/// `#[tokio::main]`. The state machine enqueues 42 then closes, so the read
/// loop yields `{ done: false }` then `{ done: true }`. A failure here means
/// the `Streams` helper, the controller-type registration, or the read-future
/// state machine regressed. WinterTC Web APIs never degrade — pure static Rust.
#[test]
fn wpt_readable_stream_push_source_compiles_and_runs() {
    let raw = RawFeature {
        id: "wpt.streams_push_smoke".into(),
        category: "smoke".into(),
        fixture: "async function readStream(): Promise<void> {\n  const s = new ReadableStream({ start(c) { c.enqueue(42); c.close(); } });\n  const r = s.getReader();\n  const x = await r.read();\n  assert_equals(x.done, false);\n  const y = await r.read();\n  assert_equals(y.done, true);\n}\npromise_test(readStream, \"readable stream push source\");\n".into(),
        expect: None,
        expect_output: None,
        note: String::new(),
        features: Vec::new(),
    };
    let tmp = TempDir::new().expect("tempdir");
    let project = tmp.path().join("probe");
    let target_dir = tmp.path().join("target");
    fs::create_dir_all(project.join("src")).expect("probe src");
    let (status, detail) = run_wpt(&raw, &project, &target_dir);
    assert_eq!(
        status, "supported",
        "readable stream push-source smoke should be supported: {detail}"
    );
}

/// `new CompressionStream("gzip")` + `cs.writable.getWriter()` + `writer.write(
/// bytes)` + `writer.close()` + `cs.readable.getReader()` + `await reader.read()`
/// — the WHATWG Streams compression one-shot transform, end-to-end on the
/// static path. The constructor lowers to `DsCompressionStream::new(Gzip)`; the
/// `writable`/`readable` fields are plain `pub` field access; `getWriter()`/
/// `getReader()` → `get_writer()`/`get_reader()` (their callee-return paths type
/// the writer/reader locals); `writer.write(bytes)` buffers, `writer.close()`
/// runs the one-shot `flate2` compression, and `reader.read()` yields the
/// single compressed chunk `{ done: false, value: Some(…) }`. A failure here
/// means the `Compression` helper, the field-receiver dispatch, or the
/// `flate2` integration regressed. WinterTC Web APIs never degrade — pure
/// static Rust backed by `flate2`'s default `miniz_oxide` (safe Rust) backend.
#[test]
fn wpt_compression_stream_gzip_round_trip_compiles_and_runs() {
    let raw = RawFeature {
        id: "wpt.compression_gzip_smoke".into(),
        category: "smoke".into(),
        fixture: "async function compress(): Promise<void> {\n  const cs = new CompressionStream(\"gzip\");\n  const writer = cs.writable.getWriter();\n  await writer.write(new Uint8Array([72, 101, 108, 108, 111]));\n  await writer.close();\n  const reader = cs.readable.getReader();\n  const chunk = await reader.read();\n  assert_equals(chunk.done, false);\n}\npromise_test(compress, \"gzip round-trip\");\n".into(),
        expect: None,
        expect_output: None,
        note: String::new(),
        features: Vec::new(),
    };
    let tmp = TempDir::new().expect("tempdir");
    let project = tmp.path().join("probe");
    let target_dir = tmp.path().join("target");
    fs::create_dir_all(project.join("src")).expect("probe src");
    let (status, detail) = run_wpt(&raw, &project, &target_dir);
    assert_eq!(
        status, "supported",
        "compression stream gzip round-trip smoke should be supported: {detail}"
    );
}

/// `new DecompressionStream("gzip")` end-to-end — the decode side of the WHATWG
/// Streams compression API. It lowers to the SAME `DsCompressionStream` type as
/// `CompressionStream` (so `writable`/`readable`/`getWriter`/`getReader`/
/// `write`/`close`/`read` dispatch is shared verbatim), differing only in the
/// `DsCodecDir::Decompress` arg, which routes `close()` through
/// `flate2::read::GzDecoder`. The fixture feeds a real `gzip`-of-"Hello" byte
/// sequence (the 25-byte stream `node:zlib.gzipSync` produces) and asserts the
/// reader yields `{ done: false }` — proving the decode path compiles and runs.
/// A failure means the shared-type design, the `Decompress` direction routing,
/// or the `flate2` read decoder integration regressed.
#[test]
fn wpt_decompression_stream_gzip_compiles_and_runs() {
    let raw = RawFeature {
        id: "wpt.decompression_gzip_smoke".into(),
        category: "smoke".into(),
        fixture: "async function decompress(): Promise<void> {\n  const ds = new DecompressionStream(\"gzip\");\n  const writer = ds.writable.getWriter();\n  await writer.write(new Uint8Array([31, 139, 8, 0, 0, 0, 0, 0, 0, 10, 243, 72, 205, 201, 201, 7, 0, 130, 137, 209, 247, 5, 0, 0, 0]));\n  await writer.close();\n  const reader = ds.readable.getReader();\n  const chunk = await reader.read();\n  assert_equals(chunk.done, false);\n}\npromise_test(decompress, \"gzip decompress smoke\");\n".into(),
        expect: None,
        expect_output: None,
        note: String::new(),
        features: Vec::new(),
    };
    let tmp = TempDir::new().expect("tempdir");
    let project = tmp.path().join("probe");
    let target_dir = tmp.path().join("target");
    fs::create_dir_all(project.join("src")).expect("probe src");
    let (status, detail) = run_wpt(&raw, &project, &target_dir);
    assert_eq!(
        status, "supported",
        "decompression stream gzip smoke should be supported: {detail}"
    );
}

/// `await new Promise(executor)` — the Promise constructor on the static track,
/// end-to-end. A bare `new Promise(…)` degrades to the engine (a sync `fn main`
/// never polls the future); only the awaited form maps, because the `.await`
/// drives the `ds_promise_new` future. The executor runs synchronously under a
/// clonable `DsResolver`, `resolve(42)` settles the shared cell, and the await
/// polls it to `Ready(42)`. Wrapping in `promise_test(async () => …)` is what
/// drives the async block (the WPT harness makes `main` async and awaits it);
/// without that driver the await would never run.
#[test]
fn wpt_await_new_promise_compiles_and_runs() {
    let raw = RawFeature {
        id: "wpt.await_new_promise".into(),
        category: "smoke".into(),
        fixture:
            "promise_test(async () => { const v = await new Promise((resolve) => { resolve(42); }); assert_equals(v, 42); }, \"await new Promise\");\n".into(),
        expect: None,
        expect_output: None,
        note: String::new(),
        features: Vec::new(),
    };
    let tmp = TempDir::new().expect("tempdir");
    let project = tmp.path().join("probe");
    let target_dir = tmp.path().join("target");
    fs::create_dir_all(project.join("src")).expect("probe src");
    let (status, detail) = run_wpt(&raw, &project, &target_dir);
    assert_eq!(
        status, "supported",
        "await new Promise(executor) should resolve and assert on the static path: {detail}"
    );
}

/// `setTimeout` + a task-queue drain at the entry end, end-to-end on the static
/// path — the WinterTC WHATWG timers API core. The drain models the ES task
/// queue: `setTimeout` enqueues into a `thread_local` queue, the implicit `fn
/// main` drains it on return (when the call stack is empty). Every fixture
/// clamps its delays to 0 (HTML: "if timeout < 0, set to 0"; a WebIDL `long`
/// truncation folds `Math.pow(2, 32)` to 0), so the drain is a deterministic
/// CPU loop with no real wait. `done` (a bare-identifier callback) lowers to
/// `wpt_done()` — the stop flag the drain checks after every fire, so the
/// `done` queued before `assert_unreached` ends the drain without firing the
/// later timer. Inline fixtures (mirroring `wpt-html.json`) so they cannot be
/// regressed by re-extraction. Covers `negative-settimeout` (delay -100 → 0,
/// `done` beats `assert_unreached` at 10) and `type-long-settimeout` (delay
/// `Math.pow(2, 32)` → WebIDL long truncation → 0).
#[test]
fn wpt_settimeout_negative_clamp_compiles_and_runs() {
    let raw = RawFeature {
        id: "wpt.timers_negative_smoke".into(),
        category: "smoke".into(),
        fixture: "function main(): void {\n\
                  \x20 setup({ single_test: true });\n\
                  \x20 setTimeout(done, -100);\n\
                  \x20 setTimeout(assert_unreached, 10);\n\
                  }\n\
                  main();\n"
            .into(),
        expect: None,
        expect_output: None,
        note: String::new(),
        features: Vec::new(),
    };
    let tmp = TempDir::new().expect("tempdir");
    let project = tmp.path().join("probe");
    let target_dir = tmp.path().join("target");
    fs::create_dir_all(project.join("src")).expect("probe src");
    let (status, detail) = run_wpt(&raw, &project, &target_dir);
    assert_eq!(
        status, "supported",
        "negative-settimeout smoke should be supported: {detail}"
    );
}

#[test]
fn wpt_settimeout_type_long_clamp_compiles_and_runs() {
    let raw = RawFeature {
        id: "wpt.timers_type_long_smoke".into(),
        category: "smoke".into(),
        fixture: "function main(): void {\n\
                  \x20 setup({ single_test: true });\n\
                  \x20 setTimeout(done, Math.pow(2, 32));\n\
                  \x20 setTimeout(assert_unreached, 100);\n\
                  }\n\
                  main();\n"
            .into(),
        expect: None,
        expect_output: None,
        note: String::new(),
        features: Vec::new(),
    };
    let tmp = TempDir::new().expect("tempdir");
    let project = tmp.path().join("probe");
    let target_dir = tmp.path().join("target");
    fs::create_dir_all(project.join("src")).expect("probe src");
    let (status, detail) = run_wpt(&raw, &project, &target_dir);
    assert_eq!(
        status, "supported",
        "type-long-settimeout smoke should be supported: {detail}"
    );
}

/// `setInterval` + `clearInterval` + a recurring callback end-to-end. The
/// interval's callback (`next`) is a nested `function` capturing the outer
/// `interval` handle (so it can `clearInterval(interval)` on its own id) — the
/// closure-degradation path (#456/#457) lowers it to an `FnMut` closure the
/// drain re-queues after every fire. `type-long-setinterval`: `next` clears
/// itself + `done()` on the first fire (delay `Math.pow(2, 32)` → 0), beating
/// `assert_unreached` at 100. `negative-setinterval`: `next` counts to 20 then
/// clears + `done()`, exercising the re-queue (the callback is written back
/// after each fire) and the take-out-of-slot pattern (the 20 fires register no
/// re-borrow).
#[test]
#[ignore = "interval handle capture (var t; fn cb() { clearInterval(t); }; t = setInterval(cb, …)) needs a local Cell/RefCell model — tracked separately"]
fn wpt_setinterval_type_long_compiles_and_runs() {
    let raw = RawFeature {
        id: "wpt.timers_interval_type_long_smoke".into(),
        category: "smoke".into(),
        fixture: "function main(): void {\n\
                  \x20 setup({ single_test: true });\n\
                  \x20 var interval;\n\
                  \x20 function next() {\n\
                  \x20   clearInterval(interval);\n\
                  \x20   done();\n\
                  \x20 }\n\
                  \x20 interval = setInterval(next, Math.pow(2, 32));\n\
                  \x20 setTimeout(assert_unreached, 100);\n\
                  }\n\
                  main();\n"
            .into(),
        expect: None,
        expect_output: None,
        note: String::new(),
        features: Vec::new(),
    };
    let tmp = TempDir::new().expect("tempdir");
    let project = tmp.path().join("probe");
    let target_dir = tmp.path().join("target");
    fs::create_dir_all(project.join("src")).expect("probe src");
    let (status, detail) = run_wpt(&raw, &project, &target_dir);
    assert_eq!(
        status, "supported",
        "type-long-setinterval smoke should be supported: {detail}"
    );
}

#[test]
#[ignore = "interval handle capture (var t; fn cb() { clearInterval(t); }; t = setInterval(cb, …)) needs a local Cell/RefCell model — tracked separately"]
fn wpt_setinterval_negative_requeue_compiles_and_runs() {
    let raw = RawFeature {
        id: "wpt.timers_interval_negative_smoke".into(),
        category: "smoke".into(),
        fixture: "function main(): void {\n\
                  \x20 setup({ single_test: true });\n\
                  \x20 var i = 0;\n\
                  \x20 var interval;\n\
                  \x20 function next() {\n\
                  \x20   i++;\n\
                  \x20   if (i === 20) {\n\
                  \x20     clearInterval(interval);\n\
                  \x20     done();\n\
                  \x20   }\n\
                  \x20 }\n\
                  \x20 setTimeout(assert_unreached, 1000);\n\
                  \x20 interval = setInterval(next, -100);\n\
                  }\n\
                  main();\n"
            .into(),
        expect: None,
        expect_output: None,
        note: String::new(),
        features: Vec::new(),
    };
    let tmp = TempDir::new().expect("tempdir");
    let project = tmp.path().join("probe");
    let target_dir = tmp.path().join("target");
    fs::create_dir_all(project.join("src")).expect("probe src");
    let (status, detail) = run_wpt(&raw, &project, &target_dir);
    assert_eq!(
        status, "supported",
        "negative-setinterval smoke should be supported: {detail}"
    );
}

/// `queueMicrotask(cb)` — HTML's microtask queue (a WinterTC §5.2 global). The
/// callback runs at the next microtask checkpoint — before any `setTimeout`
/// macrotask drains. `microtask-before-timer`: an arrow
/// `queueMicrotask(() => done())` fires during the leading
/// `wpt_drain_microtasks` (before `wpt_run_timers`), setting DONE; the later
/// `setTimeout(assert_unreached, 0)` then sees DONE and never fires. This pins
/// both the microtask-before-macro ordering and the arrow-callback thunk path
/// (a named `done` callback is covered by the timer smokes — `done` is the
/// special-cased thunk in `timer_callback_thunk`).
#[test]
fn wpt_queue_microtask_runs_before_timer() {
    let raw = RawFeature {
        id: "wpt.microtask_before_timer_smoke".into(),
        category: "smoke".into(),
        fixture: "function main(): void {\n\
                  \x20 setup({ single_test: true });\n\
                  \x20 queueMicrotask(() => { done(); });\n\
                  \x20 setTimeout(assert_unreached, 0);\n\
                  }\n\
                  main();\n"
            .into(),
        expect: None,
        expect_output: None,
        note: String::new(),
        features: Vec::new(),
    };
    let tmp = TempDir::new().expect("tempdir");
    let project = tmp.path().join("probe");
    let target_dir = tmp.path().join("target");
    fs::create_dir_all(project.join("src")).expect("probe src");
    let (status, detail) = run_wpt(&raw, &project, &target_dir);
    assert_eq!(
        status, "supported",
        "queueMicrotask-before-setTimeout smoke should be supported: {detail}"
    );
}

/// Once-per-run check that a degrading fixture's emit assembles into a
/// building cargo project: a reflection `.ts` source → `translate_with_deps`
/// (flips `needs_engine`) → `write_project` (injects `__ds_engine` +
/// `wire_web_apis` + the `rquickjs` dep) → `cargo check`. Every degrading
/// fixture now takes this same compile path (the binary's embedded QuickJS
/// runs the body); this smoke test verifies the emit once, ahead of the
/// per-fixture runs. Fails loudly if the engine Rust template, the
/// `__ds_engine` module, or the dep wiring regresses.
#[test]
fn engine_path_compiles_to_valid_rust_project() {
    // Top-level reflection → whole-program `run` path. The emitted crate is a
    // single `fn main { __ds_engine::run(js) }`; cargo check confirms it (and
    // the embedded `__ds_engine` helper) compile against rquickjs + serde_json.
    let tmp = TempDir::new().expect("tempdir");
    let project = tmp.path().join("probe");
    let target_dir = tmp.path().join("target");
    fs::create_dir_all(project.join("src")).expect("probe src");
    let src = "Object.defineProperty({}, \"x\", { value: 1 });\nconsole.log(\"ok\");\n";
    let (rust, deps) = Translator::new()
        .translate_with_deps(src)
        .expect("translate reflection source");
    assert!(
        deps.needs_engine(),
        "Object.defineProperty should flip needs_engine"
    );
    write_project(&project, &rust, &deps);
    let (ok, err) = cargo(
        &project,
        &target_dir,
        &["check", "--quiet", "--message-format=short"],
    );
    assert!(
        ok,
        "engine path must compile to a valid cargo project: {err}"
    );
}

#[test]
fn per_function_path_compiles_to_valid_rust_project() {
    // Reflection inside a top-level `function` → per-function degradation: the
    // function keeps its Rust signature but its body is `call_fn`, the struct
    // argument derives `Serialize`/`Deserialize`, and a `__DS_MODULE_JS` const
    // carries the stripped JS. cargo check confirms the whole emitted crate
    // compiles (the marshal boundary + the `__ds_engine` helper).
    let tmp = TempDir::new().expect("tempdir");
    let project = tmp.path().join("probe");
    let target_dir = tmp.path().join("target");
    fs::create_dir_all(project.join("src")).expect("probe src");
    let src = "interface Item { v: number }\nfunction reflect(b: Item): string {\n  Object.defineProperty(b, \"k\", { value: 1 });\n  return \"done\";\n}\nconst x: Item = { v: 2 };\nconsole.log(reflect(x));\n";
    let (rust, deps) = Translator::new()
        .translate_with_deps(src)
        .expect("translate per-function source");
    assert!(deps.needs_engine(), "per-function reflection needs engine");
    write_project(&project, &rust, &deps);
    let (ok, err) = cargo(
        &project,
        &target_dir,
        &["check", "--quiet", "--message-format=short"],
    );
    assert!(
        ok,
        "per-function path must compile to a valid cargo project: {err}\n--- emitted ---\n{rust}"
    );
}

#[test]
fn engine_path_with_web_api_builtin_compiles() {
    // 阶段一验证缺口填补：`engine_path_compiles` / `per_function_path_compiles`
    // 用 `Object.defineProperty`（仅触发 Engine dep），不用任何 Web API —— 所以
    // `wire_web_apis` stamp 的 `register_text_encoding` / `register_crypto` 等从未
    // 在真实 probe crate 内 cargo check 过（memory `wintertc-engine-wire-webapis`
    // 标注的开放缺口）。本 fixture 在 per-function 降级的反射函数旁加一个静态
    // `TextEncoder` 函数，使 Engine dep 与 Encoding dep 同时 stamp —— 证明
    // `register_text_encoding`（Javy 模式：JS shim + 原生闭包委派同一份
    // `crate::__ds::TextEncoder`）真编译，非仅 stamp 字符串内容正确。
    let tmp = TempDir::new().expect("tempdir");
    let project = tmp.path().join("probe");
    let target_dir = tmp.path().join("target");
    fs::create_dir_all(project.join("src")).expect("probe src");
    let src = "\
interface Item { v: number }
function reflect(b: Item): string {
  Object.defineProperty(b, \"k\", { value: 1 });
  return \"done\";
}
function enc(): Uint8Array {
  const e = new TextEncoder();
  return e.encode(\"hi\");
}
const x: Item = { v: 2 };
console.log(reflect(x));
enc();
";
    let (rust, deps) = Translator::new()
        .translate_with_deps(src)
        .expect("translate engine+webapi source");
    assert!(deps.needs_engine(), "reflection should flip needs_engine");
    assert!(
        deps.has(dashscript::translator::RuntimeDep::Encoding),
        "TextEncoder use should pull Encoding dep, got: {:?}",
        deps
    );
    // `register_text_encoding` lives in the emitted `__ds_engine.rs` (the
    // engine helper module write_project drops beside `main.rs`), not the main
    // `rust` body — assert against the module directly, then let cargo check
    // prove the whole stamped module compiles against rquickjs + encoding_rs.
    let engine_mod = deps
        .engine_helper_module()
        .expect("needs_engine should yield an engine helper module");
    assert!(
        engine_mod.contains("fn register_text_encoding"),
        "engine_helper_module must stamp register_text_encoding when Engine ∧ Encoding active:\n{engine_mod}"
    );
    write_project(&project, &rust, &deps);
    let (ok, err) = cargo(
        &project,
        &target_dir,
        &["check", "--quiet", "--message-format=short"],
    );
    assert!(
        ok,
        "engine path with Web API builtin must compile: {err}\n--- emitted ---\n{rust}"
    );
}

#[test]
fn engine_loads_multi_file_js_module_graph() {
    // B6-2: the engine's `Loader`/`Resolver` loads a multi-file ESM `.js`
    // module graph (a.js imports b.js), `call_module_fn` lazily declares +
    // evaluates it, and the return marshals back. The `bytes` export returns a
    // `Uint8Array`, covering B6-1's `js_to_json` TypedArray arm end-to-end. Each
    // module's source is inlined via `register_js_module` — the emitted crate is
    // self-contained (no runtime `.js` files; `source_of` is a table lookup, not
    // a filesystem read).
    let tmp = TempDir::new().expect("tempdir");
    let project = tmp.path().join("probe");
    let target_dir = tmp.path().join("target");
    fs::create_dir_all(project.join("src")).expect("probe src");
    let a_js = "import { double } from \"./b.js\";\nexport function f(x) { return double(x) + 1; }\nexport function bytes() { return new Uint8Array([1, 2, 3]); }\n";
    let b_js = "export function double(x) { return x * 2; }\n";
    // Pull the engine dep set (needs_engine) so write_project emits
    // __ds_engine.rs + the rquickjs cargo line, without translating any .ts.
    let (_, deps) = Translator::new()
        .translate_with_deps(
            "function r(){Object.defineProperty({},\"x\",{value:1});}\nconsole.log(r());",
        )
        .expect("translate probe for engine deps");
    assert!(deps.needs_engine(), "probe must pull the engine dep set");
    let main = format!(
        "fn main() {{\n    __ds_engine::register_js_module(\"a.js\", {a:?});\n    __ds_engine::register_js_module(\"b.js\", {b:?});\n    let r = __ds_engine::call_module_fn(\"a.js\", \"f\", &[serde_json::json!(3)]);\n    println!(\"f={{}}\", r);\n    let bytes = __ds_engine::call_module_fn(\"a.js\", \"bytes\", &[]);\n    println!(\"bytes={{}}\", bytes);\n}}\n",
        a = a_js,
        b = b_js,
    );
    write_project(&project, &main, &deps);
    let (ok, out) = cargo(&project, &target_dir, &["run", "--quiet"]);
    assert!(ok, "engine module-load probe failed to run:\n{out}");
    assert!(out.contains("f=7"), "f(3) = double(3) + 1 = 7; got:\n{out}");
    assert!(
        out.contains("bytes=[1,2,3]"),
        "Uint8Array([1,2,3]) marshals as [1,2,3]; got:\n{out}"
    );
}

#[test]
fn module_let_lazy_static_compiles_to_valid_rust_project() {
    // B3-1a: a module-level non-mutated `let` (runtime initializer, here a
    // `number[]`) referenced from a function lowers to a `static OnceLock<T>`
    // + accessor fn — a module has no `fn main` to run a `let` in. cargo check
    // confirms the accessor (`fn nums() -> &'static Vec<f64>`), the reference
    // routing (`nums()[0]`), and the OnceLock cell all compile.
    let tmp = TempDir::new().expect("tempdir");
    let project = tmp.path().join("probe");
    let target_dir = tmp.path().join("target");
    fs::create_dir_all(project.join("src")).expect("probe src");
    let src =
        "let nums: number[] = [1, 2, 3];\nexport function first(): number { return nums[0]; }";
    let (rust, deps) = Translator::new()
        .translate_with_deps_as(src, FileRole::Module)
        .expect("translate module let source");
    // A module emits declarations only (no `fn main` — arch decision point 8),
    // but the probe crate is a bin target, so check needs an entry. A `fn main
    // {}` stub satisfies it without touching the module items under test.
    let rust = format!("{rust}\nfn main() {{}}\n");
    write_project(&project, &rust, &deps);
    let (ok, err) = cargo(
        &project,
        &target_dir,
        &["check", "--quiet", "--message-format=short"],
    );
    assert!(
        ok,
        "module let lazy-static path must compile: {err}\n--- emitted ---\n{rust}"
    );
}

#[test]
fn entry_let_lazy_static_compiles_to_valid_rust_project() {
    // B3-1b: an entry-file non-mutated `let` (runtime initializer) referenced
    // from a function hoists to a `static OnceLock<T>` + accessor — an entry
    // otherwise leaves it as an `fn main` local a Rust fn item cannot close
    // over. cargo check confirms the hoisted accessor, the function reading it,
    // and the `fn main` calling the function all compile.
    let tmp = TempDir::new().expect("tempdir");
    let project = tmp.path().join("probe");
    let target_dir = tmp.path().join("target");
    fs::create_dir_all(project.join("src")).expect("probe src");
    let src =
        "let nums: number[] = [1, 2, 3];\nfunction first(): number { return nums[0]; }\nconsole.log(first());";
    let (rust, deps) = Translator::new()
        .translate_with_deps(src)
        .expect("translate entry let source");
    write_project(&project, &rust, &deps);
    let (ok, err) = cargo(
        &project,
        &target_dir,
        &["check", "--quiet", "--message-format=short"],
    );
    assert!(
        ok,
        "entry let lazy-static path must compile: {err}\n--- emitted ---\n{rust}"
    );
}

/// Strip host-local absolute paths from a captured tool diagnostic so the
/// committed matrix never embeds a contributor's tempdir. Node/rustc emit the
/// full path — `C:\Users\name\…\Temp\.tmpXXX\wN\work\oracle.ts` on Windows,
/// `/tmp/.tmpXXX/…/oracle.ts` on POSIX — on a failed fixture; replace the
/// absolute prefix with `<path>` and keep the trailing file name (plus any
/// `:line:col`) so the note stays readable. Relative paths (cargo short
/// diagnostics like `src/main.rs`) are left untouched.
fn scrub_local_paths(s: &str) -> String {
    /// Length of the absolute-path token starting at `s`, or `None` if `s`
    /// does not open one. A token runs to the next whitespace or quote.
    fn path_len(s: &str) -> Option<usize> {
        let b = s.as_bytes();
        // Windows drive path: `<letter>:\` or `<letter>:/`.
        if b.len() >= 3
            && b[0].is_ascii_alphabetic()
            && b[1] == b':'
            && (b[2] == b'\\' || b[2] == b'/')
        {
            return Some(consume(b));
        }
        // POSIX absolute prefixes used by TempDir / home / system caches.
        const POSIX: &[&str] = &[
            "/tmp/",
            "/var/",
            "/home/",
            "/Users/",
            "/private/",
            "/root/",
            "/mnt/",
            "/opt/",
            "/proc/",
            "/dev/",
        ];
        POSIX.iter().find(|p| s.starts_with(*p)).map(|_| consume(b))
    }
    fn consume(b: &[u8]) -> usize {
        b.iter()
            .position(|&c| c.is_ascii_whitespace() || c == b'"' || c == b'\'')
            .unwrap_or(b.len())
    }

    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while !rest.is_empty() {
        match path_len(rest) {
            Some(n) => {
                let tail = rest[..n].rsplit(['\\', '/']).next().unwrap_or("");
                out.push_str("<path>");
                out.push_str(tail);
                rest = &rest[n..];
            }
            None => {
                let ch = rest.chars().next().expect("non-empty");
                out.push(ch);
                rest = &rest[ch.len_utf8()..];
            }
        }
    }
    out
}

fn outcome(
    raw: &RawFeature,
    layer: &str,
    status: &'static str,
    detail: String,
    correct: Option<bool>,
) -> Outcome {
    // A failed fixture's `detail` may quote a tool diagnostic (rustc) that
    // embeds the contributor's full tempdir. Scrub host-local absolute paths
    // before the value lands in the committed matrix.
    Outcome {
        id: raw.id.clone(),
        layer: layer.to_string(),
        category: raw.category.clone(),
        status,
        detail: scrub_local_paths(&detail),
        expect: raw.expect.clone(),
        correct,
        note: raw.note.clone(),
    }
}

fn write_project(project: &Path, rust: &str, deps: &RuntimeDeps) {
    // The translator always emits an implicit `fn main` (pure-TS execution
    // semantics: top-level executable statements collect into it; a file with
    // only declarations yields an empty `fn main {}`). The engine path emits
    // its own `fn main { __ds_engine::run(src) }`. Either way, no synthesis.
    let mut body = rust.to_string();
    let mut cargo_toml = MANIFEST.to_string();
    // A fixture that routes an `f64` through ES NumberToString emits a
    // `crate::__ds::number_to_string` call; the probe crate then needs the
    // `__ds` helper module (declared `mod __ds;` at its root) and the `ryu_js`
    // dependency — the same assembly `ds build` performs for a real project.
    if let Some(helper) = deps.helper_module() {
        let _ = fs::write(project.join("src").join("__ds.rs"), helper);
        if !body.contains("mod __ds;") {
            body = format!("mod __ds;\n{body}");
        }
    }
    // Engine compat: a `.ts` source using ES reflection lowers to a single
    // `__ds_engine::run(src)` call; the probe crate then needs the engine
    // helper module (declared `mod __ds_engine;`) — the same assembly `ds
    // build` performs for a real project. `rquickjs` itself lands in
    // Cargo.toml via `apply_to_cargo_toml` below (gated on `needs_engine`).
    if let Some(engine) = deps.engine_helper_module() {
        let _ = fs::write(project.join("src").join("__ds_engine.rs"), engine);
        if !body.contains("mod __ds_engine;") {
            body = format!("mod __ds_engine;\n{body}");
        }
    }
    // Dep injection is independent of the `__ds` helper module: `serde_json`
    // needs the Cargo.toml line but inlines its calls directly (no helper), so
    // apply unconditionally — `apply_to_cargo_toml` is itself a no-op when no
    // dep is flagged. (Tying it to `helper_module` missed serde_json-only files.)
    deps.apply_to_cargo_toml(&mut cargo_toml);
    let _ = fs::write(project.join("Cargo.toml"), cargo_toml);
    let _ = fs::write(project.join("src").join("main.rs"), body);
}

/// Run `cargo <args>` in `project`, sharing `target_dir` across calls.
/// Returns `(success, captured-output)` — stderr for `check`, stdout for `run`.
fn cargo(project: &Path, target_dir: &Path, args: &[&str]) -> (bool, String) {
    let is_run = args.first().is_some_and(|a| *a == "run");
    // The run path executes the probe binary; route it through
    // [`cargo_run_full`] so a hanging fixture is killed at the timeout instead
    // of stalling here on `.output()`. (`cargo check` never hangs — fixtures
    // carry no build scripts — so the check path keeps the simple form.)
    if is_run {
        // The run path executes the probe binary; route it through
        // [`cargo_run_full`] so a hanging fixture is killed at the timeout
        // instead of stalling here on `.output()`. The correctness layer diffs
        // the captured stdout against `expected`; the verdict drives the
        // test262 path. (`cargo check` never hangs — fixtures carry no build
        // scripts — so the check path keeps the simple `.output()` form.)
        let (verdict, stdout) = cargo_run_full(project, target_dir);
        if matches!(verdict, RunOutcome::Ok) {
            let trimmed = stdout
                .lines()
                .filter(|l| !l.trim().is_empty())
                .take(6)
                .collect::<Vec<_>>()
                .join("\n");
            return (true, trimmed);
        }
        return (false, String::new());
    }
    let mut cmd = Command::new("cargo");
    cmd.args(args)
        .env("CARGO_TARGET_DIR", target_dir)
        .current_dir(project);
    // After [`warm_cargo_cache`] succeeds the registry cache is fresh, so force
    // offline — workers then never contend the crates-io registry-update lock.
    if OFFLINE_READY.load(Ordering::SeqCst) {
        cmd.env("CARGO_NET_OFFLINE", "true");
    }
    let out = match cmd.output() {
        Ok(o) => o,
        Err(e) => return (false, format!("cargo invoke failed: {e}")),
    };
    let captured = String::from_utf8_lossy(&out.stderr);
    let trimmed = captured
        .lines()
        .filter(|l| !l.trim().is_empty())
        .take(6)
        .collect::<Vec<_>>()
        .join("\n");
    (out.status.success(), trimmed)
}

/// Set by [`warm_cargo_cache`] once the seed `cargo fetch` has populated
/// `~/.cargo` with every injectable runtime dep. While true, [`cargo`] forces
/// `CARGO_NET_OFFLINE=true` so the parallel workers resolve deps from the
/// cache and never race the crates-io registry-update lock (which surfaces as
/// spurious "unable to update registry `crates-io`" partials). False (warm-up
/// failed, e.g. an offline host) leaves the original online behaviour.
static OFFLINE_READY: AtomicBool = AtomicBool::new(false);

/// Populate `~/.cargo` with every runtime dep a fixture might inject, by
/// fetching from a seed project once, serially, before the parallel workers
/// start. Returns `true` when the fetch succeeded — callers then flip
/// [`OFFLINE_READY`] so workers build offline against the warm cache. Best
/// effort: a fetch failure (offline host) returns `false` and leaves the
/// original online behaviour, so a no-network machine degrades rather than
/// hard-failing every dep-injecting fixture.
fn warm_cargo_cache(seed_project: &Path) -> bool {
    const SEED_TOML: &str = "[package]\nname = \"seed\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n[dependencies]\nryu-js = \"1.0\"\nserde_json = \"1\"\nrquickjs = \"0.12\"\n";
    let _ = fs::write(seed_project.join("Cargo.toml"), SEED_TOML);
    let _ = fs::write(seed_project.join("src").join("main.rs"), "fn main() {}\n");
    let ok = Command::new("cargo")
        .args(["fetch", "--quiet"])
        .current_dir(seed_project)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if ok {
        OFFLINE_READY.store(true, Ordering::SeqCst);
    }
    ok
}

/// Translate a fixture, catching any panic — a `quote`/`Ident::new` on an
/// unsanitisable name, an unwinding translator bug, … — so one bad fixture is
/// reported as `partial` instead of aborting the whole matrix run. `translate`
/// itself returns `Result`; this wraps its panicking paths behind the same
/// error channel (`translate error: …` / `translate panic: …`).
/// The translator reaches into process-global state (the `JS_MODULES` registry,
/// `whole_module_degrade`'s thread-local, the rquickjs runtime lazily
/// initialised along the engine path). Under the matrix's parallel workers two
/// fixtures translating concurrently can observe each other's partial writes —
/// a data race that flips a handful of fixtures' `needs_engine()` verdict
/// (`false`→`true`), surfacing as ~11 phantom `supported` results that vanish
/// under single-threaded runs or any `eprintln` that serialises workers via the
/// stdout lock. The translation itself is millisecond-scale (oxc parse +
/// translate; the slow `cargo build`/run happens *outside* this lock), so
/// serialising just the translate step removes the race without starving the
/// parallel build workers that dominate wall-clock.
static TRANSLATE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn translate_catch(source: &str) -> Result<(String, RuntimeDeps), String> {
    use std::panic::AssertUnwindSafe;
    let _guard = TRANSLATE_LOCK.lock().expect("TRANSLATE_LOCK poisoned");
    std::panic::catch_unwind(AssertUnwindSafe(|| {
        Translator::new().translate_with_deps(source)
    }))
    .map_err(|p| {
        p.downcast_ref::<String>()
            .cloned()
            .or_else(|| p.downcast_ref::<&'static str>().map(|s| s.to_string()))
            .unwrap_or_else(|| "translator panic".to_string())
    })
    .and_then(|r| r.map_err(|e| format!("translate error: {e}")))
}

/// Build and run the compiled probe, returning an assert-driven verdict.
///
/// Build and run are split so a hanging fixture (catastrophic regexp
/// backtracking, an infinite loop) cannot stall the matrix: `cargo build`
/// emits the `probe` binary, then we spawn it directly and cap it at
/// [`PROBE_TIMEOUT_SECS`]. Because the binary is our own child — not a
/// grandchild under `cargo run` — `kill()` reaps it whole, leaving no orphaned
/// loop hoarding a core. The probe's exit code + stderr is the verdict: exit 0
/// (every test262 assert held) → `Ok`; a panicked `Test262Error` (assert
/// mismatch) → `AssertFailed`; a build failure or timeout → `BuildFailed`/
/// `Timeout`; any other non-zero exit → `RunError`.
fn cargo_run_full(project: &Path, target_dir: &Path) -> (RunOutcome, String) {
    use std::io::Read;
    use std::process::Stdio;
    use std::time::Duration;
    use wait_timeout::ChildExt;

    let mut build = Command::new("cargo");
    build
        .args(["build", "--quiet"])
        .env("CARGO_TARGET_DIR", target_dir)
        .current_dir(project);
    if OFFLINE_READY.load(Ordering::SeqCst) {
        build.env("CARGO_NET_OFFLINE", "true");
    }
    let build_out = match build.output() {
        Ok(o) => o,
        Err(e) => {
            return (
                RunOutcome::BuildFailed(format!("spawn: {e}")),
                String::new(),
            )
        }
    };
    if !build_out.status.success() {
        return (
            RunOutcome::BuildFailed(format!(
                "exit {}: {}",
                build_out.status,
                String::from_utf8_lossy(&build_out.stderr)
                    .chars()
                    .take(200)
                    .collect::<String>(),
            )),
            String::new(),
        );
    }

    let bin = target_dir
        .join("debug")
        .join(if cfg!(windows) { "probe.exe" } else { "probe" });
    let mut child = match Command::new(&bin)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return (RunOutcome::RunError(format!("spawn: {e}")), String::new()),
    };
    let status = match child.wait_timeout(Duration::from_secs(PROBE_TIMEOUT_SECS)) {
        Ok(Some(s)) => s,
        _ => {
            let _ = child.kill();
            let _ = child.wait();
            return (RunOutcome::Timeout, String::new());
        }
    };
    if status.success() {
        // On success stdout holds the program's normal output (the correctness
        // layer diffs it against `expected`). Reads of the already-exited
        // child's pipes never block.
        let mut stdout = String::new();
        if let Some(o) = child.stdout.as_mut() {
            let _ = o.read_to_string(&mut stdout);
        }
        return (RunOutcome::Ok, stdout);
    }
    let mut stderr = String::new();
    if let Some(e) = child.stderr.as_mut() {
        let _ = e.read_to_string(&mut stderr);
    }
    // exit≠0: a test262 assert panics `Test262Error: …`, a WPT testharness
    // assert panics `AssertionError: …` (the `__ds::wpt_*` helper prefix); any
    // other panic or non-zero exit is a runtime error. The stderr snippet
    // drives the verdict.
    let snippet = stderr.chars().take(200).collect::<String>();
    if snippet.contains("Test262Error") || snippet.contains("AssertionError") {
        (RunOutcome::AssertFailed(snippet), String::new())
    } else if snippet.contains("ReferenceError") {
        // A degraded body referenced a host global the engine (and DashScript)
        // does not ship — `$262`/`$DONE`/`ShadowRealm`/a Temporal polyfill/… The
        // `throw_msg` extractor surfaces the thrown `ReferenceError: …` as the
        // panic's leading stderr line, so this is honestly `unsupported`, not a
        // half-working `partial`.
        (RunOutcome::ReferenceError(snippet), String::new())
    } else {
        (
            RunOutcome::RunError(format!("exit {}: {}", status, snippet)),
            String::new(),
        )
    }
}

/// Verdict from running a compiled probe binary. Assert-driven: a test262
/// fixture's asserts all hold → `Ok` (supported); a thrown `Test262Error`/
/// `AssertionError` → `AssertFailed` (partial); a degraded body's
/// `ReferenceError` (a host global the engine — and DashScript — lacks:
/// `$262`/`$DONE`/`ShadowRealm`/…) → `ReferenceError` (unsupported); a build
/// or timeout → `BuildFailed`/`Timeout` (unsupported); any other non-zero
/// exit → `RunError` (partial).
enum RunOutcome {
    Ok,
    AssertFailed(String),
    ReferenceError(String),
    BuildFailed(String),
    Timeout,
    RunError(String),
}

/// Map a compiled-probe verdict to a (status, detail) row. No Node oracle: the
/// fixture carries its own expected values, and an assert panic (`Test262Error`
/// for test262, `AssertionError` for WPT) is the single failure signal.
fn judge_run(o: RunOutcome) -> (&'static str, String) {
    match o {
        RunOutcome::Ok => ("supported", String::new()),
        RunOutcome::AssertFailed(d) => ("partial", d),
        RunOutcome::ReferenceError(d) => ("unsupported", format!("engine lacks built-in: {d}")),
        RunOutcome::BuildFailed(d) => ("unsupported", format!("cargo build failed: {d}")),
        RunOutcome::Timeout => ("unsupported", "timed out".into()),
        RunOutcome::RunError(d) => ("partial", format!("runtime error: {d}")),
    }
}

/// Rewrite the extractor's `function main(): void { … } main();` wrapper to an
/// `async function main(): Promise<void>` when the body contains a top-level
/// `await` — the WPT `promise_test(async t => { … await … })` pattern. The
/// static translator lowers an `async function main` to an `async fn
/// __ds_main` awaited from a `#[tokio::main] async fn main`, so the body's
/// `.await` resolves under a runtime; a sync `function main` wrapper would
/// leave the `.await` inside a non-async fn (E0728). A sync body, or a fixture
/// not in the wrapped form, is returned unchanged.
fn rewrap_async_main(fixture: &str) -> String {
    const PREFIX: &str = "function main(): void {\n";
    const SUFFIX: &str = "\n}\nmain();\n";
    let Some(body) = fixture
        .strip_prefix(PREFIX)
        .and_then(|s| s.strip_suffix(SUFFIX))
    else {
        return fixture.to_string();
    };
    // `await` in the body, OR a `promise_test(...)` call: the harness builtin
    // lowers `promise_test` to `wpt_promise_test(name, fut).await`, dropping
    // `.await` into the body even when the source has no `await` keyword (a
    // non-async callback that returns a promise). Either way the body runs in
    // an async main so the injected `.await` resolves under tokio; without
    // this the await lands in a sync `fn main` (E0728).
    let needs_async = body.contains("await") || body.contains("promise_test(");
    if needs_async {
        format!("async function main(): Promise<void> {{\n{body}\n}}\nmain();\n")
    } else {
        fixture.to_string()
    }
}

/// Run one test262 fixture through the assert-driven compile pipeline. Returns
/// `(status, detail)`. Every fixture — static or degrading (`needs_engine`) —
/// takes the same path: `Translator::check` (translatability) → `cargo check`
/// → build + run the probe. A degrading fixture's emitted binary embeds a
/// `__ds_engine` QuickJS that runs the body with the test262 assert family
/// registered as a production builtin (Javy register pattern); a static
/// fixture runs plain Rust. Verdict: exit 0 → `supported`, a panicked
/// `Test262Error` → `partial`, a `ReferenceError` (a host global not shipped)
/// / build failure / timeout → `unsupported`. Translator scope limits stay
/// honestly `unsupported`.
fn run_test262(raw: &RawFeature, project: &Path, target_dir: &Path) -> (&'static str, String) {
    // A fixture whose `features:` the ds engine does not ship has no ds support
    // — `unsupported` up front, skipping translate + cargo.
    for feat in &raw.features {
        if UNSHIPPED_FEATURES.contains(&feat.as_str()) {
            return ("unsupported", format!("ds does not ship feature: {feat}"));
        }
    }
    let (rust, deps) = match translate_catch(&raw.fixture) {
        Ok(r) => r,
        Err(e) => {
            // The translator itself failed (a `quote`/`Ident::new` panic on a
            // construct it cannot lower — no Rust was produced). Honestly
            // `partial` (a translator gap), not an engine-rescuable semantics
            // gap the in-process testbed used to paper over.
            return ("partial", format!("translate: {e}"));
        }
    };
    // A degrading fixture (`needs_engine`) takes the SAME compile path as a
    // static one below: `write_project` emits `__ds_engine` + `wire_web_apis`
    // + the Javy-pattern `register_*` builtins, and the probe's embedded
    // QuickJS runs the degraded body with the test262 assert family + any Web
    // APIs the static path pulled in, registered as production builtins. No
    // in-process testbed — the verdict is what the production binary does.
    // Static path: `check` (translatability) → cargo check (compiles) → build
    // + run the probe (assert-driven verdict).
    let diags = Translator::new().check(&raw.fixture);
    if !diags.is_empty() {
        let msg = diags
            .iter()
            .map(|d| format!("{d}"))
            .collect::<Vec<_>>()
            .join(" | ");
        return ("unsupported", msg);
    }
    write_project(project, &rust, &deps);
    let (ok, err) = cargo(
        project,
        target_dir,
        &["check", "--quiet", "--message-format=short"],
    );
    if !ok {
        // The translator flagged this Mapped but the emitted Rust does not
        // compile (a unification the static classify could not predict).
        // Honestly `partial` — a translator gap the in-process testbed used to
        // paper over; the production binary is what conformance measures now.
        return ("partial", format!("cargo build failed: {err}"));
    }
    let (verdict, _stdout) = cargo_run_full(project, target_dir);
    // judge_run maps every RunOutcome: Ok→supported, AssertFailed→partial,
    // ReferenceError→unsupported (a host global the engine lacks), RunError→
    // partial (a runtime crash the in-process testbed used to rescue).
    judge_run(verdict)
}

fn run_wpt(raw: &RawFeature, project: &Path, target_dir: &Path) -> (&'static str, String) {
    // WinterTC fixtures take the same compile path as test262: `check` →
    // `write_project` (emit `__ds_engine` + `wire_web_apis` + the Javy-pattern
    // `register_*` builtins) → `cargo build` → run the probe. The probe's
    // embedded QuickJS runs the degraded body with the WPT testharness assert
    // family + any Web APIs registered as production builtins; a thrown
    // `AssertionError` is a real partial, a `ReferenceError` (a host global
    // DashScript does not ship) is an honest `unsupported`. No engine_eval
    // testbed — the verdict is what the production binary does.
    //
    // A WPT fixture whose body `await`s needs an async entry — rewrap the
    // extractor's sync `function main` wrapper to `async function main` so the
    // translator emits a `#[tokio::main] async fn main` that resolves the body's
    // `.await` (see [`rewrap_async_main`]). No-op for sync fixtures.
    let fixture = rewrap_async_main(&raw.fixture);
    let diags = Translator::new().check(&fixture);
    if !diags.is_empty() {
        let msg = diags
            .iter()
            .map(|d| format!("{d}"))
            .collect::<Vec<_>>()
            .join(" | ");
        return ("unsupported", msg);
    }
    let (rust, deps) = match translate_catch(&fixture) {
        Ok(r) => r,
        Err(e) => return ("partial", format!("translate: {e}")),
    };
    write_project(project, &rust, &deps);
    let (ok, err) = cargo(
        project,
        target_dir,
        &["check", "--quiet", "--message-format=short"],
    );
    if !ok {
        return ("partial", format!("cargo build failed: {err}"));
    }
    let (verdict, _stdout) = cargo_run_full(project, target_dir);
    judge_run(verdict)
}

/// Whether a WPT fixture's failure detail is a WinterTC out-of-scope pattern
/// (reflection / lone surrogate) rather than an API-behavior gap. WinterTC
/// ECMA-429 §2 conformance is "provide the interfaces/properties per their
/// W3C/WHATWG definition" — API *behavior* — and does not require JS
/// reflection (`instanceof` / `.constructor` / `hasOwnProperty` / property
/// descriptors / prototype chains / idlharness), which assumes a runtime
/// object model a static transpiler does not have. Lone surrogates cannot
/// round-trip through Rust `&str`. Such fixtures are reclassified from
/// `unsupported`/`partial` to `out-of-scope` so the conformance rate reflects
/// API behavior, not reflection parity with a JS engine.
fn wpt_out_of_scope(detail: &str) -> bool {
    const PATTERNS: &[&str] = &[
        "instanceof",
        "hasOwnProperty",
        "accessor properties",
        "reflection is unsupported",
        "verifyProperty",
        "property descriptor",
        "idlharness",
        "lone surrogate",
        "`.constructor`",
    ];
    PATTERNS.iter().any(|p| detail.contains(p))
}

/// One fixture, run against a worker-owned `project`/`target_dir` pair.
/// Unifies the test262 assert-driven path (exit code + Test262Error) and the
/// WPT testharness path (exit code + AssertionError) with the translator-tests/
/// correctness path (cargo check + optional expected-stdout run). Pure over its
/// arguments — no shared mutable state across calls — so it is safe to invoke
/// from many threads in parallel, each on its own project.
fn run_fixture(raw: &RawFeature, layer: &str, project: &Path, target_dir: &Path) -> Outcome {
    if layer == "test262" {
        let (status, detail) = run_test262(raw, project, target_dir);
        return outcome(raw, layer, status, detail, None);
    }
    if layer == "wpt" {
        let (status, detail) = run_wpt(raw, project, target_dir);
        // WinterTC ECMA-429 §2 = API behavior, not JS reflection: reclassify
        // reflection/lone-surrogate failures as out-of-scope (not API gaps).
        let status: &'static str = if status != "supported" && wpt_out_of_scope(&detail) {
            "out-of-scope"
        } else {
            status
        };
        return outcome(raw, layer, status, detail, None);
    }
    let diags = Translator::new().check(&raw.fixture);
    let (status, detail) = if !diags.is_empty() {
        let msg = diags
            .iter()
            .map(|d| format!("{d}"))
            .collect::<Vec<_>>()
            .join(" | ");
        ("unsupported", msg)
    } else {
        let (rust, deps) = match translate_catch(&raw.fixture) {
            Ok(r) => r,
            Err(e) => return outcome(raw, layer, "partial", e, None),
        };
        write_project(project, &rust, &deps);
        let (ok, err) = cargo(
            project,
            target_dir,
            &["check", "--quiet", "--message-format=short"],
        );
        if ok {
            ("supported", String::new())
        } else {
            ("partial", err)
        }
    };

    // Correctness layer — only when the feature compiles AND declares an
    // expected stdout. `console.log(x)` lowers to `println!("{}", x)`
    // (Display, not Debug): fixtures must log primitives or joined strings,
    // never bare Vec/struct (no Display => won't compile).
    let correct = if status == "supported" {
        raw.expect_output.as_ref().map(|expected| {
            let (rust, deps) = translate_catch(&raw.fixture).unwrap_or_default();
            write_project(project, &rust, &deps);
            match cargo(project, target_dir, &["run", "--quiet"]) {
                (true, stdout) => stdout.trim() == expected.trim(),
                _ => false,
            }
        })
    } else {
        None
    };

    outcome(raw, layer, status, detail, correct)
}

/// `tests/conformance/` — the dir this file lives in (data + matrix outputs).
fn conformance_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("conformance")
}

/// Every `<name>.json` stem in `dir` (sorted, lowercased) — the runtime
/// category list behind `DASH_TEST262_CATEGORIES=all` / `DASH_WPT_CATEGORIES=all`,
/// so a full run is one env var rather than a hand-maintained comma list that
/// silently runs nothing when a new category file is added but not listed.
fn discover_categories(dir: &Path) -> Vec<String> {
    let mut cats: Vec<String> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "json") {
                p.file_stem().map(|s| s.to_string_lossy().to_lowercase())
            } else {
                None
            }
        })
        .collect();
    cats.sort();
    cats
}

/// Write one matrix file per test262 category + one each for translator-tests
/// and correctness, plus a README overview. Per-category files (not one giant
/// matrix) match the per-category data and let a single-builtin run update only
/// its own slice.
fn write_matrix_split(outcomes: &[Outcome]) {
    use std::collections::HashSet;
    let dir = conformance_dir().join("matrix");
    let _ = fs::create_dir_all(&dir);

    // test262 + wpt: one file per category (sorted), prefixed by its layer.
    for layer in ["test262", "wpt"] {
        let mut cats: Vec<String> = outcomes
            .iter()
            .filter(|o| o.layer == layer)
            .map(|o| o.category.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        cats.sort();
        for cat in &cats {
            let rows: Vec<&Outcome> = outcomes
                .iter()
                .filter(|o| o.layer == layer && o.category == *cat)
                .collect();
            write_section(&dir.join(format!("{layer}-{cat}")), &rows);
        }
    }
    // translator-tests + correctness: one file each (all categories merged).
    for layer in ["translator-tests", "correctness"] {
        let rows: Vec<&Outcome> = outcomes.iter().filter(|o| o.layer == layer).collect();
        if rows.is_empty() {
            continue;
        }
        write_section(&dir.join(layer), &rows);
    }
    let _ = fs::write(dir.join("README.md"), render_overview_from_disk(&dir));
}

/// Write `<stem>.json` (pretty) + `<stem>.md` (rendered) for one group of rows.
fn write_section(stem: &Path, rows: &[&Outcome]) {
    let owned: Vec<Outcome> = rows.iter().map(|o| (*o).clone()).collect();
    let json = serde_json::to_string_pretty(&owned).unwrap_or_default();
    let _ = fs::write(
        format!("{}.json", stem.to_string_lossy()),
        format!("{json}\n"),
    );
    let _ = fs::write(
        format!("{}.md", stem.to_string_lossy()),
        render_section(&owned),
    );
}

fn render_section(outcomes: &[Outcome]) -> String {
    let total = outcomes.len();
    let supported = outcomes.iter().filter(|o| o.status == "supported").count();
    let partial = outcomes.iter().filter(|o| o.status == "partial").count();
    let unsupported = outcomes
        .iter()
        .filter(|o| o.status == "unsupported")
        .count();
    let out_of_scope = outcomes
        .iter()
        .filter(|o| o.status == "out-of-scope")
        .count();
    let untested = outcomes.iter().filter(|o| o.status == "untested").count();
    let correct = outcomes
        .iter()
        .filter(|o| matches!(o.correct, Some(true)))
        .count();

    let mut categories: Vec<&str> = outcomes.iter().map(|o| o.category.as_str()).collect();
    categories.sort();
    categories.dedup();

    let mut s = String::new();
    s.push_str("# DashScript Conformance Matrix\n\n");
    s.push_str(&format!(
        "- {total} features: **{supported}** supported, **{partial}** partial, **{unsupported}** unsupported, **{out_of_scope}** out-of-scope (reflection/non-API), **{untested}** untested\n",
    ));
    s.push_str(&format!("- correctness cases passing: {correct}\n\n"));

    for cat in categories {
        s.push_str(&format!("## {cat}\n\n"));
        s.push_str("| feature | status | detail / note |\n");
        s.push_str("| --- | --- | --- |\n");
        for o in outcomes.iter().filter(|o| o.category == cat) {
            let badge = badge(o.status);
            let note = if o.detail.is_empty() {
                o.note.clone()
            } else {
                o.detail.clone()
            };
            let note = note.replace('|', "\\|").replace(['\n', '\r'], " ");
            // `correct` folds into the detail column rather than adding a 4th
            // column — the header declares only 3, so a trailing column would
            // break the markdown table render.
            let correct_suffix = match o.correct {
                Some(c) => format!(" _correct: {}_", c),
                None => String::new(),
            };
            s.push_str(&format!(
                "| {} | {} {} | {}{} |\n",
                o.id, badge, o.status, note, correct_suffix
            ));
        }
        s.push('\n');
    }
    s.push_str("\n<!-- Generated by `cargo test -p dashscript --test conformance`. Do not edit by hand. -->\n");
    s
}

/// The matrix index: one row per (layer, category) with supported/partial/
/// unsupported counts and a link to that slice's `.md`. This is the project's
/// ECMAScript-conformance scorecard.
///
/// Aggregated from **every** matrix JSON on disk, not just this run's outcomes,
/// so a single-category run (`DASH_TEST262_CATEGORIES=number`) still lists all
/// categories — the un-run ones keep their last-run slice. Each per-slice
/// `write_section` updates that category's JSON before this is called, so the
/// overview always reflects the fresh data plus the rest of the matrix.
fn render_overview_from_disk(dir: &Path) -> String {
    use std::collections::BTreeMap;
    // A projection of `Outcome` carrying only the fields the overview counts.
    // `Outcome.status` is `&'static str` (built from literals at run time), which
    // does not deserialize from JSON; this owned subset does.
    #[derive(Deserialize)]
    struct Row {
        layer: String,
        category: String,
        status: String,
    }
    // test262: one row per category; translator-tests / correctness: a single
    // merged row (their `category` is a translator-internal path, not a builtin).
    let mut by_key: BTreeMap<(String, String), [usize; 5]> = BTreeMap::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "json") {
                continue;
            }
            let Ok(json) = fs::read_to_string(&path) else {
                continue;
            };
            let Ok(rows) = serde_json::from_str::<Vec<Row>>(&json) else {
                continue;
            };
            for r in rows {
                let key = if r.layer == "test262" || r.layer == "wpt" {
                    (r.layer, r.category)
                } else {
                    (r.layer, String::new())
                };
                let e = by_key.entry(key).or_insert([0, 0, 0, 0, 0]);
                match r.status.as_str() {
                    "supported" => e[0] += 1,
                    "partial" => e[1] += 1,
                    "unsupported" => e[2] += 1,
                    "out-of-scope" => e[3] += 1,
                    _ => e[4] += 1,
                }
            }
        }
    }
    let mut s = String::new();
    s.push_str("# DashScript ECMAScript Conformance\n\n");
    s.push_str(
        "Per-category conformance vs tc39 test262 — assert-driven (a fixture passes \
         when its asserts all hold under DashScript; no Node oracle), plus the \
         translator's own unit-test fixtures and hand-written correctness cases.\n\n",
    );
    s.push_str(
        "Generated by `cargo test -p dashscript --test conformance` — set \
         `DASH_TEST262_CATEGORIES=math,number,…` to scope the test262 layer. The \
         overview aggregates every category's last-run matrix JSON, so a \
         single-category run still shows all categories. Do not edit by hand.\n\n",
    );
    s.push_str("| layer | category | supported | partial | unsupported | out-of-scope | other |\n");
    s.push_str("| --- | --- | ---: | ---: | ---: | ---: | ---: |\n");
    for ((layer, cat), c) in &by_key {
        let link = if layer == "test262" || layer == "wpt" {
            format!("[{cat}]({layer}-{cat}.md)")
        } else {
            format!("[{layer}]({layer}.md)")
        };
        s.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} |\n",
            layer, link, c[0], c[1], c[2], c[3], c[4]
        ));
    }
    s
}

fn badge(status: &str) -> &'static str {
    match status {
        "supported" => "🟢",
        "partial" => "🟡",
        "unsupported" => "🔴",
        "untested" => "⚪",
        _ => "❓",
    }
}

#[test]
fn scrub_local_paths_strips_tempdir_only() {
    // Windows tempdir leak → drop the absolute prefix, keep file name + line.
    let win = r"C:\Users\abc\AppData\Local\Temp\.tmp0maPBl\w7\work\oracle.ts:2 var OSymbol";
    assert_eq!(scrub_local_paths(win), "<path>oracle.ts:2 var OSymbol");
    // POSIX tempdir leak.
    let posix = "/tmp/.tmpXYZ/w3/work/oracle.ts";
    assert_eq!(scrub_local_paths(posix), "<path>oracle.ts");
    // Relative paths (cargo short diagnostics) are untouched.
    assert_eq!(scrub_local_paths("src/main.rs:10:5"), "src/main.rs:10:5");
    // Prose with a colon is not mistaken for a drive path.
    assert_eq!(
        scrub_local_paths("oracle diff: 1 vs 2"),
        "oracle diff: 1 vs 2"
    );
    // Two absolute paths in one line are both scrubbed.
    let two = r"C:\Temp\a.ts and /tmp/b.ts";
    assert_eq!(scrub_local_paths(two), "<path>a.ts and <path>b.ts");
}

#[test]
fn function_expression_lowers_to_closure_not_engine() {
    // task #421: a `function` expression as a callback (or IIFE) lowers to a
    // closure (`function_expr_to_closure`), the same shape a block-body arrow
    // takes — it no longer degrades to the engine. A body using `this` keeps
    // the closure shape but its `this` emits `compile_error!`; cargo check
    // then fails and the enclosing function degrades (covered by the harness'
    // cargo-check-fail fallback, not re-asserted here).
    let classify_static = |src: &str| !Translator::new().uses_engine(src);
    // 1. classify: a callback / IIFE no longer triggers engine degrade.
    assert!(
        classify_static("[1, 2].map(function (x) { return x * 2; });"),
        "callback `function` expression must classify as Mapped"
    );
    assert!(
        classify_static("(function () { return 1; })();"),
        "IIFE `function` expression must classify as Mapped"
    );
    // A `function` expression body using `this` (no static lowering — `this` is
    // only valid in a class method) must still route to the engine, since the
    // static emit would produce `compile_error!` and break `ds build` (the
    // conformance harness' cargo-check-fail fallback is harness-only).
    assert!(
        !classify_static("[1, 2].map(function (x) { return this; });"),
        "a `function` expression body using `this` must route to the engine"
    );
    // 2. emit: translate produces a closure, not todo!().
    let (rust, deps) = Translator::new()
        .translate_with_deps("[1, 2].map(function (x) { return x * 2; });")
        .expect("translate callback source");
    assert!(
        !rust.contains("todo!()"),
        "function-expression callback must lower to a closure, not todo!():\n{rust}"
    );
    // 3. cargo check passes — the static closure is valid Rust.
    let tmp = TempDir::new().expect("tempdir");
    let project = tmp.path().join("probe");
    let target_dir = tmp.path().join("target");
    fs::create_dir_all(project.join("src")).expect("probe src");
    write_project(&project, &rust, &deps);
    let (ok, err) = cargo(
        &project,
        &target_dir,
        &["check", "--quiet", "--message-format=short"],
    );
    assert!(
        ok,
        "static function-expression callback must compile: {err}\n--- emitted ---\n{rust}"
    );
}
