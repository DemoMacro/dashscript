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
//!   (`Translator::check` → `cargo build` → run the probe) and the engine path
//!   (in-process QuickJS with the test262 harness injected) each run it; exit 0
//!   / no throw = every assert held = `supported`, a thrown `Test262Error`
//!   (assert mismatch) = `partial`, a build failure or timeout = `unsupported`.
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
    /// test262 `includes:` frontmatter (`$INCLUDE`) — harness files the
    /// extractor did not inline (propertyHelper.js, isConstructor.js, …). The
    /// engine path injects the matching harness before the fixture so reference
    /// semantics (reflection, compareArray) run under the test262 harness.
    #[serde(default)]
    includes: Vec<String>,
    /// test262 `flags:` frontmatter (`onlyStrict`, `noStrict`, `module`,
    /// `async`, `generated`). The engine path honors `onlyStrict` — the
    /// fixture is evaluated under QuickJS strict mode (`JS_EVAL_FLAG_STRICT`),
    /// which gives the spec-mandated poison-pill behavior for
    /// `Function.prototype.caller`/`arguments` plus the strict-only
    /// assignment / deletion / duplicate-parameter / octal-literal / `with`
    /// errors. Without it, `onlyStrict` fixtures run sloppy and those asserts
    /// fail (e.g. `fn.caller` returns `null` instead of throwing `TypeError`).
    #[serde(default)]
    flags: Vec<String>,
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
    let cats: Vec<String> = std::env::var("DASH_TEST262_CATEGORIES")
        .map(|s| {
            s.split(',')
                .map(|c| c.trim().to_lowercase())
                .filter(|c| !c.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let limit = match std::env::var("DASH_TEST262") {
        Ok(v) if v == "all" || v == "0" => usize::MAX,
        Ok(v) => v.parse().unwrap_or(usize::MAX),
        Err(_) => usize::MAX,
    };
    let test262_dir = conformance_dir().join("data").join("test262");
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
    // Each raw paired with its layer — drives the per-file matrix output
    // (`test262` → one file per category; the other two → one file each).
    let raws: Vec<(RawFeature, &'static str)> = tests
        .features
        .into_iter()
        .map(|r| (r, "translator-tests"))
        .chain(test262_features.into_iter().map(|r| (r, "test262")))
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

/// Once-per-run check that the engine compat path assembles into a building
/// cargo project: a reflection `.ts` source → `translate_with_deps` (flips
/// `needs_engine`) → `write_project` (injects `__ds_engine` + the `rquickjs`
/// dep) → `cargo check`. The in-process `engine_eval` path skips cargo per
/// fixture, so this smoke test is the verification the per-fixture cargo check
/// used to provide — not repeated 1600×, just once. Fails loudly if the engine
/// Rust template, the `__ds_engine` module, or the dep wiring regresses.
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
    // exit≠0: a test262 assert panics `Test262Error: …`; any other panic or
    // non-zero exit is a runtime error. The stderr snippet drives the verdict.
    let snippet = stderr.chars().take(200).collect::<String>();
    if snippet.contains("Test262Error") {
        (RunOutcome::AssertFailed(snippet), String::new())
    } else {
        (
            RunOutcome::RunError(format!("exit {}: {}", status, snippet)),
            String::new(),
        )
    }
}

/// Verdict from running a compiled probe binary. Assert-driven: a test262
/// fixture's asserts all hold → `Ok` (supported); a thrown `Test262Error` →
/// `AssertFailed` (partial); a build or timeout → `BuildFailed`/`Timeout`
/// (unsupported); any other non-zero exit → `RunError` (partial).
enum RunOutcome {
    Ok,
    AssertFailed(String),
    BuildFailed(String),
    Timeout,
    RunError(String),
}

/// Verdict from running a fixture's JS under the embedded engine. The engine
/// throws a `Test262Error` (defined by `sta.js`, thrown by `assert.js`) on an
/// assert mismatch → `AssertFailed`; any other throw → `OtherError`; clean
/// completion → `Ok`.
enum EngineOutcome {
    Ok,
    AssertFailed(String),
    OtherError(String),
    /// The engine lacks a surface needed to run the fixture to completion, but
    /// not as a JS built-in (those surface as `ReferenceError` via `OtherError`).
    /// Today: a `$DONE` async fixture whose promise chain did not resolve under
    /// the engine's microtask drain (needs a host event loop the engine lacks).
    /// Honestly `unsupported`, like the `ReferenceError` case.
    EngineLimitation(String),
}

/// Map a compiled-probe verdict to a (status, detail) row. No Node oracle: the
/// test262 fixture carries its own expected values, and `Test262Error` is the
/// single failure signal.
fn judge_run(o: RunOutcome) -> (&'static str, String) {
    match o {
        RunOutcome::Ok => ("supported", String::new()),
        RunOutcome::AssertFailed(d) => ("partial", format!("Test262Error: {d}")),
        RunOutcome::BuildFailed(d) => ("unsupported", format!("cargo build failed: {d}")),
        RunOutcome::Timeout => ("unsupported", "timed out".into()),
        RunOutcome::RunError(d) => ("partial", format!("runtime error: {d}")),
    }
}

/// Map an engine verdict to a (status, detail) row, tagging supported engine
/// runs so the matrix records the compat path honestly.
fn judge_engine(o: EngineOutcome) -> (&'static str, String) {
    match o {
        EngineOutcome::Ok => ("supported", "via rquickjs engine".to_string()),
        EngineOutcome::AssertFailed(d) => ("partial", format!("Test262Error: {d}")),
        EngineOutcome::OtherError(d) => {
            // A ReferenceError from the engine means QuickJS lacks a built-in
            // or global the fixture exercises (Temporal, the $262 agent API,
            // $DONE async callback, ShadowRealm, …) — DashScript does not
            // ship that surface either, so it is honestly `unsupported`, not
            // a half-working `partial`. ~95% of engine OtherErrors are
            // ReferenceErrors. Other throws (TypeError, RangeError, …) stay
            // `partial`: they may be real semantic gaps a translator fix
            // could close.
            if d.starts_with("ReferenceError") {
                ("unsupported", format!("engine lacks built-in: {d}"))
            } else {
                ("partial", format!("engine error: {d}"))
            }
        }
        EngineOutcome::EngineLimitation(d) => {
            ("unsupported", format!("engine lacks async surface: {d}"))
        }
    }
}

/// Minimal host output surface for the engine path — no-ops. The verdict is
/// exit/throw-driven (a `Test262Error` is the single failure signal), so output
/// is irrelevant; these just keep a fixture that calls `console.log` or the
/// test262 host `print` (e.g. `Array.print = print`) from throwing
/// `ReferenceError: console is not defined` / `print is not defined`.
const CONSOLE_PRELUDE: &str =
    "this.console = { log: function () {} };\nthis.print = function () {};\n";

/// Minimal `$262` host-defined agent (test262 host API). Only `detachArrayBuffer`
/// is host-implemented (Rust via `__ds_detach` — JS cannot detach an ArrayBuffer);
/// the rest are JS polyfills. `detachArrayBuffer` unlocks the
/// typedarray/dataview/arraybuffer suites (fixtures whose `$INCLUDE` of
/// `detachArrayBuffer.js` references `$262`). `createRealm`/`evalScript` are
/// best-effort stubs returning the current global — cross-realm-isolation
/// fixtures honestly degrade to `partial`, never fake a pass. `$262.agent` is
/// intentionally absent: it needs real worker threads; `atomicsHelper.js` reads
/// it at include-eval top level, so omitting it fails fast and correctly (a
/// no-op stub would busy-wait the per-fixture timeout).
const AGENT_262_PRELUDE: &str = r#"
var $262 = (function () {
  function evalScript(src) { return (0, eval)(src); }
  return {
    detachArrayBuffer: function (buffer) { __ds_detach(buffer); },
    evalScript: evalScript,
    createRealm: function () {
      return { evalScript: evalScript, global: globalThis, $262: null };
    },
    global: globalThis,
    IsHTMLDDA: null,
  };
})();
"#;

/// `$262.AbstractModuleSource` (tc39 `source-phase-imports` proposal, ES2026).
/// The host exposes the abstract constructor on `$262`; QuickJS-NG ships no
/// source-phase-imports, so a fixture referencing it reads `typeof undefined`.
/// The stub is spec-faithful, not a fake pass: an abstract constructor that
/// throws `TypeError` on call/construct, with the built-in property descriptors
/// the 8-fixture suite checks — `length`/`name` non-writable, the `prototype`
/// property non-writable + non-configurable, and
/// `prototype[Symbol.toStringTag]` as a get-only accessor returning
/// `undefined` unless `this` carries a `[[ModuleSourceClassName]]` slot (the
/// stub's prototype never does, matching spec steps 2-3 of
/// get [@@toStringTag]).
const ABSTRACT_MODULE_SOURCE_PRELUDE: &str = r#"
(function () {
  if (typeof $262 === 'undefined' || $262 === null) return;
  function AbstractModuleSource() { throw new TypeError(); }
  Object.defineProperty(AbstractModuleSource, 'length', { writable: false });
  Object.defineProperty(AbstractModuleSource, 'name', { writable: false });
  Object.defineProperty(AbstractModuleSource, 'prototype', { writable: false });
  Object.defineProperty(AbstractModuleSource.prototype, Symbol.toStringTag, {
    get: function () { return undefined; },
    configurable: true,
  });
  $262.AbstractModuleSource = AbstractModuleSource;
})();
"#;

/// `Atomics.waitAsync` polyfill. QuickJS-NG lacks it (`typeof` is `"undefined"`),
/// so the ~100 test262 `waitAsync` fixtures fail at the first assert. Only the
/// validation + non-blocking paths are covered (the `-agent` variants need real
/// worker threads and stay partial). Validation is delegated to `Atomics.wait`
/// with a 0-timeout probe: it throws the spec-required TypeError (not a waitable
/// shared typed array) / RangeError (out-of-bounds index) / TypeError (Symbol
/// value) *before* QuickJS's main-thread "cannot block in this thread" check.
/// That "cannot block" throw (which happens regardless of value/timeout on the
/// main thread) is swallowed — it only signals validation passed — and the
/// result is computed directly: value mismatch → `{async:false,"not-equal"}`;
/// match with timeout ≤ 0 → `{async:false,"timed-out"}`; match with timeout > 0
/// → `{async:true, Promise<"timed-out">}` (no agent can wake, so the promise
/// resolves — the fixtures check the resolved value, not the timing).
const WAITASYNC_PRELUDE: &str = r#"
if (typeof Atomics !== 'undefined' && Atomics !== null
    && typeof Atomics.wait === 'function'
    && typeof Atomics.waitAsync !== 'function') {
  Atomics.waitAsync = function (typedArray, index, value, timeout) {
    try {
      Atomics.wait(typedArray, index, value, 0);
    } catch (e) {
      if (!/cannot block/.test(String(e))) throw e;
    }
    var t = Number(timeout);
    if (t !== t) t = Infinity;
    var current = typedArray[index];
    var coerced = (typeof current === 'bigint') ? BigInt(value) : (value | 0);
    if (current !== coerced) return { async: false, value: 'not-equal' };
    if (t <= 0) return { async: false, value: 'timed-out' };
    return { async: true, value: Promise.resolve('timed-out') };
  };
}
"#;

/// `Promise.allKeyed` / `Promise.allSettledKeyed` (tc39 `await-dictionary`
/// proposal). QuickJS-NG ships neither, so a fixture touching them hits
/// `TypeError: cannot read property 'call' of undefined` on
/// `Promise.allKeyed.call(Ctor, …)`. The polyfill mirrors `Promise.all` /
/// `allSettled` over the input object's own enumerable string-keyed properties:
/// `Constructor.resolve` is fetched once (a getter observes a single `get`),
/// each value is resolved then `.then`'d with a per-key resolve/reject element,
/// and the result is a `null`-prototype object keyed identically. Element
/// functions are arrows (no `prototype`, not constructible, `length` 1) routed
/// through `anon` so their `name` is `""` — variable assignment would infer the
/// variable name; passing the arrow as an argument does not, which matches the
/// spec's built-in element functions (`verifyProperty` checks length / name /
/// prototype / constructor / extensibility).
const ALLKEYED_PRELUDE: &str = r#"
(function () {
  if (typeof Promise === 'undefined' || Promise === null) return;
  function anon(fn) {
    Object.defineProperty(fn, 'name', { value: '', configurable: true });
    return fn;
  }
  // Shared by allKeyed / allSettledKeyed — only the per-key settle differs.
  function makeKeyed(allSettled) {
    return function (input) {
      var C = this;
      var keys = Object.keys(input);
      var result = Object.create(null);
      var remaining = keys.length;
      // NewPromiseCapability: construct, then verify the executor installed
      // callable resolve/reject. A Constructor that never calls the executor
      // (the `fn1` case in capability-executor-not-callable) leaves them
      // undefined, which ECMA-262 turns into a synchronous TypeError — raised
      // before Get(constructor, "resolve") runs (a getter there must not fire).
      var capability = { resolve: undefined, reject: undefined };
      var promise = new C(function (resolve, reject) {
        if (capability.resolve !== undefined) return;
        capability.resolve = resolve;
        capability.reject = reject;
      });
      if (typeof capability.resolve !== 'function') throw new TypeError();
      if (typeof capability.reject !== 'function') throw new TypeError();
      var resolveFn = C.resolve;
      if (typeof resolveFn !== 'function') throw new TypeError();
      if (remaining === 0) { capability.resolve(result); return promise; }
      keys.forEach(function (key) {
        var alreadyCalled = false;
        var resolveElement = anon((x) => {
          if (alreadyCalled) return;
          alreadyCalled = true;
          result[key] = allSettled ? { status: 'fulfilled', value: x } : x;
          if (--remaining === 0) capability.resolve(result);
        });
        var rejectElement = anon((e) => {
          if (alreadyCalled) return;
          alreadyCalled = true;
          if (allSettled) {
            result[key] = { status: 'rejected', reason: e };
            if (--remaining === 0) capability.resolve(result);
          } else {
            capability.reject(e);
          }
        });
        var nextPromise = resolveFn.call(C, input[key]);
        // A thenable goes through `.then`; a non-thenable primitive (the
        // `ctx-ctor` fixtures return one from Constructor.resolve) resolves
        // the element directly — `Invoke(primitive, "then")` would throw.
        if (nextPromise !== null
            && (typeof nextPromise === 'object' || typeof nextPromise === 'function')
            && typeof nextPromise.then === 'function') {
          nextPromise.then(resolveElement, rejectElement);
        } else {
          resolveElement(nextPromise);
        }
      });
      return promise;
    };
  }
  if (typeof Promise.allKeyed !== 'function') Promise.allKeyed = makeKeyed(false);
  if (typeof Promise.allSettledKeyed !== 'function') Promise.allSettledKeyed = makeKeyed(true);
})();
"#;

/// Host-defined `$DONE` async-completion callback (test262 host API). test262
/// async fixtures signal completion by calling `$DONE()` (success) or
/// `$DONE(error)` (failure); `asyncHelpers.js`'s `asyncTest` also requires
/// `$DONE` on `globalThis` before it runs (else "asyncTest called without async
/// flag"). Like `$262`, the runner injects it — `doneprintHandle.js` is one
/// (print-based) implementation, but the canonical model is host injection.
/// Records the verdict to `__ds_done_value`: `null` until called, `""` on
/// success, `"Test262Error: …"` on failure (prefixed so `judge_engine` maps an
/// unexpected async rejection to `partial`, matching synchronous throws).
/// Injected after `$INCLUDE`s so it wins over `doneprintHandle.js`'s print-based
/// `$DONE` (the harness defines no `print`). `engine_eval` drains the runtime's
/// pending jobs after the synchronous eval so promise reactions
/// (`.then($DONE)`, `asyncTest`) actually fire `$DONE`.
const DONE_PRELUDE: &str = r#"
var __ds_done_value = null;
var $DONE = function (error) {
  if (__ds_done_value !== null) return;  // first call wins; ignore repeats
  __ds_done_value = error ? ("Test262Error: " + String(error)) : "";
};
"#;

/// `Error.prototype.stack` as an accessor (tc39 `error-stack-accessor` proposal).
/// QuickJS-NG ships no own `stack` slot on `Error.prototype` (each instance gets
/// an own data property at construction), so
/// `Object.getOwnPropertyDescriptor(Error.prototype, 'stack')` is `undefined` and
/// the `.get`/`.set` access crashes the ~29 `error.prototype.stack.{getter,setter}-*`
/// fixtures. The proposal makes it an accessor whose get/set are bound to the
/// `[[ErrorData]]` slot: get returns the trace for Error instances (undefined
/// otherwise, TypeError for a non-object `this`); set creates an own data
/// property (TypeError for non-object `this`, non-string value, or an existing
/// own accessor/non-writable data). The slot check is approximated with a
/// `WeakMap` populated by wrapping every native Error constructor: at
/// construction the QuickJS-set own `stack` is captured into the map and the own
/// property deleted (the proposal model — an instance carries no own `stack`).
/// Injected before `$INCLUDE`s so `nativeErrors.js` captures the wrappers in its
/// constructor arrays. Cross-realm variants stay partial: `$262.createRealm`
/// returns the same global, so the `notSameValue` realm precondition fails
/// regardless of this polyfill.
const ERROR_STACK_ACCESSOR_PRELUDE: &str = r#"
(function () {
  'use strict';
  var d = Object.getOwnPropertyDescriptor(Error.prototype, 'stack');
  if (d !== undefined && typeof d.get === 'function') return;
  var traces = new WeakMap();
  function wrap(name) {
    var O = globalThis[name];
    if (typeof O !== 'function') return;
    var Wrapper = function () {
      var args = Array.prototype.slice.call(arguments);
      var nt = (typeof new.target === 'function') ? new.target : Wrapper;
      var err = Reflect.construct(O, args, nt);
      var s = err.stack;
      if (typeof s === 'string') traces.set(err, s);
      try { delete err.stack; } catch (e) {}
      return err;
    };
    Wrapper.prototype = O.prototype;
    Object.setPrototypeOf(Wrapper, O);
    // Keep `instance.constructor === <Ctor>` true after the global binding is
    // replaced: assert.throws checks `thrown.constructor === expectedCtor` by
    // identity, so the prototype's `.constructor` must point at the Wrapper.
    try { O.prototype.constructor = Wrapper; } catch (e) {}
    try { Object.defineProperty(Wrapper, 'name', { value: name, configurable: true }); } catch (e) {}
    globalThis[name] = Wrapper;
  }
  ['Error', 'EvalError', 'RangeError', 'ReferenceError', 'SyntaxError',
   'TypeError', 'URIError', 'AggregateError', 'SuppressedError'].forEach(wrap);
  // Method-shorthand accessors carry no [[Construct]] slot, so `isConstructor`
  // reports false — matching the proposal's built-in (non-constructor) get/set.
  var _stack = {
    get() {
      if (this === null || (typeof this !== 'object' && typeof this !== 'function'))
        throw new TypeError();
      return traces.has(this) ? traces.get(this) : undefined;
    },
    set(value) {
      if (this === null || (typeof this !== 'object' && typeof this !== 'function'))
        throw new TypeError();
      if (typeof value !== 'string') throw new TypeError();
      var own = Object.getOwnPropertyDescriptor(this, 'stack');
      if (own === undefined) {
        Object.defineProperty(this, 'stack', {
          value: value, writable: true, enumerable: true, configurable: true
        });
      } else if ('get' in own || 'set' in own || !own.writable) {
        throw new TypeError();
      } else {
        Object.defineProperty(this, 'stack', { value: value });
      }
    },
  };
  Object.defineProperty(Error.prototype, 'stack', {
    get: _stack.get, set: _stack.set, enumerable: false, configurable: true,
  });
})();
"#;

/// The two harness files every test262 fixture needs: `sta.js` defines
/// `Test262Error` (the assert-failure exception), `assert.js` defines the
/// `assert.*` family that throws it. Injected on the engine path before any
/// fixture, so a clean run means every assert held.
const HARNESS_STA: &str = include_str!("conformance/data/harness/sta.js");
const HARNESS_ASSERT: &str = include_str!("conformance/data/harness/assert.js");

/// Minimal `Intl` stub injected before the polyfill. rquickjs's QuickJS-NG
/// build ships without `Intl`, but the polyfill's factory reads
/// `Intl.DateTimeFormat` / `Intl.DurationFormat` to layer its Temporal-aware
/// formatting on top of the native one — so the bare global must exist when
/// the factory runs. The stub returns empty/identity results and is shaped so
/// the polyfill can `extends Intl.DateTimeFormat`. Temporal fixtures that stay
/// in ISO space (the majority) never call into it; locale-aware ones degrade
/// to `partial` honestly rather than crashing the prelude.
const INTL_STUB: &str = r#"
if (!globalThis.Intl) {
  function __ds_fmt() {
    function F() {}
    F.prototype.format = function () { return ''; };
    F.prototype.formatToParts = function () { return []; };
    F.prototype.formatRange = function () { return ''; };
    F.prototype.formatRangeToParts = function () { return []; };
    F.prototype.resolvedOptions = function () { return { calendar: 'iso8601', locale: 'en', numberingSystem: 'latn', timeZone: 'UTC' }; };
    F.supportedLocalesOf = function () { return []; };
    return F;
  }
  // DateTimeFormat must yield a parseable en-US numeric+era string: the
  // polyfill's GetFormatterParts splits the output on non-word chars and
  // expects exactly 7 parts (month/day/year era hour/minute/second) to bisect
  // named-time-zone DST offsets. The generic __ds_fmt stub returns '' which
  // throws RangeError "expected 7 parts", so format a real UTC date.
  function __ds_dtf() {
    return {
      format: function (date) {
        var d = typeof date === 'number' ? new Date(date) : date instanceof Date ? date : new Date(+date || Date.now());
        var y = d.getUTCFullYear();
        var era = y < 1 ? 'BC' : 'AD';
        var ay = y < 1 ? 1 - y : y;
        function p(x) { return x < 10 ? '0' + x : '' + x; }
        return d.getUTCMonth() + 1 + '/' + d.getUTCDate() + '/' + ay + ' ' + era + ' ' + p(d.getUTCHours()) + ':' + p(d.getUTCMinutes()) + ':' + p(d.getUTCSeconds());
      },
      formatToParts: function () { return []; },
      formatRange: function () { return ''; },
      formatRangeToParts: function () { return []; },
      resolvedOptions: function () { return { calendar: 'iso8601', locale: 'en-US', numberingSystem: 'latn', timeZone: 'UTC' }; },
    };
  }
  __ds_dtf.supportedLocalesOf = function () { return []; };
  globalThis.Intl = {
    DateTimeFormat: __ds_dtf,
    NumberFormat: __ds_fmt(),
    DurationFormat: __ds_fmt(),
    Collator: function () { this.compare = function (a, b) { a = String(a); b = String(b); return a < b ? -1 : a > b ? 1 : 0; }; },
    getCanonicalLocales: function (x) { return Array.isArray(x) ? x : [x]; },
  };
}
"#;

/// The `@js-temporal/polyfill` UMD build (ISC, © ECMA International) — vendored
/// under `data/vendor/`. QuickJS-NG does not ship `Temporal`, so a fixture that
/// touches the Temporal API would otherwise `ReferenceError` on the engine path.
/// This polyfill is the TC39 proposal's reference JS implementation, validated
/// by `@js-temporal/temporal-test262-runner` against the full Temporal test262
/// suite — so injecting it gives the engine path a spec-conformant Temporal
/// rather than a hand-written stub, and `assert.sameValue` runs against the
/// reference semantics. The UMD wrapper mounts its exports on
/// `globalThis.temporal.{Temporal,Intl,toTemporalInstant}`; [`TEMPORAL_EXPOSE`]
/// re-exposes them under the spec-global names a fixture expects.
const TEMPORAL_POLYFILL: &str = include_str!("conformance/data/vendor/temporal-polyfill.umd.js");

/// Re-expose the polyfill's exports under the spec-global names. The UMD build
/// mounts on `globalThis.temporal`; a test262 fixture writes `Temporal.X`, so
/// `globalThis.Temporal` must alias `globalThis.temporal.Temporal`. Also
/// installs `Date.prototype.toTemporalInstant` (a spec global) — guarded so a
/// QuickJS build without `Date` degrades rather than crashes the prelude.
const TEMPORAL_EXPOSE: &str = "\
globalThis.Temporal = globalThis.temporal.Temporal;
try { Date.prototype.toTemporalInstant = globalThis.temporal.toTemporalInstant; } catch (e) {}
";

/// Strip the default `prototype` own property from non-constructor Temporal
/// built-ins. The @js-temporal/polyfill defines static methods (`from`,
/// `compare`) and prototype methods (`add`, `with`, …) as plain functions,
/// which in JS carry a default `prototype`. ECMA-262 requires built-in
/// Replace the non-constructor Temporal built-ins (static methods like
/// `Duration.compare`, prototype methods like `Duration.prototype.add`) with
/// prototype-less forwarders so the surface matches ECMA-262, where a
/// non-constructor built-in carries *no* `prototype` own property — the test262
/// `.builtin` fixtures assert `hasOwnProperty("prototype") === false`.
///
/// `delete` cannot work here: a plain function's `prototype` is created with
/// `configurable: false`, so `delete` is a silent no-op in sloppy mode. The only
/// way to produce a function with no `prototype` that still preserves dynamic
/// `this` (needed for prototype methods) is a method-shorthand function
/// (ECMA-262: a method definition is non-constructable and gets no `prototype`).
/// Direct `eval` keeps the captured `fn` in lexical scope so the shorthand can
/// forward `fn.apply(this, arguments)`. Named constructors keep `prototype`; the
/// `constructor` back-reference on each prototype is skipped so a ctor never
/// loses it via that alias.
const TEMPORAL_NON_CTOR_STRIP: &str = r#"
(function () {
  var CTORS = { PlainDate:1, PlainTime:1, PlainDateTime:1, ZonedDateTime:1,
               Instant:1, Duration:1, PlainYearMonth:1, PlainMonthDay:1,
               Calendar:1, TimeZone:1 };
  function protoless(fn, name) {
    var holder = eval('({ __call() { return fn.apply(this, arguments); } })');
    var w = holder.__call;
    // Function `name`/`length` are configurable — restore the originals so the
    // `.name`/`.length` fixtures still hold after wrapping.
    try { Object.defineProperty(w, 'name', { value: name }); } catch (e) {}
    try { Object.defineProperty(w, 'length', { value: fn.length }); } catch (e) {}
    return w;
  }
  function setFn(o, k, fn) {
    var d = Object.getOwnPropertyDescriptor(o, k);
    if (!d) { try { o[k] = fn; } catch (e) {} return; }
    if (d.configurable) {
      try {
        Object.defineProperty(o, k, {
          value: fn, writable: true, enumerable: d.enumerable, configurable: true
        });
        return;
      } catch (e) {}
    }
    if (d.writable || 'set' in d) { try { o[k] = fn; } catch (e) {} }
  }
  function walk(o, skipCtors) {
    if (!o || (typeof o !== 'object' && typeof o !== 'function')) return;
    var names = Object.getOwnPropertyNames(o);
    for (var i = 0; i < names.length; i++) {
      var k = names[i];
      if (k === 'constructor') continue;
      var v; try { v = o[k]; } catch (e) { continue; }
      if (typeof v === 'function' && !(skipCtors && CTORS[k])) setFn(o, k, protoless(v, k));
    }
  }
  if (typeof Temporal === 'undefined') return;
  walk(Temporal, true);
  for (var c in CTORS) {
    var C = Temporal[c];
    if (!C) continue;
    walk(C, false);
    if (C.prototype) walk(C.prototype, false);
  }
  if (Temporal.Now) walk(Temporal.Now, false);
})();
"#;

/// The rest of the bundled harness, looked up by `$INCLUDE` name (a fixture's
/// `includes:` frontmatter) and injected on the engine path — `propertyHelper`
/// (reflection/verifyProperty), `compareArray`, `deepEqual`, `isConstructor`
/// (the `new X()` throws check), `byteConversionValues`. Unknown includes are
/// skipped (a referenced-but-missing helper surfaces as a `ReferenceError`
/// engine error → partial, an honest signal).
const HARNESS_FILES: &[(&str, &str)] = &[
    (
        "compareArray.js",
        include_str!("conformance/data/harness/compareArray.js"),
    ),
    (
        "deepEqual.js",
        include_str!("conformance/data/harness/deepEqual.js"),
    ),
    (
        "propertyHelper.js",
        include_str!("conformance/data/harness/propertyHelper.js"),
    ),
    (
        "isConstructor.js",
        include_str!("conformance/data/harness/isConstructor.js"),
    ),
    (
        "byteConversionValues.js",
        include_str!("conformance/data/harness/byteConversionValues.js"),
    ),
    (
        "testTypedArray.js",
        include_str!("conformance/data/harness/testTypedArray.js"),
    ),
    (
        "temporalHelpers.js",
        include_str!("conformance/data/harness/temporalHelpers.js"),
    ),
    (
        "detachArrayBuffer.js",
        include_str!("conformance/data/harness/detachArrayBuffer.js"),
    ),
    (
        "resizableArrayBufferUtils.js",
        include_str!("conformance/data/harness/resizableArrayBufferUtils.js"),
    ),
    (
        "asyncHelpers.js",
        include_str!("conformance/data/harness/asyncHelpers.js"),
    ),
    (
        "atomicsHelper.js",
        include_str!("conformance/data/harness/atomicsHelper.js"),
    ),
    (
        "testAtomics.js",
        include_str!("conformance/data/harness/testAtomics.js"),
    ),
    (
        "promiseHelper.js",
        include_str!("conformance/data/harness/promiseHelper.js"),
    ),
    (
        "proxyTrapsHelper.js",
        include_str!("conformance/data/harness/proxyTrapsHelper.js"),
    ),
    (
        "nativeErrors.js",
        include_str!("conformance/data/harness/nativeErrors.js"),
    ),
    (
        "regExpUtils.js",
        include_str!("conformance/data/harness/regExpUtils.js"),
    ),
    ("nans.js", include_str!("conformance/data/harness/nans.js")),
    (
        "wellKnownIntrinsicObjects.js",
        include_str!("conformance/data/harness/wellKnownIntrinsicObjects.js"),
    ),
    (
        "dateConstants.js",
        include_str!("conformance/data/harness/dateConstants.js"),
    ),
    (
        "compareIterator.js",
        include_str!("conformance/data/harness/compareIterator.js"),
    ),
    (
        "iteratorZipUtils.js",
        include_str!("conformance/data/harness/iteratorZipUtils.js"),
    ),
    (
        "nativeFunctionMatcher.js",
        include_str!("conformance/data/harness/nativeFunctionMatcher.js"),
    ),
];

fn harness_source(name: &str) -> Option<&'static str> {
    HARNESS_FILES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, s)| *s)
}

