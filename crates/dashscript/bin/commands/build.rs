//! `ds build [--target] [--filter]`: compile a native binary (default), emit
//! the translated Rust crate (`--target rust`), or build every workspace member
//! at a workspace root.

use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

use dashscript::Manifest;

use super::project::{
    cache_project_dir, default_manifest, emit_cargo_project, invoke_cargo, project_name,
    read_manifest, resolve_entry, resolve_target, status_to_code, translate_sources,
};

/// Parsed `ds build` flags: optional entry file, optional `--target`, optional
/// `--filter` (workspace member).
pub(crate) type BuildArgs = (Option<String>, Option<String>, Option<String>);

/// Parse `ds build` arguments: an optional `.ds` file, an optional
/// `--target <bin|rust>` override, and an optional `--filter <name>` (workspace
/// member). Returns an error message on misuse (shown as usage). No file means
/// build the project entry (`manifest.entry`/`main.ds`) — or, at a workspace
/// root, every member.
pub(crate) fn parse_build_args(args: &[String]) -> Result<BuildArgs, String> {
    let mut file = None;
    let mut target = None;
    let mut filter = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--target" => {
                if i + 1 < args.len() {
                    target = Some(args[i + 1].clone());
                    i += 2;
                } else {
                    return Err(
                        "usage: ds build [<file.ds>] [--target <bin|rust>] [--filter <name>]"
                            .into(),
                    );
                }
            }
            "--filter" => {
                if i + 1 < args.len() {
                    filter = Some(args[i + 1].clone());
                    i += 2;
                } else {
                    return Err(
                        "usage: ds build [<file.ds>] [--target <bin|rust>] [--filter <name>]"
                            .into(),
                    );
                }
            }
            f if !f.starts_with('-') => {
                file = Some(f.to_string());
                i += 1;
            }
            other => return Err(format!("ds build: unknown option '{other}'")),
        }
    }
    Ok((file, target, filter))
}

/// Build a `.ds` file or the project entry. `--target rust` emits the
/// translated Rust crate under `dist/<name>/` (no `target/`); the default
/// `bin` target compiles (`cargo build --release`) and copies the native
/// binary to `dist/<name>`. The compile uses the shared cache
/// (`cache_project_dir`), so `target/` never lands in `dist/`.
pub(crate) fn build(
    file: Option<&str>,
    target_override: Option<&str>,
) -> Result<ExitCode, Box<dyn Error>> {
    let file = match file {
        Some(f) => f.to_string(),
        None => resolve_entry()?,
    };
    build_at(&file, target_override, Path::new("dist"))
}

/// Core build (single package): translate `entry`, then emit a native binary
/// (`bin`) or Rust crate (`rust`) to `<dest_root>/<name>`. A single package
/// passes `dist`; a workspace member passes its own `<member>/dist` so each
/// package's artifact stays independent (publishable on its own, like a pnpm
/// workspace package). Workspace bin builds go through [`workspace_build`]
/// instead (one cargo workspace, shared `target/`).
pub(crate) fn build_at(
    entry: &str,
    target_override: Option<&str>,
    dest_root: &Path,
) -> Result<ExitCode, Box<dyn Error>> {
    let path = Path::new(entry);
    let src =
        fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let name = project_name(path);
    let target = resolve_target(path, target_override);

    // Clear any prior output at <dest_root>/<name> so switching targets
    // (bin ↔ rust) does not collide: a `bin` build leaves a file, a `rust` build
    // a dir.
    fs::create_dir_all(dest_root)?;
    let dest_base = dest_root.join(&name);
    let _ = fs::remove_dir_all(&dest_base);
    let _ = fs::remove_file(&dest_base);
    if cfg!(windows) {
        let _ = fs::remove_file(format!("{}.exe", dest_base.display()));
    }

    match target.as_str() {
        "rust" => {
            emit_cargo_project(&src, path, &dest_base)?;
            // `dist/` holds a clean crate — drop any `target/` a prior run left.
            let _ = fs::remove_dir_all(dest_base.join("target"));
            println!("ds: emitted {} (Rust crate)", dest_base.display());
            Ok(ExitCode::SUCCESS)
        }
        "bin" => {
            let cache = cache_project_dir(path);
            emit_cargo_project(&src, path, &cache)?;
            let status = invoke_cargo(&cache, ["build", "--release", "--quiet"])?;
            if !status.success() {
                return Ok(status_to_code(status));
            }
            // cargo names the binary `<name>` (Unix) / `<name>.exe` (Windows);
            // the dist output mirrors that so the file is runnable as-is.
            let bin_name = if cfg!(windows) {
                format!("{name}.exe")
            } else {
                name.clone()
            };
            let bin = cache.join("target").join("release").join(&bin_name);
            let dest = dest_base.with_file_name(&bin_name);
            fs::copy(&bin, &dest)?;
            println!("ds: built {}", dest.display());
            Ok(ExitCode::SUCCESS)
        }
        other => Err(format!(
            "ds build: target '{other}' not yet supported (use --target <bin|rust>)"
        )
        .into()),
    }
}

