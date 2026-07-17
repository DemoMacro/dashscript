//! `ds check`, `ds lint`, and `ds fmt`: in-process translatability and format,
//! built on the `oxc_parser` AST (no oxlint/oxfmt dependency). Each takes an
//! optional file — no argument runs over every `.ds` in the project, like
//! `vp check` / `oxlint`. `ds check --fix` writes formatting fixes in place.

use std::{error::Error, fs, path::PathBuf, process::ExitCode};

use dashscript::Translator;

use super::project::collect_ds_files;

/// Resolve the `.ds` targets for a check/lint/fmt command: a named file, or —
/// with no argument — every `.ds` under the project root. Errors when no
/// argument is given and no `.ds` files are found.
fn targets_for(target: Option<&str>) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    match target {
        Some(file) => Ok(vec![PathBuf::from(file)]),
        None => {
            let files = collect_ds_files();
            if files.is_empty() {
                Err(
                    "ds: no .ds files found (pass <file.ds>, or run inside a DashScript project)"
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
/// No argument → every `.ds` in the project. Fails if any file surfaces an
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

/// Lint translatability only (`ds lint [<file>]`, the old `ds check`): syntax
/// errors (from `oxc_parser`) plus any top-level statement the translator
/// cannot lower to Rust. No argument → every `.ds` in the project. No external
/// oxlint dependency.
pub(crate) fn lint(target: Option<&str>) -> Result<ExitCode, Box<dyn Error>> {
    let mut failed = false;
    for path in targets_for(target)? {
        let source = fs::read_to_string(&path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let diagnostics = Translator::new().check(&source);
        if diagnostics.is_empty() {
            println!("ds: no issues found in {}", path.display());
            continue;
        }
        failed = true;
        for diag in &diagnostics {
            // `with_source_code` attaches the file text so the fancy Debug
            // render (miette `fancy-no-syscall`) can print line/column + context.
            let report = diag.clone().with_source_code(source.clone());
            eprintln!("{report:?}");
        }
    }
    Ok(if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

/// Format `.ds` in place with `oxc_codegen` (`ds fmt [<file>]`). No argument →
/// every `.ds` in the project. No external oxfmt dependency.
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
