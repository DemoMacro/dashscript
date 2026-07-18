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

/// Translate one `.ds` file to `src/<stem>.rs`, prefixing `mod <module>;` for
/// each of its local imports (deduped). The imported files are translated
/// separately by [`translate_project`]'s directory walk — this emits only the
/// current file and the modules it declares.
fn translate_one_with_mods(ds: &Path, project_dir: &Path) -> Result<(), Box<dyn Error>> {
    let src = fs::read_to_string(ds).map_err(|e| format!("cannot read {}: {e}", ds.display()))?;
    let rust = Translator::new()
        .translate(&src)
        .map_err(|e| format!("translate {}: {e}", ds.display()))?;
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut mod_decls = String::new();
    for imp in Translator::new().imports(&src) {
        if seen.insert(imp.module.clone()) {
            mod_decls.push_str(&format!("mod {};\n", imp.module));
        }
    }
    let body = if mod_decls.is_empty() {
        rust
    } else {
        format!("{mod_decls}\n{rust}")
    };
    fs::write(
        project_dir.join("src").join(format!("{}.rs", stem_of(ds))),
        body,
    )?;
    Ok(())
}

/// A project's resolved targets for `Cargo.toml`: the `(bin_name, ds_path)`
/// pairs for `[[bin]]`, plus the `[lib]` entry path.
type ProjectTargets = (Vec<(String, String)>, Option<String>);

/// Translate every `.ds` under a manifest root into one multi-target crate at
/// `project_dir/src/`: each file becomes `src/<stem>.rs` (prefixed with its
/// `mod` declarations), and the manifest's `bin`/`lib` entries become the
/// crate's `[[bin]]`/`[lib]` targets. Returns the resolved targets for
/// `Cargo.toml` emission.
///
/// Two project-level guards: a stem collision (two files flatten to the same
/// `src/<stem>.rs` — nested directories are not yet modeled as sub-modules),
/// and a bin importing another bin (cargo forbids it; shared code must go
/// through `[lib]`).
fn translate_project(
    root: &Path,
    manifest: &Manifest,
    project_dir: &Path,
) -> Result<ProjectTargets, Box<dyn Error>> {
    let src_dir = project_dir.join("src");
    fs::create_dir_all(&src_dir)?;
    // Clear stale translations from a prior run (a renamed bin, or a switch
    // between lone-file and project mode) so cargo never sees orphan modules.
    clean_src_dir(&src_dir)?;

    let mut files = Vec::new();
    walk_ds(root, &mut files);
    files.sort();

    let mut seen_stems: std::collections::HashMap<String, PathBuf> =
        std::collections::HashMap::new();
    for ds in &files {
        let stem = stem_of(ds);
        if let Some(prev) = seen_stems.insert(stem.clone(), ds.clone()) {
            return Err(format!(
                "dashscript: name collision — stem '{stem}' appears in both {} and {}; \
                 rename one (nested directories are not yet modeled as modules)",
                prev.display(),
                ds.display()
            )
            .into());
        }
        translate_one_with_mods(ds, project_dir)?;
    }

    let bins = manifest.bin_entries();
    let lib = manifest.lib.clone();
    detect_bin_imports_bin(root, &bins)?;
    Ok((bins, lib))
}

/// Guard: no bin may import another bin. cargo forbids one `[[bin]]` from
/// `mod`-ing another, so shared code must live in a `[lib]` module. Compares
/// canonical file paths so the check holds regardless of how the import is
/// written.
fn detect_bin_imports_bin(root: &Path, bins: &[(String, String)]) -> Result<(), Box<dyn Error>> {
    let mut bin_files: std::collections::HashMap<PathBuf, String> =
        std::collections::HashMap::new();
    for (bin_name, ds_path) in bins {
        if let Ok(canon) = root.join(ds_path).canonicalize() {
            bin_files.insert(canon, bin_name.clone());
        }
    }
    for (bin_name, ds_path) in bins {
        let file = root.join(ds_path);
        let Ok(src) = fs::read_to_string(&file) else {
            continue;
        };
        let base = file.parent().unwrap_or_else(|| Path::new(""));
        for imp in Translator::new().imports(&src) {
            let Ok(dep) = resolve_local_module(base, &imp.source) else {
                continue; // a missing module surfaces at `cargo build`
            };
            if let Ok(canon) = dep.canonicalize() {
                if let Some(other) = bin_files.get(&canon) {
                    if other != bin_name {
                        return Err(format!(
                            "dashscript: bin '{bin_name}' imports bin '{other}' (from {}); \
                             move the shared code into a lib module (a .ds that is not a bin \
                             entry) — cargo forbids one bin from mod-ing another",
                            imp.source
                        )
                        .into());
                    }
                }
            }
        }
    }
    Ok(())
}

/// Remove every `.rs` under `src/` so a prior translation (a renamed bin, or a
/// lone-file `main.rs` left after switching to project mode) cannot leave an
/// orphan module cargo would try to compile.
fn clean_src_dir(src: &Path) -> std::io::Result<()> {
    if let Ok(entries) = fs::read_dir(src) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                let _ = fs::remove_file(&path);
            }
        }
    }
    Ok(())
}