/// Whether `dir` is a workspace root: its `manifest.json` has a non-empty
/// `workspace` member-glob list that resolves to at least one member.
pub(crate) fn is_workspace_root(dir: &Path) -> bool {
    !discover_members(dir).is_empty()
}

/// Build the workspace at `root` — every member, or just the one named by
/// `--filter` (manifest name or member directory). For `bin`, members are
/// emitted under `.cache/dash/members/<name>/` of one cargo workspace, so they
/// share a single `target/` and `Cargo.lock`: a dependency two members use
/// compiles once (cargo's native hoisted-`node_modules`). For `rust`, each
/// member's crate is emitted independently to `dist/<name>/` (no compilation,
/// nothing to share).
pub(crate) fn workspace_build(
    root: &Path,
    filter: Option<&str>,
    target_override: Option<&str>,
) -> Result<ExitCode, Box<dyn Error>> {
    let members = discover_members(root);
    if members.is_empty() {
        return Err(
            "ds build: no workspace members matched (check `workspaces` globs in manifest.json)"
                .into(),
        );
    }

    // Select members, applying --filter (manifest name or member directory).
    let mut selected: Vec<(String, PathBuf, String)> = Vec::new();
    for member in &members {
        let dir_name = member_name_fallback(member);
        let name = manifest_name_of(member).unwrap_or_else(|| dir_name.clone());
        if let Some(want) = filter {
            if name != want && dir_name != want {
                continue;
            }
        }
        let entry = resolve_member_entry(member)?;
        selected.push((name, member.to_path_buf(), entry));
    }
    if selected.is_empty() {
        return Err(format!(
            "ds build: --filter '{}' matched no workspace member",
            filter.unwrap_or("?")
        )
        .into());
    }

    let target = target_override
        .map(|t| t.to_string())
        .unwrap_or_else(|| "bin".to_string());
    if target == "rust" {
        // Rust crates are emitted, not compiled — no shared `target/` to gain.
        // Each member's crate lands in its own `<member>/dist/<name>/` so the
        // package stays independently publishable.
        for (name, member_dir, entry) in &selected {
            println!("ds: {name} (workspace member, rust crate)");
            build_at(entry, Some("rust"), &member_dir.join("dist"))?;
        }
        return Ok(ExitCode::SUCCESS);
    }
    if target != "bin" {
        return Err(format!(
            "ds build: target '{target}' not yet supported (use --target <bin|rust>)"
        )
        .into());
    }

    // bin: emit one cargo workspace — members share `target/` + `Cargo.lock`.
    // Member dirs sit directly under the cache root (`<cache>/<name>/`), mirroring
    // the single-package `.cache/dash/<name>/`; a stale member from a prior run
    // (renamed/removed) is simply absent from `members` and ignored by cargo —
    // `ds cache clean` reclaims the space.
    let cache = root.join(".cache").join("dash");
    fs::create_dir_all(&cache)?;
    let names: Vec<String> = selected.iter().map(|(n, _, _)| n.clone()).collect();
    fs::write(
        cache.join("Cargo.toml"),
        Manifest::workspace_root_toml(&names),
    )?;

    for (name, member_dir, entry) in &selected {
        let path = Path::new(entry);
        let src =
            fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let member_manifest =
            read_manifest(&member_dir.join("manifest.json")).unwrap_or_else(|_| default_manifest());
        let member_cache = cache.join(name);
        fs::create_dir_all(member_cache.join("src"))?;
        fs::write(
            member_cache.join("Cargo.toml"),
            member_manifest.to_member_toml(),
        )?;
        translate_sources(&src, path, &member_cache)?;
        println!("ds: {name} (workspace member)");
    }

    println!("ds: building workspace (shared target)...");
    let status = invoke_cargo(&cache, ["build", "--release", "--quiet"])?;
    if !status.success() {
        return Ok(status_to_code(status));
    }

    // Copy each member binary to its own `<member>/dist/<name>` — not the
    // workspace root — so each package's artifact is independent and
    // publishable (like a pnpm workspace package's own dist).
    for (name, member_dir, _) in &selected {
        let bin_name = if cfg!(windows) {
            format!("{name}.exe")
        } else {
            name.clone()
        };
        let bin = cache.join("target").join("release").join(&bin_name);
        let dest_dir = member_dir.join("dist");
        let _ = fs::remove_dir_all(&dest_dir);
        fs::create_dir_all(&dest_dir)?;
        let dest = dest_dir.join(&bin_name);
        fs::copy(&bin, &dest)?;
        println!("ds: built {}", dest.display());
    }
    Ok(ExitCode::SUCCESS)
}

