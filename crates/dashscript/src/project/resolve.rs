use super::*;

/// Resolve a relative `.ts` import (`"./other"` or `"./other.ts"`) against the
/// importing file's directory. Errors clearly when no matching file exists.
/// What kind of file a resolved dependency is — decides how the build
/// pipeline lowers it. A `.ts` or `.js` dep is transpiled as a Rust module
/// (transpile-first: a `.js` is JS-flavored TS, and the translator already
/// handles untyped params and literal type inference). A pure `.d.ts` (an
/// `@types/*` package with no `.js`) carries types only — its `interface`/
/// `type` declarations become Rust items, and a value import surfaces as a
/// `cargo check` error honestly. A `.d.ts` with a sibling `.js` is a typed
/// package awaiting type injection (a later batch).
#[derive(Clone, Debug, PartialEq)]
pub enum DepKind {
    Ts,
    DtsOnly,
    DtsWithJs { dts_path: PathBuf, js_path: PathBuf },
    Js,
}

/// The kind of a resolved entry path. `.d.ts` is detected by file name:
/// `Path::extension` returns `"ts"` for `index.d.ts` (it takes the last
/// `.`-segment), so the extension alone cannot tell `.d.ts` from `.ts`. A
/// `.d.ts`/`.js` pair is a typed package — the `.js` is the implementation;
/// a lone `.d.ts` is pure types; a lone `.js` is untyped.
pub(crate) fn dep_kind_of(entry: &Path) -> DepKind {
    let is_dts = entry
        .file_name()
        .is_some_and(|n| n.to_string_lossy().ends_with(".d.ts"));
    if is_dts {
        return match sibling_with_ext(entry, "js") {
            Some(js_path) => DepKind::DtsWithJs {
                dts_path: entry.to_path_buf(),
                js_path,
            },
            None => DepKind::DtsOnly,
        };
    }
    match entry.extension().and_then(|e| e.to_str()) {
        Some("js" | "mjs" | "cjs") => match sibling_with_ext(entry, "d.ts") {
            Some(dts_path) => DepKind::DtsWithJs {
                dts_path,
                js_path: entry.to_path_buf(),
            },
            None => DepKind::Js,
        },
        _ => DepKind::Ts,
    }
}

/// A sibling file with the same stem but a different extension: `index.d.ts`
/// → `index.js`. Returns the path only when it exists. The stem is the first
/// `.`-delimited segment, so `index.d.ts` and `foo.ts` both strip correctly.
pub(crate) fn sibling_with_ext(entry: &Path, new_ext: &str) -> Option<PathBuf> {
    let stem = entry.file_name()?.to_str()?.split('.').next()?;
    let candidate = entry.with_file_name(format!("{stem}.{new_ext}"));
    candidate.exists().then_some(candidate)
}

/// The resolver shared across the build pipeline. Configured for a TypeScript
/// project: `.ts`/`.d.ts` extensions first (DashScript lowers `.ts` to Rust),
/// then the JS variants for npm packages; `package.json` field priority favors
/// declarations (`types`/`typings`) and ESM (`module`) over legacy `main`; the
/// `exports` field is read with ESM-import/types/default conditions — the
/// modern npm shape (pure ESM + `.d.ts`). `module_type: true` computes whether
/// a resolved `.js` is ESM or CommonJS, used when wiring the engine. tsconfig
/// path aliases (`@alias/*`-style) are auto-discovered — the caller resolves
/// via `resolve_file`, which walks up from the importer to find `tsconfig.json`;
/// `symlinks: true` follows pnpm/npm `node_modules` links.
pub(crate) fn ds_resolver() -> oxc_resolver::Resolver {
    use oxc_resolver::{ResolveOptions, Resolver};
    Resolver::new(ResolveOptions {
        extensions: vec![
            ".ts".into(),
            ".tsx".into(),
            ".d.ts".into(),
            ".js".into(),
            ".mjs".into(),
            ".cjs".into(),
        ],
        main_fields: vec![
            "types".into(),
            "typings".into(),
            "module".into(),
            "main".into(),
        ],
        condition_names: vec!["import".into(), "types".into(), "default".into()],
        main_files: vec!["index".into()],
        module_type: true,
        tsconfig: Some(oxc_resolver::TsconfigDiscovery::Auto),
        symlinks: true,
        ..ResolveOptions::default()
    })
}

