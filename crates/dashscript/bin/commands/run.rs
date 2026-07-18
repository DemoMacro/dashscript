//! `ds <file.ds>` (run a file directly) and `ds run <script>` (run a manifest
//! script). [`list_scripts`] backs `ds run` with no argument.

use std::{
    error::Error,
    path::Path,
    process::{Command, ExitCode},
};

use super::project::{
    cache_project_dir, default_manifest, emit_cargo_project, find_manifest_root, invoke_cargo,
    manifest_root, read_manifest, status_to_code,
};

/// Translate a `.ds` file into its cached Cargo project and `cargo run` it
/// (`ds <file.ds>`).
///
/// The cache is resolved Deno-style (`cache_project_dir`): in-project
/// `.cache/dash/<project>/` when a `manifest.json` is found walking up, else a
/// global `~/.cache/dash/<hash>/`. Execution is delegated to the system `cargo`
/// for now — a DashScript-managed toolchain (downloaded on demand, no `rustup`)
/// will replace this later.
pub(crate) fn run_file(file: &str) -> Result<ExitCode, Box<dyn Error>> {
    let path = Path::new(file);
    let src = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let project = cache_project_dir(path);
    emit_cargo_project(&src, path, &project)?;

    // Project mode (manifest declares bins): run the bin this file declares
    // (`cargo run --bin <name>`). A lone file, or a manifest with no bins,
    // runs the single `src/main.rs` (`cargo run`).
    let status = match project_bin_for(path)? {
        Some(name) => invoke_cargo(&project, ["run", "--quiet", "--bin", name.as_str()])?,
        None => invoke_cargo(&project, ["run", "--quiet"])?,
    };
    Ok(status_to_code(status))
}

/// Resolve the bin name `path` declares under its manifest, so `cargo run`
/// targets it. `Ok(None)` = lone-file mode (no bins, `cargo run` finds
/// `src/main.rs`); `Err` = project mode but the file is not a declared bin.
fn project_bin_for(path: &Path) -> Result<Option<String>, Box<dyn Error>> {
    let Some(root) = find_manifest_root(path) else {
        return Ok(None);
    };
    let Ok(manifest) = read_manifest(&root.join("manifest.json")) else {
        return Ok(None);
    };
    if manifest.bin.is_none() {
        return Ok(None);
    }
    let canon = path.canonicalize()?;
    for (name, ds_path) in manifest.bin_entries() {
        if root.join(ds_path).canonicalize().is_ok_and(|c| c == canon) {
            return Ok(Some(name));
        }
    }
    Err(format!(
        "dashscript: {} is not a declared bin entry; add it under `bin` in manifest.json",
        path.display()
    )
    .into())
}

/// Run a `manifest.json` script by name (`ds run <script>`), executing its
/// value through the system shell — so a script may be any shell command
/// (`"ds main.ds"`, `"cargo test"`, …), like a `package.json` script.
pub(crate) fn run_script(script: &str) -> Result<ExitCode, Box<dyn Error>> {
    let manifest_path = manifest_root().join("manifest.json");
    let manifest = read_manifest(&manifest_path)?;
    let command = manifest
        .scripts
        .get(script)
        .ok_or_else(|| format!("no script '{script}' in {}", manifest_path.display()))?;
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

/// List the scripts in `manifest.json` (`ds run` with no argument) — like
/// `pnpm run` with no script name.
pub(crate) fn list_scripts() -> Result<ExitCode, Box<dyn Error>> {
    let manifest = read_manifest(&manifest_root().join("manifest.json"))
        .unwrap_or_else(|_| default_manifest());
    if manifest.scripts.is_empty() {
        eprintln!("ds: no scripts in manifest.json");
        return Ok(ExitCode::SUCCESS);
    }
    println!("available scripts:");
    for (name, cmd) in &manifest.scripts {
        println!("  {name}");
        println!("    {cmd}");
    }
    Ok(ExitCode::SUCCESS)
}