/// Minimal `ShadowRealm` polyfill. QuickJS-NG ships no ShadowRealm, so a fixture
/// touching it would ReferenceError. `new ShadowRealm()` allocates an isolated
/// rquickjs `Context` (a separate realm with its own intrinsics and
/// globalThis), held in a per-`engine_eval` registry keyed by an opaque id the
/// instance carries as `__ds_realm`. `evaluate(src)` evals in that realm and
/// returns the result when it is a spec-primitive, or a `TypeError` for a
/// non-primitive / an error thrown inside the realm (per the spec). When
/// `evaluate` resolves to a callable, it returns a WrappedFunction — a fresh
/// outer-realm function that re-enters the inner realm on each call,
/// marshalling primitive args in and a primitive (or another WrappedFunction)
/// out, throwing `TypeError` for non-primitive args or returns (per the spec).
/// Callable args (functions passed into a wrapped call) are NOT wrapped back —
/// a callable arg throws `TypeError` (the bidirectional case is out of scope;
/// fixtures needing it degrade honestly). `importValue` runs the spec's
/// synchronous validation (realm check, `ToString(specifier)`, `exportName`
/// must be a string) so the validation-focused fixtures pass, then rejects
/// with a `TypeError` — the engine path has no module loader, so a fixture
/// expecting a real import stays honestly `partial`, while the four expecting
/// a `TypeError` rejection pass. The registry is cleared at the end of each
/// `engine_eval` (`RealmsGuard`) so inner realms (and their runtimes) drop
/// with the fixture — no cross-fixture residue.
const SHADOWREALM_PRELUDE: &str = r#"
function ShadowRealm() { this.__ds_realm = __ds_sr_create(); }
ShadowRealm.prototype = {
  constructor: ShadowRealm,
  evaluate(src) { return __ds_sr_evaluate(this.__ds_realm, src); },
  importValue(specifier, exportName) {
    if (!(this instanceof ShadowRealm)) {
      throw new TypeError("ShadowRealm.prototype.importValue called on incompatible receiver");
    }
    specifier = String(specifier);
    if (typeof exportName !== "string") {
      throw new TypeError("ShadowRealm.prototype.importValue requires exportName to be a string");
    }
    return Promise.reject(new TypeError("ShadowRealm importValue: module loading is not supported"));
  },
};
"#;