/// Resolve a workspace-local package import — a bare specifier (`@scope/pkg`
/// or `@scope/pkg/sub`) whose `node_modules` entry is a symlink to a sibling
/// package under the monorepo root — to that package's `src/` source, bypassing
/// the `package.json` `exports`/`main` fields that point at the built `dist/`
/// (which DashScript, a source translator, never consumes).
///
/// DashScript does **not** parse `pnpm-workspace.yaml` or `package.json`
/// `workspaces`: that is the package manager's job. It trusts `node_modules`
/// instead — every package manager (npm/yarn/pnpm/bun) materializes workspace
/// deps as a symlink under `node_modules/<pkg>` after `install`. A symlink
/// whose target is outside the pnpm virtual store (`node_modules/.pnpm/`) is a
/// workspace-local source package; a `.pnpm`-store target (or a plain hoisted
/// directory/file) is a registry package and is left to [`ds_resolver`].
/// Returns `None` for relative/cargo specifiers and for registry packages.
pub(crate) fn resolve_workspace_dep(base: &Path, source: &str) -> Option<(PathBuf, DepKind)> {
    if source.starts_with('.') || source.starts_with("cargo:") {
        return None;
    }
    let (pkg_root, subpath) = split_package_spec(source)?;
    for ancestor in base.ancestors() {
        let entry = ancestor.join("node_modules").join(&pkg_root);
        let Ok(meta) = fs::symlink_metadata(&entry) else {
            continue; // no node_modules/<pkg> at this layer; walk up
        };
        // Only a symlinked entry is a workspace-local package; a plain
        // directory/file is a hoisted registry package (npm/yarn) — leave it to
        // the standard resolver.
        if !meta.is_symlink() {
            return None;
        }
        let Ok(real) = fs::canonicalize(&entry) else {
            return None;
        };
        // A pnpm-store package is also a symlink, but its target lives under
        // `node_modules/.pnpm/`; a workspace-local target is the source dir.
        if real.components().any(|c| c.as_os_str() == ".pnpm") {
            return None;
        }
        return resolve_local_src(&real, subpath.as_deref()).map(|p| (p, DepKind::Ts));
    }
    None
}

/// Split a bare specifier into `(package_root, optional subpath)`. A scoped
/// package is one unit: `@scope/pkg/sub` → `("@scope/pkg", Some("sub"))`; a
/// plain package is the first segment: `pkg/sub` → `("pkg", Some("sub"))`.
pub(crate) fn split_package_spec(source: &str) -> Option<(String, Option<String>)> {
    if let Some(rest) = source.strip_prefix('@') {
        let slash = rest.find('/')?;
        let (scope, after) = rest.split_at(slash); // `after` starts with '/'
        let after = &after[1..]; // pkg[/sub...]
        return match after.find('/') {
            Some(sub_idx) => {
                let (pkg, sub) = after.split_at(sub_idx); // `sub` starts with '/'
                Some((format!("@{scope}/{pkg}"), Some(sub[1..].to_string())))
            }
            None => Some((format!("@{scope}/{after}"), None)),
        };
    }
    match source.find('/') {
        Some(slash) => Some((
            source[..slash].to_string(),
            Some(source[slash + 1..].to_string()),
        )),
        None => Some((source.to_string(), None)),
    }
}

/// Map a workspace-local package directory to its source entry under `src/`:
/// barrel `@scope/pkg` → `src/index.ts`; subpath `@scope/pkg/chart` →
/// `src/chart/index.ts` (directory barrel, tried first) or `src/chart.ts`.
/// Returns `None` when no source entry exists (e.g. the package ships `dist/`
/// only).
pub(crate) fn resolve_local_src(pkg_dir: &Path, subpath: Option<&str>) -> Option<PathBuf> {
    let src = pkg_dir.join("src");
    let candidate = match subpath {
        None => src.join("index.ts"),
        Some(sub) => {
            let dir_barrel = src.join(sub).join("index.ts");
            if dir_barrel.exists() {
                return Some(dir_barrel);
            }
            src.join(format!("{sub}.ts"))
        }
    };
    candidate.exists().then_some(candidate)
}

