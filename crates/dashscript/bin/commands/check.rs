//! `ds check`, `ds lint`, and `ds fmt`: in-process translatability and format,
//! built on the `oxc_parser` AST (no oxlint/oxfmt dependency). Each takes an
//! optional file — no argument runs over every `.ts` in the project, like
//! `vp check` / `oxlint`. `ds check --fix` writes formatting fixes in place.

use std::{error::Error, fs, path::PathBuf, process::ExitCode};

use dashscript::Translator;

use super::project::collect_ts_files;

/// Resolve the `.ts` targets for a check/lint/fmt command: a named file, or —
/// with no argument — every `.ts` under the project root. Errors when no
/// argument is given and no `.ts` files are found.
fn targets_for(target: Option<&str>) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    match target {
        Some(file) => Ok(vec![PathBuf::from(file)]),
        None => {
            let files = collect_ts_files();
            if files.is_empty() {
                Err(
                    "ds: no .ts files found (pass <file.ts>, or run inside a DashScript project)"
                        .into(),
                )
            } else {
                Ok(files)
            }
        }
    }
}

/// The composite check (`ds check [--fix] [<file>]`, like `vp check`):
/// translatability plus format. Without `--fix`, a format mismatch is reported
/// (no write); with `--fix`, the formatted source is written. Translatability
/// issues are always reported (they are structural and cannot be auto-fixed).
/// No argument → every `.ts` in the project. Fails if any file surfaces an
/// issue `--fix` cannot clear.
pub(crate) fn check(target: Option<&str>, fix: bool) -> Result<ExitCode, Box<dyn Error>> {
    let mut any_failed = false;
    for path in targets_for(target)? {
        let source = fs::read_to_string(&path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let mut file_failed = false;

        // 1. translatability — reported, never auto-fixed (a structural gap).
        let diagnostics = Translator::new().check(&source);
        for diag in &diagnostics {
            let report = diag.clone().with_source_code(source.clone());
            eprintln!("{report:?}");
        }
        if !diagnostics.is_empty() {
            file_failed = true;
        }

        // 2. format — `--fix` writes it, otherwise just report the mismatch.
        let formatted = Translator::new().format(&source)?;
        if formatted != source {
            if fix {
                fs::write(&path, &formatted)?;
                println!("ds: fixed formatting in {}", path.display());
            } else {
                eprintln!(
                    "ds: {} is not formatted (run `ds check --fix` or `ds fmt`)",
                    path.display()
                );
                file_failed = true;
            }
        }

        if !file_failed {
            println!("ds: no issues found in {}", path.display());
        }
        any_failed |= file_failed;
    }
    Ok(if any_failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

/// Lint translatability only (`ds lint [<file>] [--json]`, the old `ds check`):
/// syntax errors (from `oxc_parser`) plus any top-level statement the
/// translator cannot lower to Rust. No argument → every `.ts` in the project.
/// No external oxlint dependency.
///
/// `--json` emits a machine-readable array (one object per diagnostic, 1-based
/// line/column) on stdout — the `@dashscript/typescript-plugin` spawns this to
/// surface translatability diagnostics in the editor. Without `--json`, the
/// human-readable miette render goes to stderr as before.
pub(crate) fn lint(target: Option<&str>, json: bool) -> Result<ExitCode, Box<dyn Error>> {
    let mut failed = false;
    let mut json_out: Vec<serde_json::Value> = Vec::new();
    for path in targets_for(target)? {
        let source = fs::read_to_string(&path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let diagnostics = Translator::new().check(&source);
        if json {
            for diag in &diagnostics {
                json_out.push(diagnostic_to_json(&path, diag, &source));
            }
        } else if diagnostics.is_empty() {
            println!("ds: no issues found in {}", path.display());
            continue;
        } else {
            for diag in &diagnostics {
                // `with_source_code` attaches the file text so the fancy Debug
                // render can print line/column + context.
                let report = diag.clone().with_source_code(source.clone());
                eprintln!("{report:?}");
            }
        }
        if !diagnostics.is_empty() {
            failed = true;
        }
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&json_out)?);
    }
    Ok(if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

/// One translatability diagnostic as JSON for the TS plugin. Line/column are
/// 1-based (LSP convention); the first label is the primary span —
/// translatability diagnostics are single-span, so further labels are ignored.
fn diagnostic_to_json(
    path: &std::path::Path,
    diag: &oxc_diagnostics::OxcDiagnostic,
    source: &str,
) -> serde_json::Value {
    // `OxcDiagnostic` derefs to `OxcDiagnosticInner`, whose `severity`
    // (`Severity`, a miette type alias) and `labels` (`Labels`) fields are
    // `pub`. The `Diagnostic` trait — which provides the `severity()` /
    // `labels()` *methods* — is not re-exported by oxc_diagnostics (its
    // `use miette::Diagnostic` is private), so direct field access is the
    // supported path here.
    let severity = match diag.severity {
        oxc_diagnostics::Severity::Warning => "warning",
        _ => "error",
    };
    let (line, column, end_line, end_column) = diag
        .labels
        .as_slice()
        .first()
        .map(|label| {
            let start = label.offset();
            let end = start + label.len();
            let (sl, sc) = byte_to_line_col(source, start as usize);
            let (el, ec) = byte_to_line_col(source, end as usize);
            (sl, sc, el, ec)
        })
        .unwrap_or((1, 1, 1, 1));
    serde_json::json!({
        "file": path.display().to_string(),
        "line": line,
        "column": column,
        "endLine": end_line,
        "endColumn": end_column,
        "message": diag.to_string(),
        "severity": severity,
    })
}

/// Map a byte offset into `source` to a 1-based `(line, column)`.
fn byte_to_line_col(source: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(source.len());
    let mut line = 1;
    let mut last_line_start = 0;
    for (i, ch) in source.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            last_line_start = i + 1;
        }
    }
    let column = source[last_line_start..offset].chars().count() + 1;
    (line, column)
}

/// Format `.ts` in place with `oxc_codegen` (`ds fmt [<file>]`). No argument →
/// every `.ts` in the project. No external oxfmt dependency.
pub(crate) fn fmt(target: Option<&str>) -> Result<ExitCode, Box<dyn Error>> {
    for path in targets_for(target)? {
        let source = fs::read_to_string(&path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let formatted = Translator::new().format(&source)?;
        fs::write(&path, formatted)?;
        println!("ds: formatted {}", path.display());
    }
    Ok(ExitCode::SUCCESS)
}