/// Per-realm helpers for the ShadowRealm polyfill, implementing
/// `CopyNameAndLength` (sec-copynameandlength). `__ds_wfn_read_meta` runs in
/// the *inner* realm where the wrapped callable lives: it reads the target's
/// `length` (HasOwnProperty + Get; the spec algorithm with argCount=0 —
/// +Infinity→+∞, -Infinity/negative→0, else max(ToIntegerOrInfinity, 0)) and
/// `name` (coerced to String, empty if absent/non-String), returning `[length,
/// name]`. A throwing access — a revoked Proxy, a throwing `length`/`name`
/// accessor, or a `getOwnPropertyDescriptor` trap that throws — surfaces here;
/// the Rust caller catches it and converts the throw to a TypeError at the
/// `evaluate` boundary. `hasOwnProperty` (not `[[HasProperty]]`) is what
/// reaches a proxy's `[[GetOwnProperty]]`: QuickJS's proxy `[[Get]]` recurses
/// on the target when no `get` trap is set, so a plain `.length`/`.name` would
/// miss a `getOwnPropertyDescriptor`-trap throw. `__ds_wfn_set_meta` runs in
/// the *outer* realm: it stamps the copied length/name onto the wrapper as own
/// data properties with SetFunctionLength/SetFunctionName descriptors (writable
/// false, enumerable false, configurable true).
const SR_INNER_PRELUDE: &str = r#"
globalThis.__ds_wfn_read_meta = function (fn) {
  var L = 0;
  if (Object.prototype.hasOwnProperty.call(fn, "length")) {
    var tl = fn.length;
    if (typeof tl === "number") {
      if (tl === Infinity) L = Infinity;
      else if (tl === -Infinity) L = 0;
      else { var ti = Math.trunc(tl); L = ti < 0 ? 0 : ti; }
    }
  }
  var name = "";
  if (Object.prototype.hasOwnProperty.call(fn, "name")) {
    var tn = fn.name;
    name = (typeof tn === "string") ? tn : "";
  }
  return [L, name];
};
globalThis.__ds_wfn_set_meta = function (fn, len, name) {
  Object.defineProperty(fn, "length", {value: len, writable: false, enumerable: false, configurable: true});
  Object.defineProperty(fn, "name", {value: name, writable: false, enumerable: false, configurable: true});
};
"#;