/// Resolve an import specifier (relative `./foo` or bare `pkg`) to a file path
/// and its [`DepKind`]. A workspace-local bare specifier (`@scope/pkg`,
/// installed as a `node_modules` symlink to a sibling package) resolves to that
/// package's `src/` via [`resolve_workspace_dep`], bypassing the `exports`/
/// `main` fields that point at the unbuilt `dist/`. Every other specifier — a
/// registry package, a relative path, or `cargo:` — delegates to `oxc_resolver`
/// (the canonical Node resolution algorithm, webpack `enhanced-resolve` port):
/// it handles `node_modules/` walk-up, `package.json` `exports`/`main`/
/// `module`/`types`, scoped packages, and tsconfig paths — so DashScript
/// reuses the standard resolver rather than hand-writing a subset. The
/// `DepKind` is decided from the resolved path's extension and sibling files.
pub fn resolve_local_module(
    base: &Path,
    source: &str,
) -> Result<(PathBuf, DepKind), Box<dyn Error>> {
    if let Some(ws) = resolve_workspace_dep(base, source) {
        return Ok(ws);
    }
    // tsconfig path-alias discovery needs an importer *file*: `resolve_file`
    // walks up from it to find `tsconfig.json` (Auto), whereas `resolve` (a
    // directory) skips tsconfig. `base` is the importer's directory, so
    // synthesize a file inside it — resolve_file uses only its parent (= base)
    // for resolution and walks up for tsconfig; it never stats the name.
    let resolved = ds_resolver().resolve_file(base.join("index.ts"), source);
    let resolution = resolved.map_err(|e| {
        let mut msg = format!("dashscript: import '{source}' did not resolve: {e}");
        // A bare specifier that failed to resolve, with no `node_modules`
        // anywhere up the tree, almost always means deps were not installed —
        // point the user at `install` instead of leaving the raw resolver error.
        if !source.starts_with('.') && !base.ancestors().any(|a| a.join("node_modules").is_dir()) {
            msg.push_str(" (no node_modules found — run pnpm/npm/yarn/bun install first)");
        }
        msg
    })?;
    let path = resolution.into_path_buf();
    let kind = dep_kind_of(&path);
    Ok((path, kind))
}

/// Resolve the Cargo package for `src_path`: the `package.json` found walking
/// up from the file (Deno-style), otherwise a minimal package named after the
/// project (`project_name`).
pub fn resolve_package(src_path: &Path) -> String {
    if let Some(root) = find_package_root(src_path) {
        if let Ok(json) = fs::read_to_string(root.join("package.json")) {
            if let Ok(package) = Package::from_json(&json) {
                return package.to_cargo_toml();
            }
        }
    }
    Package {
        name: project_name(src_path),
        ..Package::default()
    }
    .to_cargo_toml()
}

/// The cache directory for a `.ts` entry file, Deno-style: walk up from the
/// file for a `package.json`; found → in-project `.cache/dash/<project>/` —
/// **one per project** (keyed by project name, not the entry stem, so two
/// `main.ts` files in different projects don't collide and one project's
/// entries share a cache); not found (a lone file) → global
/// `~/.cache/dash/<hash>/`. The `dash` segment mirrors the global cache root,
/// so DashScript owns one namespace under `.cache/`. `run`, `build`, and
/// `install` all share this directory, so repeat invocations reuse cargo's
/// incremental `target/` instead of recompiling std and every dependency from
/// scratch. Falls back to a temp dir if no platform cache dir is resolvable,
/// so a lone file always runs.
pub fn cache_project_dir(src_path: &Path) -> PathBuf {
    if let Some(root) = find_package_root(src_path) {
        return root
            .join(".cache")
            .join("dash")
            .join(project_name(src_path));
    }
    global_cache_dir(src_path)
}

/// Walk up from the `.ts` file's directory for the nearest `package.json`,
/// returning its directory (the project root) if one exists.
pub fn find_package_root(src_path: &Path) -> Option<PathBuf> {
    let dir = src_path.parent()?;
    for ancestor in dir.ancestors() {
        if ancestor.join("package.json").exists() {
            return Some(ancestor.to_path_buf());
        }
    }
    None
}

/// Find the nearest `package.json` walking up from the **cwd** (whereas
/// [`find_package_root`] starts from a `.ts` file's directory). Used by
/// cwd-based commands (`install`, `add`, `remove`, `run`) so they work from a
/// subdirectory — mirroring pnpm/cargo, which find the workspace root from any
/// nested dir. Falls back to the cwd when no package is found, so callers
/// report "no package.json here" instead of panicking.
pub fn package_root() -> PathBuf {
    let Ok(cwd) = std::env::current_dir() else {
        return PathBuf::from(".");
    };
    for ancestor in cwd.ancestors() {
        if ancestor.join("package.json").exists() {
            return ancestor.to_path_buf();
        }
    }
    PathBuf::from(".")
}

/// Collect every `.ts` file under the current project (the nearest
/// `package.json` walking up, else the cwd), skipping generated/vendored
/// directories (`target`, `.cache`, `dist`, `node_modules`, `.git`). Used by
/// `ds lint` with no argument — the way `vp check` and
/// `oxlint` check the whole project when given no target. Sorted for stable
/// output.
pub fn collect_ts_files() -> Vec<PathBuf> {
    let root = package_root();
    let mut out = Vec::new();
    walk_ts(&root, &mut out);
    out.sort();
    out
}

