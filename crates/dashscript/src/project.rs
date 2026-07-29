//! Project-level packaging: translate a package's `.ts` into one multi-target
//! Rust crate — source/module resolution, Cargo project emission, path & cache
//! discovery, import-cycle guards, and cargo invocation. The `ds` subcommands
//! (`build`, `run`, `deps`, `check`, `lsp`) are thin callers over this.

use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::{Command, ExitCode, ExitStatus},
};

use crate::{FileRole, Package, RuntimeDeps, Translator};

/// Translate one resolved dependency to the Rust source for `src/<module>.rs`,
/// merging its runtime deps into `deps`. The `DepKind` picks the path: a `.ts`
/// or untyped `.js` dep is transpiled as a Rust module (transpile-first); a
/// pure `.d.ts` yields its `interface`/`type` items; a `.d.ts` + `.js` pair
/// yields the `.d.ts` types plus the transpiled `.js` (the `.d.ts`'s value
/// signatures are not yet injected into the `.js` — a non-numeric param stays
/// `f64` and fails `cargo check` honestly).
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
        DepKind::DtsWithJs { dts_path, js_path } => {
            // A typed package: the `.d.ts` carries `interface`/`type`
            // declarations (and the value signatures we do not yet inject);
            // the `.js` is the implementation. Transpile the `.js` (batch C
            // path — untyped params default to `f64`) and prepend the `.d.ts`'s
            // type items so cross-module type imports resolve. `declare
            // function` emits nothing — the `.js` provides the implementation,
            // and signature type injection is a later enhancement.
            let dts_src = fs::read_to_string(&dts_path)
                .map_err(|e| format!("cannot read import {}: {e}", dts_path.display()))?;
            let js_src = fs::read_to_string(&js_path)
                .map_err(|e| format!("cannot read import {}: {e}", js_path.display()))?;
            let dts_items = translator.translate_dts(&dts_src);
            let (js_rust, dep_deps) = translator
                .translate_with_deps_as(&js_src, FileRole::Module)
                .map_err(|e| format!("translate {}: {e}", js_path.display()))?;
            deps.merge(&dep_deps);
            if dts_items.trim().is_empty() {
                Ok(js_rust)
            } else {
                Ok(format!("{dts_items}\n{js_rust}"))
            }
        }
    }
}

/// Walk the import graph from `src` and aggregate every `.ts`/`.d.ts` file's
/// optional (`?:`) field names, so each file sees imported interfaces'
/// optionals — a cross-file `opts?.field ?? d` needs to know `field` is
/// optional, but each file builds its own `TypeRegistry`. Pure-`.js` deps (no
/// type annotations) are skipped. This is the cross-file half of each file's
/// per-file registry; the union is injected via `with_extra_optionals`.
fn collect_package_optionals(
    src: &str,
    src_path: &Path,
) -> Result<std::collections::HashMap<String, std::collections::HashSet<String>>, Box<dyn Error>> {
    use std::collections::{HashMap, HashSet, VecDeque};
    let collector = Translator::new();
    let mut shared: HashMap<String, HashSet<String>> = collector
        .collect_optionals(src)
        .map_err(|e| format!("collect optionals {}: {e}", src_path.display()))?;
    let base = src_path.parent().unwrap_or_else(|| Path::new(""));
    let mut seen: HashSet<String> = HashSet::new();
    let mut worklist: VecDeque<(String, PathBuf)> = VecDeque::new();
    for imp in collector.imports(src) {
        if seen.insert(imp.module.clone()) {
            let (dep_path, kind) = resolve_local_module(base, &imp.source)?;
            if !matches!(kind, DepKind::Js) {
                worklist.push_back((imp.module, dep_path));
            }
        }
    }
    while let Some((_module, path)) = worklist.pop_front() {
        let dep_src = fs::read_to_string(&path)
            .map_err(|e| format!("cannot read import {}: {e}", path.display()))?;
        for (k, v) in collector
            .collect_optionals(&dep_src)
            .map_err(|e| format!("collect optionals {}: {e}", path.display()))?
        {
            shared.entry(k).or_insert_with(|| v);
        }
        let dep_base = path.parent().unwrap_or_else(|| Path::new(""));
        for imp in collector.imports(&dep_src) {
            if seen.insert(imp.module.clone()) {
                let (dep_path, kind) = resolve_local_module(dep_base, &imp.source)?;
                if !matches!(kind, DepKind::Js) {
                    worklist.push_back((imp.module, dep_path));
                }
            }
        }
    }
    Ok(shared)
}