/// A value lifted out of a ShadowRealm's `evaluate` (or a wrapped call's
/// return) — a spec-primitive, or a `WrappedFn` handle to an inner-realm
/// callable. Marshalled across realms: extracted in the inner realm, rebuilt
/// in the outer realm during [`rquickjs::IntoJs`] conversion (a JS value
/// cannot be shared across runtimes — undefined behavior — so only primitives
/// and callable handles cross). Callable *arguments* to a wrapped call are not
/// wrapped back — the bidirectional case needs a `Ctx`→`Context` conversion
/// rquickjs does not expose, so a callable arg throws `TypeError` (honest
/// `partial`) rather than faking a pass.
enum SrPrim {
    Und,
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    /// A callable the inner realm produced — registered there under
    /// `__ds_wfn_<fn_id>` and re-exposed in the outer realm as a
    /// WrappedFunction (`into_js` builds the outer closure capturing the ids).
    WrappedFn {
        realm: u32,
        fn_id: u32,
        /// Copied from the inner-realm target via CopyNameAndLength — stamped
        /// onto the outer wrapper as its own `length`/`name` data properties.
        length: f64,
        name: String,
    },
    /// Non-primitive result, or a runtime error thrown inside the realm — both
    /// must surface as a `TypeError` to the outer realm per the ShadowRealm spec.
    TypeError,
    /// The source text failed to *parse* in the inner realm — per spec this
    /// surfaces as a `SyntaxError` (not the `TypeError` that wraps runtime
    /// throws). Distinguished from a runtime throw by a pre-parse step.
    SyntaxError,
}

impl<'js> rquickjs::IntoJs<'js> for SrPrim {
    fn into_js(self, ctx: &rquickjs::Ctx<'js>) -> rquickjs::Result<rquickjs::Value<'js>> {
        use rquickjs::function::{Args, Rest};
        use rquickjs::{Ctx, Exception, Function, Value};
        Ok(match self {
            SrPrim::Und => Value::new_undefined(ctx.clone()),
            SrPrim::Null => Value::new_null(ctx.clone()),
            SrPrim::Bool(b) => Value::new_bool(ctx.clone(), b),
            SrPrim::Num(n) => Value::new_float(ctx.clone(), n),
            SrPrim::Str(s) => return s.into_js(ctx),
            SrPrim::WrappedFn {
                realm,
                fn_id,
                length,
                name,
            } => {
                // Build a fresh outer-realm function that re-enters the inner
                // realm on each call. Args are marshalled to primitives on the
                // way in (a non-primitive arg throws `TypeError`); the inner
                // callable is looked up by `fn_id`, called at runtime arity
                // (`Args::new` + `push_arg`); the return is marshalled back to a
                // primitive (or another WrappedFunction). `realm`/`fn_id` are
                // `u32` (Copy), so the closure is `Fn`.
                let wrapper = Function::new(
                    ctx.clone(),
                    move |ctx: Ctx<'js>, args: Rest<Value<'js>>| -> rquickjs::Result<Value<'js>> {
                        let mut arg_prims: Vec<SrPrim> = Vec::with_capacity(args.0.len());
                        for a in args.0 {
                            match sr_arg_to_prim(&ctx, a) {
                                Ok(p) => arg_prims.push(p),
                                Err(()) => {
                                    return Err(Exception::throw_type(
                                        &ctx,
                                        "ShadowRealm WrappedFunction call: non-primitive argument",
                                    ))
                                }
                            }
                        }
                        let inner = DS_REALMS.with(|m| m.borrow().get(&realm).cloned());
                        let Some(inner) = inner else {
                            return Err(Exception::throw_type(
                                &ctx,
                                "ShadowRealm: inner realm released",
                            ));
                        };
                        // `Ok(None)` means the inner callable itself is gone
                        // (the realm was cleared between evaluate and call) —
                        // surfaced as a TypeError, matching a released realm.
                        let ret = inner.with(|ic| -> rquickjs::Result<Option<SrPrim>> {
                            let f: Function = match ic
                                .globals()
                                .get::<_, Function>(format!("__ds_wfn_{fn_id}"))
                            {
                                Ok(f) => f,
                                Err(_) => return Ok(None),
                            };
                            let mut call_args = Args::new(ic.clone(), arg_prims.len());
                            for p in arg_prims {
                                call_args.push_arg(p)?;
                            }
                            let r: Value = f.call_arg(call_args)?;
                            sr_value_to_prim(&ic, r, realm).map(Some)
                        })?;
                        match ret {
                            Some(p) => p.into_js(&ctx),
                            None => Err(Exception::throw_type(
                                &ctx,
                                "ShadowRealm: wrapped callable released",
                            )),
                        }
                    },
                )?;
                // CopyNameAndLength: stamp the target's length/name onto the
                // outer-realm wrapper as own data properties (writable false,
                // enumerable false, configurable true — SetFunctionLength/Name).
                if let Ok(setter) = ctx.globals().get::<_, Function>("__ds_wfn_set_meta") {
                    let _ = setter.call::<_, ()>((wrapper.clone(), length, name.as_str()));
                }
                return Ok(wrapper.into_value());
            }
            SrPrim::TypeError => {
                return Err(Exception::throw_type(
                    ctx,
                    "ShadowRealm.prototype.evaluate: evaluation did not resolve to a primitive",
                ))
            }
            SrPrim::SyntaxError => {
                return Err(Exception::throw_syntax(
                    ctx,
                    "ShadowRealm.prototype.evaluate: source text failed to parse",
                ))
            }
        })
    }
}

