//! Conformance / support-matrix harness for DashScript.
//!
//! Three data sources merged into one feature list:
//! - `tests-fixtures.json` — auto-extracted from `translator/tests/*.rs` by
//!   `scripts/extract-tests.mjs` (**zero hand-written fixtures**). Each entry is
//!   a verified-translatable `.ds` snippet; the runner cargo-checks it
//!   informationally (`translator/tests` only asserts the translated Rust
//!   *contains* a substring — it never compiles). No `expect`, so the run
//!   reports the current state without asserting it.
//! - `test262.json` — auto-extracted from tc39 test262 by
//!   `scripts/extract-test262.mjs`. The differential layer: each fixture is
//!   wrapped in a `main()` that logs its asserts, then Node (oracle) and `ds`
//!   (actual) run the same source and stdout is diffed line by line. Result
//!   `supported` (match) | `partial` (compile fail or stdout diff); Node absent
//!   → oracle skipped (compile-only).
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

use dashscript::{RuntimeDeps, Translator};
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

/// test262 `features:` that neither the Node oracle nor the ds engine ships — a
/// fixture exercising one has neither an oracle nor ds support, so it is
/// honestly `unsupported` without running anything. Today: `Temporal` (ES stage
/// 3; node v24 and QuickJS both lack it). When ds gains a feature here (e.g.
/// via `temporal-rs`), drop it from this list and route those fixtures through
/// a ds-only oracle path.
const UNSHIPPED_FEATURES: &[&str] = &["Temporal"];

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

/// Differential-test result against the Node oracle (test262 features only).
#[derive(Debug, Clone, Serialize)]
struct Oracle {
    status: &'static str, // matched | diff | node-error | node-missing
    #[serde(skip_serializing_if = "String::is_empty")]
    detail: String,
}

impl Oracle {
    fn matched() -> Self {
        Self {
            status: "matched",
            detail: String::new(),
        }
    }
    fn diff(detail: String) -> Self {
        Self {
            status: "diff",
            detail,
        }
    }
    fn err(detail: String) -> Self {
        Self {
            status: "node-error",
            detail,
        }
    }
    fn missing() -> Self {
        Self {
            status: "node-missing",
            detail: String::new(),
        }
    }
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
    #[serde(skip_serializing_if = "Option::is_none")]
    oracle: Option<Oracle>,
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

    // N independent probe projects, each with its own `target/` + `work/`,
    // run in parallel. cargo's incremental build is keyed per-target-dir, so a
    // single shared `target/` forces the whole matrix to serialize on one
    // linker lock — every tiny `main.rs` revision re-links under it. Splitting
    // the fixtures across workers gives cargo N independent `target/`s to
    // compile into concurrently; each worker pays a one-time std compile,
    // amortized across its share. `node_oracle` writes a fixed `oracle.ts`
    // into `work/`, so each worker needs its own `work/` too (a shared one
    // would clobber the file mid-run).
    let n_workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .clamp(1, 8);
    let tmp = TempDir::new().expect("tempdir");
    let workers: Vec<(PathBuf, PathBuf, PathBuf)> = (0..n_workers)
        .map(|i| {
            let root = tmp.path().join(format!("w{i}"));
            let project = root.join("probe");
            let target_dir = root.join("target");
            let work = root.join("work");
            fs::create_dir_all(project.join("src")).expect("create probe src");
            fs::create_dir_all(&work).expect("create work");
            (project, target_dir, work)
        })
        .collect();

    // Populate `~/.cargo` once (serially) with every runtime dep a fixture might
    // inject, so the N parallel workers don't race the crates-io registry
    // update lock on their first `cargo` call. On success the workers then build
    // offline (`CARGO_NET_OFFLINE=true`), eliminating the registry-lock race
    // entirely — without this the losers fail "unable to update registry
    // `crates-io`" and the fixture is mis-recorded as `partial`.
    warm_cargo_cache(&workers[0].0);

    // Node is the test262 ground-truth oracle. Probe once; if absent, the
    // differential layer degrades to compile-only (oracle → node-missing).
    let node_ok = node_available();

