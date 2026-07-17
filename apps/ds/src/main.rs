//! `ds` — the DashScript toolchain entry point.
//!
//! Wired: `<file.ds>` (run a file), `run <script>`, `build [--target]`, `add`,
//! `remove`, `lint`, `check`, `fmt`, `install`, `cache clean`, `lsp`. Each
//! command lives in [`commands`] (one module per group); this file is just the
//! dispatch, the help text, and a couple of small helpers.

use std::{error::Error, path::Path, process::ExitCode};

mod commands;
mod lsp;

use commands::{build, cache, check, deps, run};

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        // Standard flags at the top level: `ds --help` / `ds -h` / `ds help`
        // print usage; `ds --version` / `ds -v` print the version.
        Some("-h") | Some("--help") | Some("help") => {
            print_help();
            ExitCode::SUCCESS
        }
        Some("-v") | Some("--version") => {
            print_version();
            ExitCode::SUCCESS
        }
        // `ds <file.ds>` — run a file directly (like `node a.js` / `vp node`).
        Some(arg) if is_ds_file(arg) => report(run::run_file(arg)),
        // `ds run <script>` — run a `manifest.json` script (like `pnpm run`).
        // `run` is always explicit: `ds <script>` would collide with `ds <file.ds>`.
        Some("run") => match args.next() {
            Some(script) => report(run::run_script(&script)),
            // `ds run` with no script lists available scripts (like `pnpm run`).
            None => report(run::list_scripts()),
        },
        Some("build") => {
            let rest: Vec<String> = args.collect();
            match build::parse_build_args(&rest) {
                Ok((file, target, filter)) => {
                    // No file at a workspace root → build every member (--filter
                    // picks one). `--filter` is workspace-only.
                    if file.is_none() && build::is_workspace_root(Path::new(".")) {
                        report(build::workspace_build(
                            Path::new("."),
                            filter.as_deref(),
                            target.as_deref(),
                        ))
                    } else if filter.is_some() {
                        usage_exit("ds build: --filter <name> only applies at a workspace root")
                    } else {
                        report(build::build(file.as_deref(), target.as_deref()))
                    }
                }
                Err(msg) => usage_exit(&msg),
            }
        }
        Some("add") => match args.next() {
            Some(spec) => report(deps::add(&spec)),
            None => usage_exit("usage: ds add <crate|rust:crate|file.rs>"),
        },
        Some("remove") => match args.next() {
            Some(name) => report(deps::remove(&name)),
            None => usage_exit("usage: ds remove <crate|rust:crate>"),
        },
        // `ds lint` = translatability only (the old `ds check`). `ds check`
        // below is the composite lint + fmt check, matching `vp check`.
        Some("lint") => match args.next() {
            Some(file) => report(check::lint(&file)),
            None => usage_exit("usage: ds lint <file.ds>"),
        },
        Some("check") => match args.next() {
            Some(file) => report(check::check(&file)),
            None => usage_exit("usage: ds check <file.ds>"),
        },
        Some("fmt") => match args.next() {
            Some(file) => report(check::fmt(&file)),
            None => usage_exit("usage: ds fmt <file.ds>"),
        },
        // `ds install` = ensure manifest deps are fetched + a Cargo.lock exists
        // (like `pnpm install` / `vp install`). No node_modules equivalent —
        // cargo's `~/.cargo/registry` is the dependency store.
        Some("install") => report(deps::install()),
        Some("cache") => match args.next().as_deref() {
            Some("clean") => report(cache::cache_clean()),
            Some(other) => usage_exit(&format!("ds cache: unknown action '{other}' (try 'clean')")),
            None => usage_exit("usage: ds cache clean"),
        },
        Some("lsp") => match lsp::run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("ds lsp: {err}");
                ExitCode::FAILURE
            }
        },
        Some(other) => {
            eprintln!("ds: unknown command '{other}'");
            eprintln!("run `ds --help` for the list of commands");
            ExitCode::FAILURE
        }
        None => {
            // No command: print the full help, but exit non-zero so shells and
            // scripts can tell "nothing was run".
            print_help();
            ExitCode::FAILURE
        }
    }
}

/// Print the grouped command reference (like `vp --help` / `pnpm --help`):
/// commands grouped by purpose, each with a one-line description, plus the
/// `Usage:` and `-h/-v` lines.
fn print_help() {
    println!("DashScript — TypeScript ergonomics, Rust performance, compiled to native.");
    println!();
    println!("Usage: ds <command> [args]");
    println!("       ds <file.ds>              run a file directly (like `node a.js`)");
    println!("       ds [ -h | --help | -v | --version ]");
    println!();
    println!("Run:");
    println!("  <file.ds>            Run a file (translate → compile → run)");
    println!("  run [<script>]       Run a manifest.json script (no arg lists scripts)");
    println!();
    println!("Build:");
    println!("  build [<file>]       Compile a native binary to dist/<name> (default)");
    println!("    --target rust        emit the translated Rust crate instead");
    println!("    --filter <name>      build one workspace member");
    println!();
    println!("Check & format:");
    println!("  lint <file>          Translatability check (in-process)");
    println!("  check <file>         Lint + format check (in-process)");
    println!("  fmt <file>           Format .ds in place (in-process)");
    println!();
    println!("Dependencies:");
    println!("  add <crate|file.rs>  Add a crate (rust:<name>) or bindgen a local .rs");
    println!("  remove <crate>       Remove a crate dependency");
    println!("  install              Fetch manifest deps + write Cargo.lock");
    println!();
    println!("Cache & editor:");
    println!("  cache clean          Remove the in-project .cache/");
    println!("  lsp                  Run the language server (for editor extensions)");
}

/// Print the version (`ds --version` / `ds -v`), from the crate version.
fn print_version() {
    println!("ds {} (DashScript)", env!("CARGO_PKG_VERSION"));
}

/// Report a command result, printing any error to stderr.
fn report(result: Result<ExitCode, Box<dyn Error>>) -> ExitCode {
    match result {
        Ok(code) => code,
        Err(err) => {
            eprintln!("ds: {err}");
            ExitCode::FAILURE
        }
    }
}

fn usage_exit(msg: &str) -> ExitCode {
    eprintln!("{msg}");
    ExitCode::FAILURE
}

/// Whether an argument is a direct `.ds` file run (`ds main.ds`). We only look
/// at the suffix — a missing file is reported by `run_file`, not the dispatch.
fn is_ds_file(arg: &str) -> bool {
    arg.ends_with(".ds")
}