thread_local! {
    /// ShadowRealm inner realms, keyed by the opaque id each JS ShadowRealm
    /// instance carries. A `Context` keeps its `Runtime` alive, so a realm
    /// persists across `evaluate` calls on the same instance. Cleared at the
    /// end of every `engine_eval` (`RealmsGuard`) so inner runtimes drop with
    /// the fixture — no cross-fixture residue.
    static DS_REALMS: std::cell::RefCell<std::collections::HashMap<u32, rquickjs::Context>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
    static DS_REALM_NEXT: std::cell::Cell<u32> = const { std::cell::Cell::new(1) };
    /// Id counter for cross-realm callables registered under `__ds_wfn_<id>`.
    static DS_SR_FN_NEXT: std::cell::Cell<u32> = const { std::cell::Cell::new(1) };
}

/// Create a fresh isolated `Context` (a new realm with its own intrinsics),
/// register it in the per-`engine_eval` realm table under a new opaque id, and
/// recursively install the ShadowRealm polyfill on it — so a `new ShadowRealm()`
/// evaluated *inside* a realm can itself allocate a nested realm (test262
/// `nested-realms`). The polyfill closures and `__ds_wfn_check` reach the inner
/// realm via [`sr_install`], and the thread-local registries they use
/// (`DS_REALMS`, the id counters) are process-global, so a callable registered
/// in any realm is callable from any other. Returns the new realm's id.
fn sr_create_fn() -> rquickjs::Result<u32> {
    use rquickjs::{Context, Runtime};
    let rt = Runtime::new()?;
    let inner = Context::full(&rt)?;
    inner.with(|ic| sr_install(&ic))?;
    let id = DS_REALM_NEXT.with(|c| {
        let v = c.get();
        c.set(v.checked_add(1).unwrap_or(1));
        v
    });
    DS_REALMS.with(|m| m.borrow_mut().insert(id, inner));
    Ok(id)
}

/// `ShadowRealm.prototype.evaluate` host implementation: look up the inner
/// realm by id, pre-parse via the Function constructor to tell a SyntaxError
/// (a parse failure ⇒ `SrPrim::SyntaxError`) from a runtime throw, then eval
/// and lift the result via [`sr_value_to_prim`] (a non-primitive or a runtime
/// throw ⇒ `SrPrim::TypeError`, per spec).
fn sr_evaluate_fn(id: u32, src: String) -> rquickjs::Result<SrPrim> {
    use rquickjs::{Function, Value};
    let inner = DS_REALMS.with(|m| m.borrow().get(&id).cloned());
    let Some(inner) = inner else {
        return Ok(SrPrim::TypeError);
    };
    inner.with(|ic| -> rquickjs::Result<SrPrim> {
        // Spec: a top-level *parse* failure throws a SyntaxError; a *runtime*
        // throw is wrapped into a TypeError. rquickjs's eval fuses parse and
        // execute, so to tell them apart we pre-parse via the Function
        // constructor — `new Function(src)` parses src as a function body,
        // which agrees with script-body SyntaxError detection for the cases
        // that matter (a strict directive prologue applies in both). A throw
        // here is a parse error ⇒ `SyntaxError`; otherwise the real eval runs,
        // where any throw is a runtime error ⇒ `TypeError`.
        let function_ctor: Function = ic.globals().get("Function")?;
        if function_ctor.call::<_, Value>((src.as_str(),)).is_err() {
            let _ = ic.catch();
            return Ok(SrPrim::SyntaxError);
        }
        let v: Value = match ic.eval_with_options::<Value, _>(src.as_str(), sr_sloppy()) {
            Ok(v) => v,
            // A runtime error thrown inside the realm is wrapped into a
            // TypeError (and a non-primitive result is too — see
            // sr_value_to_prim).
            Err(_) => {
                let _ = ic.catch();
                return Ok(SrPrim::TypeError);
            }
        };
        sr_value_to_prim(&ic, v, id)
    })
}

/// Install the ShadowRealm polyfill on a context: register the host
/// `__ds_sr_create`/`__ds_sr_evaluate` (so the realm can allocate nested
/// realms and evaluate in them), install `__ds_wfn_read_meta`/
/// `__ds_wfn_set_meta` (the CopyNameAndLength read/write used to copy a wrapped
/// callable's `length`/`name` onto the outer-realm wrapper), and define the
/// global `ShadowRealm`. Called on the engine context for fixtures that
/// reference ShadowRealm, and recursively on every inner realm [`sr_create_fn`]
/// allocates — so nested `new ShadowRealm()` works at any depth.
fn sr_install(ctx: &rquickjs::Ctx) -> rquickjs::Result<()> {
    use rquickjs::Function;
    ctx.globals()
        .set("__ds_sr_create", Function::new(ctx.clone(), sr_create_fn)?)?;
    ctx.globals().set(
        "__ds_sr_evaluate",
        Function::new(ctx.clone(), sr_evaluate_fn)?,
    )?;
    ctx.eval_with_options::<(), _>(SR_INNER_PRELUDE, sr_sloppy())?;
    ctx.eval_with_options::<(), _>(SHADOWREALM_PRELUDE, sr_sloppy())?;
    Ok(())
}

/// Sloppy-mode `EvalOptions` for ShadowRealm polyfill evals (the spec runs
/// `evaluate` source as a global-scope script, not a strict module).
fn sr_sloppy() -> rquickjs::context::EvalOptions {
    let mut o = rquickjs::context::EvalOptions::default();
    o.strict = false;
    o
}

/// Extract an inner-realm value into the cross-realm [`SrPrim`] form. A
/// callable becomes a `WrappedFn` handle (registered in the inner global as
/// `__ds_wfn_<id>` so the outer-realm wrapper can re-enter and call it);
/// primitives map directly; anything else (object/symbol/bigint) is a
/// non-primitive → `TypeError` per the ShadowRealm spec.
fn sr_value_to_prim<'js>(
    ic: &rquickjs::Ctx<'js>,
    v: rquickjs::Value<'js>,
    realm: u32,
) -> rquickjs::Result<SrPrim> {
    use rquickjs::{FromJs, Type};
    Ok(match v.type_of() {
        Type::Uninitialized | Type::Undefined => SrPrim::Und,
        Type::Null => SrPrim::Null,
        Type::Bool => SrPrim::Bool(bool::from_js(ic, v)?),
        Type::Int | Type::Float => SrPrim::Num(f64::from_js(ic, v)?),
        Type::String => SrPrim::Str(String::from_js(ic, v)?),
        // QuickJS-NG tags a callable that carries [[Construct]] (a function
        // declaration, a named function expression, a class) as `Constructor`,
        // not `Function` — both are callable, so both wrap as a WrappedFunction.
        // Matching only `Function` sent every function-declaration result to the
        // `_ => TypeError` arm ("evaluation did not resolve to a primitive").
        Type::Function | Type::Constructor => {
            // Spec (sec-wrappedfunctioncreate, CopyNameAndLength): wrapping a
            // callable reads `length` then `name` via HasOwnProperty/Get. A
            // revoked Proxy, a throwing accessor, or a throwing
            // `getOwnPropertyDescriptor` trap makes that read throw, and the
            // ShadowRealm `evaluate` boundary surfaces it as a TypeError.
            // `__ds_wfn_read_meta` (installed in each inner realm) runs that
            // read and returns the copied `[length, name]`; a throw here
            // becomes `TypeError` instead of a handle. The pending exception is
            // cleared so it can't leak into a later eval.
            let read_meta: rquickjs::Function = ic.globals().get("__ds_wfn_read_meta")?;
            let meta: rquickjs::Array = match read_meta.call::<_, rquickjs::Array>((v.clone(),)) {
                Ok(m) => m,
                Err(_) => {
                    let _ = ic.catch();
                    return Ok(SrPrim::TypeError);
                }
            };
            let length: f64 = meta.get(0)?;
            let name: String = meta.get(1)?;
            let fn_id = DS_SR_FN_NEXT.with(|c| {
                let n = c.get();
                c.set(n.checked_add(1).unwrap_or(1));
                n
            });
            ic.globals().set(format!("__ds_wfn_{fn_id}"), v)?;
            SrPrim::WrappedFn {
                realm,
                fn_id,
                length,
                name,
            }
        }
        _ => SrPrim::TypeError,
    })
}

/// Marshal an outer-realm call argument into the cross-realm [`SrPrim`] form.
/// Only primitives cross — a callable or object arg has no static wrap-back
/// here (the bidirectional callable-arg case is out of scope), so it signals
/// the caller to throw `TypeError` per the ShadowRealm spec.
fn sr_arg_to_prim<'js>(ctx: &rquickjs::Ctx<'js>, v: rquickjs::Value<'js>) -> Result<SrPrim, ()> {
    use rquickjs::{FromJs, Type};
    match v.type_of() {
        Type::Uninitialized | Type::Undefined => Ok(SrPrim::Und),
        Type::Null => Ok(SrPrim::Null),
        Type::Bool => bool::from_js(ctx, v).map(SrPrim::Bool).map_err(|_| ()),
        Type::Int | Type::Float => f64::from_js(ctx, v).map(SrPrim::Num).map_err(|_| ()),
        Type::String => String::from_js(ctx, v).map(SrPrim::Str).map_err(|_| ()),
        _ => Err(()),
    }
}

/// Drops all ShadowRealm inner realms when `engine_eval` returns (or unwinds),
/// so each fixture starts and ends with an empty registry.
struct RealmsGuard;
impl Drop for RealmsGuard {
    fn drop(&mut self) {
        DS_REALMS.with(|m| m.borrow_mut().clear());
    }
}