    // Split the fixtures into `n_workers` contiguous chunks — one per worker
    // thread. Each thread runs its chunk sequentially against its own
    // project/target/work triple, so the parallelism is across workers (N
    // simultaneous cargo invocations), not within one.
    let chunk_size = raws.len().div_ceil(n_workers).max(1);
    let handles: Vec<_> = raws
        .chunks(chunk_size)
        .enumerate()
        .map(|(i, chunk)| {
            let (project, target_dir, work) = workers[i].clone();
            let chunk: Vec<(RawFeature, &'static str)> = chunk.to_vec();
            std::thread::spawn(move || {
                chunk
                    .into_iter()
                    .map(|(raw, layer)| {
                        run_fixture(&raw, layer, &project, &target_dir, &work, node_ok)
                    })
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
/// cargo project: a reflection `.ds` source → `translate_with_deps` (flips
/// `needs_engine`) → `write_project` (injects `__ds_engine` + the `rquickjs`
/// dep) → `cargo check`. The in-process `engine_eval` path skips cargo per
/// fixture, so this smoke test is the verification the per-fixture cargo check
/// used to provide — not repeated 1600×, just once. Fails loudly if the engine
/// Rust template, the `__ds_engine` module, or the dep wiring regresses.
#[test]
fn engine_path_compiles_to_valid_rust_project() {
    let tmp = TempDir::new().expect("tempdir");
    let project = tmp.path().join("probe");
    let target_dir = tmp.path().join("target");
    fs::create_dir_all(project.join("src")).expect("probe src");
    let src = "function main(): void {\n  const o = {};\n  Object.defineProperty(o, \"x\", { value: 1 });\n  console.log(o.x);\n}";
    let (rust, deps) = Translator::new()
        .translate_with_deps(src)
        .expect("translate reflection source");
    assert!(
        deps.needs_engine(),
        "Object.defineProperty should flip needs_engine"
    );
    write_project(&project, &rust, src, &deps);
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
    mut oracle: Option<Oracle>,
) -> Outcome {
    // A failed fixture's `detail` may quote a tool diagnostic (node/rustc) that
    // embeds the contributor's full tempdir. Scrub host-local absolute paths
    // before the value lands in the committed matrix — both the top-level
    // `detail` and the embedded `oracle.detail`.
    if let Some(o) = oracle.as_mut() {
        o.detail = scrub_local_paths(&o.detail);
    }
    Outcome {
        id: raw.id.clone(),
        layer: layer.to_string(),
        category: raw.category.clone(),
        status,
        detail: scrub_local_paths(&detail),
        expect: raw.expect.clone(),
        correct,
        oracle,
        note: raw.note.clone(),
    }
}

fn write_project(project: &Path, rust: &str, ds_source: &str, deps: &RuntimeDeps) {
    // `cargo check` on a bin crate requires a `main` (E0601). Most translator-tests
    // fixtures are bare declarations with no `main`, so synthesize an empty one
    // when the `.ds` source has no `function main()`. Correctness fixtures declare
    // their own, which lowers to `fn main` and is left untouched. AST-level
    // (`has_main`), so a `"fn main"` string literal never trips a false positive.
    let mut body = if Translator::new().has_main(ds_source) {
        rust.to_string()
    } else {
        format!("{rust}\nfn main() {{}}\n")
    };
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
    // Engine compat: a `.ds` source using ES reflection lowers to a single
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
    let captured = String::from_utf8_lossy(if is_run { &out.stdout } else { &out.stderr });
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
fn translate_catch(source: &str) -> Result<(String, RuntimeDeps), String> {
    use std::panic::AssertUnwindSafe;
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

/// Whether `node` is on PATH (the test262 oracle). Probed once per run.
fn node_available() -> bool {
    Command::new("node")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Node oracle outcome for a test262 fixture. The fixture defines `main()` but
/// never calls it (the extractor wraps asserts in a declaration); `ds` lowers
/// that to `fn main` (the entry), so we append `main();` for Node to match.
enum NodeResult {
    Ok(String),
    Error(String),
    Missing,
}

fn node_oracle(fixture: &str, work: &Path) -> NodeResult {
    let source = format!("{fixture}\nmain();\n");
    let file = work.join("oracle.ts");
    if fs::write(&file, &source).is_err() {
        return NodeResult::Error("failed to write oracle.ts".into());
    }
    match Command::new("node").arg(&file).output() {
        Ok(o) if o.status.success() => {
            NodeResult::Ok(String::from_utf8_lossy(&o.stdout).into_owned())
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            NodeResult::Error(format!(
                "exit {}: {}",
                o.status,
                stderr.chars().take(120).collect::<String>()
            ))
        }
        Err(_) => NodeResult::Missing,
    }
}

/// Full stdout of `cargo run` (untrimmed) for the test262 differential — the
/// shared `cargo()` truncates to 6 lines, which would mask multi-assert diffs.
fn cargo_run_full(project: &Path, target_dir: &Path) -> Option<String> {
    let out = Command::new("cargo")
        .args(["run", "--quiet"])
        .env("CARGO_TARGET_DIR", target_dir)
        .current_dir(project)
        .output()
        .ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        None
    }
}

/// Line-by-line diff of `ds` stdout vs the Node oracle. `None` = equivalent;
/// `Some` = up to the first 3 differing lines (or a line-count mismatch).
/// Equivalence (not raw string equality) is via `lines_equiv`, which normalizes
/// the display-layer differences between Rust `f64` Display and JS
/// `Number.toString` — see its doc comment for why.
fn diff_stdout(ds: &str, oracle: &str) -> Option<String> {
    let d: Vec<&str> = ds.lines().filter(|l| !l.trim().is_empty()).collect();
    let o: Vec<&str> = oracle.lines().filter(|l| !l.trim().is_empty()).collect();
    let all_equiv = d.len() == o.len() && d.iter().zip(o.iter()).all(|(a, b)| lines_equiv(a, b));
    if all_equiv {
        return None;
    }
    let mut diffs = Vec::new();
    for (i, (a, b)) in d.iter().zip(o.iter()).enumerate() {
        if !lines_equiv(a, b) {
            diffs.push(format!("line {}: ds={:?} node={:?}", i + 1, a, b));
        }
        if diffs.len() >= 3 {
            break;
        }
    }
    if diffs.is_empty() {
        diffs.push(format!("line count: ds={} node={}", d.len(), o.len()));
    }
    Some(diffs.join("; "))
}

/// Whether a `ds` stdout line and a Node-oracle line are semantically equivalent.
/// Identical strings match; otherwise both are parsed as f64 — the same numeric
/// value counts as a match even when Rust Display and JS `Number.toString`
/// disagree on the *spelling* (`inf`/`Infinity`, `-inf`/`-Infinity`,
/// `1000000000000000000000`/`1e+21`). DashScript's semantics are Rust's; these
/// are display-layer differences, not semantic bugs, so the differential layer
/// normalizes them away rather than letting translator output mimic JS
/// `ToString`. Non-numeric lines (strings, "__OK__", constructor names from
/// `assert.throws`) fall back to exact comparison. `NaN` matches `NaN`; `-0.0`
/// matches `0.0` at this layer (the value layer already produces the right sign).
fn lines_equiv(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    match (parse_num(a), parse_num(b)) {
        (Some(x), Some(y)) => (x.is_nan() && y.is_nan()) || x == y,
        _ => false,
    }
}

/// Parse a stdout line as f64, accepting both Rust Display (`inf`, `-inf`,
/// `NaN`) and JS `Number.toString` (`Infinity`, `-Infinity`, `NaN`) spellings,
/// plus plain/scientific numerics. `None` for non-numeric lines.
fn parse_num(s: &str) -> Option<f64> {
    match s.trim() {
        "inf" | "Infinity" => Some(f64::INFINITY),
        "-inf" | "-Infinity" => Some(f64::NEG_INFINITY),
        "NaN" | "nan" => Some(f64::NAN),
        t => t.parse::<f64>().ok(),
    }
}

/// The `detail` marker for a supported run that used the embedded QuickJS
/// engine (ES reflection), so the matrix records honestly which supported
/// fixtures run via the compat path rather than the static Rust lowering.
fn engine_note(via_engine: bool) -> String {
    if via_engine {
        "via rquickjs engine".to_string()
    } else {
        String::new()
    }
}

/// QuickJS prelude that defines `console.log` with a Node-`inspect`-style
/// formatter. ES `String()` coerces objects to `[object Object]`, functions to
/// their source text, and BigInt without the `n` suffix — all diverging from
/// Node's `console.log`, which would surface as a spurious oracle diff. This
/// prelude renders objects `{ k: v }`, functions `[Function: name]`, and BigInt
/// `0n`, matching Node for the test262 fixture shapes (depth-limited,
/// single-quoted nested strings). `__ds_print_line` is the native line sink
/// [`engine_eval`] installs per call.
const INSPECT_PRELUDE: &str = r#"
this.console = {
  log: function () {
    var out = [];
    for (var i = 0; i < arguments.length; i++) {
      out.push(__ds_inspect(arguments[i], 0));
    }
    __ds_print_line(out.join(" "));
  }
};
function __ds_inspect(v, depth) {
  if (v === null) return "null";
  if (v === undefined) return "undefined";
  var t = typeof v;
  if (t === "string") return depth === 0 ? v : "'" + v + "'";
  if (t === "number" || t === "boolean") return String(v);
  if (t === "bigint") return v.toString() + "n";
  if (t === "symbol") return v.toString();
  if (t === "function") {
    var name = v.name ? v.name : "(anonymous)";
    return "[Function: " + name + "]";
  }
  if (depth >= 6) return "...";
  if (Array.isArray(v)) {
    if (v.length === 0) return "[]";
    var items = [];
    for (var i = 0; i < v.length; i++) {
      items.push(__ds_inspect(v[i], depth + 1));
    }
    return "[ " + items.join(", ") + " ]";
  }
  try {
    var keys = Object.keys(v);
  } catch (e) {
    return String(v);
  }
  if (keys.length === 0) return "{}";
  var items = [];
  for (var i = 0; i < keys.length; i++) {
    items.push(__ds_format_key(keys[i]) + ": " + __ds_inspect(v[keys[i]], depth + 1));
  }
  return "{ " + items.join(", ") + " }";
}
function __ds_format_key(k) {
  if (/^[A-Za-z_$][A-Za-z0-9_$]*$/.test(k)) return k;
  return "'" + k + "'";
}
"#;

/// Run an engine-gated fixture's ECMAScript under an embedded QuickJS engine —
/// the same engine `__ds_engine` embeds for `ds build`, but in-process —
/// capturing `console.log` output via [`INSPECT_PRELUDE`]. Skips the cargo
/// compile entirely: the engine Rust template (`fn main() {
/// __ds_engine::run(src) }`) is fixed-shape and its compile correctness is
/// covered by translator unit tests + the engine-path integration test, so
/// re-compiling a throwaway project per fixture would only burn time. Returns
/// the captured stdout, or `None` if the engine fails to start or the source
/// throws (a throw surfaces as empty stdout → an oracle diff → `partial`).
fn engine_eval(js_source: &str) -> Option<String> {
    use rquickjs::{context::EvalOptions, Context, Ctx, Runtime};
    use std::sync::{Arc, Mutex};
    let runtime = Runtime::new().ok()?;
    let ctx = Context::full(&runtime).ok()?;
    let sloppy = || {
        let mut o = EvalOptions::default();
        o.strict = false;
        o
    };
    let captured: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
    let cap = captured.clone();
    let result = ctx.with(move |ctx: Ctx<'_>| -> rquickjs::Result<()> {
        let print_line = rquickjs::Function::new(ctx.clone(), move |s: String| {
            let mut g = cap.lock().expect("engine capture lock");
            g.push_str(&s);
            g.push('\n');
        })?;
        ctx.globals().set("__ds_print_line", print_line)?;
        ctx.eval_with_options::<(), _>(INSPECT_PRELUDE, sloppy())?;
        ctx.eval_with_options::<(), _>(js_source, sloppy())?;
        ctx.eval_with_options::<(), _>("main();", sloppy())?;
        Ok(())
    });
    result.ok()?;
    let mut g = captured.lock().expect("engine capture lock");
    Some(std::mem::take(&mut g))
}

/// Compare `ds_stdout` against the Node oracle for a test262 fixture. Shared by
/// the engine path (in-process QuickJS) and the static path (cargo run) — the
/// differential logic is identical; only the success `detail` note differs.
fn compare_oracle(
    ds_stdout: &str,
    fixture: &str,
    work: &Path,
    node_ok: bool,
    via_engine: bool,
) -> (&'static str, String, Option<Oracle>) {
    if !node_ok {
        return (
            "supported",
            engine_note(via_engine),
            Some(Oracle::missing()),
        );
    }
    match node_oracle(fixture, work) {
        NodeResult::Missing => (
            "supported",
            engine_note(via_engine),
            Some(Oracle::missing()),
        ),
        NodeResult::Error(e) => {
            // Node failed. If our side (engine or cargo run) also produced no
            // output, both sides lack the feature the fixture exercises (e.g.
            // `Temporal`, which neither Node nor QuickJS ships) — that is honest
            // `unsupported`, not a spurious `supported` from empty-vs-empty
            // stdout agreement. If our side did produce output, the oracle is
            // unavailable, so we cannot claim `supported` → `partial`.
            if ds_stdout.trim().is_empty() {
                ("unsupported", format!("node + ds both error: {e}"), None)
            } else {
                (
                    "partial",
                    format!("node oracle error: {e}"),
                    Some(Oracle::err(e)),
                )
            }
        }
        NodeResult::Ok(oracle_stdout) => match diff_stdout(ds_stdout, &oracle_stdout) {
            None => (
                "supported",
                engine_note(via_engine),
                Some(Oracle::matched()),
            ),
            Some(d) => (
                "partial",
                format!("oracle diff: {d}"),
                Some(Oracle::diff(d)),
            ),
        },
    }
}

/// Run one test262 fixture through the differential pipeline. Returns
/// `(status, detail, oracle)`.
///
/// Engine path (`needs_engine`, ES reflection the static translator cannot
/// lower): run the source in-process under QuickJS vs the Node oracle — no
/// cargo compile (see [`engine_eval`]). Static path: `Translator::check`
/// (translatability) → `cargo check` (compiles) → `cargo run` vs the Node
/// oracle (semantics); translator scope limits like top-level statements stay
/// honestly `unsupported`.
fn run_test262(
    raw: &RawFeature,
    project: &Path,
    target_dir: &Path,
    work: &Path,
    node_ok: bool,
) -> (&'static str, String, Option<Oracle>) {
    // A fixture whose `features:` neither Node nor the ds engine ships has no
    // oracle and no ds support — `unsupported` up front, skipping translate +
    // node + cargo. (A1 caught this at runtime as both-fail; A2 makes it static
    // + precisely labeled.)
    for feat in &raw.features {
        if UNSHIPPED_FEATURES.contains(&feat.as_str()) {
            return (
                "unsupported",
                format!("neither node nor ds ships feature: {feat}"),
                None,
            );
        }
    }
    let (rust, deps) = match translate_catch(&raw.fixture) {
        Ok(r) => r,
        Err(e) => return ("partial", e, None),
    };
    // Engine path: ES reflection the static translator cannot lower. Run the
    // source in-process under QuickJS (the exact bytes `translate_with_deps`
    // embeds in `__ds_engine::run`), skipping the cargo compile entirely.
    if deps.needs_engine() {
        let js_source = match Translator::new().engine_source(&raw.fixture) {
            Some(s) => s,
            None => {
                return (
                    "partial",
                    "engine flag set but engine_source returned None".into(),
                    None,
                )
            }
        };
        let ds_stdout = engine_eval(&js_source).unwrap_or_default();
        return compare_oracle(&ds_stdout, &raw.fixture, work, node_ok, true);
    }
    // Static path: `check` (translatability) → cargo check (compiles) → cargo
    // run vs the Node oracle (semantics).
    let diags = Translator::new().check(&raw.fixture);
    if !diags.is_empty() {
        let msg = diags
            .iter()
            .map(|d| format!("{d}"))
            .collect::<Vec<_>>()
            .join(" | ");
        return ("unsupported", msg, None);
    }
    write_project(project, &rust, &raw.fixture, &deps);
    let (ok, err) = cargo(
        project,
        target_dir,
        &["check", "--quiet", "--message-format=short"],
    );
    if !ok {
        return ("partial", err, None);
    }
    let ds_stdout = cargo_run_full(project, target_dir).unwrap_or_default();
    compare_oracle(&ds_stdout, &raw.fixture, work, node_ok, false)
}

/// One fixture, run against a worker-owned `project`/`target_dir`/`work`
/// triple. Unifies the test262 differential path (Node oracle) with the
/// translator-tests/correctness path (cargo check + optional expected-stdout
/// run). Pure over its arguments — no shared mutable state across calls — so
/// it is safe to invoke from many threads in parallel, each on its own project.
fn run_fixture(
    raw: &RawFeature,
    layer: &str,
    project: &Path,
    target_dir: &Path,
    work: &Path,
    node_ok: bool,
) -> Outcome {
    if layer == "test262" {
        let (status, detail, oracle) = run_test262(raw, project, target_dir, work, node_ok);
        return outcome(raw, layer, status, detail, None, oracle);
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
            Err(e) => return outcome(raw, layer, "partial", e, None, None),
        };
        write_project(project, &rust, &raw.fixture, &deps);
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
            write_project(project, &rust, &raw.fixture, &deps);
            match cargo(project, target_dir, &["run", "--quiet"]) {
                (true, stdout) => stdout.trim() == expected.trim(),
                _ => false,
            }
        })
    } else {
        None
    };

    outcome(raw, layer, status, detail, correct, None)
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
            let oracle_suffix = match &o.oracle {
                Some(oracle) => format!(" _oracle: {}_", oracle.status),
                None => String::new(),
            };
            s.push_str(&format!(
                "| {} | {} {} | {}{}{} |\n",
                o.id, badge, o.status, note, correct_suffix, oracle_suffix
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
        "Per-category conformance vs tc39 test262 (Node oracle differential), plus the \
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
