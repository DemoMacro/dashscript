//! `ds <file.ts>` (run a file directly) and `ds run <script>` (run a package
//! script). [`list_scripts`] backs `ds run` with no argument.

use std::{
    error::Error,
    path::Path,
    process::{Command, ExitCode},
};

use dashscript::project::{
    cache_project_dir, default_package, emit_cargo_project, find_package_root, invoke_cargo,
    package_root, read_package, status_to_code,
};

/// Translate a `.ts` file into its cached Cargo project and `cargo run` it
/// (`ds <file.ts>`).
///
/// The cache is resolved Deno-style (`cache_project_dir`): in-project
/// `.cache/dash/<project>/` when a `package.json` is found walking up, else a
/// global `~/.cache/dash/<hash>/`. Execution is delegated to the system `cargo`
/// for now — a DashScript-managed toolchain (downloaded on demand, no `rustup`)
/// will replace this later.
pub(crate) fn run_file(file: &str) -> Result<ExitCode, Box<dyn Error>> {
    let path = Path::new(file);
    let src = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let project = cache_project_dir(path);
    emit_cargo_project(&src, path, &project)?;

    // Project mode (package declares bins): run the bin this file declares
    // (`cargo run --bin <name>`). A lone file, or a package with no bins,
    // runs the single `src/main.rs` (`cargo run`).
    let status = match project_bin_for(path)? {
        Some(name) => invoke_cargo(&project, ["run", "--quiet", "--bin", name.as_str()])?,
        None => invoke_cargo(&project, ["run", "--quiet"])?,
    };
    Ok(status_to_code(status))
}

/// Resolve the bin name `path` declares under its package, so `cargo run`
/// targets it. `Ok(None)` = lone-file mode (no bins, `cargo run` finds
/// `src/main.rs`); `Err` = project mode but the file is not a declared bin.
fn project_bin_for(path: &Path) -> Result<Option<String>, Box<dyn Error>> {
    let Some(root) = find_package_root(path) else {
        return Ok(None);
    };
    let Ok(package) = read_package(&root.join("package.json")) else {
        return Ok(None);
    };
    if package.bin.is_none() {
        return Ok(None);
    }
    let canon = path.canonicalize()?;
    for (name, ds_path) in package.bin_entries() {
        if root.join(ds_path).canonicalize().is_ok_and(|c| c == canon) {
            return Ok(Some(name));
        }
    }
    Err(format!(
        "dashscript: {} is not a declared bin entry; add it under `bin` in package.json",
        path.display()
    )
    .into())
}

/// Run a `package.json` script by name (`ds run <script>`), executing its
/// value through the system shell — so a script may be any shell command
/// (`"ds main.ts"`, `"cargo test"`, …), like a `package.json` script.
pub(crate) fn run_script(script: &str) -> Result<ExitCode, Box<dyn Error>> {
    let package_path = package_root().join("package.json");
    let package = read_package(&package_path)?;
    let command = package
        .scripts
        .get(script)
        .ok_or_else(|| format!("no script '{script}' in {}", package_path.display()))?;
    println!("ds> {script}: {command}");
    shell_exec(command)
}

/// Run a shell command string through the system shell (POSIX `sh -c` on Unix,
/// `cmd /C` on Windows), so `scripts` entries can be arbitrary shell.
fn shell_exec(command: &str) -> Result<ExitCode, Box<dyn Error>> {
    #[cfg(unix)]
    let status = Command::new("sh").arg("-c").arg(command).status();
    #[cfg(windows)]
    let status = Command::new("cmd").arg("/C").arg(command).status();
    let status = status.map_err(|e| format!("failed to spawn shell: {e}"))?;
    Ok(status_to_code(status))
}

/// List the scripts in `package.json` (`ds run` with no argument) — like
/// `pnpm run` with no script name.
pub(crate) fn list_scripts() -> Result<ExitCode, Box<dyn Error>> {
    let package =
        read_package(&package_root().join("package.json")).unwrap_or_else(|_| default_package());
    if package.scripts.is_empty() {
        eprintln!("ds: no scripts in package.json");
        return Ok(ExitCode::SUCCESS);
    }
    println!("available scripts:");
    for (name, cmd) in &package.scripts {
        println!("  {name}");
        println!("    {cmd}");
    }
    Ok(ExitCode::SUCCESS)
}