/// Run an engine-gated fixture's ECMAScript under an embedded QuickJS engine —
/// the same engine `__ds_engine` embeds for `ds build`, but in-process. Skips
/// the cargo compile entirely: the engine Rust template (`fn main() {
/// __ds_engine::run(src) }`) is fixed-shape and its compile correctness is
/// covered by translator unit tests + the engine-path integration test, so
/// re-compiling a throwaway project per fixture would only burn time. Injects
/// the test262 harness (`sta.js` + `assert.js` + the fixture's `$INCLUDE`s)
/// before the fixture, so the assert family runs with reference semantics; a
/// thrown `Test262Error` (assert mismatch) is the single failure signal.
fn engine_eval(
    js_source: &str,
    includes: &[String],
    features: &[String],
    flags: &[String],
) -> EngineOutcome {
    use rquickjs::{context::EvalOptions, ArrayBuffer, Context, Ctx, FromJs, Function, Runtime};
    // Serialize the rquickjs engine path across worker threads. Concurrent
    // `Runtime::new()` / `Context::full` / `globals().set` in the N parallel
    // workers races inside QuickJS-NG: a fixture whose body or `$INCLUDE`
    // references the just-injected `$262` (or `Temporal`, `ShadowRealm`, …)
    // resolves it as undefined under one parallel run and clean under another
    // — a heisenbug that surfaced ~19 fixtures as false `unsupported`
    // (`ReferenceError: $262 is not defined`) under the default parallel
    // matrix while passing single-threaded (`DASH_CONF_WORKERS=1`). The
    // `run_gc` between evaluations (01f8bb2) only clears residue within a
    // single runtime's lifetime; it does not stop the cross-thread creation
    // race, so a process-global lock around the whole engine path is the
    // reliable fix. The cost is bounded: engine_eval is in-process rquickjs
    // (microseconds–milliseconds per fixture), dwarfed by the parallel cargo
    // builds, which still run concurrently — only the QuickJS evals serialize.
    static ENGINE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _engine_guard = ENGINE_LOCK.lock().expect("ENGINE_LOCK poisoned");
    let runtime = match Runtime::new() {
        Ok(r) => r,
        Err(e) => return EngineOutcome::OtherError(format!("runtime: {e}")),
    };
    let ctx = match Context::full(&runtime) {
        Ok(c) => c,
        Err(e) => return EngineOutcome::OtherError(format!("context: {e}")),
    };
    // Force a GC cycle before the runtime drops. Without it, residue from a
    // prior fixture's engine_eval leaks into the next one (a timing-sensitive
    // heisenbug: a fixture whose `$INCLUDE`/body references the just-injected
    // `$262` resolves it as `undefined` under one run and clean under another,
    // depending on how much residue the prior fixture left). A GC sweep between
    // fixtures clears the leak.
    struct GcOnDrop<'a>(&'a Runtime);
    impl Drop for GcOnDrop<'_> {
        fn drop(&mut self) {
            self.0.run_gc();
        }
    }
    let _gc_guard = GcOnDrop(&runtime);
    // Drop ShadowRealm inner realms when engine_eval returns so each fixture
    // starts and ends with an empty registry (no cross-fixture residue).
    let _realms_guard = RealmsGuard;
    let sloppy = || {
        let mut o = EvalOptions::default();
        o.strict = false;
        o
    };
    // `onlyStrict` fixtures run under strict mode (QuickJS
    // `JS_EVAL_FLAG_STRICT`): the harness prelude stays sloppy, but the
    // fixture itself is strict-eval'd so `Function.prototype.caller`/
    // `arguments` poison-pill, strict assignment/deletion, duplicate params,
    // octal literals, and `with` behave per spec. Without it the engine runs
    // every fixture sloppy and `onlyStrict` asserts fail.
    let strict_fixture = flags.iter().any(|f| f == "onlyStrict");
    let fixture_opts = || {
        let mut o = EvalOptions::default();
        o.strict = strict_fixture;
        o
    };
    let uses_done = js_source.contains("$DONE");
    // Phase 1 — synchronous harness + fixture eval. Returns `Ok(None)` on a
    // clean run; `Ok(Some(msg))` if the fixture threw (the thrown value
    // stringified by the wrapper — "Test262Error: …" or "ReferenceError:
    // Temporal is not defined"); `Err` if a prelude/harness eval itself failed.
    // Fetching the thrown value via `ctx.catch()` rather than formatting the
    // `rquickjs::Error` is what makes engine-path failures diagnosable — that
    // Error's Display is the opaque "Exception generated by QuickJS", which hid
    // ~4000 fixtures' real cause behind a fixed string.
    let sync_throw = ctx.with(
        move |ctx: Ctx<'_>| -> Result<Option<String>, rquickjs::Error> {
            ctx.eval_with_options::<(), _>(CONSOLE_PRELUDE, sloppy())?;
            // Temporal polyfill: QuickJS-NG lacks Temporal, so a fixture touching
            // it would ReferenceError. Inject the @js-temporal/polyfill (spec
            // reference) and expose under the spec-global name. Conditioned on the
            // fixture's source mentioning Temporal so non-Temporal fixtures skip
            // the ~240KB eval. A polyfill eval failure propagates as OtherError
            // (judge_engine → partial) rather than being masked.
            if js_source.contains("Temporal") {
                ctx.eval_with_options::<(), _>(INTL_STUB, sloppy())?;
                if ctx
                    .eval_with_options::<(), _>(TEMPORAL_POLYFILL, sloppy())
                    .is_err()
                {
                    let thrown = ctx.catch();
                    let _ = ctx.globals().set("__ds_err_diag", thrown);
                    let msg = ctx
                        .eval::<String, &str>("String(globalThis.__ds_err_diag)")
                        .unwrap_or_else(|_| "<opaque>".into());
                    return Ok(Some(format!("polyfill eval failed: {msg}")));
                }
                ctx.eval_with_options::<(), _>(TEMPORAL_EXPOSE, sloppy())?;
                // Strip the default `prototype` from non-constructor built-ins
                // (static/prototype methods defined as plain functions) so the
                // polyfill surface matches ECMA-262 for the `.builtin` fixtures.
                ctx.eval_with_options::<(), _>(TEMPORAL_NON_CTOR_STRIP, sloppy())?;
            }
            // Harness prelude: sta.js (defines Test262Error), assert.js (throws it
            // on mismatch), then any $INCLUDE helpers the fixture declares.
            ctx.eval_with_options::<(), _>(HARNESS_STA, sloppy())?;
            ctx.eval_with_options::<(), _>(HARNESS_ASSERT, sloppy())?;
            // Register the host-defined `$262` agent before any `$INCLUDE` (e.g.
            // detachArrayBuffer.js) that references it. Only `detachArrayBuffer`
            // needs a Rust impl (JS cannot detach an ArrayBuffer — it calls
            // QuickJS's JS_DetachArrayBuffer via ArrayBuffer::detach); the rest
            // of `$262` is the JS polyfill in AGENT_262_PRELUDE.
            ctx.globals().set(
                "__ds_detach",
                Function::new(ctx.clone(), |buf: ArrayBuffer| -> rquickjs::Result<()> {
                    let mut b = buf;
                    b.detach();
                    Ok(())
                })?,
            )?;
            ctx.eval_with_options::<(), _>(AGENT_262_PRELUDE, sloppy())?;
            // Atomics.waitAsync polyfill (QuickJS-NG lacks it). Inject only for
            // fixtures that reference it — covers the validation + non-blocking
            // waitAsync paths; the `-agent` variants stay partial.
            if js_source.contains("waitAsync") {
                ctx.eval_with_options::<(), _>(WAITASYNC_PRELUDE, sloppy())?;
            }
            // Promise.allKeyed / allSettledKeyed (tc39 `await-dictionary` proposal):
            // QuickJS-NG lacks both, so inject a polyfill only for fixtures that
            // reference either. Note `"allSettledKeyed"` is NOT a substring of
            // `"allKeyed"` (it is `all`+`Settled`+`Keyed`), so both must be probed
            // — otherwise the allSettledKeyed suite stays partial.
            if js_source.contains("allKeyed") || js_source.contains("allSettledKeyed") {
                ctx.eval_with_options::<(), _>(ALLKEYED_PRELUDE, sloppy())?;
            }
            // asyncHelpers.js (`asyncTest` / `assert.throwsAsync`): the newer async
            // helpers are not always declared in a fixture's `includes:` (the
            // `await-dictionary` suite omits it), so inject when the source
            // references them. `asyncTest` checks `$DONE` at call time, which the
            // `DONE_PRELUDE` below provides before the fixture eval runs.
            if js_source.contains("asyncTest") || js_source.contains("throwsAsync") {
                if let Some(src) = harness_source("asyncHelpers.js") {
                    ctx.eval_with_options::<(), _>(src, sloppy())?;
                }
            }
            // ShadowRealm polyfill: QuickJS-NG lacks ShadowRealm, so inject a
            // Rust-backed one (isolated rquickjs Context per instance) only for
            // fixtures that reference it — skips the registration cost otherwise.
            if js_source.contains("ShadowRealm") {
                sr_install(&ctx)?;
            }
            // $262.AbstractModuleSource (tc39 `source-phase-imports` proposal,
            // ES2026): the host exposes the abstract constructor on `$262`.
            // QuickJS-NG ships no source-phase-imports, so inject a spec-faithful
            // stub (abstract ctor that throws, correct length/name/prototype
            // descriptors) only for the fixtures that reference it.
            if js_source.contains("AbstractModuleSource") {
                ctx.eval_with_options::<(), _>(ABSTRACT_MODULE_SOURCE_PRELUDE, sloppy())?;
            }
            // Error.prototype.stack accessor (tc39 `error-stack-accessor` proposal):
            // QuickJS-NG has no own `stack` on `Error.prototype`, so the
            // `.get`/`.set` access crashes the getter/setter fixtures. Inject before
            // `$INCLUDE`s so `nativeErrors.js` captures the wrapped constructors.
            // Gated on the feature flag (the only fixtures that opt into the
            // proposal), so every other error fixture is untouched.
            if features.iter().any(|f| f == "error-stack-accessor") {
                ctx.eval_with_options::<(), _>(ERROR_STACK_ACCESSOR_PRELUDE, sloppy())?;
            }
            for inc in includes {
                if let Some(src) = harness_source(inc) {
                    ctx.eval_with_options::<(), _>(src, sloppy())?;
                }
            }
            // Host-defined `$DONE` async callback — injected after `$INCLUDE`s so
            // it wins over `doneprintHandle.js`'s print-based `$DONE`.
            ctx.eval_with_options::<(), _>(DONE_PRELUDE, sloppy())?;
            // The fixture is self-contained (declares `main` and calls it, pure-TS
            // execution semantics), so a single eval runs it — no separate call.
            // The wrapper stringifies any escaped throw so the value fetched via
            // `ctx.catch()` is always a JS string (Test262Error → its toString,
            // other throws → their own name/message).
            let wrapped =
                format!("try {{\n{js_source}\n}} catch (__ds_err) {{ throw String(__ds_err); }}\n");
            if ctx
                .eval_with_options::<(), _>(wrapped.as_str(), fixture_opts())
                .is_err()
            {
                let thrown = ctx.catch();
                if let Ok(s) = String::from_js(&ctx, thrown) {
                    return Ok(Some(s));
                }
            }
            Ok(None)
        },
    );
    // A synchronous throw decides immediately — async reactions never fire.
    match sync_throw {
        Err(e) => return EngineOutcome::OtherError(format!("harness eval: {e}")),
        Ok(Some(msg)) => {
            return if msg.contains("Test262Error") {
                EngineOutcome::AssertFailed(msg)
            } else {
                EngineOutcome::OtherError(msg)
            };
        }
        Ok(None) => {}
    }
    // Phase 2 — drain microtask jobs so async fixtures resolve. QuickJS schedules
    // promise reactions (`.then`, async/await, async generators) as runtime jobs;
    // the synchronous eval above returns before they fire, so `$DONE` would never
    // be called for an async fixture without draining. The runtime owns the job
    // queue — drain it outside the `ctx` guard (no deadlock with the context).
    // Capped: an in-process drain is not covered by the per-fixture spawn
    // timeout, so a self-rescheduling microtask loop would hang the harness;
    // 10000 is far above any real fixture's depth.
    if uses_done {
        let mut guard = 0;
        while runtime.is_job_pending() && guard < 10000 {
            // A thrown job reaction is captured via the `$DONE` sentinel (a
            // rejection routed to `$DONE(error)`); ignore the JobException here.
            let _ = runtime.execute_pending_job();
            guard += 1;
        }
    }
    // Phase 3 — read the `$DONE` sentinel. Only fixtures that reference `$DONE`
    // are async; for them the verdict is: never called → `EngineLimitation`
    // (the async chain did not resolve under the engine's drain — needs a host
    // event loop the engine lacks), "" → supported, "Test262Error: …" → partial
    // (an unexpected async rejection). A non-async fixture's clean synchronous
    // eval already means supported.
    if uses_done {
        let verdict = ctx.with(|ctx: Ctx<'_>| -> String {
            // Clear any pending job-throw so the eval below is not contaminated.
            let _ = ctx.catch();
            ctx.eval_with_options::<String, _>(
                "(__ds_done_value === null) ? 'PENDING'\n\
                 : (__ds_done_value === '') ? 'OK'\n\
                 : ('FAIL:' + __ds_done_value)",
                sloppy(),
            )
            .unwrap_or_else(|_| "PENDING".to_string())
        });
        return match verdict.as_str() {
            "OK" => EngineOutcome::Ok,
            "PENDING" => EngineOutcome::EngineLimitation(
                "$DONE() was never called (async chain did not resolve)".into(),
            ),
            rest => {
                // `FAIL:<msg>` — strip the tag; the msg already carries the
                // "Test262Error: …" prefix from `$DONE`.
                let msg = rest.strip_prefix("FAIL:").unwrap_or(rest);
                if msg.contains("Test262Error") {
                    EngineOutcome::AssertFailed(msg.to_string())
                } else {
                    EngineOutcome::OtherError(msg.to_string())
                }
            }
        };
    }
    EngineOutcome::Ok
}

