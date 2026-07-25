//! Shared helpers for the `ds` subcommands: package discovery, cache
//! resolution, source translation, and cargo invocation. The command modules
//! ([`super::build`], [`super::run`], [`super::deps`], [`super::check`],
//! [`super::cache`]) build on these.

use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::{Command, ExitCode, ExitStatus},
};

use dashscript::{FileRole, Package, RuntimeDeps, Translator};

/// Translate one resolved dependency to the Rust source for `src/<module>.rs`,
/// merging its runtime deps into `deps`. The `DepKind` picks the path: a `.ts`
/// or untyped `.js` dep is transpiled as a Rust module (transpile-first); a
/// pure `.d.ts` yields its `interface`/`type` items; a `.d.ts` + `.js` pair
/// needs type injection (a later batch, raised honestly).
fn translate_dep(
    translator: &Translator,
    dep_path: &Path,
    kind: DepKind,
    deps: &mut RuntimeDeps,
) -> Result<String, Box<dyn Error>> {
    match kind {
        DepKind::Ts | DepKind::Js => {
            // Transpile-first: a `.js` file is JS-flavored TypeScript, and the
            // translator already handles untyped params (default `f64`) and
            // literal type inference — so a pure-JS source that is a TS subset
            // lowers to Rust the same way a `.ts` module does, no engine.
            // Dynamic JS the table cannot lower (`typeof` / prototype / `eval`)
            // surfaces as a `cargo check` error honestly; the engine fallback
            // is a later batch for those. An ESM `.js` (`export function`)
            // works; a CommonJS `.js` (`module.exports = …` — a top-level
            // executable statement) is rejected by `FileRole::Module`.
            let dep_src = fs::read_to_string(dep_path)
                .map_err(|e| format!("cannot read import {}: {e}", dep_path.display()))?;
            let (rust, dep_deps) = translator
                .translate_with_deps_as(&dep_src, FileRole::Module)
                .map_err(|e| format!("translate {}: {e}", dep_path.display()))?;
            deps.merge(&dep_deps);
            Ok(rust)
        }
        DepKind::DtsOnly => {
            let dep_src = fs::read_to_string(dep_path)
                .map_err(|e| format!("cannot read import {}: {e}", dep_path.display()))?;
            Ok(translator.translate_dts(&dep_src))
        }
        DepKind::DtsWithJs { js_path } => Err(format!(
            "dashscript: dependency '{}' is a typed JavaScript package (.d.ts + {}); type \
             injection from the `.d.ts` into the `.js` transpile is a later batch",
            dep_path.display(),
            js_path.display()
        )
        .into()),
    }
}

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
) -> Result<RuntimeDeps, Box<dyn Error>> {
    let translator = Translator::new();
    let (rust, mut deps) = translator
        .translate_with_deps(src)
        .map_err(|e| format!("translate {}: {e}", src_path.display()))?;

    let base = src_path.parent().unwrap_or_else(|| Path::new(""));
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut mod_decls = String::new();
    for imp in translator.imports(src) {
        if !seen.insert(imp.module.clone()) {
            continue; // dedupe repeated imports of the same module
        }
        let (dep_path, kind) = resolve_local_module(base, &imp.source)?;
        let dep_rust = translate_dep(&translator, &dep_path, kind, &mut deps)?;
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
    Ok(deps)
}

/// Translate one `.ts` file to `src/<stem>.rs`, prefixing `mod <module>;` for
/// each of its imports (deduped). A relative import's file is translated
/// separately by [`translate_project`]'s directory walk (it sits in the project
/// tree); a bare (npm) import lives under `node_modules/` — which the walk
/// skips — so it is resolved and translated here alongside the entry.
fn translate_one_with_mods(
    ds: &Path,
    project_dir: &Path,
    role: FileRole,
) -> Result<RuntimeDeps, Box<dyn Error>> {
    let translator = Translator::new();
    let src = fs::read_to_string(ds).map_err(|e| format!("cannot read {}: {e}", ds.display()))?;
    let (rust, mut deps) = translator
        .translate_with_deps_as(&src, role)
        .map_err(|e| format!("translate {}: {e}", ds.display()))?;
    let base = ds.parent().unwrap_or_else(|| Path::new(""));
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut mod_decls = String::new();
    for imp in translator.imports(&src) {
        if !seen.insert(imp.module.clone()) {
            continue;
        }
        mod_decls.push_str(&format!("mod {};\n", imp.module));
        // A bare (npm) import is outside the project tree; translate it here so
        // its `mod` decl resolves. A relative import is translated by the walk.
        if !imp.source.starts_with('.') {
            let (dep_path, kind) = resolve_local_module(base, &imp.source)?;
            let dep_rust = translate_dep(&translator, &dep_path, kind, &mut deps)?;
            fs::write(
                project_dir.join("src").join(format!("{}.rs", imp.module)),
                dep_rust,
            )?;
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
    Ok(deps)
}

/// A project's resolved targets for `Cargo.toml`: the `(bin_name, ds_path)`
/// pairs for `[[bin]]`, plus the `[lib]` entry path.
type ProjectTargets = (Vec<(String, String)>, Option<String>);

/// Translate every `.ts` under a package root into one multi-target crate at
/// `project_dir/src/`: each file becomes `src/<stem>.rs` (prefixed with its
/// `mod` declarations), and the package's `bin`/`lib` entries become the
/// crate's `[[bin]]`/`[lib]` targets. Returns the resolved targets for
/// `Cargo.toml` emission.
///
/// Two project-level guards: a stem collision (two files flatten to the same
/// `src/<stem>.rs` — nested directories are not yet modeled as sub-modules),
/// and a bin importing another bin (cargo forbids it; shared code must go
/// through `[lib]`).
pub(crate) fn translate_project(
    root: &Path,
    package: &Package,
    project_dir: &Path,
) -> Result<(ProjectTargets, RuntimeDeps), Box<dyn Error>> {
    let src_dir = project_dir.join("src");
    fs::create_dir_all(&src_dir)?;
    // Clear stale translations from a prior run (a renamed bin, or a switch
    // between lone-file and project mode) so cargo never sees orphan modules.
    clean_src_dir(&src_dir)?;

    let bins = package.bin_entries();
    // package.json `main` → the crate's `[lib]` target (shared code bins `use`).
    let lib = package.main.clone();
    // 文件角色（架构决策点 8）：bin/lib entry 收顶层可执行语句进 `fn main`；
    // 其余文件（被 import 的 module）只声明、不执行 —— `Module` 角色对其顶层
    // 可执行语句报错（模块语义）。按 canonical 路径比对，使判定不受 import
    // 写法或 `bin` 相对/绝对写法影响。
    let entry_paths: std::collections::HashSet<PathBuf> = bins
        .iter()
        .filter_map(|(_, p)| root.join(p).canonicalize().ok())
        .chain(lib.as_ref().and_then(|p| root.join(p).canonicalize().ok()))
        .collect();

    let mut files = Vec::new();
    walk_ts(root, &mut files);
    files.sort();

    let mut seen_stems: std::collections::HashMap<String, PathBuf> =
        std::collections::HashMap::new();
    let mut deps = RuntimeDeps::default();
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
        let canon = ds.canonicalize().unwrap_or_else(|_| ds.clone());
        let role = if entry_paths.contains(&canon) {
            FileRole::BinEntry
        } else {
            FileRole::Module
        };
        let file_deps = translate_one_with_mods(ds, project_dir, role)?;
        deps.merge(&file_deps);
    }

    detect_bin_imports_bin(root, &bins)?;
    detect_circular_imports(&files)?;
    Ok(((bins, lib), deps))
}

/// Guard: detect circular imports. Rust forbids circular module dependencies
/// (`mod a` → `mod b` → `mod a`), which cargo reports as a vague error; this
/// surfaces the cycle explicitly with the files involved. Each file's imports
/// are resolved to canonical paths so the graph holds regardless of how an
/// import is written.
fn detect_circular_imports(files: &[PathBuf]) -> Result<(), Box<dyn Error>> {
    let known: Vec<PathBuf> = files
        .iter()
        .map(|p| p.canonicalize().unwrap_or_else(|_| p.clone()))
        .collect();
    let mut graph: std::collections::HashMap<PathBuf, Vec<PathBuf>> =
        std::collections::HashMap::new();
    for f in files {
        let Ok(src) = fs::read_to_string(f) else {
            continue;
        };
        let base = f.parent().unwrap_or_else(|| Path::new(""));
        let key = f.canonicalize().unwrap_or_else(|_| f.clone());
        for imp in Translator::new().imports(&src) {
            if let Ok((dep, _)) = resolve_local_module(base, &imp.source) {
                let dep = dep.canonicalize().unwrap_or(dep);
                if known.contains(&dep) {
                    graph.entry(key.clone()).or_default().push(dep);
                }
            }
        }
    }
    // DFS cycle detection (white=0 / gray=1 / black=2). A back edge to a gray
    // node closes a cycle.
    let mut color: std::collections::HashMap<PathBuf, u8> = std::collections::HashMap::new();
    for start in graph.keys() {
        if color.get(start).copied().unwrap_or(0) != 0 {
            continue;
        }
        let mut stack: Vec<PathBuf> = Vec::new();
        if let Some(cycle) = dfs_cycle(start, &graph, &mut color, &mut stack) {
            let names: Vec<String> = cycle
                .iter()
                .map(|p| {
                    p.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default()
                })
                .collect();
            return Err(format!(
                "dashscript: circular import detected — {}; refactor to break the cycle \
                 (Rust forbids circular module dependencies)",
                names.join(" → ")
            )
            .into());
        }
    }
    Ok(())
}

/// DFS helper for [`detect_circular_imports`]: returns the cycle path when a
/// back edge to a node already on the stack (gray) is found. Color: 0=white,
/// 1=gray (on stack), 2=black (fully explored).
fn dfs_cycle(
    node: &Path,
    graph: &std::collections::HashMap<PathBuf, Vec<PathBuf>>,
    color: &mut std::collections::HashMap<PathBuf, u8>,
    stack: &mut Vec<PathBuf>,
) -> Option<Vec<PathBuf>> {
    color.insert(node.to_path_buf(), 1);
    stack.push(node.to_path_buf());
    for dep in graph.get(node).into_iter().flatten() {
        match color.get(dep).copied().unwrap_or(0) {
            1 => {
                // Back edge → cycle. Slice from the dep's first occurrence and
                // close the loop for display.
                let start = stack.iter().position(|n| n == dep).unwrap();
                let mut cycle = stack[start..].to_vec();
                cycle.push(dep.clone());
                return Some(cycle);
            }
            0 => {
                if let Some(found) = dfs_cycle(dep, graph, color, stack) {
                    return Some(found);
                }
            }
            _ => {}
        }
    }
    stack.pop();
    color.insert(node.to_path_buf(), 2);
    None
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
            let Ok((dep, _)) = resolve_local_module(base, &imp.source) else {
                continue; // a missing module surfaces at `cargo build`
            };
            if let Ok(canon) = dep.canonicalize() {
                if let Some(other) = bin_files.get(&canon) {
                    if other != bin_name {
                        return Err(format!(
                            "dashscript: bin '{bin_name}' imports bin '{other}' (from {}); \
                             move the shared code into a lib module (a .ts that is not a bin \
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

/// Translate a `.ts` entry into a buildable Cargo project at `project_dir`.
///
/// Project mode (a package declares `bin` or `lib`): every `.ts` under the
/// root becomes `src/<stem>.rs` in one crate, and the declared entries become
/// `[[bin]]`/`[lib]` targets — so a project's entries share one cache and never
/// overwrite each other. Otherwise (a lone file, or a package with no declared
/// targets): a minimal package + a single `src/main.rs`.
pub(crate) fn emit_cargo_project(
    src: &str,
    src_path: &Path,
    project_dir: &Path,
) -> Result<(), Box<dyn Error>> {
    if let Some(root) = find_package_root(src_path) {
        if let Ok(package) = read_package(&root.join("package.json")) {
            if package.bin.is_some() || package.main.is_some() {
                let ((bins, lib), deps) = translate_project(&root, &package, project_dir)?;
                let mut cargo_toml = package.to_cargo_toml_with_bins(&bins, lib.as_deref());
                deps.apply_to_cargo_toml(&mut cargo_toml);
                fs::write(project_dir.join("Cargo.toml"), cargo_toml)?;
                apply_runtime_deps(project_dir, &deps, &bin_lib_stems(&bins, lib.as_deref()))?;
                return Ok(());
            }
        }
    }
    let mut cargo_toml = resolve_package(src_path);
    fs::create_dir_all(project_dir.join("src"))?;
    let deps = translate_sources(src, src_path, project_dir)?;
    deps.apply_to_cargo_toml(&mut cargo_toml);
    fs::write(project_dir.join("Cargo.toml"), cargo_toml)?;
    apply_runtime_deps(project_dir, &deps, &["main".to_string()])?;
    Ok(())
}

/// The crate-root file stems (`src/<stem>.rs`) that declare modules — each bin
/// entry's stem plus the lib stem, if any. The `__ds` helper is declared
/// (`mod __ds;`) at each crate root so every translated file reaches it as
/// `crate::__ds::…`.
pub(crate) fn bin_lib_stems(bins: &[(String, String)], lib: Option<&str>) -> Vec<String> {
    let mut stems: Vec<String> = bins
        .iter()
        .map(|(_, ds_path)| stem_of(Path::new(ds_path)))
        .collect();
    if let Some(lib_path) = lib {
        stems.push(stem_of(Path::new(lib_path)));
    }
    stems
}

/// Write the runtime helper modules and declare them at each crate root, when
/// the translated sources reference them: `__ds` (`ryu_js`) and `__ds_engine`
/// (the `rquickjs` compat engine). A no-op when no runtime dep is set.
pub(crate) fn apply_runtime_deps(
    project_dir: &Path,
    deps: &RuntimeDeps,
    root_stems: &[String],
) -> Result<(), Box<dyn Error>> {
    if let Some(helper) = deps.helper_module() {
        inject_helper_module(project_dir, "__ds", &helper, root_stems)?;
    }
    if let Some(engine) = deps.engine_helper_module() {
        inject_helper_module(project_dir, "__ds_engine", engine, root_stems)?;
    }
    Ok(())
}

/// Write a helper module (`__ds.rs` / `__ds_engine.rs`) to `src/` and prepend
/// `mod <name>;` to each crate root that may reference it. A root already
/// declaring the module is left untouched. Both helpers are addressed as
/// `crate::<name>::…`, so every crate root that lowered a call needing one must
/// declare it.
fn inject_helper_module(
    project_dir: &Path,
    mod_name: &str,
    source: &str,
    root_stems: &[String],
) -> Result<(), Box<dyn Error>> {
    fs::write(
        project_dir.join("src").join(format!("{mod_name}.rs")),
        source,
    )?;
    let decl = format!("mod {mod_name};");
    for stem in root_stems {
        let path = project_dir.join("src").join(format!("{stem}.rs"));
        let Ok(body) = fs::read_to_string(&path) else {
            continue;
        };
        if body.contains(&decl) {
            continue;
        }
        fs::write(&path, format!("{decl}\n{body}"))?;
    }
    Ok(())
}

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
#[derive(Clone, Debug)]
pub(crate) enum DepKind {
    Ts,
    DtsOnly,
    DtsWithJs { js_path: PathBuf },
    Js,
}

/// The kind of a resolved entry path. `.d.ts` is detected by file name:
/// `Path::extension` returns `"ts"` for `index.d.ts` (it takes the last
/// `.`-segment), so the extension alone cannot tell `.d.ts` from `.ts`. A
/// `.d.ts`/`.js` pair is a typed package — the `.js` is the implementation;
/// a lone `.d.ts` is pure types; a lone `.js` is untyped.
fn dep_kind_of(entry: &Path) -> DepKind {
    let is_dts = entry
        .file_name()
        .is_some_and(|n| n.to_string_lossy().ends_with(".d.ts"));
    if is_dts {
        return match sibling_with_ext(entry, "js") {
            Some(js_path) => DepKind::DtsWithJs { js_path },
            None => DepKind::DtsOnly,
        };
    }
    match entry.extension().and_then(|e| e.to_str()) {
        Some("js" | "mjs" | "cjs") => match sibling_with_ext(entry, "d.ts") {
            Some(_) => DepKind::DtsWithJs {
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
fn sibling_with_ext(entry: &Path, new_ext: &str) -> Option<PathBuf> {
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
/// a resolved `.js` is ESM or CommonJS, used when wiring the engine.
fn ds_resolver() -> oxc_resolver::Resolver {
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
        ..ResolveOptions::default()
    })
}

/// Resolve an import specifier (relative `./foo` or bare `pkg`) to a file path
/// and its [`DepKind`]. Delegates to `oxc_resolver` — the canonical Node
/// resolution algorithm (webpack `enhanced-resolve` port), which handles
/// `node_modules/` walk-up, `package.json` `exports`/`main`/`module`/`types`,
/// scoped packages (`@scope/pkg`), and tsconfig paths — so DashScript reuses
/// the standard resolver rather than hand-writing a subset. The `DepKind` is
/// decided from the resolved path's extension and sibling files.
pub(crate) fn resolve_local_module(
    base: &Path,
    source: &str,
) -> Result<(PathBuf, DepKind), Box<dyn Error>> {
    let resolution = ds_resolver()
        .resolve(base, source)
        .map_err(|e| format!("dashscript: import '{source}' did not resolve: {e}"))?;
    let path = resolution.into_path_buf();
    let kind = dep_kind_of(&path);
    Ok((path, kind))
}

/// Resolve the Cargo package for `src_path`: the `package.json` found walking
/// up from the file (Deno-style), otherwise a minimal package named after the
/// project (`project_name`).
pub(crate) fn resolve_package(src_path: &Path) -> String {
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
pub(crate) fn cache_project_dir(src_path: &Path) -> PathBuf {
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
pub(crate) fn find_package_root(src_path: &Path) -> Option<PathBuf> {
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
pub(crate) fn package_root() -> PathBuf {
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
pub(crate) fn collect_ts_files() -> Vec<PathBuf> {
    let root = package_root();
    let mut out = Vec::new();
    walk_ts(&root, &mut out);
    out.sort();
    out
}

/// Recursive worker for [`collect_ts_files`].
fn walk_ts(dir: &Path, out: &mut Vec<PathBuf>) {
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
        } else if path.extension().and_then(|e| e.to_str()) == Some("ts") {
            out.push(path);
        }
    }
}

/// The global fallback cache for a lone `.ts` file (no `package.json` found
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

/// The file stem of a path as an owned `String` ("main.ts" → "main").
pub(crate) fn stem_of(path: &Path) -> String {
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
pub(crate) fn project_name(src_path: &Path) -> String {
    if let Some(root) = find_package_root(src_path) {
        if let Ok(json) = fs::read_to_string(root.join("package.json")) {
            if let Ok(package) = Package::from_json(&json) {
                if !package.name.trim().is_empty() {
                    return package.name;
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
pub(crate) fn resolve_entry() -> Result<String, Box<dyn Error>> {
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
pub(crate) fn resolve_target(src_path: &Path, override_target: Option<&str>) -> String {
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
pub(crate) fn read_package(path: &Path) -> Result<Package, Box<dyn Error>> {
    let json =
        fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    Ok(Package::from_json(&json)?)
}

/// A package named after the current directory, with defaults.
pub(crate) fn default_package() -> Package {
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

    fn package_at(root: &Path) -> Package {
        read_package(&root.join("package.json")).unwrap()
    }

    #[test]
    fn translate_project_emits_per_file_bins() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(
            root,
            "package.json",
            r#"{ "name": "app", "bin": { "a": "a.ts", "b": "b.ts" } }"#,
        );
        write(root, "a.ts", "function main() { console.log(1); }");
        write(root, "b.ts", "function main() { console.log(2); }");

        let out = tmp.path().join("out");
        let ((bins, lib), _deps) = translate_project(root, &package_at(root), &out).unwrap();
        let names: Vec<&str> = bins.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"a"), "bins: {bins:?}");
        assert!(names.contains(&"b"), "bins: {bins:?}");
        assert!(lib.is_none());
        assert!(out.join("src").join("a.rs").exists(), "src/a.rs missing");
        assert!(out.join("src").join("b.rs").exists(), "src/b.rs missing");
    }

    #[test]
    fn translate_project_detects_stem_collision() {
        // MVP flattens every .ts to src/<stem>.rs; two files with the same stem
        // would clobber each other, so the translation refuses.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("sub")).unwrap();
        write(
            root,
            "package.json",
            r#"{ "name": "app", "bin": "main.ts" }"#,
        );
        write(root, "main.ts", "function main() {}");
        write(root, "dup.ts", "function helper() {}");
        write(&root.join("sub"), "dup.ts", "function other() {}");

        let out = tmp.path().join("out");
        let err = translate_project(root, &package_at(root), &out).unwrap_err();
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
            "package.json",
            r#"{ "name": "app", "bin": { "a": "a.ts", "b": "b.ts" } }"#,
        );
        write(
            root,
            "a.ts",
            "import { x } from \"./b\";\nfunction main() {}",
        );
        write(root, "b.ts", "export function x() {}\nfunction main() {}");

        let out = tmp.path().join("out");
        let err = translate_project(root, &package_at(root), &out).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("bin 'a' imports bin 'b'"), "got: {msg}");
    }

    #[test]
    fn resolve_local_module_finds_bare_stem() {
        // `./foo` resolves to `foo.ts` (extensionless, bundler-style).
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(root, "foo.ts", "export function foo() {}");
        let (resolved, _) = resolve_local_module(root, "./foo").unwrap();
        assert_eq!(resolved, root.join("foo.ts"));
    }

    #[test]
    fn resolve_local_module_honors_explicit_extension() {
        // An explicit `./foo.ts` is honored as-is.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(root, "foo.ts", "export function foo() {}");
        let (resolved, _) = resolve_local_module(root, "./foo.ts").unwrap();
        assert_eq!(resolved, root.join("foo.ts"));
    }

    #[test]
    fn resolve_local_module_falls_back_to_index_barrel() {
        // No `foo.ts` → fall back to `foo/index.ts` (bundler barrel).
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("foo")).unwrap();
        write(&root.join("foo"), "index.ts", "export function foo() {}");
        let (resolved, _) = resolve_local_module(root, "./foo").unwrap();
        assert_eq!(resolved, root.join("foo").join("index.ts"));
    }

    #[test]
    fn resolve_local_module_prefers_direct_file_over_barrel() {
        // `foo.ts` wins over `foo/index.ts` (bundler order).
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("foo")).unwrap();
        write(root, "foo.ts", "export function direct() {}");
        write(&root.join("foo"), "index.ts", "export function barrel() {}");
        let (resolved, _) = resolve_local_module(root, "./foo").unwrap();
        assert_eq!(resolved, root.join("foo.ts"));
    }

    #[test]
    fn resolve_node_module_package_root_via_main() {
        // A bare `my-pkg` resolves under `node_modules/my-pkg/`, its entry from
        // `package.json` `main`. A `.ts` entry translates.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let pkg = root.join("node_modules").join("my-pkg");
        fs::create_dir_all(&pkg).unwrap();
        write(
            &pkg,
            "package.json",
            r#"{ "name": "my-pkg", "main": "index.ts" }"#,
        );
        write(
            &pkg,
            "index.ts",
            "export function foo(): number { return 1; }",
        );
        let (resolved, _) = resolve_local_module(root, "my-pkg").unwrap();
        assert_eq!(resolved, pkg.join("index.ts"));
    }

    #[test]
    fn resolve_node_module_subpath_direct_file() {
        // A bare subpath `my-pkg/util` resolves directly to `util.ts` under the
        // package dir (no `package.json` consultation).
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let pkg = root.join("node_modules").join("my-pkg");
        fs::create_dir_all(&pkg).unwrap();
        write(&pkg, "util.ts", "export function u(): number { return 1; }");
        let (resolved, _) = resolve_local_module(root, "my-pkg/util").unwrap();
        assert_eq!(resolved, pkg.join("util.ts"));
    }

    #[test]
    fn resolve_node_module_js_entry_is_js_kind() {
        // A package whose entry is `.js` (no `.ts`/`.d.ts`) resolves to a `Js`
        // kind. Resolve no longer errors on `.js`; the translate loop transpiles
        // it first (a `.js` is JS-flavored TS) and falls back to the engine only
        // for dynamic JS the table cannot lower.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let pkg = root.join("node_modules").join("js-pkg");
        fs::create_dir_all(&pkg).unwrap();
        write(
            &pkg,
            "package.json",
            r#"{ "name": "js-pkg", "main": "index.js" }"#,
        );
        write(&pkg, "index.js", "export function x() { return 1; }");
        let (resolved, kind) = resolve_local_module(root, "js-pkg").unwrap();
        assert_eq!(resolved, pkg.join("index.js"));
        assert!(
            matches!(kind, DepKind::Js),
            "js entry should be Js kind: {kind:?}"
        );
    }

    #[test]
    fn resolve_node_module_walks_up_for_node_modules() {
        // Node resolution walks up for `node_modules/`: a package imported from
        // a nested source dir resolves to the project-root `node_modules/`.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let pkg = root.join("node_modules").join("shared");
        fs::create_dir_all(&pkg).unwrap();
        write(
            &pkg,
            "package.json",
            r#"{ "name": "shared", "main": "index.ts" }"#,
        );
        write(
            &pkg,
            "index.ts",
            "export function s(): number { return 1; }",
        );
        let nested = root.join("src").join("deep");
        fs::create_dir_all(&nested).unwrap();
        let (resolved, _) = resolve_local_module(&nested, "shared").unwrap();
        assert_eq!(resolved, pkg.join("index.ts"));
    }

    #[test]
    fn resolve_node_module_scoped_package() {
        // A scoped package `@scope/pkg` resolves under
        // `node_modules/@scope/pkg/`. The hand-written resolver split on the
        // first `/` and mishandled scopes; oxc_resolver handles them natively.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let pkg = root.join("node_modules").join("@scope").join("pkg");
        fs::create_dir_all(&pkg).unwrap();
        write(
            &pkg,
            "package.json",
            r#"{ "name": "@scope/pkg", "main": "index.ts" }"#,
        );
        write(
            &pkg,
            "index.ts",
            "export function s(): number { return 1; }",
        );
        let (resolved, _) = resolve_local_module(root, "@scope/pkg").unwrap();
        assert_eq!(resolved, pkg.join("index.ts"));
    }

    #[test]
    fn resolve_node_module_exports_field_import_condition() {
        // A modern package with an `exports` field: the `import` condition
        // points at the ESM entry, `require` at the CJS one. oxc_resolver reads
        // `exports` with the configured conditions — the hand-written resolver
        // could not (it only scanned `main`/`module`/`types`).
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let pkg = root.join("node_modules").join("mod-pkg");
        fs::create_dir_all(&pkg).unwrap();
        write(
            &pkg,
            "package.json",
            r#"{ "name": "mod-pkg", "exports": { ".": { "import": "./esm.mjs", "require": "./cjs.cjs" } } }"#,
        );
        write(&pkg, "esm.mjs", "export function x() { return 1; }");
        write(&pkg, "cjs.cjs", "module.exports = {};");
        let (resolved, kind) = resolve_local_module(root, "mod-pkg").unwrap();
        // The `import` condition wins (configured `condition_names`), so the
        // ESM entry resolves. `.mjs` is a `Js` kind — transpiled first (ESM
        // `export function` lowers like a `.ts` module); the CJS `.cjs` arm is
        // reached only under a `require` condition.
        assert_eq!(resolved, pkg.join("esm.mjs"));
        assert!(
            matches!(kind, DepKind::Js),
            ".mjs entry should be Js kind: {kind:?}"
        );
    }

    #[test]
    fn stem_of_index_in_subdir_is_parent_dir() {
        // foo/index.ts is a barrel → module name "foo" (the parent dir).
        assert_eq!(stem_of(Path::new("foo/index.ts")), "foo");
    }

    #[test]
    fn stem_of_root_index_keeps_own_stem() {
        // A root index.ts is an entry, not a barrel — keeps stem "index".
        assert_eq!(stem_of(Path::new("index.ts")), "index");
    }

    #[test]
    fn translate_project_emits_barrel_module() {
        // entry imports "./foo" → foo/index.ts → src/foo.rs (project mode),
        // so `mod foo;` resolves. The barrel must not emit src/index.rs.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("foo")).unwrap();
        write(
            root,
            "package.json",
            r#"{ "name": "app", "bin": "main.ts" }"#,
        );
        write(
            root,
            "main.ts",
            "import { foo } from \"./foo\";\nfunction main() { foo(); }",
        );
        write(&root.join("foo"), "index.ts", "export function foo() {}");
        let out = tmp.path().join("out");
        translate_project(root, &package_at(root), &out).unwrap();
        assert!(
            out.join("src").join("foo.rs").exists(),
            "barrel src/foo.rs missing"
        );
        assert!(
            !out.join("src").join("index.rs").exists(),
            "barrel should not emit src/index.rs"
        );
    }

    #[test]
    fn translate_project_detects_circular_import() {
        // a → b → a: Rust forbids circular modules; the guard surfaces the cycle
        // with the files involved instead of letting cargo report a vague error.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(
            root,
            "package.json",
            r#"{ "name": "app", "bin": "main.ts" }"#,
        );
        write(
            root,
            "main.ts",
            "import { a } from \"./a\";\nfunction main() { a(); }",
        );
        write(
            root,
            "a.ts",
            "import { b } from \"./b\";\nexport function a() { b(); }",
        );
        write(
            root,
            "b.ts",
            "import { a } from \"./a\";\nexport function b() { a(); }",
        );
        let out = tmp.path().join("out");
        let err = translate_project(root, &package_at(root), &out).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("circular import"), "got: {msg}");
        assert!(msg.contains("a.ts"), "got: {msg}");
        assert!(msg.contains("b.ts"), "got: {msg}");
    }

    #[test]
    fn translate_project_module_file_has_no_main() {
        // 文件角色（架构决策点 8）：bin entry 收顶层语句进 `fn main`；被 import
        // 的 module 文件只声明、不执行 → 无 `fn main`（crate 内模块，由 entry 经
        // `mod` 引入）。一个 bin import 一个辅助 module：
        //   main.ts (bin)    → src/main.rs 有 `fn main`
        //   util.ts  (module) → src/util.rs  无 `fn main`
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(
            root,
            "package.json",
            r#"{ "name": "app", "bin": "main.ts" }"#,
        );
        write(
            root,
            "main.ts",
            "import { helper } from \"./util\";\nfunction main(): void { helper(); }",
        );
        write(root, "util.ts", "export function helper(): void {}");
        let out = tmp.path().join("out");
        translate_project(root, &package_at(root), &out).unwrap();
        let main_rs = fs::read_to_string(out.join("src").join("main.rs")).unwrap();
        let util_rs = fs::read_to_string(out.join("src").join("util.rs")).unwrap();
        assert!(
            main_rs.contains("fn main"),
            "bin entry missing fn main: {main_rs}"
        );
        assert!(
            !util_rs.contains("fn main"),
            "module file should not have fn main: {util_rs}"
        );
    }
}