/// Resolve a root manifest's `workspaces` globs (e.g. `["apps/*", "packages/*"]`)
/// into member directories — each a subdirectory holding its own `manifest.json`.
/// Empty if `root` has no `workspaces` field or no members match.
fn discover_members(root: &Path) -> Vec<PathBuf> {
    let Ok(json) = fs::read_to_string(root.join("manifest.json")) else {
        return Vec::new();
    };
    let Ok(manifest) = Manifest::from_json(&json) else {
        return Vec::new();
    };
    if manifest.workspaces.is_empty() {
        return Vec::new();
    }
    let mut members = Vec::new();
    for glob in &manifest.workspaces {
        for member in expand_member_glob(root, glob) {
            if !members.contains(&member) {
                members.push(member);
            }
        }
    }
    members
}

/// Expand one workspace glob (`<dir>/*`) relative to `root` into the
/// subdirectories of `<dir>` that hold a `manifest.json`. Only the trailing
/// `/*` form is supported (the common pnpm-workspace / cargo-members case).
fn expand_member_glob(root: &Path, glob: &str) -> Vec<PathBuf> {
    let Some(dir_name) = glob.strip_suffix("/*") else {
        return Vec::new();
    };
    let dir = root.join(dir_name);
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.join("manifest.json").exists())
        .collect();
    out.sort();
    out
}

/// A member's entry: its manifest `entry`, else `main.ds` inside the member.
fn resolve_member_entry(member: &Path) -> Result<String, Box<dyn Error>> {
    let manifest_path = member.join("manifest.json");
    if let Ok(manifest) = read_manifest(&manifest_path) {
        if let Some(entry) = &manifest.entry {
            let p = member.join(entry);
            if p.exists() {
                return Ok(p.to_string_lossy().into_owned());
            }
        }
    }
    let main = member.join("main.ds");
    if main.exists() {
        return Ok(main.to_string_lossy().into_owned());
    }
    Err(format!(
        "ds build: member {} has no entry (set manifest entry or add main.ds)",
        member.display()
    )
    .into())
}

/// Read a member's manifest `name` (for `--filter` matching and display).
fn manifest_name_of(member: &Path) -> Option<String> {
    read_manifest(&member.join("manifest.json"))
        .ok()
        .map(|m| m.name)
}

/// Fallback member name: the directory's own name.
fn member_name_fallback(member: &Path) -> String {
    member
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("member")
        .to_string()
}
