//! Conformance / support-matrix harness for DashScript.
//!
//! Three data sources merged into one feature list:
//! - `tests-fixtures.json` — auto-extracted from `translator/tests/*.rs` by
//!   `scripts/extract-tests.mjs` (**zero hand-written fixtures**). Each entry is
//!   a verified-translatable `.ds` snippet; the runner cargo-checks it
//!   informationally (`translator/tests` only asserts the translated Rust
//!   *contains* a substring — it never compiles). No `expect`, so the run
//!   reports the current state without asserting it.
//! - `bcd-catalog.json` — ES built-in API catalog auto-derived from MDN
//!   browser-compat-data by `scripts/sync-bcd.mjs`. Coverage-gap data only (bcd
//!   lists APIs, never call sites) → recorded `untested`, never run.
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
const BCD_JSON: &str = include_str!("conformance/data/bcd-catalog.json");
const CORRECTNESS_JSON: &str = include_str!("conformance/data/correctness.json");

/// A minimal binary manifest — conformance fixtures exercise built-in APIs only
/// (no crate dependencies), and `cargo check` does not require `main`, so a bare
/// declaration compiles. `cargo run` (the correctness layer) does require `main`,
/// which correctness fixtures provide.
const MANIFEST: &str = "[package]\nname = \"probe\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n[dependencies]\n";

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
    note: String,
}