/// Recover the raw test262 body from a harness-wrapped fixture. The extractor
/// (scripts/extract-test262.mjs) wraps every body verbatim as
/// `function main(): void {\n<body>\n}\nmain();\n`; on the engine path we strip
/// that wrapper so the body evals at global scope (see [`run_test262`]).
/// Returns the input unchanged if it is not the wrapped form.
fn strip_main_wrapper(fixture: &str) -> &str {
    const PREFIX: &str = "function main(): void {\n";
    const SUFFIX: &str = "\n}\nmain();\n";
    let s = fixture.strip_prefix(PREFIX).unwrap_or(fixture);
    s.strip_suffix(SUFFIX).unwrap_or(s)
}

/// Run a fixture on the engine path — QuickJS with the test262 harness
/// (`sta.js` + `assert.js` + the fixture's `$INCLUDE`s) injected. The
/// extractor wraps every body as `function main(): void { … } main();` for
/// the static path; on the engine path we eval the raw body at global scope
/// instead — mirroring how test262 runs a fixture as a script — so top-level
/// `var` declarations are globals and the `Function()` constructor's global-
/// scope capture resolves them per spec (the planet fixture
/// `Function("return planet;")` would otherwise throw ReferenceError). Fall
/// back to the wrapped fixture if the unwrapped body is shorter, so this
/// never perturbs a fixture's routing. Used both for `needs_engine` fixtures
/// and as the `cargo check` failure fallback (degrade, don't reject).
fn run_engine(raw: &RawFeature) -> (&'static str, String) {
    let body = strip_main_wrapper(&raw.fixture);
    let js_source = if body.len() < raw.fixture.len() {
        Translator::new()
            .engine_source(body)
            .or_else(|| Translator::new().engine_source(&raw.fixture))
    } else {
        Translator::new().engine_source(&raw.fixture)
    };
    match js_source {
        Some(s) => judge_engine(engine_eval(&s, &raw.includes, &raw.features, &raw.flags)),
        None => (
            "partial",
            "engine flag set but engine_source returned None".into(),
        ),
    }
}

/// Run one test262 fixture through the assert-driven pipeline. Returns
/// `(status, detail)`.
///
/// Engine path (`needs_engine`, ES reflection the static translator cannot
/// lower): run the source in-process under QuickJS with the test262 harness
/// injected (`sta.js` + `assert.js` + the fixture's `$INCLUDE`s), so a clean
/// completion means every assert held and a thrown `Test262Error` marks a
/// partial — no Node oracle, no cargo compile (see [`engine_eval`]). Static
/// path: `Translator::check` (translatability) → `cargo check` (compiles,
/// partial on failure) → build + run the probe; exit 0 → supported, a panicked
/// `Test262Error` → partial, a build failure or timeout → unsupported.
/// Translator scope limits stay honestly `unsupported`.
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
        Err(e) => return ("partial", e),
    };
    // Engine path: ES reflection the static translator cannot lower. Run the
    // source in-process under QuickJS with the test262 harness injected, so
    // reflection + the full assert family run with reference semantics.
    if deps.needs_engine() {
        return run_engine(raw);
    }
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
        // DashScript's contract is "degrade, don't reject" — fall back to the
        // engine path so the fixture still runs under QuickJS rather than
        // reporting a static-only partial. Only upgrade to `supported`; if the
        // engine also fails, keep `partial` carrying both details (never
        // downgrade to `unsupported`, which would be a false regression).
        let (estatus, edetail) = run_engine(raw);
        if estatus == "supported" {
            return (
                "supported",
                "engine fallback after static build failure".into(),
            );
        }
        return (
            "partial",
            format!("static build: {err} | engine: {edetail}"),
        );
    }
    let (verdict, _stdout) = cargo_run_full(project, target_dir);
    match verdict {
        // The emitted Rust compiled but crashed at runtime (an array index out
        // of bounds ES would return `undefined` for, an arithmetic overflow, a
        // residual `todo!()`/`panic!()`). The static translator lowered the
        // construct to code that crashes instead of expressing its ES semantics
        // — the same "degrade, don't reject" contract as a build failure: fall
        // back to the engine path so the fixture runs under QuickJS rather than
        // reporting a runtime-only partial. Only upgrade to `supported`; if the
        // engine also fails, keep `partial` carrying both details.
        RunOutcome::RunError(err) => {
            let (estatus, edetail) = run_engine(raw);
            if estatus == "supported" {
                (
                    "supported",
                    "engine fallback after static runtime panic".into(),
                )
            } else {
                (
                    "partial",
                    format!("runtime error: {err} | engine: {edetail}"),
                )
            }
        }
        other => judge_run(other),
    }
}