/// Walk the import graph from `src` and aggregate every `.ts`/`.d.ts` file's
/// interface field signatures (name, translated type, optional flag), so each
/// file sees imported interfaces' field types — a cross-file unwrap
/// (`f(obj.opt_field)` into the field's inner type) needs the field's type,
/// but each file builds its own `TypeRegistry`. The field-type analogue of
/// [`collect_package_optionals`]; the union is injected via `with_extra_fields`.
fn collect_package_fields(
    src: &str,
    src_path: &Path,
) -> Result<std::collections::HashMap<String, Vec<crate::translator::InterfaceField>>, Box<dyn Error>>
{
    use std::collections::{HashMap, HashSet, VecDeque};
    let collector = Translator::new();
    let mut shared: HashMap<String, Vec<crate::translator::InterfaceField>> = collector
        .collect_fields(src)
        .map_err(|e| format!("collect fields {}: {e}", src_path.display()))?;
    let base = src_path.parent().unwrap_or_else(|| Path::new(""));
    let mut seen: HashSet<String> = HashSet::new();
    let mut worklist: VecDeque<(String, PathBuf)> = VecDeque::new();
    for imp in collector.imports(src) {
        if seen.insert(imp.module.clone()) {
            let (dep_path, kind) = resolve_local_module(base, &imp.source)?;
            if !matches!(kind, DepKind::Js) {
                worklist.push_back((imp.module, dep_path));
            }
        }
    }
    while let Some((_module, path)) = worklist.pop_front() {
        let dep_src = fs::read_to_string(&path)
            .map_err(|e| format!("cannot read import {}: {e}", path.display()))?;
        for (k, v) in collector
            .collect_fields(&dep_src)
            .map_err(|e| format!("collect fields {}: {e}", path.display()))?
        {
            shared.entry(k).or_insert_with(|| v);
        }
        let dep_base = path.parent().unwrap_or_else(|| Path::new(""));
        for imp in collector.imports(&dep_src) {
            if seen.insert(imp.module.clone()) {
                let (dep_path, kind) = resolve_local_module(dep_base, &imp.source)?;
                if !matches!(kind, DepKind::Js) {
                    worklist.push_back((imp.module, dep_path));
                }
            }
        }
    }
    Ok(shared)
}

/// Walk the import graph from `src` and aggregate every `.ts`/`.d.ts` file's
/// inline scalar-union enums (`__DsUnion…`), so each file recognizes imported
/// interfaces' union-typed fields — a cross-file `return element.text` (a
/// union) into a `String` coerces via the union's `Display` impl, but each file
/// builds its own `TypeRegistry`. The union-enum analogue of
/// [`collect_package_fields`]; the union is injected via
/// `with_extra_union_enums`.
fn collect_package_union_enums(
    src: &str,
    src_path: &Path,
) -> Result<std::collections::HashMap<syn::Ident, syn::ItemEnum>, Box<dyn Error>> {
    use std::collections::{HashMap, HashSet, VecDeque};
    let collector = Translator::new();
    let mut shared: HashMap<syn::Ident, syn::ItemEnum> = collector
        .collect_union_enums(src)
        .map_err(|e| format!("collect unions {}: {e}", src_path.display()))?;
    let base = src_path.parent().unwrap_or_else(|| Path::new(""));
    let mut seen: HashSet<String> = HashSet::new();
    let mut worklist: VecDeque<(String, PathBuf)> = VecDeque::new();
    for imp in collector.imports(src) {
        if seen.insert(imp.module.clone()) {
            let (dep_path, kind) = resolve_local_module(base, &imp.source)?;
            if !matches!(kind, DepKind::Js) {
                worklist.push_back((imp.module, dep_path));
            }
        }
    }
    while let Some((_module, path)) = worklist.pop_front() {
        let dep_src = fs::read_to_string(&path)
            .map_err(|e| format!("cannot read import {}: {e}", path.display()))?;
        for (k, v) in collector
            .collect_union_enums(&dep_src)
            .map_err(|e| format!("collect unions {}: {e}", path.display()))?
        {
            shared.entry(k).or_insert_with(|| v);
        }
        let dep_base = path.parent().unwrap_or_else(|| Path::new(""));
        for imp in collector.imports(&dep_src) {
            if seen.insert(imp.module.clone()) {
                let (dep_path, kind) = resolve_local_module(dep_base, &imp.source)?;
                if !matches!(kind, DepKind::Js) {
                    worklist.push_back((imp.module, dep_path));
                }
            }
        }
    }
    Ok(shared)
}

