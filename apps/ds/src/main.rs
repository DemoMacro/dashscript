//! `ds` — the DashScript toolchain entry point.
//!
//! Currently wired: `run`. Planned: `build`, `check`, `fmt`, `add`, `test`.

use std::{
    error::Error,
    fs,
    path::Path,
    process::{Command, ExitCode},
};

use dashscript::{Manifest, Translator};

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("run") => match args.next() {
            Some(file) => match run(&file) {
                Ok(code) => code,
                Err(err) => {
                    eprintln!("ds: {err}");
                    ExitCode::FAILURE
                }
            },
            None => {
                eprintln!("usage: ds run <file.ds>");
                ExitCode::FAILURE
            }
        },
        Some(other) => {
            eprintln!("ds: unknown subcommand '{other}'");
            eprintln!("available: run <file.ds>");
            ExitCode::FAILURE
        }
        None => {
            eprintln!("ds: DashScript toolchain");
            eprintln!("usage: ds <command> [args]");
            eprintln!("commands: run <file.ds>");
            ExitCode::FAILURE
        }
    }
}

/// Build a `.ds` file into a Cargo project in a temp dir and `cargo run` it.
///
/// Translation emits `src/main.rs`; the sibling `manifest.json` (if present)
/// is turned into `Cargo.toml`. Execution is delegated to the system `cargo`
/// for now — a DashScript-managed Rust toolchain (downloaded on demand, no
/// `rustup`) will replace this later.
fn run(file: &str) -> Result<ExitCode, Box<dyn Error>> {
    let path = Path::new(file);
    let src = fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;

    let rust = Translator::new()
        .translate(&src)
        .map_err(|e| format!("translate {}: {e}", path.display()))?;

    // Resolve the manifest beside the source; fall back to the file's stem.
    let dir = path.parent().unwrap_or_else(|| Path::new(""));
    let cargo_toml = match fs::read_to_string(dir.join("manifest.json"))
        .ok()
        .and_then(|json| Manifest::from_json(&json).ok())
    {
        Some(manifest) => manifest.to_cargo_toml(),
        None => {
            let manifest = Manifest {
                name: path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("dashscript")
                    .to_string(),
                ..Manifest::default()
            };
            manifest.to_cargo_toml()
        }
    };

    // Emit a buildable Cargo project in the system temp dir.
    let project = std::env::temp_dir().join(format!("dashscript-{}", std::process::id()));
    let src_dir = project.join("src");
    fs::create_dir_all(&src_dir)?;
    fs::write(project.join("Cargo.toml"), cargo_toml)?;
    fs::write(src_dir.join("main.rs"), rust)?;

    let status = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&project)
        .status()
        .map_err(|e| format!("failed to invoke cargo (is it on PATH?): {e}"))?;

    Ok(if status.success() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}
