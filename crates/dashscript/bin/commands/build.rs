//! `ds build [--target] [--filter]`: compile a native binary (default), emit
//! the translated Rust crate (`--target rust`), or build every workspace member
//! at a workspace root.

use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

use dashscript::Package;

use dashscript::project::{
    apply_runtime_deps, bin_lib_stems, cache_project_dir, default_package, emit_cargo_project,
    find_package_root, invoke_cargo, project_name, read_package, resolve_entry, resolve_target,
    status_to_code, translate_project,
};

/// Parsed `ds build` flags: optional entry file, optional `--target`, optional
/// `--filter` (workspace member).
pub(crate) type BuildArgs = (Option<String>, Option<String>, Option<String>);

/// Parse `ds build` arguments: an optional `.ts` file, an optional
/// `--target <bin|rust>` override, and an optional `--filter <name>` (workspace
/// member). Returns an error message on misuse (shown as usage). No file means
/// build the project entry (`package.json bin`/`main.ts`) — or, at a workspace
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
                        "usage: ds build [<file.ts>] [--target <bin|rust>] [--filter <name>]"
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
                        "usage: ds build [<file.ts>] [--target <bin|rust>] [--filter <name>]"
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

/// Build a `.ts` file or the project entry. `--target rust` emits the
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
    // Absolute the entry so a relative `ds build main.ts` still resolves
    // cross-directory imports: `Path::new("main.ts").parent()` is empty, which
    // starves the resolver's base. Joining cwd (rather than canonicalize, which
    // on Windows yields a `\\?\` verbatim path that leaks into cargo/dist
    // output) keeps paths display-clean while making `parent()` correct.
    let owned = std::env::current_dir()
        .map(|cwd| cwd.join(entry))
        .unwrap_or_else(|_| PathBuf::from(entry));
    let path = owned.as_path();
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
            let bins = project_bins(path);
            if bins.is_empty() {
                // Lone file: a single binary named after the project.
                copy_release_bin(&cache, &name, dest_root)?;
            } else {
                // Project mode: every declared bin is a dist artifact.
                for (bin_name, _) in &bins {
                    copy_release_bin(&cache, bin_name, dest_root)?;
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        other => Err(format!(
            "ds build: target '{other}' not yet supported (use --target <bin|rust>)"
        )
        .into()),
    }
}

/// The package bins declared for `path`'s project, or empty for a lone file
/// (no package, or a package with no `bin`). `build` copies every declared
/// binary into `dist/`.
fn project_bins(path: &Path) -> Vec<(String, String)> {
    find_package_root(path)
        .and_then(|root| read_package(&root.join("package.json")).ok())
        .map(|m| m.bin_entries())
        .unwrap_or_default()
}

/// Copy `cargo build --release`'s output for `bin_name` from `cache` to
/// `dest_root/<bin_name>`, mirroring cargo's binary naming (`.exe` on Windows).
fn copy_release_bin(cache: &Path, bin_name: &str, dest_root: &Path) -> Result<(), Box<dyn Error>> {
    let bin_file = if cfg!(windows) {
        format!("{bin_name}.exe")
    } else {
        bin_name.to_string()
    };
    let bin = cache.join("target").join("release").join(&bin_file);
    fs::create_dir_all(dest_root)?;
    let dest = dest_root.join(&bin_file);
    fs::copy(&bin, &dest)?;
    println!("ds: built {}", dest.display());
    Ok(())
}

/// Whether `dir` is a workspace root: its `package.json` has a non-empty
/// `workspace` member-glob list that resolves to at least one member.
pub(crate) fn is_workspace_root(dir: &Path) -> bool {
    !discover_members(dir).is_empty()
}