/// Translate `src` and write `src/main.rs` (plus each imported local module as
/// `src/<module>.rs`, declared with a leading `mod <module>;`) into
/// `project_dir/src/`. The caller writes `Cargo.toml`. Shared by a single-
/// package build ([`emit_cargo_project`]) and by workspace members (whose
/// Cargo.toml the workspace root owns). Imports are followed transitively: an
/// imported module that itself imports is lowered too (deduped by module name),
/// so a multi-file package lowers fully rather than stopping at the first hop.
pub fn translate_sources(
    src: &str,
    src_path: &Path,
    project_dir: &Path,
) -> Result<RuntimeDeps, Box<dyn Error>> {
    // Probe whether any reachable file (entry + recursive imports) degrades to
    // the engine, then hold the project-wide serde-derive flag for the whole
    // translate. A degraded function marshals cross-file types through
    // `serde_json::Value`, so every type it touches needs `Serialize`/
    // `Deserialize` — including the union enums hoisted to the crate root below.
    let probe = Translator::new();
    let project_uses_engine = probe_sources_use_engine(
        &probe,
        src,
        src_path.parent().unwrap_or_else(|| Path::new("")),
    );
    Translator::set_force_serde_derive(project_uses_engine);
    // Reset the (thread-local) flag when this translate ends — success or
    // error — so it does not leak into a later `translate` call.
    struct SerdeGuard;
    impl Drop for SerdeGuard {
        fn drop(&mut self) {
            Translator::set_force_serde_derive(false);
        }
    }
    let _serde_guard = SerdeGuard;

    // Aggregate optional (`?:`) field names across the whole import graph
    // first, so every file — the entry and each dep — sees imported
    // interfaces' optionals. A cross-file `opts?.field ?? d` needs to know
    // `field` is optional, but each file builds its own `TypeRegistry`.
    let shared_optionals = collect_package_optionals(src, src_path)?;
    let shared_fields = collect_package_fields(src, src_path)?;
    let shared_unions = collect_package_union_enums(src, src_path)?;
    let translator = Translator::new()
        .with_extra_optionals(shared_optionals)
        .with_extra_fields(shared_fields)
        .with_extra_union_enums(shared_unions);
    let (rust, mut deps) = translator
        .translate_with_deps(src)
        .map_err(|e| format!("translate {}: {e}", src_path.display()))?;

    let base = src_path.parent().unwrap_or_else(|| Path::new(""));
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut mod_decls = String::new();
    // The inline `__DsUnion…` enums the entry already emits at the crate root
    // (it lowers as `BinEntry`). A dependency may name a union the entry never
    // uses directly — its module references `crate::__DsUnion…`, but no
    // definition reaches the crate root. So each dep's unions are collected
    // too, and any the entry did not already emit are prepended to `main.rs`.
    let mut emitted_enums: std::collections::HashSet<String> = translator
        .union_enum_items(src)
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    let mut extra_enum_text = String::new();
    // Worklist of `(module, source_spec, base_dir)` — each popped dep is
    // translated once, then its own imports extend the worklist. A cycle
    // (a.ts ↔ b.ts) terminates: `seen` dedupes by module name, so the second
    // edge to an already-translated module is a no-op.
    let mut worklist: std::collections::VecDeque<(String, String, PathBuf)> =
        std::collections::VecDeque::new();
    for imp in translator.imports(src) {
        if seen.insert(imp.module.clone()) {
            worklist.push_back((imp.module, imp.source, base.to_path_buf()));
        }
    }
    while let Some((module, source, dir)) = worklist.pop_front() {
        let (dep_path, kind) = resolve_local_module(&dir, &source)?;
        let dep_rust = translate_dep(&translator, &dep_path, kind, &mut deps)?;
        fs::write(
            project_dir.join("src").join(format!("{module}.rs")),
            dep_rust,
        )?;
        mod_decls.push_str(&format!("mod {module};\n"));
        let dep_src = fs::read_to_string(&dep_path)
            .map_err(|e| format!("cannot read import {}: {e}", dep_path.display()))?;
        for (name, text) in translator.union_enum_items(&dep_src) {
            if emitted_enums.insert(name) {
                extra_enum_text.push_str(&text);
                extra_enum_text.push('\n');
            }
        }
        let dep_base = dep_path.parent().unwrap_or_else(|| Path::new(""));
        for imp in translator.imports(&dep_src) {
            if seen.insert(imp.module.clone()) {
                worklist.push_back((imp.module, imp.source, dep_base.to_path_buf()));
            }
        }
    }

    let main = if mod_decls.is_empty() && extra_enum_text.is_empty() {
        rust
    } else {
        format!("{extra_enum_text}{mod_decls}\n{rust}")
    };
    fs::write(project_dir.join("src").join("main.rs"), main)?;
    Ok(deps)
}

