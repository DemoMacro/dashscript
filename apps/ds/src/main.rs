//! `ds` — the DashScript toolchain entry point.
//!
//! Wired: `run`, `build`. Planned: `check`, `fmt`, `add`, `test`.

use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::{Command, ExitCode, ExitStatus},
};

use dashscript::{Manifest, Translator};

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("run") => match args.next() {
            Some(file) => run_cmd(&file, CommandKind::Run),
            None => usage_exit("usage: ds run <file.ds>"),
        },
        Some("build") => match args.next() {
            Some(file) => run_cmd(&file, CommandKind::Build),
            None => usage_exit("usage: ds build <file.ds>"),
        },
        Some(other) => {
            eprintln!("ds: unknown subcommand '{other}'");
            eprintln!("available: run <file.ds>, build <file.ds>");
            ExitCode::FAILURE
        }
        None => {
            eprintln!("ds: DashScript toolchain");
            eprintln!("usage: ds <command> [args]");
            eprintln!("commands: run <file.ds>, build <file.ds>");
            ExitCode::FAILURE
        }
    }
}

enum CommandKind {
    Run,
    Build,
}

fn run_cmd(file: &str, kind: CommandKind) -> ExitCode {
    let result = match kind {
        CommandKind::Run => run(file),
        CommandKind::Build => build(file),
    };
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

/// Translate a `.ds` file into a buildable Cargo project at `project_dir`.
fn emit_cargo_project(src_path: &Path, project_dir: &Path) -> Result<(), Box<dyn Error>> {
    let src = fs::read_to_string(src_path)
        .map_err(|e| format!("cannot read {}: {e}", src_path.display()))?;
    let rust = Translator::new()
        .translate(&src)
        .map_err(|e| format!("translate {}: {e}", src_path.display()))?;
    let cargo_toml = resolve_manifest(src_path);
    fs::create_dir_all(project_dir.join("src"))?;
    fs::write(project_dir.join("Cargo.toml"), cargo_toml)?;
    fs::write(project_dir.join("src").join("main.rs"), rust)?;
    Ok(())
}

/// Resolve the Cargo manifest for `src_path`: the sibling `manifest.json` if
/// present, otherwise a minimal manifest named after the file's stem.
fn resolve_manifest(src_path: &Path) -> String {
    let dir = src_path.parent().unwrap_or_else(|| Path::new(""));
    if let Some(manifest) = fs::read_to_string(dir.join("manifest.json"))
        .ok()
        .and_then(|json| Manifest::from_json(&json).ok())
    {
        return manifest.to_cargo_toml();
    }
    let name = src_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("dashscript");
    Manifest {
        name: name.to_string(),
        ..Manifest::default()
    }
    .to_cargo_toml()
}

/// Build a `.ds` file into a Cargo project in a temp dir and `cargo run` it.
///
/// Execution is delegated to the system `cargo` for now — a DashScript-managed
/// Rust toolchain (downloaded on demand, no `rustup`) will replace this later.
fn run(file: &str) -> Result<ExitCode, Box<dyn Error>> {
    let path = Path::new(file);
    let project = std::env::temp_dir().join(format!("dashscript-{}", std::process::id()));
    emit_cargo_project(path, &project)?;
    let status = invoke_cargo(&project, ["run", "--quiet"])?;
    Ok(status_to_code(status))
}

/// Emit a `.ds` file's Cargo project under `dist/<stem>/` and verify it with
/// `cargo check` — the generated source is inspectable there.
fn build(file: &str) -> Result<ExitCode, Box<dyn Error>> {
    let path = Path::new(file);
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or("invalid file path")?;
    let project = PathBuf::from("dist").join(stem);
    emit_cargo_project(path, &project)?;
    println!("ds: emitted {}", project.display());
    let status = invoke_cargo(&project, ["check", "--quiet"])?;
    Ok(status_to_code(status))
}

fn invoke_cargo<const N: usize>(project: &Path, args: [&str; N]) -> Result<ExitStatus, Box<dyn Error>> {
    Command::new("cargo")
        .args(args)
        .current_dir(project)
        .status()
        .map_err(|e| format!("failed to invoke cargo (is it on PATH?): {e}").into())
}

fn status_to_code(status: ExitStatus) -> ExitCode {
    if status.success() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
