//! `ds` — the DashScript toolchain entry point.
//!
//! Wired: `run`, `build`, `add`, `check`, `fmt`, `lsp`. Planned: `test`.

use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::{Command, ExitCode, ExitStatus},
};

use dashscript::{fetch, Bindgen, Manifest, Translator};

mod lsp;

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
        Some("add") => match args.next() {
            Some(spec) => report(add(&spec)),
            None => usage_exit("usage: ds add <crate|rust:crate|file.rs>"),
        },
        Some("remove") => match args.next() {
            Some(name) => report(remove(&name)),
            None => usage_exit("usage: ds remove <crate|rust:crate>"),
        },
        Some("check") => match args.next() {
            Some(file) => report(check(&file)),
            None => usage_exit("usage: ds check <file.ds>"),
        },
        Some("fmt") => match args.next() {
            Some(file) => report(fmt(&file)),
            None => usage_exit("usage: ds fmt <file.ds>"),
        },
        Some("lsp") => match lsp::run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("ds lsp: {err}");
                ExitCode::FAILURE
            }
        },
        Some(other) => {
            eprintln!("ds: unknown subcommand '{other}'");
            eprintln!("available: run <file.ds>, build <file.ds>, add <crate|file.rs>, remove <crate>, check <file.ds>, fmt <file.ds>");
            ExitCode::FAILURE
        }
        None => {
            eprintln!("ds: DashScript toolchain");
            eprintln!("usage: ds <command> [args]");
            eprintln!("commands: run <file.ds>, build <file.ds>, add <crate|file.rs>, remove <crate>, check <file.ds>, fmt <file.ds>");
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
    report(result)
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

/// Translate a `.ds` file into a buildable Cargo project at `project_dir`.
///
/// Each local module the file imports (`import { x } from "./other"`) is
/// translated to `src/<module>.rs` and declared with a leading `mod <module>;`
/// so the main file's `use <module>::x;` resolves. v1: a single layer — an
/// imported module that itself imports is not followed.
fn emit_cargo_project(src_path: &Path, project_dir: &Path) -> Result<(), Box<dyn Error>> {
    let src = fs::read_to_string(src_path)
        .map_err(|e| format!("cannot read {}: {e}", src_path.display()))?;
    let rust = Translator::new()
        .translate(&src)
        .map_err(|e| format!("translate {}: {e}", src_path.display()))?;
    let cargo_toml = resolve_manifest(src_path);
    fs::create_dir_all(project_dir.join("src"))?;
    fs::write(project_dir.join("Cargo.toml"), cargo_toml)?;

    let base = src_path.parent().unwrap_or_else(|| Path::new(""));
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut mod_decls = String::new();
    for imp in Translator::new().imports(&src) {
        if !seen.insert(imp.module.clone()) {
            continue; // dedupe repeated imports of the same module
        }
        let dep_path = resolve_local_module(base, &imp.source)?;
        let dep_src = fs::read_to_string(&dep_path)
            .map_err(|e| format!("cannot read import {}: {e}", dep_path.display()))?;
        let dep_rust = Translator::new()
            .translate(&dep_src)
            .map_err(|e| format!("translate {}: {e}", dep_path.display()))?;
        fs::write(
            project_dir.join("src").join(format!("{}.rs", imp.module)),
            dep_rust,
        )?;
        mod_decls.push_str(&format!("mod {};\n", imp.module));
    }

    let main = if mod_decls.is_empty() {
        rust
    } else {
        format!("{mod_decls}\n{rust}")
    };
    fs::write(project_dir.join("src").join("main.rs"), main)?;
    Ok(())
}

/// Resolve a relative `.ds` import (`"./other"` or `"./other.ds"`) against the
/// importing file's directory. Errors clearly when no matching file exists.
fn resolve_local_module(base: &Path, source: &str) -> Result<PathBuf, Box<dyn Error>> {
    let candidate = if source.ends_with(".ds") {
        base.join(source)
    } else {
        base.join(format!("{source}.ds"))
    };
    if candidate.exists() {
        return Ok(candidate);
    }
    Err(format!(
        "dashscript: import '{source}' does not resolve to a .ds file (tried {})",
        candidate.display()
    )
    .into())
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

/// Add a dependency to the project.
///
/// A `.rs` path runs bindgen on that local file (writes `<stem>.ds` beside
/// it — the `bindgen-demo` flow). Any other spec is a crate name, with or
/// without a `rust:` prefix: cargo downloads it into its global registry and
/// DashScript records it in `manifest.json`. No `.ds` declaration is generated
/// — type information comes from the crate source itself (read directly by the
/// language server, the way rust-analyzer reads `~/.cargo`).
fn add(spec: &str) -> Result<ExitCode, Box<dyn Error>> {
    if spec.ends_with(".rs") {
        return add_local_file(spec);
    }
    let crate_name = spec.strip_prefix("rust:").unwrap_or(spec);
    let version = fetch::add_via_cargo(crate_name, cargo_bin())
        .map_err(|e| format!("ds add {crate_name}: {e}"))?;
    let manifest_path = Path::new("manifest.json");
    let mut manifest = read_manifest(manifest_path).unwrap_or_else(|_| default_manifest());
    manifest.add_dependency("rust", crate_name, &version);
    fs::write(manifest_path, format!("{}\n", manifest.to_json()?))?;
    println!("ds: added rust:{crate_name} = {version}");
    Ok(ExitCode::SUCCESS)
}

/// Remove a crate dependency from `manifest.json`.
fn remove(spec: &str) -> Result<ExitCode, Box<dyn Error>> {
    let name = spec.strip_prefix("rust:").unwrap_or(spec);
    let manifest_path = Path::new("manifest.json");
    let mut manifest = read_manifest(manifest_path)?;
    if !manifest.remove_dependency("rust", name) {
        return Err(format!("rust:{name} is not in {}", manifest_path.display()).into());
    }
    fs::write(manifest_path, format!("{}\n", manifest.to_json()?))?;
    println!("ds: removed rust:{name}");
    Ok(ExitCode::SUCCESS)
}

/// Generate a `.ds` type declaration from a local Rust source file (bindgen),
/// written beside it as `<stem>.ds`.
fn add_local_file(file: &str) -> Result<ExitCode, Box<dyn Error>> {
    let path = Path::new(file);
    let rust = fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let ds = Bindgen::new()
        .generate(&rust)
        .map_err(|e| format!("bindgen {}: {e}", path.display()))?;
    let out = path.with_extension("ds");
    fs::write(&out, ds)?;
    println!("ds: generated {}", out.display());
    Ok(ExitCode::SUCCESS)
}

/// Path to the cargo binary — the system `cargo` today; a DashScript-managed
/// toolchain replaces this once the self-contained Rust layer lands.
fn cargo_bin() -> &'static Path {
    Path::new("cargo")
}

/// A manifest named after the current directory, with defaults.
fn default_manifest() -> Manifest {
    let name = std::env::current_dir()
        .ok()
        .and_then(|d| d.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "dashscript".to_string());
    Manifest {
        name,
        ..Manifest::default()
    }
}

/// Read and parse a `manifest.json`.
fn read_manifest(path: &Path) -> Result<Manifest, Box<dyn Error>> {
    let json = fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    Ok(Manifest::from_json(&json)?)
}

/// Check a `.ds` file for translatability in-process: syntax errors (from
/// `oxc_parser`) plus any top-level statement the translator cannot lower to
/// Rust. No external oxlint dependency.
fn check(file: &str) -> Result<ExitCode, Box<dyn Error>> {
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

/// Format a `.ds` file in place with `oxc_codegen` (pretty-print). No external
/// oxfmt dependency.
fn fmt(file: &str) -> Result<ExitCode, Box<dyn Error>> {
    let path = Path::new(file);
    let source = fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let formatted = Translator::new().format(&source)?;
    fs::write(path, formatted)?;
    println!("ds: formatted {file}");
    Ok(ExitCode::SUCCESS)
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