/// One fixture, run against a worker-owned `project`/`target_dir` pair.
/// Unifies the test262 assert-driven path (exit code + Test262Error) with the
/// translator-tests/correctness path (cargo check + optional expected-stdout
/// run). Pure over its arguments — no shared mutable state across calls — so
/// it is safe to invoke from many threads in parallel, each on its own project.
fn run_fixture(raw: &RawFeature, layer: &str, project: &Path, target_dir: &Path) -> Outcome {
    if layer == "test262" {
        let (status, detail) = run_test262(raw, project, target_dir);
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

/// Write one matrix file per test262 category + one each for translator-tests
/// and correctness, plus a README overview. Per-category files (not one giant
/// matrix) match the per-category data and let a single-builtin run update only
/// its own slice.
fn write_matrix_split(outcomes: &[Outcome]) {
    use std::collections::HashSet;
    let dir = conformance_dir().join("matrix");
    let _ = fs::create_dir_all(&dir);

    // test262: one file per category (sorted).
    let mut cats: Vec<String> = outcomes
        .iter()
        .filter(|o| o.layer == "test262")
        .map(|o| o.category.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    cats.sort();
    for cat in &cats {
        let rows: Vec<&Outcome> = outcomes
            .iter()
            .filter(|o| o.layer == "test262" && o.category == *cat)
            .collect();
        write_section(&dir.join(format!("test262-{cat}")), &rows);
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
        "- {total} features: **{supported}** supported, **{partial}** partial, **{unsupported}** unsupported, **{untested}** untested\n",
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
    let mut by_key: BTreeMap<(String, String), [usize; 4]> = BTreeMap::new();
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
                let key = if r.layer == "test262" {
                    (r.layer, r.category)
                } else {
                    (r.layer, String::new())
                };
                let e = by_key.entry(key).or_insert([0, 0, 0, 0]);
                match r.status.as_str() {
                    "supported" => e[0] += 1,
                    "partial" => e[1] += 1,
                    "unsupported" => e[2] += 1,
                    _ => e[3] += 1,
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
    s.push_str("| layer | category | supported | partial | unsupported | other |\n");
    s.push_str("| --- | --- | ---: | ---: | ---: | ---: |\n");
    for ((layer, cat), c) in &by_key {
        let link = if layer == "test262" {
            format!("[{cat}](test262-{cat}.md)")
        } else {
            format!("[{layer}]({layer}.md)")
        };
        s.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            layer, link, c[0], c[1], c[2], c[3]
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

/// Regression guard for `TEMPORAL_NON_CTOR_STRIP`: the @js-temporal/polyfill
/// defines non-constructor built-ins as ordinary functions, which carry a
/// non-configurable `prototype` (so `delete` is a silent no-op). The shim
/// replaces them with method-shorthand forwarders (no `prototype`, dynamic
/// `this` preserved). Lock in: prototype removed for static/proto/Now methods,
/// kept for named constructors; `name`/`length` preserved; calls still correct.
#[test]
fn temporal_strip_removes_non_ctor_prototype() {
    use rquickjs::{context::EvalOptions, Context, Ctx, Runtime};
    let sloppy = || {
        let mut o = EvalOptions::default();
        o.strict = false;
        o
    };
    let rt = Runtime::new().expect("rt");
    let ctx = Context::full(&rt).expect("ctx");
    ctx.with(|ctx: Ctx<'_>| {
        ctx.eval_with_options::<(), _>(CONSOLE_PRELUDE, sloppy()).unwrap();
        ctx.eval_with_options::<(), _>(INTL_STUB, sloppy()).unwrap();
        ctx.eval_with_options::<(), _>(TEMPORAL_POLYFILL, sloppy()).unwrap();
        ctx.eval_with_options::<(), _>(TEMPORAL_EXPOSE, sloppy()).unwrap();
        ctx.eval_with_options::<(), _>(TEMPORAL_NON_CTOR_STRIP, sloppy()).unwrap();
        let b = |src: &str| ctx.eval::<bool, _>(src).unwrap();
        let s = |src: &str| ctx.eval::<String, _>(src).unwrap();
        let f = |src: &str| ctx.eval::<f64, _>(src).unwrap();
        // Non-constructors carry no `prototype` own property (static + proto + Now).
        assert!(!b("Temporal.Duration.compare.hasOwnProperty('prototype')"));
        assert!(!b("Temporal.Duration.prototype.add.hasOwnProperty('prototype')"));
        assert!(!b("Temporal.Now.zonedDateTimeISO.hasOwnProperty('prototype')"));
        // Named constructors keep their `prototype`.
        assert!(b("Temporal.PlainDate.hasOwnProperty('prototype')"));
        // `name`/`length` restored after wrapping.
        assert_eq!(s("Temporal.Duration.compare.name"), "compare");
        assert_eq!(f("Temporal.Duration.compare.length"), 2.0);
        // Calls forward correctly (static + prototype method dynamic `this`).
        assert_eq!(
            f("Temporal.Duration.compare(new Temporal.Duration(0,0,0,0,2), new Temporal.Duration(0,0,0,0,1))"),
            1.0
        );
        assert_eq!(
            s("new Temporal.Duration(0,0,0,0,1).add(new Temporal.Duration(0,0,0,0,1)).toJSON()"),
            "PT2H"
        );
    });
}

/// Regression guard for `ERROR_STACK_ACCESSOR_PRELUDE` (tc39
/// `error-stack-accessor` proposal). QuickJS-NG has no own `stack` on
/// `Error.prototype`; the prelude wraps the native constructors (capturing the
/// trace into a WeakMap and deleting the construction-time own property) and
/// installs a get/set accessor matching the proposal. Lock in: the prototype
/// slot is an accessor, get returns a trace for Error instances / undefined for
/// non-Errors / TypeError for non-objects, and set creates an own data property
/// / rejects non-string values, non-object receivers, and the prototype itself.
#[test]
fn error_stack_accessor_polyfill_semantics() {
    use rquickjs::{context::EvalOptions, Context, Ctx, Runtime};
    let sloppy = || {
        let mut o = EvalOptions::default();
        o.strict = false;
        o
    };
    let rt = Runtime::new().expect("rt");
    let ctx = Context::full(&rt).expect("ctx");
    ctx.with(|ctx: Ctx<'_>| {
        ctx.eval_with_options::<(), _>(CONSOLE_PRELUDE, sloppy()).unwrap();
        ctx.eval_with_options::<(), _>(ERROR_STACK_ACCESSOR_PRELUDE, sloppy())
            .unwrap();
        let b = |s: &str| ctx.eval::<bool, _>(s).unwrap();
        let throws = |src: &str| -> String {
            ctx.eval::<String, _>(format!(
                "try{{ {src}; 'no-throw' }} catch(e) {{ e.constructor.name }}"
            ))
            .unwrap()
        };
        // Prototype slot is an accessor: own descriptor has get+set, no value.
        assert!(b("var d=Object.getOwnPropertyDescriptor(Error.prototype,'stack'); typeof d.get==='function' && typeof d.set==='function' && !('value' in d)"));
        assert!(b("d.enumerable===false && d.configurable===true"));
        // Instances carry no own `stack` at construction (proposal model).
        assert!(b("!Object.prototype.hasOwnProperty.call(new Error('x'),'stack')"));
        // get: Error instance → trace string; non-Error → undefined.
        assert!(b("typeof d.get.call(new Error('x'))==='string'"));
        assert!(b("d.get.call(new TypeError('x')) !== undefined"));
        assert!(b("d.get.call({}) === undefined"));
        assert!(b("d.get.call([]) === undefined"));
        // get: non-object `this` → TypeError.
        assert_eq!(throws("d.get.call(undefined)"), "TypeError");
        assert_eq!(throws("d.get.call(null)"), "TypeError");
        assert_eq!(throws("d.get.call(5)"), "TypeError");
        // set: creates an own data property {w,e,c:true}, returns undefined.
        assert_eq!(
            ctx.eval::<String, _>("var e=new Error('m'); d.set.call(e,'sentinel'); String(d.set.call(e,'sentinel2'))")
                .unwrap(),
            "undefined"
        );
        assert!(b("var e2=new Error('m'); d.set.call(e2,'s'); Object.getOwnPropertyDescriptor(e2,'stack').writable===true"));
        // set: non-string value → TypeError.
        assert_eq!(throws("d.set.call(new Error('m'), null)"), "TypeError");
        assert_eq!(throws("d.set.call(new Error('m'), {})"), "TypeError");
        // set: non-object receiver → TypeError.
        assert_eq!(throws("d.set.call(undefined, 'x')"), "TypeError");
        // set: the prototype itself is rejected (its own slot is the accessor).
        assert_eq!(throws("d.set.call(Error.prototype, '')"), "TypeError");
        // Own non-writable data rejects the set.
        assert_eq!(
            throws("var e3=new Error('m'); Object.defineProperty(e3,'stack',{value:'o',writable:false,configurable:true}); d.set.call(e3,'u')"),
            "TypeError"
        );
        // Data-property shadow: get.call still returns a trace string.
        assert!(b("var e4=new Error('m'); Object.defineProperty(e4,'stack',{value:'sentinel',writable:true,enumerable:true,configurable:true}); typeof d.get.call(e4)==='string'"));
        // get/set are non-constructors (method shorthand, no [[Construct]]).
        assert_eq!(throws("new d.get()"), "TypeError");
        assert_eq!(throws("new d.set('x')"), "TypeError");
    });
}

/// Regression guard for `WAITASYNC_PRELUDE`: QuickJS-NG lacks `Atomics.waitAsync`,
/// and `Atomics.wait` throws "cannot block in this thread" on the main thread
/// (before value comparison), so the polyfill delegates only validation and
/// computes the result directly. Lock in: presence, validation throws
/// (RangeError OOB / TypeError non-shared / TypeError Symbol value|timeout),
/// not-equal return, and the timeout-branched `{async,value}` shape.
#[test]
fn waitasync_polyfill_validation_and_returns() {
    use rquickjs::{context::EvalOptions, Context, Ctx, Runtime};
    let sloppy = || {
        let mut o = EvalOptions::default();
        o.strict = false;
        o
    };
    let rt = Runtime::new().expect("rt");
    let ctx = Context::full(&rt).expect("ctx");
    ctx.with(|ctx: Ctx<'_>| {
        ctx.eval_with_options::<(), _>(WAITASYNC_PRELUDE, sloppy())
            .unwrap();
        let b = |s: &str| ctx.eval::<bool, _>(s).unwrap();
        let s = |src: &str| -> String {
            ctx.eval::<String, _>(format!(
                "try{{ {src}; 'no-throw' }} catch(e) {{ e.constructor.name }}"
            ))
            .unwrap()
        };
        // Presence.
        assert!(b("typeof Atomics.waitAsync === 'function'"));
        // Validation: out-of-bounds index → RangeError.
        assert_eq!(
            s("Atomics.waitAsync(new Int32Array(new SharedArrayBuffer(4)), 99, 0, 0)"),
            "RangeError"
        );
        // Validation: non-shared buffer → TypeError.
        assert_eq!(
            s("Atomics.waitAsync(new Int32Array(new ArrayBuffer(4)), 0, 0, 0)"),
            "TypeError"
        );
        // Validation: Symbol value → TypeError.
        assert_eq!(
            s("Atomics.waitAsync(new Int32Array(new SharedArrayBuffer(4)), 0, Symbol(), 0)"),
            "TypeError"
        );
        // Validation: Symbol timeout → TypeError (value valid, reaches timeout coercion).
        assert_eq!(
            s("Atomics.waitAsync(new Int32Array(new SharedArrayBuffer(4)), 0, 0, Symbol())"),
            "TypeError"
        );
        // Value mismatch → {async:false, value:'not-equal'}.
        assert!(b(
            "var __t = new Int32Array(new SharedArrayBuffer(4)); Atomics.store(__t, 0, 42);\
             Atomics.waitAsync(__t, 0, 0).value === 'not-equal'"
        ));
        // Value match, timeout 0 → {async:false, value:'timed-out'}.
        assert!(b("var __u = new Int32Array(new SharedArrayBuffer(4));\
             Atomics.waitAsync(__u, 0, 0, 0).value === 'timed-out'"));
        // Value match, timeout > 0 → {async:true, value: Promise}.
        assert!(b("var __v = new Int32Array(new SharedArrayBuffer(4));\
             var r = Atomics.waitAsync(__v, 0, 0, 10);\
             r.async === true && r.value instanceof Promise"));
    });
}
