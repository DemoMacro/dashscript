//! `ds <file.ds>` (run a file directly) and `ds run <script>` (run a manifest
//! script). [`list_scripts`] backs `ds run` with no argument.

use std::{
    error::Error,
    path::Path,
    process::{Command, ExitCode},
};

use super::project::{
    cache_project_dir, default_manifest, emit_cargo_project, invoke_cargo, manifest_root,
    read_manifest, status_to_code,
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
    let status = invoke_cargo(&project, ["run", "--quiet"])?;
    Ok(status_to_code(status))
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
    let manifest =
        read_manifest(&manifest_root().join("manifest.json")).unwrap_or_else(|_| default_manifest());
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
