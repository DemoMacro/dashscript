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

use std::{fs, path::Path, process::Command};

use dashscript::Translator;
use serde::{Deserialize, Serialize};
use tempfile::TempDir;

const TESTS_JSON: &str = include_str!("conformance/data/tests-fixtures.json");
const CORRECTNESS_JSON: &str = include_str!("conformance/data/correctness.json");
const TEST262_JSON: &str = include_str!("conformance/data/test262.json");

/// A minimal binary manifest — conformance fixtures exercise built-in APIs only
/// (no crate dependencies), and `cargo check` does not require `main`, so a bare
/// declaration compiles. `cargo run` (the correctness layer) does require `main`,
/// which correctness fixtures provide.
const MANIFEST: &str =
    "[package]\nname = \"probe\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n[dependencies]\n";

#[derive(Debug, Deserialize)]
struct FeatureFile {
    features: Vec<RawFeature>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawFeature {
    id: String,
    category: String,
    #[serde(default)]
    source: String,
    #[serde(default)]
    fixture: String,
    expect: Option<String>,
    expect_output: Option<String>,
    #[serde(default)]
    note: String,
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
    let test262: FeatureFile = serde_json::from_str(TEST262_JSON).expect("parse test262.json");
    // test262 has thousands of fixtures; bound the sample so a local `cargo
    // test` stays fast. The default 120 covers all of `Math/` — where the
    // translator gaps cluster (E0689 `{float}` on variable args, round -0,
    // hypot∞); `DASH_TEST262=all` (or `0`) runs the full set.
    let test262_limit = match std::env::var("DASH_TEST262") {
        Ok(v) if v == "all" || v == "0" => usize::MAX,
        Ok(v) => v.parse().unwrap_or(120),
        Err(_) => 120,
    };
    let raws: Vec<RawFeature> = tests
        .features
        .into_iter()
        .chain(test262.features.into_iter().take(test262_limit))
        .chain(correct.features)
        .collect();

    // One shared temp project + target dir so every `cargo check` reuses the
    // incremental build (std compiles once; each case only recompiles a tiny
    // main.rs). This is what keeps a 140+ feature matrix tractable.
    let tmp = TempDir::new().expect("tempdir");
    let project = tmp.path().join("probe");
    let target_dir = tmp.path().join("target");
    fs::create_dir_all(project.join("src")).expect("create probe src");

    let mut outcomes: Vec<Outcome> = Vec::with_capacity(raws.len());

    // Node is the test262 ground-truth oracle. Probe once; if absent, the
    // differential layer degrades to compile-only (oracle → node-missing).
    let node_ok = node_available();

    // Phase 1 — run translator-tests + correctness fixtures (the slow cargo
    // part) and test262 differential cases.
    for raw in &raws {
        if raw.source == "test262" {
            let (status, detail, oracle) =
                run_test262(raw, &project, &target_dir, tmp.path(), node_ok);
            outcomes.push(outcome(raw, status, detail, None, oracle));
            continue;
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
            let rust = match Translator::new().translate(&raw.fixture) {
                Ok(r) => r,
                Err(e) => {
                    outcomes.push(outcome(
                        raw,
                        "partial",
                        format!("translate error: {e}"),
                        None,
                        None,
                    ));
                    continue;
                }
            };
            write_project(&project, &rust);
            let (ok, err) = cargo(
                &project,
                &target_dir,
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
                let rust = Translator::new()
                    .translate(&raw.fixture)
                    .unwrap_or_default();
                write_project(&project, &rust);
                match cargo(&project, &target_dir, &["run", "--quiet"]) {
                    (true, stdout) => stdout.trim() == expected.trim(),
                    _ => false,
                }
            })
        } else {
            None
        };

        outcomes.push(outcome(raw, status, detail, correct, None));
    }

