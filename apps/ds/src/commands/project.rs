//! Shared helpers for the `ds` subcommands: manifest discovery, cache
//! resolution, source translation, and cargo invocation. The command modules
//! ([`super::build`], [`super::run`], [`super::deps`], [`super::check`],
//! [`super::cache`]) build on these.

use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::{Command, ExitCode, ExitStatus},
};

use dashscript::{Manifest, Translator};

/// Translate `src` and write `src/main.rs` (plus each imported local module as
/// `src/<module>.rs`, declared with a leading `mod <module>;`) into
/// `project_dir/src/`. The caller writes `Cargo.toml`. Shared by a single-
/// package build ([`emit_cargo_project`]) and by workspace members (whose
/// Cargo.toml the workspace root owns). v1: a single layer of imports — an
/// imported module that itself imports is not followed.
pub(crate) fn translate_sources(
    src: &str,
    src_path: &Path,
    project_dir: &Path,
) -> Result<(), Box<dyn Error>> {
    let rust = Translator::new()
        .translate(src)
        .map_err(|e| format!("translate {}: {e}", src_path.display()))?;

    let base = src_path.parent().unwrap_or_else(|| Path::new(""));
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut mod_decls = String::new();
    for imp in Translator::new().imports(src) {
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

/// Translate a `.ds` file into a buildable Cargo project at `project_dir`: the
/// resolved manifest as `Cargo.toml` + the translated source as `src/main.rs`.
pub(crate) fn emit_cargo_project(
    src: &str,
    src_path: &Path,
    project_dir: &Path,
) -> Result<(), Box<dyn Error>> {
    let cargo_toml = resolve_manifest(src_path);
    fs::create_dir_all(project_dir.join("src"))?;
    fs::write(project_dir.join("Cargo.toml"), cargo_toml)?;
    translate_sources(src, src_path, project_dir)?;
    Ok(())
}

/// Resolve a relative `.ds` import (`"./other"` or `"./other.ds"`) against the
/// importing file's directory. Errors clearly when no matching file exists.
pub(crate) fn resolve_local_module(base: &Path, source: &str) -> Result<PathBuf, Box<dyn Error>> {
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

/// Resolve the Cargo manifest for `src_path`: the `manifest.json` found walking
/// up from the file (Deno-style), otherwise a minimal manifest named after the
/// project (`project_name`).
pub(crate) fn resolve_manifest(src_path: &Path) -> String {
    if let Some(root) = find_manifest_root(src_path) {
        if let Ok(json) = fs::read_to_string(root.join("manifest.json")) {
            if let Ok(manifest) = Manifest::from_json(&json) {
                return manifest.to_cargo_toml();
            }
        }
    }
    Manifest {
        name: project_name(src_path),
        ..Manifest::default()
    }
    .to_cargo_toml()
}

/// The cache directory for a `.ds` entry file, Deno-style: walk up from the
/// file for a `manifest.json`; found → in-project `.cache/dash/<project>/` —
/// **one per project** (keyed by project name, not the entry stem, so two
/// `main.ds` files in different projects don't collide and one project's
/// entries share a cache); not found (a lone file) → global
/// `~/.cache/dash/<hash>/`. The `dash` segment mirrors the global cache root,
/// so DashScript owns one namespace under `.cache/`. `run`, `build`, and
/// `install` all share this directory, so repeat invocations reuse cargo's
/// incremental `target/` instead of recompiling std and every dependency from
/// scratch. Falls back to a temp dir if no platform cache dir is resolvable,
/// so a lone file always runs.
pub(crate) fn cache_project_dir(src_path: &Path) -> PathBuf {
    if let Some(root) = find_manifest_root(src_path) {
        return root
            .join(".cache")
            .join("dash")
            .join(project_name(src_path));
    }
    global_cache_dir(src_path)
}

/// Walk up from the `.ds` file's directory for the nearest `manifest.json`,
/// returning its directory (the project root) if one exists.
pub(crate) fn find_manifest_root(src_path: &Path) -> Option<PathBuf> {
    let dir = src_path.parent()?;
    for ancestor in dir.ancestors() {
        if ancestor.join("manifest.json").exists() {
            return Some(ancestor.to_path_buf());
        }
    }
    None
}

/// Find the nearest `manifest.json` walking up from the **cwd** (whereas
/// [`find_manifest_root`] starts from a `.ds` file's directory). Used by
/// cwd-based commands (`install`, `add`, `remove`, `run`) so they work from a
/// subdirectory — mirroring pnpm/cargo, which find the workspace root from any
/// nested dir. Falls back to the cwd when no manifest is found, so callers
/// report "no manifest.json here" instead of panicking.
pub(crate) fn manifest_root() -> PathBuf {
    let Ok(cwd) = std::env::current_dir() else {
        return PathBuf::from(".");
    };
    for ancestor in cwd.ancestors() {
        if ancestor.join("manifest.json").exists() {
            return ancestor.to_path_buf();
        }
    }
    PathBuf::from(".")
}

/// The global fallback cache for a lone `.ds` file (no `manifest.json` found
/// walking up): `~/.cache/dash/<hash(canonical_path)>/`, keyed by the file's
/// canonical path so the same file reuses it across runs.
pub(crate) fn global_cache_dir(src_path: &Path) -> PathBuf {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let key = {
        let canonical = fs::canonicalize(src_path).unwrap_or_else(|_| src_path.to_path_buf());
        let mut hasher = DefaultHasher::new();
        canonical.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    };
    match dirs::cache_dir() {
        Some(cache) => cache.join("dash").join(&key),
        None => std::env::temp_dir().join(format!("dash-{key}")),
    }
}

/// The file stem of a path as an owned `String` ("main.ds" → "main").
pub(crate) fn stem_of(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("dash")
        .to_string()
}

/// The build output name: the `manifest.json` `name` if present, else the
/// project directory name, else the file stem — never the bare stem when a
/// project exists, so two entry files don't clobber `dist/<name>`.
pub(crate) fn project_name(src_path: &Path) -> String {
    if let Some(root) = find_manifest_root(src_path) {
        if let Ok(json) = fs::read_to_string(root.join("manifest.json")) {
            if let Ok(manifest) = Manifest::from_json(&json) {
                if !manifest.name.trim().is_empty() {
                    return manifest.name;
                }
            }
        }
        if let Some(dir) = root.file_name().and_then(|s| s.to_str()) {
            if !dir.is_empty() {
                return dir.to_string();
            }
        }
    }
    stem_of(src_path)
}

/// Resolve the project entry file for a file-less `ds build`: the
/// `manifest.json` `entry` if it exists, else `main.ds` in the cwd.
pub(crate) fn resolve_entry() -> Result<String, Box<dyn Error>> {
    if let Ok(manifest) = read_manifest(Path::new("manifest.json")) {
        if let Some(entry) = &manifest.entry {
            if Path::new(entry).exists() {
                return Ok(entry.clone());
            }
        }
    }
    if Path::new("main.ds").exists() {
        return Ok("main.ds".to_string());
    }
    Err("ds build: no entry file (pass <file.ds>, set manifest entry, or add main.ds)".into())
}

/// The build target for `src_path`: the `--target` override, else the
/// `manifest.json` `target`, else `bin`.
pub(crate) fn resolve_target(src_path: &Path, override_target: Option<&str>) -> String {
    if let Some(t) = override_target {
        return t.to_string();
    }
    if let Some(root) = find_manifest_root(src_path) {
        if let Ok(json) = fs::read_to_string(root.join("manifest.json")) {
            if let Ok(manifest) = Manifest::from_json(&json) {
                return manifest.target;
            }
        }
    }
    "bin".to_string()
}

/// Read and parse a `manifest.json`.
pub(crate) fn read_manifest(path: &Path) -> Result<Manifest, Box<dyn Error>> {
    let json = fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    Ok(Manifest::from_json(&json)?)
}

/// A manifest named after the current directory, with defaults.
pub(crate) fn default_manifest() -> Manifest {
    let name = std::env::current_dir()
        .ok()
        .and_then(|d| d.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "dashscript".to_string());
    Manifest {
        name,
        ..Manifest::default()
    }
}

/// Path to the cargo binary — the system `cargo` today; a DashScript-managed
/// toolchain replaces this once the self-contained Rust layer lands.
pub(crate) fn cargo_bin() -> &'static Path {
    Path::new("cargo")
}

/// Invoke `cargo` with `args` inside `project`, inheriting stdio. Errors if
/// cargo is not on PATH.
pub(crate) fn invoke_cargo<const N: usize>(
    project: &Path,
    args: [&str; N],
) -> Result<ExitStatus, Box<dyn Error>> {
    Command::new("cargo")
        .args(args)
        .current_dir(project)
        .status()
        .map_err(|e| format!("failed to invoke cargo (is it on PATH?): {e}").into())
}

/// Map an [`ExitStatus`] to an [`ExitCode`].
pub(crate) fn status_to_code(status: ExitStatus) -> ExitCode {
    if status.success() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