/// Recursive worker for [`collect_ts_files`].
pub(crate) fn walk_ts(dir: &Path, out: &mut Vec<PathBuf>) {
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
            walk_ts(&path, out);
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            // JS and TS are both first-class source — oxc parses either, and the
            // translator lowers a `.js` as JS-flavored TypeScript (untyped params
            // default to `f64`, literal types infer). `.mjs`/`.cjs` carry only
            // module-system intent (ESM/CJS), not new syntax; `.jsx`/`.tsx` add
            // JSX (not yet lowered, but collected so a mixed project still sees
            // every source file). Test/benchmark co-files (`.spec`/`.test`/
            // `.bench`) exercise the crate, they are not part of it — including
            // them pulls top-level assertions or a timing harness into a module
            // file, which has no entry to run them.
            if !matches!(ext, "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs") {
                continue;
            }
            let is_test = path
                .file_stem()
                .and_then(|s| s.to_str())
                .is_some_and(|stem| {
                    stem.ends_with(".spec") || stem.ends_with(".test") || stem.ends_with(".bench")
                });
            if is_test {
                continue;
            }
            out.push(path);
        }
    }
}

/// The global fallback cache for a lone `.ts` file (no `package.json` found
/// walking up): `~/.cache/dash/<hash(canonical_path)>/`, keyed by the file's
/// canonical path so the same file reuses it across runs.
pub fn global_cache_dir(src_path: &Path) -> PathBuf {
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

/// The file stem of a path as an owned `String` ("main.ts" → "main").
pub fn stem_of(path: &Path) -> String {
    // An index.ts in a subdirectory is a bundler barrel (architecture-proposal
    // decision 4): foo/index.ts names its module after the parent dir
    // (crate::foo), so `import { x } from "./foo"` lands on `mod foo; src/foo.rs`.
    // A root index.ts keeps its own stem — it is an entry, not a barrel.
    if path.file_name().and_then(|n| n.to_str()) == Some("index.ts") {
        if let Some(dir) = path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
        {
            if !dir.is_empty() {
                return dir.to_string();
            }
        }
    }
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("dash")
        .to_string()
}

/// The build output name: the `package.json` `name` if present, else the
/// project directory name, else the file stem — never the bare stem when a
/// project exists, so two entry files don't clobber `dist/<name>`.
pub fn project_name(src_path: &Path) -> String {
    if let Some(root) = find_package_root(src_path) {
        if let Ok(json) = fs::read_to_string(root.join("package.json")) {
            if let Ok(package) = Package::from_json(&json) {
                if !package.name.trim().is_empty() {
                    return package.cargo_name();
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
/// else `main.ts` in the cwd.
pub fn resolve_entry() -> Result<String, Box<dyn Error>> {
    if let Ok(package) = read_package(Path::new("package.json")) {
        if let Some((_, bin_path)) = package.bin_entries().into_iter().next() {
            if Path::new(&bin_path).exists() {
                return Ok(bin_path);
            }
        }
    }
    if Path::new("main.ts").exists() {
        return Ok("main.ts".to_string());
    }
    Err("ds build: no entry file (pass <file.ts>, set package.json bin, or add main.ts)".into())
}

/// The build target for `src_path`: the `--target` override, else the
/// `package.json` `target`, else `bin`.
pub fn resolve_target(src_path: &Path, override_target: Option<&str>) -> String {
    if let Some(t) = override_target {
        return t.to_string();
    }
    if let Some(root) = find_package_root(src_path) {
        if let Ok(json) = fs::read_to_string(root.join("package.json")) {
            if let Ok(package) = Package::from_json(&json) {
                return package.dashscript.target;
            }
        }
    }
    "bin".to_string()
}

/// Read and parse a `package.json`.
pub fn read_package(path: &Path) -> Result<Package, Box<dyn Error>> {
    let json =
        fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    Ok(Package::from_json(&json)?)
}

/// A package named after the current directory, with defaults.
pub fn default_package() -> Package {
    let name = std::env::current_dir()
        .ok()
        .and_then(|d| d.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "dashscript".to_string());
    Package {
        name,
        ..Package::default()
    }
}

/// Path to the cargo binary — the system `cargo` today; a DashScript-managed
/// toolchain replaces this once the self-contained Rust layer lands.
pub fn cargo_bin() -> &'static Path {
    Path::new("cargo")
}

/// Invoke `cargo` with `args` inside `project`, inheriting stdio. Errors if
/// cargo is not on PATH.
pub fn invoke_cargo<const N: usize>(
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
pub fn status_to_code(status: ExitStatus) -> ExitCode {
    if status.success() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