    write_matrix(&outcomes);

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

fn outcome(
    raw: &RawFeature,
    status: &'static str,
    detail: String,
    correct: Option<bool>,
    oracle: Option<Oracle>,
) -> Outcome {
    Outcome {
        id: raw.id.clone(),
        category: raw.category.clone(),
        status,
        detail,
        expect: raw.expect.clone(),
        correct,
        oracle,
        note: raw.note.clone(),
    }
}

fn write_project(project: &Path, rust: &str) {
    // `cargo check` on a bin crate requires a `main` (E0601). Most translator-tests
    // fixtures are bare declarations with no `main`, so synthesize an empty one
    // when the translated source lacks it. Correctness fixtures declare their own
    // `function main()`, which lowers to `fn main` and is left untouched.
    let body = if rust.contains("fn main") {
        rust.to_string()
    } else {
        format!("{rust}\nfn main() {{}}\n")
    };
    let _ = fs::write(project.join("Cargo.toml"), MANIFEST);
    let _ = fs::write(project.join("src").join("main.rs"), body);
}

/// Run `cargo <args>` in `project`, sharing `target_dir` across calls.
/// Returns `(success, captured-output)` — stderr for `check`, stdout for `run`.
fn cargo(project: &Path, target_dir: &Path, args: &[&str]) -> (bool, String) {
    let is_run = args.first().is_some_and(|a| *a == "run");
    let out = match Command::new("cargo")
        .args(args)
        .env("CARGO_TARGET_DIR", target_dir)
        .current_dir(project)
        .output()
    {
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

/// Run one test262 fixture through the differential pipeline:
/// `Translator::check` (gate) → `translate` + `cargo check` (compiles) →
/// `cargo run` vs the Node oracle (semantics). Returns `(status, detail, oracle)`.
fn run_test262(
    raw: &RawFeature,
    project: &Path,
    target_dir: &Path,
    work: &Path,
    node_ok: bool,
) -> (&'static str, String, Option<Oracle>) {
    let diags = Translator::new().check(&raw.fixture);
    if !diags.is_empty() {
        let msg = diags
            .iter()
            .map(|d| format!("{d}"))
            .collect::<Vec<_>>()
            .join(" | ");
        return ("unsupported", msg, None);
    }
    let rust = match Translator::new().translate(&raw.fixture) {
        Ok(r) => r,
        Err(e) => return ("partial", format!("translate error: {e}"), None),
    };
    write_project(project, &rust);
    let (ok, err) = cargo(
        project,
        target_dir,
        &["check", "--quiet", "--message-format=short"],
    );
    if !ok {
        return ("partial", err, None);
    }
    if !node_ok {
        return ("supported", String::new(), Some(Oracle::missing()));
    }
    let ds_stdout = cargo_run_full(project, target_dir).unwrap_or_default();
    match node_oracle(&raw.fixture, work) {
        NodeResult::Missing => ("supported", String::new(), Some(Oracle::missing())),
        NodeResult::Error(e) => ("supported", String::new(), Some(Oracle::err(e))),
        NodeResult::Ok(oracle_stdout) => match diff_stdout(&ds_stdout, &oracle_stdout) {
            None => ("supported", String::new(), Some(Oracle::matched())),
            Some(d) => (
                "partial",
                format!("oracle diff: {d}"),
                Some(Oracle::diff(d)),
            ),
        },
    }
}

fn write_matrix(outcomes: &[Outcome]) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("conformance");
    let json = serde_json::to_string_pretty(outcomes).unwrap_or_default();
    let _ = fs::write(dir.join("matrix.json"), format!("{json}\n"));
    let _ = fs::write(dir.join("matrix.md"), render_md(outcomes));
}

fn render_md(outcomes: &[Outcome]) -> String {
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

fn badge(status: &str) -> &'static str {
    match status {
        "supported" => "🟢",
        "partial" => "🟡",
        "unsupported" => "🔴",
        "untested" => "⚪",
        _ => "❓",
    }
}