/// Translate a `.ds` entry into a buildable Cargo project at `project_dir`.
///
/// Project mode (a manifest declares `bin` or `lib`): every `.ds` under the
/// root becomes `src/<stem>.rs` in one crate, and the declared entries become
/// `[[bin]]`/`[lib]` targets — so a project's entries share one cache and never
/// overwrite each other. Otherwise (a lone file, or a manifest with no declared
/// targets): a minimal manifest + a single `src/main.rs`.
pub(crate) fn emit_cargo_project(
    src: &str,
    src_path: &Path,
    project_dir: &Path,
) -> Result<(), Box<dyn Error>> {
    if let Some(root) = find_manifest_root(src_path) {
        if let Ok(manifest) = read_manifest(&root.join("manifest.json")) {
            if manifest.bin.is_some() || manifest.lib.is_some() {
                let (bins, lib) = translate_project(&root, &manifest, project_dir)?;
                let cargo_toml = manifest.to_cargo_toml_with_bins(&bins, lib.as_deref());
                fs::write(project_dir.join("Cargo.toml"), cargo_toml)?;
                return Ok(());
            }
        }
    }
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

/// Collect every `.ds` file under the current project (the nearest
/// `manifest.json` walking up, else the cwd), skipping generated/vendored
/// directories (`target`, `.cache`, `dist`, `node_modules`, `.git`). Used by
/// `ds lint` / `ds check` / `ds fmt` with no argument — the way `vp check` and
/// `oxlint` check the whole project when given no target. Sorted for stable
/// output.
pub(crate) fn collect_ds_files() -> Vec<PathBuf> {
    let root = manifest_root();
    let mut out = Vec::new();
    walk_ds(&root, &mut out);
    out.sort();
    out
}

/// Recursive worker for [`collect_ds_files`].
fn walk_ds(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if matches!(name, "target" | ".cache" | "dist" | "node_modules" | ".git") {
                    continue;
                }
            }
            walk_ds(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("ds") {
            out.push(path);
        }
    }
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

/// Resolve the project entry file for a file-less `ds build`: the first
/// declared `bin` (the project builds every bin; any one anchors the lookup),
/// else the legacy `entry`, else `main.ds` in the cwd.
pub(crate) fn resolve_entry() -> Result<String, Box<dyn Error>> {
    if let Ok(manifest) = read_manifest(Path::new("manifest.json")) {
        if let Some((_, bin_path)) = manifest.bin_entries().into_iter().next() {
            if Path::new(&bin_path).exists() {
                return Ok(bin_path);
            }
        }
        if let Some(entry) = &manifest.entry {
            if Path::new(entry).exists() {
                return Ok(entry.clone());
            }
        }
    }
    if Path::new("main.ds").exists() {
        return Ok("main.ds".to_string());
    }
    Err("ds build: no entry file (pass <file.ds>, set manifest bin/entry, or add main.ds)".into())
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
    let json =
        fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(dir: &Path, name: &str, body: &str) {
        fs::write(dir.join(name), body).unwrap();
    }

    fn manifest_at(root: &Path) -> Manifest {
        read_manifest(&root.join("manifest.json")).unwrap()
    }

    #[test]
    fn translate_project_emits_per_file_bins() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(
            root,
            "manifest.json",
            r#"{ "name": "app", "bin": { "a": "a.ds", "b": "b.ds" } }"#,
        );
        write(root, "a.ds", "function main() { console.log(1); }");
        write(root, "b.ds", "function main() { console.log(2); }");

        let out = tmp.path().join("out");
        let (bins, lib) = translate_project(root, &manifest_at(root), &out).unwrap();
        let names: Vec<&str> = bins.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"a"), "bins: {bins:?}");
        assert!(names.contains(&"b"), "bins: {bins:?}");
        assert!(lib.is_none());
        assert!(out.join("src").join("a.rs").exists(), "src/a.rs missing");
        assert!(out.join("src").join("b.rs").exists(), "src/b.rs missing");
    }

    #[test]
    fn translate_project_detects_stem_collision() {
        // MVP flattens every .ds to src/<stem>.rs; two files with the same stem
        // would clobber each other, so the translation refuses.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("sub")).unwrap();
        write(
            root,
            "manifest.json",
            r#"{ "name": "app", "bin": "main.ds" }"#,
        );
        write(root, "main.ds", "function main() {}");
        write(root, "dup.ds", "function helper() {}");
        write(&root.join("sub"), "dup.ds", "function other() {}");

        let out = tmp.path().join("out");
        let err = translate_project(root, &manifest_at(root), &out).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("name collision"), "got: {msg}");
        assert!(msg.contains("dup"), "got: {msg}");
    }

    #[test]
    fn translate_project_detects_bin_imports_bin() {
        // cargo forbids one bin from mod-ing another; shared code must go
        // through a lib module. The guard surfaces this before cargo does.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(
            root,
            "manifest.json",
            r#"{ "name": "app", "bin": { "a": "a.ds", "b": "b.ds" } }"#,
        );
        write(
            root,
            "a.ds",
            "import { x } from \"./b\";\nfunction main() {}",
        );
        write(root, "b.ds", "export function x() {}\nfunction main() {}");

        let out = tmp.path().join("out");
        let err = translate_project(root, &manifest_at(root), &out).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("bin 'a' imports bin 'b'"), "got: {msg}");
    }
}