/// Build the workspace at `root` — every member, or just the one named by
/// `--filter` (package name or member directory). For `bin`, members are
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
            "ds build: no workspace members matched (check `workspaces` globs in package.json)"
                .into(),
        );
    }

    // Select members, applying --filter (package name or member directory).
    let mut selected: Vec<(String, PathBuf, String)> = Vec::new();
    for member in &members {
        let dir_name = member_name_fallback(member);
        let name = package_name_of(member).unwrap_or_else(|| dir_name.clone());
        if let Some(want) = filter {
            if name != want && dir_name != want {
                continue;
            }
        }
        let entry = resolve_member_entry(root, member, &name)?;
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
    // Member dirs sit directly under the cache root (`<cache>/<name>/`),
    // mirroring the single-package `.cache/dash/<name>/`; a stale member from
    // a prior run is ignored by cargo, `ds cache clean` reclaims the space.
    let cache = root.join(".cache").join("dash");
    fs::create_dir_all(&cache)?;

    let member_packages: Vec<Package> = selected
        .iter()
        .map(|(_, dir, _)| {
            read_package(&dir.join("package.json")).unwrap_or_else(|_| default_package())
        })
        .collect();

    // bin names must be unique across the workspace (cargo builds each into
    // one shared target/).
    let mut bin_owners: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for (name, package) in selected.iter().map(|(n, _, _)| n).zip(&member_packages) {
        for (bin_name, _) in package.bin_entries() {
            if let Some(prev) = bin_owners.insert(bin_name.clone(), name.clone()) {
                return Err(format!(
                    "dashscript: bin name '{bin_name}' is declared in members '{prev}' and \
                     '{name}'; bin names must be unique within a workspace"
                )
                .into());
            }
        }
    }
    // crate names must be unique too. The injective `ds_`-prefixed escape
    // (`translator::imports::npm_to_ds_ident`) means two distinct npm names can
    // never share one cargo_name, so this is unreachable for real packages —
    // but a lone-file/default-package fallback could still produce a duplicate,
    // and cargo's own error is less actionable than naming the pair.
    let mut crate_owners: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for (npm_name, package) in selected.iter().map(|(n, _, _)| n).zip(&member_packages) {
        let cn = package.cargo_name();
        if let Some(prev) = crate_owners.insert(cn, npm_name.clone()) {
            return Err(format!(
                "dashscript: workspace members '{prev}' and '{npm_name}' normalize to the same \
                 crate name; package names must be distinct after npm→cargo normalization"
            )
            .into());
        }
    }

    // [workspace.dependencies] = union of member cargo deps, so a dep two
    // members use is declared once (cargo's hoisted node_modules). Each dep
    // carries its full Cargo.toml spec (version/features/...) — JSON↔TOML zero
    // loss. Only `dashscript.cargo.dependencies` flow here; package.json
    // `dependencies` are npm packages (node_modules), not cargo crates.
    let mut all_deps: std::collections::BTreeMap<String, dashscript::CargoDepSpec> =
        std::collections::BTreeMap::new();
    for package in &member_packages {
        for (name, spec) in &package.dashscript.cargo.dependencies {
            all_deps.entry(name.clone()).or_insert_with(|| spec.clone());
        }
    }
    let inherited: std::collections::BTreeSet<String> = all_deps.keys().cloned().collect();

    // `[workspace] members` paths are the member crate directories under the
    // cache, named by cargo_name — npm names carry `@`/`/` (illegal path chars
    // on Windows), and member_cache below uses the same cargo_name so path deps
    // resolve to sibling `../<cargo-name>` directories.
    let names: Vec<String> = member_packages.iter().map(|p| p.cargo_name()).collect();
    let root_package =
        read_package(&root.join("package.json")).unwrap_or_else(|_| default_package());
    fs::write(
        cache.join("Cargo.toml"),
        root_package.workspace_root_toml(&names, &all_deps),
    )?;

    for ((name, member_dir, _), package) in selected.iter().zip(&member_packages) {
        let member_cache = cache.join(package.cargo_name());
        // The member's `[lib]` entry: the root tsconfig `paths[name]` mapping
        // is the authoritative source→path declaration (office-open maps
        // `@office-open/xml` → `packages/xml/src/index.ts`); without it,
        // translate_project falls back to `main` only if it is a source file.
        // Never reverse-map a dist artifact.
        let lib_entry = entry_from_tsconfig_paths(root, name);
        let ((bins, lib), deps) =
            translate_project(member_dir, package, &member_cache, lib_entry.as_deref())?;
        let path_deps: Vec<String> = deps.path_deps().iter().cloned().collect();
        let mut cargo_toml = package.to_member_toml(&bins, lib.as_deref(), &inherited, &path_deps);
        deps.apply_to_cargo_toml(&mut cargo_toml);
        fs::write(member_cache.join("Cargo.toml"), cargo_toml)?;
        apply_runtime_deps(&member_cache, &deps, &bin_lib_stems(&bins, lib.as_deref()))?;
        println!("ds: {name} (workspace member)");
    }

    println!("ds: building workspace (shared target)...");
    let status = invoke_cargo(&cache, ["build", "--release", "--quiet"])?;
    if !status.success() {
        return Ok(status_to_code(status));
    }

    // Copy each member binary to its own `<member>/dist/` — not the workspace
    // root — so each package's artifact is independent and publishable.
    for ((name, member_dir, _), package) in selected.iter().zip(&member_packages) {
        let dest_dir = member_dir.join("dist");
        let _ = fs::remove_dir_all(&dest_dir);
        fs::create_dir_all(&dest_dir)?;
        let bins = package.bin_entries();
        if bins.is_empty() {
            // No declared bins → cargo built src/main.rs as <member name>.
            copy_release_bin(&cache, name, &dest_dir)?;
        } else {
            for (bin_name, _) in &bins {
                copy_release_bin(&cache, bin_name, &dest_dir)?;
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// Discover workspace members from any supported manifest. Tried in order:
/// `package.json` `workspaces` (npm/yarn/bun) → `pnpm-workspace.yaml`
/// `packages:` (pnpm) → `deno.json` `workspace` (deno, singular). The first
/// manifest that is present and declares members wins; the rest are ignored.
/// Within a source, negative globs (`!...`) subtract from the positive ones.
/// Empty if `root` has no workspace manifest or no members match.
fn discover_members(root: &Path) -> Vec<PathBuf> {
    let globs = workspace_member_globs(root);
    if globs.is_empty() {
        return Vec::new();
    }
    expand_member_globs(root, &globs)
}

/// The workspace member globs declared by whichever manifest is present, tried
/// in the order npm/yarn/bun → pnpm → deno.
fn workspace_member_globs(root: &Path) -> Vec<String> {
    if let Some(g) = package_json_workspace_globs(root) {
        return g;
    }
    if let Some(g) = pnpm_workspace_globs(root) {
        return g;
    }
    if let Some(g) = deno_workspace_globs(root) {
        return g;
    }
    Vec::new()
}

/// npm/yarn/bun: `package.json` `workspaces` (string / array / `{packages:[]}`).
fn package_json_workspace_globs(root: &Path) -> Option<Vec<String>> {
    let json = fs::read_to_string(root.join("package.json")).ok()?;
    let package = Package::from_json(&json).ok()?;
    if package.workspaces.is_empty() {
        return None;
    }
    Some(package.workspaces)
}

/// pnpm: `pnpm-workspace.yaml` `packages:` list. The file is small and
/// top-level simple, so a dependency-free line-based parser avoids pulling a
/// yaml crate (and skips `allowBuilds`/`patchedDependencies`/etc.). Returns
/// None when the file is absent or declares no packages.
fn pnpm_workspace_globs(root: &Path) -> Option<Vec<String>> {
    let content = fs::read_to_string(root.join("pnpm-workspace.yaml")).ok()?;
    let mut globs = Vec::new();
    let mut in_packages = false;
    for line in content.lines() {
        let is_indent = line.starts_with(' ') || line.starts_with('\t');
        // A non-indented, non-blank line is a new top-level key — toggle which
        // section we are reading. `packages:` begins the member list; any other
        // key ends it.
        if !is_indent && !line.trim().is_empty() {
            in_packages = line.trim_start().starts_with("packages:");
            continue;
        }
        if in_packages {
            if let Some(item) = line.trim().strip_prefix("- ") {
                let g = item.trim().trim_matches(|c| c == '"' || c == '\'');
                if !g.is_empty() {
                    globs.push(g.to_string());
                }
            }
        }
    }
    if globs.is_empty() {
        None
    } else {
        Some(globs)
    }
}

/// deno: `deno.json` `workspace` (singular) path/glob array (Deno 1.45+).
fn deno_workspace_globs(root: &Path) -> Option<Vec<String>> {
    let content = fs::read_to_string(root.join("deno.json")).ok()?;
    let value: serde_json::Value = serde_json::from_str(&content).ok()?;
    let arr = value.get("workspace")?.as_array()?;
    let globs: Vec<String> = arr
        .iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect();
    if globs.is_empty() {
        None
    } else {
        Some(globs)
    }
}

/// Expand workspace globs into member directories, honoring negative globs
/// (`!...`) that subtract matches. A member is a directory holding its own
/// manifest (`package.json` for npm/yarn/bun/pnpm, `deno.json` for deno).
fn expand_member_globs(root: &Path, globs: &[String]) -> Vec<PathBuf> {
    let mut excluded: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    for glob in globs.iter().filter_map(|g| g.strip_prefix('!')) {
        for member in expand_single_member_glob(root, glob) {
            excluded.insert(member);
        }
    }
    let mut members = Vec::new();
    for glob in globs {
        if glob.starts_with('!') {
            continue;
        }
        for member in expand_single_member_glob(root, glob) {
            if excluded.contains(&member) || members.contains(&member) {
                continue;
            }
            members.push(member);
        }
    }
    members.sort();
    members
}

/// One workspace glob → member directories. Supports `dir/*` and `dir/**`
/// (direct children — pnpm's recursive `**` flattens to the same set for the
/// common single-level layout), a bare `dir` (one explicit member), and a
/// `./dir` (deno) form. Each candidate must be a dir with its own manifest.
fn expand_single_member_glob(root: &Path, glob: &str) -> Vec<PathBuf> {
    let glob = glob.strip_prefix("./").unwrap_or(glob);
    let dir_name = glob.strip_suffix("/**").or_else(|| glob.strip_suffix("/*"));
    let dirs: Vec<PathBuf> = match dir_name {
        Some(dir_name) => {
            let dir = root.join(dir_name);
            fs::read_dir(&dir)
                .map(|entries| {
                    entries
                        .flatten()
                        .map(|e| e.path())
                        .filter(|p| p.is_dir())
                        .collect()
                })
                .unwrap_or_default()
        }
        None => vec![root.join(glob)],
    };
    dirs.into_iter()
        .filter(|p| p.is_dir() && (p.join("package.json").exists() || p.join("deno.json").exists()))
        .collect()
}

/// Resolve a workspace member's source entry, in priority order:
///
/// 1. `package.json` `bin` — a binary member (cargo `[[bin]]`).
/// 2. root `tsconfig.json` `compilerOptions.paths[name]` — the alias map is the
///    authoritative source→path declaration when present (office-open maps
///    `@office-open/xml` → `./packages/xml/src/index.ts`).
/// 3. `package.json` `source` — the microbundle/jvdx convention for an explicit
///    source entry, distinct from `main`/`exports` (which point at dist output).
/// 4. `main.ts` — DashScript's explicit entry convention.
///
/// A dist artifact (`main: "dist/index.mjs"`) is build output, never a source
/// entry — it is not reverse-mapped (the source `index.ts` may build to any
/// dist name, so dist→src is not recoverable). A member with no resolvable
/// source entry is treated as a plain npm package (integrated as a dependency,
/// not force-translated). Nothing is hardcoded to `index.ts`: every step reads
/// a real declaration.
fn resolve_member_entry(root: &Path, member: &Path, name: &str) -> Result<String, Box<dyn Error>> {
    // 1. binary member
    let package = read_package(&member.join("package.json")).ok();
    if let Some(pkg) = &package {
        if let Some((_, bin_path)) = pkg.bin_entries().into_iter().next() {
            let p = member.join(&bin_path);
            if p.exists() {
                return Ok(p.to_string_lossy().into_owned());
            }
        }
    }

    // 2. root tsconfig.json paths[name] — the authoritative source→path map.
    if let Some(entry) = entry_from_tsconfig_paths(root, name) {
        return Ok(entry);
    }

    let entries = read_member_entries(member);
    // 3. package.json "source" field — an explicit source entry (not a dist one).
    if let Some(src) = entries.as_ref().and_then(|e| e.source.as_deref()) {
        let p = member.join(src);
        if p.exists() {
            return Ok(p.to_string_lossy().into_owned());
        }
    }

    // 4. DashScript explicit entry
    let main = member.join("main.ts");
    if main.exists() {
        return Ok(main.to_string_lossy().into_owned());
    }

    Err(format!(
        "ds build: member {} has no entry (declare tsconfig paths[\"{name}\"], \
         set package.json \"source\" or bin, or add main.ts)",
        member.display()
    )
    .into())
}

/// Read `compilerOptions.paths[name]` from the root `tsconfig.json`. Returns the
/// first target (paths values are arrays) joined to `root`, if the file exists.
/// `extends` is not followed — a root tsconfig that declares workspace aliases
/// does so inline.
fn entry_from_tsconfig_paths(root: &Path, name: &str) -> Option<String> {
    let text = fs::read_to_string(root.join("tsconfig.json")).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    let targets = json
        .get("compilerOptions")?
        .get("paths")?
        .get(name)?
        .as_array()?;
    let first = targets.first()?.as_str()?;
    let p = root.join(first);
    p.exists().then(|| p.to_string_lossy().into_owned())
}

/// The `source` field read from a member's `package.json` as raw JSON — the
/// microbundle/jvdx explicit source-entry convention, distinct from `main`/
/// `exports` (which point at dist output and are deliberately not read here:
/// a dist path is build output, not a source entry).
struct MemberEntries {
    source: Option<String>,
}

fn read_member_entries(member: &Path) -> Option<MemberEntries> {
    let text = fs::read_to_string(member.join("package.json")).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    Some(MemberEntries {
        source: json
            .get("source")
            .and_then(|v| v.as_str())
            .map(String::from),
    })
}

/// Read a member's package `name` (for `--filter` matching and display).
fn package_name_of(member: &Path) -> Option<String> {
    read_package(&member.join("package.json"))
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQ: AtomicU64 = AtomicU64::new(0);

    /// A throwaway workspace root under the system temp dir, removed on drop.
    struct TmpRoot(PathBuf);
    impl TmpRoot {
        fn new(label: &str) -> Self {
            let n = SEQ.fetch_add(1, Ordering::SeqCst);
            let dir = std::env::temp_dir()
                .join(format!("ds-build-test-{label}-{n}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            TmpRoot(dir)
        }
        fn path(&self) -> &Path {
            &self.0
        }
        fn write(&self, rel: &str, content: &str) {
            let p = self.0.join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, content).unwrap();
        }
        /// A member under `packages/<name>` with a `package.json` (and an extra
        /// `deno.json` when `with_deno`), so the manifest check accepts it.
        fn member(&self, name: &str, with_deno: bool) {
            let m = self.0.join("packages").join(name);
            fs::create_dir_all(&m).unwrap();
            fs::write(m.join("package.json"), "{}").unwrap();
            if with_deno {
                fs::write(m.join("deno.json"), "{}").unwrap();
            }
        }
    }
    impl Drop for TmpRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Sorted member directory basenames — order-independent comparison.
    fn names(members: &[PathBuf]) -> Vec<String> {
        let mut out: Vec<String> = members
            .iter()
            .filter_map(|m| m.file_name().and_then(|s| s.to_str()).map(str::to_string))
            .collect();
        out.sort();
        out
    }

    #[test]
    fn discover_members_reads_package_json_workspaces_array() {
        let t = TmpRoot::new("npm-array");
        t.write(
            "package.json",
            r#"{"name":"root","workspaces":["packages/*"]}"#,
        );
        t.member("a", false);
        t.member("b", false);
        assert_eq!(names(&discover_members(t.path())), vec!["a", "b"]);
    }

    #[test]
    fn discover_members_reads_package_json_workspaces_object() {
        // yarn classic { "packages": [...] } object form.
        let t = TmpRoot::new("yarn-obj");
        t.write(
            "package.json",
            r#"{"name":"root","workspaces":{"packages":["packages/*"]}}"#,
        );
        t.member("a", false);
        t.member("b", false);
        assert_eq!(names(&discover_members(t.path())), vec!["a", "b"]);
    }

    #[test]
    fn discover_members_reads_pnpm_workspace_yaml() {
        let t = TmpRoot::new("pnpm");
        t.write(
            "pnpm-workspace.yaml",
            "packages:\n  - packages/**\n  - \"!**/test/**\"\nallowBuilds:\n  sharp: true\n",
        );
        t.member("core", false);
        t.member("xml", false);
        assert_eq!(names(&discover_members(t.path())), vec!["core", "xml"]);
    }

    #[test]
    fn discover_members_reads_deno_workspace_singular() {
        // deno.json uses a SINGULAR `workspace` field (Deno 1.45+).
        let t = TmpRoot::new("deno");
        t.write(
            "deno.json",
            r#"{"workspace":["./packages/a","./packages/b"]}"#,
        );
        t.member("a", true);
        t.member("b", true);
        assert_eq!(names(&discover_members(t.path())), vec!["a", "b"]);
    }

    #[test]
    fn expand_member_globs_subtracts_negative() {
        let t = TmpRoot::new("neg");
        t.member("keep", false);
        t.member("skip", false);
        let globs = vec!["packages/*".to_string(), "!packages/skip".to_string()];
        assert_eq!(names(&expand_member_globs(t.path(), &globs)), vec!["keep"]);
    }

    /// Member with a `src/<entry>.ts` (non-`index`) file and a package.json name
    /// matching a root tsconfig alias — the alias wins, locating the real entry.
    #[test]
    fn resolve_member_entry_prefers_tsconfig_paths() {
        let t = TmpRoot::new("entry-tsconfig");
        t.write(
            "tsconfig.json",
            r#"{"compilerOptions":{"paths":{"@scope/pkg":["./packages/pkg/src/lib.ts"]}}}"#,
        );
        let m = t.path().join("packages").join("pkg");
        fs::create_dir_all(m.join("src")).unwrap();
        fs::write(m.join("package.json"), r#"{"name":"@scope/pkg"}"#).unwrap();
        fs::write(m.join("src").join("lib.ts"), "export const x = 1;").unwrap();
        let entry = resolve_member_entry(t.path(), &m, "@scope/pkg").unwrap();
        assert!(entry.contains("src") && entry.ends_with("lib.ts"));
    }

    /// No tsconfig alias → package.json `source` field (microbundle convention).
    #[test]
    fn resolve_member_entry_uses_source_field() {
        let t = TmpRoot::new("entry-source");
        let m = t.path().join("packages").join("pkg");
        fs::create_dir_all(m.join("src")).unwrap();
        fs::write(
            m.join("package.json"),
            r#"{"name":"pkg","source":"src/entry.ts"}"#,
        )
        .unwrap();
        fs::write(m.join("src").join("entry.ts"), "export const x = 1;").unwrap();
        let entry = resolve_member_entry(t.path(), &m, "pkg").unwrap();
        assert!(entry.ends_with("entry.ts"));
    }

    /// No alias, no `source` → reverse-map `main` dist artifact to `.ts` source.
    #[test]
    fn resolve_member_entry_reverse_maps_dist_main_to_src() {
        let t = TmpRoot::new("entry-main");
        let m = t.path().join("packages").join("pkg");
        fs::create_dir_all(m.join("src")).unwrap();
        fs::write(
            m.join("package.json"),
            r#"{"name":"pkg","main":"dist/index.mjs"}"#,
        )
        .unwrap();
        fs::write(m.join("src").join("index.ts"), "export const x = 1;").unwrap();
        let entry = resolve_member_entry(t.path(), &m, "pkg").unwrap();
        assert!(entry.contains("src") && entry.ends_with("index.ts"));
    }

    /// No declaration of any kind → error. Nothing silently falls back to a
    /// hardcoded `src/index.ts`.
    #[test]
    fn resolve_member_entry_errors_without_declaration() {
        let t = TmpRoot::new("entry-none");
        let m = t.path().join("packages").join("pkg");
        fs::create_dir_all(&m).unwrap();
        fs::write(m.join("package.json"), r#"{"name":"pkg"}"#).unwrap();
        assert!(resolve_member_entry(t.path(), &m, "pkg").is_err());
    }
}
