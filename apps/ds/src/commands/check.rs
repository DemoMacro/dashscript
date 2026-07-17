//! `ds lint`, `ds check`, and `ds fmt`: in-process translatability and format,
//! built on the `oxc_parser` AST (no oxlint/oxfmt dependency).

use std::{error::Error, fs, path::Path, process::ExitCode};

use dashscript::Translator;

/// Lint a `.ds` file for translatability in-process: syntax errors (from
/// `oxc_parser`) plus any top-level statement the translator cannot lower to
/// Rust. No external oxlint dependency.
pub(crate) fn lint(file: &str) -> Result<ExitCode, Box<dyn Error>> {
    let path = Path::new(file);
    let source = fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let diagnostics = Translator::new().check(&source);
    if diagnostics.is_empty() {
        println!("ds: no issues found in {file}");
        return Ok(ExitCode::SUCCESS);
    }
    for diag in &diagnostics {
        // `with_source_code` attaches the file text so the fancy Debug render
        // (miette `fancy-no-syscall`) can print line/column + context.
        let report = diag.clone().with_source_code(source.clone());
        eprintln!("{report:?}");
    }
    Ok(ExitCode::FAILURE)
}

/// Check a `.ds` file the way `vp check` does: translatability (`lint`) plus a
/// format check (reports whether `ds fmt` would change the file — does not
/// write). Fails if either surfaces an issue.
pub(crate) fn check(file: &str) -> Result<ExitCode, Box<dyn Error>> {
    let path = Path::new(file);
    let source = fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;

    let mut failed = false;

    // 1. translatability
    let diagnostics = Translator::new().check(&source);
    for diag in &diagnostics {
        let report = diag.clone().with_source_code(source.clone());
        eprintln!("{report:?}");
    }
    if !diagnostics.is_empty() {
        failed = true;
    }

    // 2. format check (no write)
    let formatted = Translator::new().format(&source)?;
    if formatted != source {
        eprintln!("ds: {file} is not formatted (run `ds fmt {file}`)");
        failed = true;
    }

    if !failed {
        println!("ds: no issues found in {file}");
        Ok(ExitCode::SUCCESS)
    } else {
        Ok(ExitCode::FAILURE)
    }
}

/// Format a `.ds` file in place with `oxc_codegen` (pretty-print). No external
/// oxfmt dependency.
pub(crate) fn fmt(file: &str) -> Result<ExitCode, Box<dyn Error>> {
    let path = Path::new(file);
    let source = fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let formatted = Translator::new().format(&source)?;
    fs::write(path, formatted)?;
    println!("ds: formatted {file}");
    Ok(ExitCode::SUCCESS)
}