/// Probe whether any file reachable from `src` (itself + its recursive imports)
/// degrades to the engine — mirrors [`translate_sources`]'s own import-graph
/// walk so the probe covers exactly the files that will be translated. Returns
/// true so the caller can set the project-wide serde-derive flag.
fn probe_sources_use_engine(probe: &Translator, src: &str, base: &Path) -> bool {
    if probe.uses_engine(src) {
        return true;
    }
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut worklist: std::collections::VecDeque<(String, PathBuf)> =
        std::collections::VecDeque::new();
    for imp in probe.imports(src) {
        if seen.insert(imp.module.clone()) {
            worklist.push_back((imp.source, base.to_path_buf()));
        }
    }
    while let Some((source, dir)) = worklist.pop_front() {
        let Ok((dep_path, _)) = resolve_local_module(&dir, &source) else {
            continue;
        };
        let Ok(dep_src) = fs::read_to_string(&dep_path) else {
            continue;
        };
        if probe.uses_engine(&dep_src) {
            return true;
        }
        let dep_base = dep_path.parent().unwrap_or_else(|| Path::new(""));
        for imp in probe.imports(&dep_src) {
            if seen.insert(imp.module.clone()) {
                worklist.push_back((imp.source, dep_base.to_path_buf()));
            }
        }
    }
    false
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
pub fn translate_project(
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
    // File role (arch decision point 8): a bin/lib entry collects top-level
    // executable statements into `fn main`; every other file (an imported
    // module) only declares, never executes — the `Module` role errors on its
    // top-level executable statements (module semantics). Compared by canonical
    // path so the call/import form or a relative/absolute `bin` spelling does
    // not affect the decision.
    let entry_paths: std::collections::HashSet<PathBuf> = bins
        .iter()
        .filter_map(|(_, p)| root.join(p).canonicalize().ok())
        .chain(lib.as_ref().and_then(|p| root.join(p).canonicalize().ok()))
        .collect();

    let mut files = Vec::new();
    walk_ts(root, &mut files);
    files.sort();

    // Probe whether any file degrades to the engine. A degraded function
    // marshals its arguments as `serde_json::Value`, which needs
    // `Serialize`/`Deserialize` on every type crossing the boundary — including
    // types a non-degraded file defines (a degraded function in another file
    // may take or return them). If any file degrades, every file derives serde;
    // set the project-level flag once here, read during each translate below.
    let probe = Translator::new();
    let project_uses_engine = files.iter().any(|ds| {
        fs::read_to_string(ds)
            .map(|src| probe.uses_engine(&src))
            .unwrap_or(false)
    });
    Translator::set_force_serde_derive(project_uses_engine);
    // Reset the (thread-local) flag when this project translate ends — success
    // or error — so it does not leak into a later `translate` call.
    struct SerdeGuard;
    impl Drop for SerdeGuard {
        fn drop(&mut self) {
            Translator::set_force_serde_derive(false);
        }
    }
    let _serde_guard = SerdeGuard;

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
pub fn emit_cargo_project(
    src: &str,
    src_path: &Path,
    project_dir: &Path,
) -> Result<(), Box<dyn Error>> {
    if let Some(root) = find_package_root(src_path) {
        if let Ok(package) = read_package(&root.join("package.json")) {
            // Project mode (directory walk) needs a DashScript source entry — a
            // `bin`/`main` pointing at a `.ts` file. A package whose entries
            // are JS build artifacts (e.g. `main: "dist/index.mjs"`) has no
            // source entry to anchor the walk, so it falls back to lone-file
            // mode below (the caller's `src_path` + its import graph).
            let has_ts_entry = package.bin_entries().iter().any(|(_, p)| {
                root.join(p)
                    .extension()
                    .is_some_and(|e| e.to_str() == Some("ts"))
            }) || package.main.as_ref().is_some_and(|p| {
                root.join(p)
                    .extension()
                    .is_some_and(|e| e.to_str() == Some("ts"))
            });
            if has_ts_entry {
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
pub fn bin_lib_stems(bins: &[(String, String)], lib: Option<&str>) -> Vec<String> {
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
pub fn apply_runtime_deps(
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
fn dep_kind_of(entry: &Path) -> DepKind {
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
fn resolve_workspace_dep(base: &Path, source: &str) -> Option<(PathBuf, DepKind)> {
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
fn split_package_spec(source: &str) -> Option<(String, Option<String>)> {
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
fn resolve_local_src(pkg_dir: &Path, subpath: Option<&str>) -> Option<PathBuf> {
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
    let resolution = ds_resolver().resolve(base, source).map_err(|e| {
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

    /// Create a directory symlink, cross-platform. Used by the workspace-dep
    /// tests to model a pnpm/npm `node_modules/<pkg>` entry pointing at a
    /// sibling package. Returns an error where symlinks are unsupported or
    /// (Windows) without developer mode / admin — the caller treats that as a
    /// skip, not a failure.
    fn make_symlink(original: &Path, link: &Path) -> std::io::Result<()> {
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(original, link)
        }
        #[cfg(windows)]
        {
            std::os::windows::fs::symlink_dir(original, link)
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = (original, link);
            Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "symlinks unsupported on this platform",
            ))
        }
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
    fn resolve_workspace_dep_follows_symlink_to_src() {
        // A workspace-local package: `node_modules/@scope/b` symlinks to a
        // sibling package dir; its `src/` source is resolved (bypassing
        // `exports`/`main`, which would point at an unbuilt `dist/`).
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let pkg_b = root.join("packages").join("b");
        fs::create_dir_all(pkg_b.join("src").join("sub")).unwrap();
        write(&pkg_b.join("src"), "index.ts", "export function b() {}");
        write(
            &pkg_b.join("src").join("sub"),
            "index.ts",
            "export function sub() {}",
        );
        let scope_nm = root.join("node_modules").join("@scope");
        fs::create_dir_all(&scope_nm).unwrap();
        if make_symlink(&pkg_b, &scope_nm.join("b")).is_err() {
            eprintln!(
                "resolve_workspace_dep_follows_symlink_to_src: skipped (no symlink privilege)"
            );
            return;
        }
        // Resolve from a sibling package `packages/a/src` — the resolver walks
        // up to the shared `node_modules`.
        let base = root.join("packages").join("a").join("src");
        fs::create_dir_all(&base).unwrap();
        let (resolved, kind) = resolve_workspace_dep(&base, "@scope/b").expect("local pkg");
        assert!(matches!(kind, DepKind::Ts), "got: {kind:?}");
        assert_eq!(
            fs::canonicalize(&resolved).unwrap(),
            fs::canonicalize(pkg_b.join("src").join("index.ts")).unwrap()
        );
        // Subpath → `src/sub/index.ts` (directory barrel).
        let (resolved, _) = resolve_workspace_dep(&base, "@scope/b/sub").expect("subpath");
        assert_eq!(
            fs::canonicalize(&resolved).unwrap(),
            fs::canonicalize(pkg_b.join("src").join("sub").join("index.ts")).unwrap()
        );
        // Also reachable through the public `resolve_local_module`.
        let (resolved, _) = resolve_local_module(&base, "@scope/b").unwrap();
        assert_eq!(
            fs::canonicalize(&resolved).unwrap(),
            fs::canonicalize(pkg_b.join("src").join("index.ts")).unwrap()
        );
    }

    #[test]
    fn resolve_workspace_dep_ignores_pnpm_store() {
        // A pnpm-store package is also a symlink, but its target is under
        // `node_modules/.pnpm/` — it must NOT be treated as a local source
        // package (it is a registry package; left to the standard resolver).
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let store_pkg = root
            .join("node_modules")
            .join(".pnpm")
            .join("reg@1.0.0")
            .join("node_modules")
            .join("reg");
        fs::create_dir_all(store_pkg.join("src")).unwrap();
        write(&store_pkg.join("src"), "index.ts", "export function r() {}");
        let nm = root.join("node_modules").join("reg");
        if make_symlink(&store_pkg, &nm).is_err() {
            eprintln!("resolve_workspace_dep_ignores_pnpm_store: skipped (no symlink privilege)");
            return;
        }
        let base = root.join("packages").join("a").join("src");
        fs::create_dir_all(&base).unwrap();
        assert!(
            resolve_workspace_dep(&base, "reg").is_none(),
            "a .pnpm-store symlink must not be treated as a local source package"
        );
    }

    #[test]
    fn resolve_node_module_dts_with_js_pair() {
        // A package with both `.d.ts` and `.js` (a typed npm package) resolves
        // to `DtsWithJs` — the `.d.ts` entry (the `types` field wins over
        // `main`) carries its sibling `.js` as the implementation path. The
        // translate loop then emits the `.d.ts` types alongside the transpiled
        // `.js`.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let pkg = root.join("node_modules").join("typed-pkg");
        fs::create_dir_all(&pkg).unwrap();
        write(
            &pkg,
            "package.json",
            r#"{ "name": "typed-pkg", "main": "index.js", "types": "index.d.ts" }"#,
        );
        write(
            &pkg,
            "index.d.ts",
            "export declare function f(a: number): number;",
        );
        write(&pkg, "index.js", "export function f(a) { return a; }");
        let (resolved, kind) = resolve_local_module(root, "typed-pkg").unwrap();
        assert_eq!(resolved, pkg.join("index.d.ts"));
        match kind {
            DepKind::DtsWithJs { dts_path, js_path } => {
                assert_eq!(dts_path, pkg.join("index.d.ts"));
                assert_eq!(js_path, pkg.join("index.js"));
            }
            other => panic!("expected DtsWithJs, got {other:?}"),
        }
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
        // File role (arch decision point 8): a bin entry collects top-level
        // statements into `fn main`; an imported module file only declares,
        // never executes → no `fn main` (a crate-internal module, brought in
        // by the entry via `mod`). A bin importing one helper module:
        //   main.ts (bin)    → src/main.rs has `fn main`
        //   util.ts  (module) → src/util.rs  has no `fn main`
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