#[test]
fn conformance_matrix() {
    let tests: FeatureFile = serde_json::from_str(TESTS_JSON).expect("parse tests-fixtures.json");
    let bcd: FeatureFile = serde_json::from_str(BCD_JSON).expect("parse bcd-catalog.json");
    let correct: FeatureFile = serde_json::from_str(CORRECTNESS_JSON).expect("parse correctness.json");
    let raws: Vec<RawFeature> = tests
        .features
        .into_iter()
        .chain(bcd.features)
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
    // (fixture text, observed status) of run features, for bcd association.
    let mut tested: Vec<(String, &'static str)> = Vec::new();

    // Phase 1 — run translator-tests + correctness fixtures (the slow cargo
    // part). bcd catalog entries carry no fixture and are deferred to phase 2.
    for raw in &raws {
        if raw.source == "bcd" {
            continue;
        }
        let diags = Translator::new().check(&raw.fixture);
        let (status, detail) = if !diags.is_empty() {
            let msg = diags.iter().map(|d| format!("{d}")).collect::<Vec<_>>().join(" | ");
            ("unsupported", msg)
        } else {
            let rust = match Translator::new().translate(&raw.fixture) {
                Ok(r) => r,
                Err(e) => {
                    outcomes.push(outcome(raw, "partial", format!("translate error: {e}"), None));
                    continue;
                }
            };
            write_project(&project, &rust);
            let (ok, err) = cargo(&project, &target_dir, &["check", "--quiet", "--message-format=short"]);
            if ok { ("supported", String::new()) } else { ("partial", err) }
        };

        // Correctness layer — only when the feature compiles AND declares an
        // expected stdout. `console.log(x)` lowers to `println!("{}", x)`
        // (Display, not Debug): fixtures must log primitives or joined strings,
        // never bare Vec/struct (no Display => won't compile).
        let correct = if status == "supported" {
            raw.expect_output.as_ref().map(|expected| {
                let rust = Translator::new().translate(&raw.fixture).unwrap_or_default();
                write_project(&project, &rust);
                match cargo(&project, &target_dir, &["run", "--quiet"]) {
                    (true, stdout) => stdout.trim() == expected.trim(),
                    _ => false,
                }
            })
        } else {
            None
        };

        tested.push((raw.fixture.clone(), status));
        outcomes.push(outcome(raw, status, detail, correct));
    }

    // Phase 2 — associate bcd catalog entries with a run fixture. math/global
    // have an unambiguous call form (`Math.abs(`, `parseInt(`); a fixture
    // containing it proves the API is exercised and inherits that status.
    // String/array/number methods are ambiguous (`.includes(`) → stay untested.
    for raw in &raws {
        if raw.source != "bcd" {
            continue;
        }
        if let Some(token) = probe_token(&raw.id) {
            if let Some(&(_, st)) = tested.iter().find(|(f, _)| f.contains(token.as_str())) {
                outcomes.push(outcome(raw, st, String::new(), None));
                continue;
            }
        }
        outcomes.push(outcome(raw, "untested", String::new(), None));
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
        .map(|o| format!("  - {}: expected {:?}, got {} — {}", o.id, o.expect, o.status, o.detail))
        .collect::<Vec<_>>()
        .join("\n");
    panic!("{} conformance expectation(s) not met:\n{}", mismatches.len(), report);
}

/// For a bcd catalog id with an unambiguous call form, the substring a
/// translator-tests fixture would contain if it exercised that API — used to
/// associate bcd entries with already-run fixtures. `math.abs` → `Math.abs(`,
/// `math.PI` → `Math.PI`, `global.parseInt` → `parseInt(`, `object.keys` →
/// `Object.keys(`. String/array instance methods are receiver-ambiguous
/// (`.includes(` could be either) and stay `None`.
fn probe_token(id: &str) -> Option<String> {
    let (cat, name) = id.split_once('.')?;
    let const_like = name.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_');
    match cat {
        "math" if const_like => Some(format!("Math.{name}")),
        "math" => Some(format!("Math.{name}(")),
        // `isNaN`/`isFinite` as bare globals aren't supported (DashScript uses
        // `Number.isNaN`), and `isNaN(` would accidentally match `Number.isNaN(`
        // in a fixture — so leave them untested rather than mis-associate.
        "global" if matches!(name, "isNaN" | "isFinite") => None,
        "global" => Some(format!("{name}(")),
        // Number has three call shapes, all under the `number.<name>` bcd id:
        // constants `Number.<CONST>` (EPSILON, NaN — uppercase, no call), static
        // methods `Number.<m>(` (isNaN/isFinite/isInteger/isSafeInteger/
        // parseFloat/parseInt), and instance methods `.<m>(` (toFixed,
        // toExponential, valueOf). `NaN` is mixed-case so first-letter case is
        // the constant/non-constant split; the static set is enumerated.
        "number" if name.starts_with(|c: char| c.is_ascii_uppercase()) => {
            Some(format!("Number.{name}"))
        }
        "number"
            if matches!(
                name,
                "isNaN" | "isFinite" | "isInteger" | "isSafeInteger"
                    | "parseFloat" | "parseInt"
            ) =>
        {
            Some(format!("Number.{name}("))
        }
        "number" => Some(format!(".{name}(")),
        // Object static methods (Object.keys(m)) are unambiguous — no
        // instance shares the `Object.<m>(` form. keys/values/entries are
        // mapped; the rest have no fixture and stay untested.
        "object" => Some(format!("Object.{name}(")),
        // String/Array instance methods: `.method(` is receiver-ambiguous in
        // principle, but bcd's String and Array method sets don't share a name
        // the translator maps on only one side — the shared names
        // (at/concat/includes/indexOf/lastIndexOf/slice) are mapped on both —
        // so associating by method name is safe. `toString` is left out: it's
        // shared with Object/number and only supported on the scalar receivers.
        "string" | "array"
            if !const_like && name != "toString" =>
        {
            Some(format!(".{name}("))
        }
        _ => None,
    }
}

fn outcome(raw: &RawFeature, status: &'static str, detail: String, correct: Option<bool>) -> Outcome {
    Outcome {
        id: raw.id.clone(),
        category: raw.category.clone(),
        status,
        detail,
        expect: raw.expect.clone(),
        correct,
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

fn write_matrix(outcomes: &[Outcome]) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("conformance");
    let json = serde_json::to_string_pretty(outcomes).unwrap_or_default();
    let _ = fs::write(dir.join("matrix.json"), format!("{json}\n"));
    let _ = fs::write(dir.join("matrix.md"), render_md(outcomes));
}

fn render_md(outcomes: &[Outcome]) -> String {
    let total = outcomes.len();
    let supported = outcomes.iter().filter(|o| o.status == "supported").count();
    let partial = outcomes.iter().filter(|o| o.status == "partial").count();
    let unsupported = outcomes.iter().filter(|o| o.status == "unsupported").count();
    let untested = outcomes.iter().filter(|o| o.status == "untested").count();
    let correct = outcomes.iter().filter(|o| matches!(o.correct, Some(true))).count();

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
            let note = if o.detail.is_empty() { o.note.clone() } else { o.detail.clone() };
            let note = note.replace('|', "\\|").replace(['\n', '\r'], " ");
            // `correct` folds into the detail column rather than adding a 4th
            // column — the header declares only 3, so a trailing column would
            // break the markdown table render.
            let correct_suffix = match o.correct {
                Some(c) => format!(" _correct: {}_", c),
                None => String::new(),
            };
            s.push_str(&format!("| {} | {} {} | {}{} |\n", o.id, badge, o.status, note, correct_suffix));
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
