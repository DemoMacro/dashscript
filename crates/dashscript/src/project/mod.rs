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

mod collect;
mod cycle;
mod dep;
mod emit;
mod resolve;

pub(crate) use self::{collect::*, cycle::*, dep::*};
pub use self::{emit::*, resolve::*};

#[cfg(test)]
mod tests;

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

    // Bare workspace-member specifiers (`@scope/core`) resolve (via a
    // node_modules symlink) to a local `src/`, so `ds build` translates them
    // into a `mod` of this crate. Collect the import graph's such specifiers
    // before the entry translates, so the entry and each recursive dep lower
    // their `@scope/…` imports to `crate::mod` — not a bare `mod`, which Rust
    // 2018 path clarity rejects from a submodule. Cleared on translate end.
    //
    // Routing note (office-open migration phases A–D): `ds build` now routes
    // any file inside a workspace member to workspace_build → translate_project,
    // which turns cross-member specifiers into cargo path deps (independent
    // crates). So this merge path is reached only for lone files / packages
    // with no `.ts` entry — it carries no workspace context in the common case,
    // and the member-prefix machinery below (dep_mod_name's member branch,
    // CURRENT_MEMBER) is dormant on live builds. Retained for the
    // translate_sources unit tests and the rare non-member file that imports
    // one; a full removal was evaluated and deferred — dead-code hygiene that
    // is not worth risking the 800+ test foundation.
    let workspace_deps =
        collect_workspace_deps(src, src_path.parent().unwrap_or_else(|| Path::new("")));
    crate::translator::imports::set_workspace_deps(workspace_deps);
    struct WorkspaceDepsGuard;
    impl Drop for WorkspaceDepsGuard {
        fn drop(&mut self) {
            crate::translator::imports::clear_workspace_deps();
        }
    }
    let _ws_guard = WorkspaceDepsGuard;

    // Aggregate optional (`?:`) field names across the whole import graph
    // first, so every file — the entry and each dep — sees imported
    // interfaces' optionals. A cross-file `opts?.field ?? d` needs to know
    // `field` is optional, but each file builds its own `TypeRegistry`.
    let shared_optionals = collect_package_optionals(src, src_path)?;
    let shared_fields = collect_package_fields(src, src_path)?;
    let shared_unions = collect_package_union_enums(src, src_path)?;
    let shared_signatures = collect_package_function_signatures(src, src_path)?;
    let shared_lazy_statics = collect_package_lazy_statics(src, src_path)?;
    let translator = Translator::new()
        .with_extra_optionals(shared_optionals)
        .with_extra_fields(shared_fields)
        .with_extra_union_enums(shared_unions)
        .with_extra_function_signatures(shared_signatures);
    // Publish the import graph's lazy-static exports so every consumer file
    // recognizes an imported lazy static (accessor-name `use`, accessor-call
    // reference, `HashMap` index). Cleared on translate end (the guard) so it
    // does not leak into a later translate.
    crate::translator::imports::set_lazy_static_exports(shared_lazy_statics);
    struct LazyStaticGuard;
    impl Drop for LazyStaticGuard {
        fn drop(&mut self) {
            crate::translator::imports::clear_lazy_static_exports();
        }
    }
    let _lazy_static_guard = LazyStaticGuard;
    let (rust, mut deps) = translator
        .translate_with_deps(src)
        .map_err(|e| format!("translate {}: {e}", src_path.display()))?;

    let base = src_path.parent().unwrap_or_else(|| Path::new(""));
    // Pre-walk the import graph to assign each reachable file a canonical emit
    // name *before* any dep translates. A barrel (`locking/index.ts`) and a
    // same-stem definition file (`locking/locking.ts`) both flatten to
    // `src/locking.rs` if deduped by the specifier-derived mod name
    // (`dep_mod_name("./locking", …) == "locking"` for both, since the
    // specifier carries no resolution context). Deduping by canonical file
    // path instead keeps both files in the worklist, and the colliding defn is
    // suffixed (`locking__ds_defn`) so the barrel keeps the bare name (its
    // re-exports of *other* modules — `export * from "./connection"` — must
    // stay reachable as `crate::data_model::*`). The barrel's self re-export
    // (`pub use crate::locking::X` where X lives in the defn) is rerouted to
    // `crate::locking__ds_defn::X` via per-file emit-name overrides set around
    // each dep translate. Flatten collisions (`chart/types.ts`,
    // `descriptor/types.ts`, `patch/types.ts` all flatten to `types.rs`) are
    // resolved the same way: the first to claim a name keeps it, later
    // arrivals take the suffix. See [`compute_emit_name_map`].
    let entry_spec = src_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    let emit_name_map = compute_emit_name_map(&translator, src, base)?;
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    // Per-tier emit state: local relative deps land under app/ (flat, single
    // segment, declared in a synthesized app/mod.rs); bare npm deps land under
    // third_party/<segment-path> (preserving the specifier tree) and go through
    // emit_tree, which synthesizes third_party/mod.rs + every interior mod.rs.
    let mut app_mods = String::new();
    let mut tp_files: Vec<EmitFile> = Vec::new();
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
    // Worklist of `(module, source_spec, base_dir, specifier, member)` — each
    // popped dep is translated once, then its own imports extend the worklist.
    // A cycle (a.ts ↔ b.ts) terminates: `seen` dedupes by the dep's canonical
    // file path (so a barrel + defn pair, distinct files, both translate).
    // `specifier` is the dep's DsResolver specifier (bare verbatim, relative
    // joined onto the importer's), so a degraded `.js` registers under the key
    // the runtime resolver finds it under. `member` is the workspace member the
    // dep lives in (`Some("member_crate")` for a file reached through a
    // workspace-member barrel; `None` for the entry's own package): the emit
    // filename and `mod` decl use [`dep_mod_name`] so a relative import inside a
    // member carries the member prefix and does not collide with a same-stem
    // file in another package. The entry's own specifier is its file stem.
    let mut worklist: std::collections::VecDeque<(
        String,
        String,
        PathBuf,
        String,
        Option<String>,
    )> = std::collections::VecDeque::new();
    for imp in translator.imports(src) {
        let member = workspace_member_crate(base, &imp.source);
        let spec = ds_resolve_specifier(&entry_spec, &imp.source);
        let canon = canon_key(
            base,
            &imp.source,
            &dep_mod_name(&imp.source, &imp.module, &member),
        )?;
        if seen.insert(canon) {
            worklist.push_back((imp.module, imp.source, base.to_path_buf(), spec, member));
        }
    }
    while let Some((module, source, dir, spec, member)) = worklist.pop_front() {
        let (dep_path, kind) = resolve_local_module(&dir, &source)?;
        let canon = canon_string(&dep_path);
        // Per-file emit-name overrides: each of this dep's import specifiers,
        // resolved to its canonical path, looks up the emit name that file
        // will be emitted under. A specifier that lands on a suffixed defn
        // (`./locking` → `locking__ds_defn`) is rerouted in the translator's
        // `mod_use_path` so the barrel's self re-export points at the defn,
        // not itself.
        let override_map = collect_emit_overrides(&translator, &dep_path, &emit_name_map);
        crate::translator::imports::set_emit_name_overrides(override_map);
        struct OverrideGuard;
        impl Drop for OverrideGuard {
            fn drop(&mut self) {
                crate::translator::imports::clear_emit_name_overrides();
            }
        }
        let _override_guard = OverrideGuard;
        let dep_rust = translate_dep(
            &translator,
            &dep_path,
            kind,
            &spec,
            member.clone(),
            &mut deps,
        )?;
        // The emit name this dep's `mod` decl and filename take. Collisions
        // (barrel + defn, or three same-stem `types.ts` files flattening)
        // resolve via the pre-walked map; the common no-collision case keeps
        // the specifier-derived name.
        let emit_name = emit_name_map
            .get(&canon)
            .cloned()
            .unwrap_or_else(|| dep_mod_name(&source, &module, &member));
        // Local relative deps go under app/ (flat single segment); bare npm
        // deps go under third_party/<segment-path> via emit_tree (which also
        // synthesizes the mod.rs chain). Workspace members are path deps,
        // already skipped above.
        let is_local = source.starts_with('.');
        // A degraded module with no `export function` yields an empty body;
        // its source is still registered in `__DS_MODULE_SOURCES`, but no
        // `.rs` stub or `mod` declaration is emitted for it.
        if !dep_rust.is_empty() {
            if is_local {
                let app_dir = project_dir.join("src").join("app");
                fs::create_dir_all(&app_dir)?;
                fs::write(
                    app_dir.join(format!("{}.rs", mod_file_stem(&emit_name))),
                    dep_rust,
                )?;
                app_mods.push_str(&format!("pub mod {emit_name};\n"));
            } else {
                tp_files.push(EmitFile {
                    rel_path: format!("third_party/{emit_name}"),
                    content: dep_rust,
                    is_barrel: false,
                    is_root_entry: false,
                });
            }
        }
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
            let child_member = workspace_member_crate(dep_base, &imp.source).or(member.clone());
            let child_spec = ds_resolve_specifier(&spec, &imp.source);
            let canon = canon_key(
                dep_base,
                &imp.source,
                &dep_mod_name(&imp.source, &imp.module, &child_member),
            )?;
            if seen.insert(canon) {
                worklist.push_back((
                    imp.module,
                    imp.source,
                    dep_base.to_path_buf(),
                    child_spec,
                    child_member,
                ));
            }
        }
    }

    // Each tier's deps are declared in a synthesized mod.rs; the entry's crate
    // root declares the tiers it uses (plus any hoisted union enums). app/ is
    // flat (one mod.rs of `mod <seg>;` lines); third_party/ preserves the
    // specifier tree, so emit_tree writes its files and synthesizes the whole
    // mod.rs chain (third_party/mod.rs + every interior directory).
    let mut root_decls = String::new();
    if !app_mods.is_empty() {
        let app_dir = project_dir.join("src").join("app");
        fs::create_dir_all(&app_dir)?;
        fs::write(app_dir.join("mod.rs"), &app_mods)?;
        root_decls.push_str("mod app;\n");
    }
    if !tp_files.is_empty() {
        emit_tree(&project_dir.join("src"), &tp_files)?;
        root_decls.push_str("mod third_party;\n");
    }
    let main = if root_decls.is_empty() && extra_enum_text.is_empty() {
        rust
    } else {
        format!("{extra_enum_text}{root_decls}\n{rust}")
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

/// Whether `src` directly imports an **npm** `.js` module that degrades to the
/// engine. This is the B6-5c trigger for whole-module degrade: an npm package
/// may export a generic-callable the translator cannot specialize into a stub
/// (e.g. `export const sha512 = createHasher(…)`, which has no `export
/// function`, so the stub loop emits nothing and `sha512` stays unresolved).
/// With no callable stub, the file's own functions cannot call it statically —
/// so the whole module degrades: every function runs under the engine, whose
/// module loader resolves the import itself. A *local* degraded `.js` (a class
/// `extends`) does not trigger this: it still emits a `call_module_fn` stub for
/// each `export function`, so callers stay static (static-first). Only direct
/// imports are checked — a `.js` reached through another `.ts` degrades that
/// `.ts`, which becomes a normal stub crate this file calls.
fn src_imports_degraded_js(src: &str, base: &Path) -> bool {
    for imp in Translator::new().imports(src) {
        let Ok((dep_path, kind)) = resolve_local_module(base, &imp.source) else {
            continue;
        };
        let js_path = match &kind {
            DepKind::Js => &dep_path,
            DepKind::DtsWithJs { js_path, .. } => js_path,
            _ => continue,
        };
        if is_npm_js(js_path) {
            return true;
        }
    }
    false
}

/// Walk the import graph from `src` and collect every bare specifier that
/// resolves to a workspace member (a `node_modules` symlink to a local `src/`),
/// so the translator lowers those imports to `crate::mod` — they translate
/// into a `mod` of this crate, just like a relative `./m`. Mirrors
/// [`probe_sources_use_engine`]'s walk so it covers exactly the files that
/// translate. Registry specifiers (`.pnpm` store, plain dirs) and `cargo:` /
/// relative imports are not collected.
fn collect_workspace_deps(src: &str, base: &Path) -> std::collections::HashSet<String> {
    let probe = Translator::new();
    let mut deps = std::collections::HashSet::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut worklist: std::collections::VecDeque<(String, PathBuf)> =
        std::collections::VecDeque::new();
    for imp in probe.imports(src) {
        record_workspace_dep(&imp.source, base, &mut deps);
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
        let dep_base = dep_path.parent().unwrap_or_else(|| Path::new(""));
        for imp in probe.imports(&dep_src) {
            record_workspace_dep(&imp.source, dep_base, &mut deps);
            if seen.insert(imp.module.clone()) {
                worklist.push_back((imp.source, dep_base.to_path_buf()));
            }
        }
    }
    deps
}

/// Record `source` as a local module so its `use` path is `crate::…`. Two bare
/// specifiers lower to an in-crate `mod`: a symlinked workspace member (mapped
/// by [`resolve_workspace_dep`] to a local `src/`), and a registry npm package
/// whose resolved entry is a `.js` (or `.js`+`.d.ts` pair) — that degrades to a
/// mod stub (e.g. an npm hash library), so the consumer's `use` must be
/// `crate::…` to resolve from a sibling module. Relative (`.`) and `cargo:`
/// imports are skipped (the former is already local; the latter is an extern
/// crate). A bare spec that fails to resolve (deps not installed) is skipped
/// too — it surfaces later.
fn record_workspace_dep(source: &str, base: &Path, deps: &mut std::collections::HashSet<String>) {
    if source.starts_with('.') || source.starts_with("cargo:") {
        return;
    }
    if resolve_workspace_dep(base, source).is_some() {
        deps.insert(source.to_string());
        return;
    }
    if let Ok((_, kind)) = resolve_local_module(base, source) {
        if matches!(kind, DepKind::Js | DepKind::DtsWithJs { .. }) {
            deps.insert(source.to_string());
        }
    }
}

/// Translate one `.ts` file to an [`EmitFile`] (its translated body, no `mod`
/// declarations — those are synthesized by [`emit_tree`] from the path tree),
/// plus the flat emit files for any non-walk-TS deps it pulls in (a relative
/// `.js`/`.d.ts`, or a bare npm import the walk skips). A relative `.ts` dep is
/// scanned + translated separately by [`translate_project`]'s directory walk and
/// reaches this file through the per-importer emit-path overrides
/// ([`crate::translator::imports::set_emit_name_overrides`]); a bare specifier
/// resolving to a workspace member is recorded as a cargo path dep, not emitted.
fn translate_one_with_mods(
    translator: &Translator,
    ds: &Path,
    root: &Path,
    role: FileRole,
    is_root_entry: bool,
    deps: &mut RuntimeDeps,
) -> Result<(EmitFile, Vec<EmitFile>), Box<dyn Error>> {
    let src = fs::read_to_string(ds).map_err(|e| format!("cannot read {}: {e}", ds.display()))?;
    let base = ds.parent().unwrap_or_else(|| Path::new(""));
    // The file's DsResolver specifier is its stem; a degraded `.js` it imports
    // registers under the specifier the runtime resolver finds. Set before the
    // translate so a per-function-degraded file whose JS still carries ESM
    // imports (B6-5b), or a whole-module-degraded one (B6-5c), routes its
    // functions to `call_module_fn` keyed by it. Cleared on return.
    let entry_spec = stem_of(ds);
    crate::translator::imports::set_current_module_specifier(Some(entry_spec.clone()));
    struct SpecifierGuard;
    impl Drop for SpecifierGuard {
        fn drop(&mut self) {
            crate::translator::imports::clear_current_module_specifier();
        }
    }
    let _specifier_guard = SpecifierGuard;
    // B6-5c: same as `translate_dep`, a file that directly imports a degraded
    // `.js` module degrades the whole module (its functions run under
    // `call_module_fn`, the loader resolves the imports). Cleared on return.
    Translator::set_whole_module_degrade(src_imports_degraded_js(&src, base));
    struct DegradeGuard;
    impl Drop for DegradeGuard {
        fn drop(&mut self) {
            Translator::set_whole_module_degrade(false);
        }
    }
    let _degrade_guard = DegradeGuard;
    let (rust, file_deps) = translator
        .translate_with_deps_as(&src, role)
        .map_err(|e| format!("translate {}: {e}", ds.display()))?;
    deps.merge(&file_deps);
    // Flat emit files for non-walk-TS deps (a relative `.js`/`.d.ts`, or a bare
    // npm import `walk_ts` skips). A relative `.ts` dep is emitted by the walk
    // and reaches this file via `emit_tree`'s module tree. Each flat dep lands
    // at the crate root (`src/<module>.rs`), declared by the root entry's
    // top-level `mod` list.
    let mut flat_deps: Vec<EmitFile> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for imp in translator.imports(&src) {
        if !seen.insert(imp.module.clone()) {
            continue;
        }
        // A bare specifier resolving to a sibling workspace member is an
        // independent crate (cargo path dep), not a local module: record the
        // dep and skip the `mod` decl + emit. The translator already lowered
        // the import to a bare-crate `use` (`office_open_xml::X`); a local mod
        // would shadow that crate and re-merge the member's source.
        if let Some(crate_ident) = workspace_member_crate(base, &imp.source) {
            deps.add_path_dep(&crate_ident);
            continue;
        }
        let (dep_path, kind) = match resolve_local_module(base, &imp.source) {
            Ok(v) => v,
            Err(_) => continue,
        };
        // A relative `.ts` dep is scanned + translated by `walk_ts`; everything
        // else (a relative `.js`/`.d.ts`, or a bare npm import the walk skips)
        // is translated here so it lands in the emit set.
        let scanned_by_walk = imp.source.starts_with('.') && matches!(kind, DepKind::Ts);
        if scanned_by_walk {
            continue;
        }
        let spec = ds_resolve_specifier(&entry_spec, &imp.source);
        let dep_rust = translate_dep(translator, &dep_path, kind, &spec, None, deps)?;
        // A degraded module with no `export function` yields an empty body;
        // its source is still registered in `__DS_MODULE_SOURCES`, so skip
        // emitting an empty `.rs` stub and its `mod` declaration.
        if !dep_rust.is_empty() {
            flat_deps.push(EmitFile {
                rel_path: format!(
                    "third_party/{}",
                    crate::translator::imports::npm_third_party_module_path(&imp.source)
                ),
                content: dep_rust,
                is_barrel: false,
                is_root_entry: false,
            });
        }
    }
    let base_rel = rel_emit_path(ds, root);
    let main = EmitFile {
        // Root entry stays at the src/ root; a non-entry local file goes under
        // app/, matching the emit_rel map resolve_emit_paths built.
        rel_path: if is_root_entry {
            base_rel
        } else {
            format!("app/{base_rel}")
        },
        content: rust,
        is_barrel: is_barrel_index(ds, root),
        is_root_entry,
    };
    Ok((main, flat_deps))
}

/// A project's resolved targets for `Cargo.toml`: the `(bin_name, ds_path)`
/// pairs for `[[bin]]`, plus the `[lib]` entry path.
type ProjectTargets = (Vec<(String, String)>, Option<String>);

/// Resolve a `bin`/`lib` entry string to a canonical absolute path. An entry
/// may be relative to the package `root` (a `package.json` `bin`/`main` field)
/// or to the workspace root: a `tsconfig.json` `paths` value is resolved
/// relative to the tsconfig file itself when no `baseUrl` is set (per the
/// TSConfig spec), and the caller passes that value verbatim, so it is
/// relative to the workspace root, not this member `root`. Try `root.join`
/// first (the bin/main case); if that is not a file, resolve `p` against the
/// process cwd (the workspace root when `ds build` runs there). Both this and
/// `walk_ts` output canonicalize to absolute paths, so the root-entry and
/// import-closure comparisons stay symmetric.
fn resolve_entry_canon(root: &Path, p: &str) -> Option<PathBuf> {
    let joined = root.join(p);
    if joined.is_file() {
        joined.canonicalize().ok()
    } else {
        Path::new(p).canonicalize().ok().filter(|c| c.is_file())
    }
}

/// The transitive import closure reachable from the `bin`/`lib` entry files.
/// A package's published source is its entries plus everything they import;
/// files outside the closure — demos, build configs, scripts that `import
/// "@scope/pkg"` to showcase the *published* API — are consumers or tooling,
/// not crate members, so they are not compiled (mirroring Node's `dist/`,
/// the compiled entry closure). Compiling them would pull a self-reference
/// (`@scope/pkg` resolving to this crate) into a cargo self-dep, and lift
/// non-ident stems (`1-basic.ts`) into illegal `mod` names. Each entry's
/// imports resolve through the tsconfig-aware resolver, so relative imports
/// and tsconfig `paths` aliases (`@parts/*` → `./src/parts/*`) extend the
/// closure, while a bare workspace-member specifier resolves to a sibling
/// crate's source and adds no local file.
fn entry_import_closure(
    files: &[PathBuf],
    entries: &std::collections::HashSet<PathBuf>,
) -> std::collections::HashSet<PathBuf> {
    let local: std::collections::HashSet<PathBuf> = files
        .iter()
        .map(|f| f.canonicalize().unwrap_or_else(|_| f.clone()))
        .collect();
    let probe = Translator::new();
    let mut reachable: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    let mut worklist: std::collections::VecDeque<PathBuf> = entries.iter().cloned().collect();
    while let Some(p) = worklist.pop_front() {
        if !reachable.insert(p.clone()) {
            continue;
        }
        let Ok(src) = fs::read_to_string(&p) else {
            continue;
        };
        let base = p.parent().unwrap_or_else(|| Path::new(""));
        for imp in probe.imports(&src) {
            if let Ok((dep, _)) = resolve_local_module(base, &imp.source) {
                let dep = dep.canonicalize().unwrap_or(dep);
                if local.contains(&dep) && !reachable.contains(&dep) {
                    worklist.push_back(dep);
                }
            }
        }
    }
    reachable
}

/// Translate every `.ts` under a package root into one multi-target crate at
/// `project_dir/src/`, preserving the source directory tree: each file becomes
/// `src/<rel_path>.rs` (a subdirectory barrel at `src/<dir>/mod.rs`), each
/// interior directory gets a `mod.rs` declaring its children, and the package's
/// `bin`/`lib` entries become the crate's `[[bin]]`/`[lib]` targets. Returns
/// the resolved targets for `Cargo.toml` emission.
///
/// Two project-level guards: a bin importing another bin (cargo forbids it;
/// shared code must go through `[lib]`), and a circular import (Rust forbids
/// circular modules).
pub fn translate_project(
    root: &Path,
    package: &Package,
    project_dir: &Path,
    lib_entry: Option<&str>,
) -> Result<(ProjectTargets, RuntimeDeps), Box<dyn Error>> {
    let src_dir = project_dir.join("src");
    fs::create_dir_all(&src_dir)?;
    // Clear stale translations from a prior run (a renamed bin, or a switch
    // between lone-file and project mode) so cargo never sees orphan modules.
    clean_src_dir(&src_dir)?;

    let bins = package.bin_entries();
    // The crate's `[lib]` target. `lib_entry` is the caller-discovered source
    // entry — for a workspace member, the root `tsconfig.json` `paths[name]`
    // mapping (the authoritative source→path declaration, e.g. office-open's
    // `@office-open/xml` → `packages/xml/src/index.ts`); without it, fall back
    // to `package.json` `main` only when it points at a real source file
    // (`.ts`/`.tsx`/`.js`/`.jsx`/`.mjs`/`.cjs`). A dist artifact
    // (`dist/index.mjs`) is build output, not a source entry — it is never
    // reverse-mapped (the source `index.ts` may build to any dist name).
    // `None` → no `[lib]` target (a bin-only crate).
    let lib = lib_entry.map(String::from).or_else(|| {
        package.main.as_ref().and_then(|p| {
            let is_source = matches!(
                root.join(p).extension().and_then(|e| e.to_str()),
                Some("ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs")
            );
            is_source.then(|| p.clone())
        })
    });
    // File role (arch decision point 8): a bin/lib entry collects top-level
    // executable statements into `fn main`; every other file (an imported
    // module) only declares, never executes — the `Module` role errors on its
    // top-level executable statements (module semantics). Compared by canonical
    // path so the call/import form or a relative/absolute `bin` spelling does
    // not affect the decision. Canonicalize failures (symlink loop, missing
    // privilege) fall back to the joined path on BOTH sides — the per-file
    // check below uses the same fallback — so the comparison stays symmetric
    // instead of silently dropping an entry whose canonicalization failed.
    let entry_paths: std::collections::HashSet<PathBuf> = bins
        .iter()
        .map(|(_, p)| resolve_entry_canon(root, p))
        .chain(lib.as_ref().map(|p| resolve_entry_canon(root, p)))
        .flatten()
        .collect();

    let mut files = Vec::new();
    walk_ts(root, &mut files);
    files.sort();
    // Keep only files reachable from a `bin`/`lib` entry — the package's
    // published source. Orphans outside the closure (demos, build configs,
    // showcase scripts) are consumers/tooling, not crate members. Skipped
    // when there is no entry (a bin/lib-less project compiles everything).
    if !entry_paths.is_empty() {
        let reachable = entry_import_closure(&files, &entry_paths);
        files.retain(|f| {
            let c = f.canonicalize().unwrap_or_else(|_| f.clone());
            reachable.contains(&c)
        });
    }

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

    // Phase A: resolve each walk-TS file to its emit rel-path, preserving the
    // source directory tree under app/ (an `__ds_defn` suffix breaks the one
    // collision a tree cannot — a file whose stem equals its barrel directory's
    // name). Root entries stay at the src/ root.
    let resolved = resolve_emit_paths(&files, root, &entry_paths);
    let emit_rel: std::collections::HashMap<String, (String, bool)> = resolved
        .iter()
        .map(|(f, rel, barrel)| (canon_string(f), (rel.clone(), *barrel)))
        .collect();
    // Phase B: translate each file (no `mod` declarations — `emit_tree`
    // synthesizes those from the path tree), collecting emit files and the
    // per-importer path overrides that route nested imports to `crate::<tree>`.
    //
    // Aggregate interface field types + optional field names across the
    // member's files before translating, so each file sees imported
    // interfaces' fields — a cross-file `obj?.field ?? d` (chain `and_then`),
    // `for (const c of obj.field ?? [])` (flatten-iter), or `obj.field == v`
    // (as_deref compare) needs the imported interface's field type/optional
    // flag, but each file builds its own `TypeRegistry`. The lone-file path
    // (`translate_sources`) does the equivalent via `collect_package_*`; a
    // workspace member's files are already enumerated in `files`, so a
    // per-file collect + merge covers the member's intra-package imports.
    let collector = Translator::new();
    let mut shared_fields: std::collections::HashMap<
        String,
        Vec<crate::translator::InterfaceField>,
    > = std::collections::HashMap::new();
    let mut shared_optionals: std::collections::HashMap<String, std::collections::HashSet<String>> =
        std::collections::HashMap::new();
    // Workspace members this member imports are independent sibling crates
    // (cargo path deps), so their bare specifier must lower to a bare extern
    // `use` (`ds_office_openSxml::X`) — not a `crate::…` mod (that is the
    // lone-file merge model) nor a `third_party::` path (an npm dep). Collect
    // the specifiers across the member's files before translating so every
    // file's import lowering routes them through the extern prelude.
    let mut member_crates: std::collections::HashSet<String> = std::collections::HashSet::new();
    for ds in &files {
        let Ok(src) = fs::read_to_string(ds) else {
            continue;
        };
        for (name, fields) in collector.collect_fields(&src).unwrap_or_default() {
            shared_fields.entry(name).or_insert_with(|| fields);
        }
        for (name, opts) in collector.collect_optionals(&src).unwrap_or_default() {
            shared_optionals.entry(name).or_insert_with(|| opts);
        }
        let base = ds.parent().unwrap_or_else(|| Path::new(""));
        for imp in collector.imports(&src) {
            if workspace_member_crate(base, &imp.source).is_some() {
                member_crates.insert(imp.source);
            }
        }
    }
    let translator = collector
        .with_extra_fields(shared_fields)
        .with_extra_optionals(shared_optionals);
    crate::translator::imports::set_workspace_member_crates(member_crates);
    struct MemberCrateGuard;
    impl Drop for MemberCrateGuard {
        fn drop(&mut self) {
            crate::translator::imports::clear_workspace_member_crates();
        }
    }
    let _member_crate_guard = MemberCrateGuard;
    let mut deps = RuntimeDeps::default();
    let mut emit_files: Vec<EmitFile> = Vec::new();
    for ds in &files {
        let canon = ds.canonicalize().unwrap_or_else(|_| ds.clone());
        let is_root_entry = entry_paths.contains(&canon);
        let role = if is_root_entry {
            FileRole::BinEntry
        } else {
            FileRole::Module
        };
        // Per-importer emit-path overrides: each relative import of this file,
        // resolved to its target, maps to the target's crate-local path so the
        // translator emits `crate::<rel_path>` use paths. `./locking` resolves
        // to the barrel from a sibling directory but to the defn from inside it
        // — the per-file map carries that distinction.
        let overrides = collect_member_overrides(&translator, ds, &emit_rel);
        crate::translator::imports::set_emit_name_overrides(overrides);
        struct OverrideGuard;
        impl Drop for OverrideGuard {
            fn drop(&mut self) {
                crate::translator::imports::clear_emit_name_overrides();
            }
        }
        let _override_guard = OverrideGuard;
        let (main, flat) =
            translate_one_with_mods(&translator, ds, root, role, is_root_entry, &mut deps)?;
        emit_files.push(main);
        emit_files.extend(flat);
    }
    // A flat dep (a relative `.js`/`.d.ts` reached via `translate_dep`) may
    // duplicate a walk-TS file's rel-path — keep the first occurrence.
    let mut seen_rel: std::collections::HashSet<String> = std::collections::HashSet::new();
    emit_files.retain(|f| seen_rel.insert(f.rel_path.clone()));
    // Collect every file's inline union-enum definitions and prepend them to
    // the crate-root entry. A barrel entry re-exports only; the union types
    // live in the modules it pulls in, which reference `crate::__DsUnion…` but
    // emit no definition. Without this the crate root lacks the union
    // definitions every module names (`translate_sources` does the equivalent
    // at its lone-file crate root — `ds build` collects across entry + deps).
    let mut emitted_union_enums: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    let mut extra_union_enums = String::new();
    for ds in &files {
        let Ok(src) = fs::read_to_string(ds) else {
            continue;
        };
        for (name, text) in translator.union_enum_items(&src) {
            if emitted_union_enums.insert(name) {
                extra_union_enums.push_str(&text);
                extra_union_enums.push('\n');
            }
        }
    }
    if !extra_union_enums.is_empty() {
        if let Some(entry) = emit_files.iter_mut().find(|f| f.is_root_entry) {
            entry.content = format!("{extra_union_enums}{}", entry.content);
        }
    }
    emit_tree(&src_dir, &emit_files)?;

    detect_bin_imports_bin(root, &bins)?;
    // Rust forbids `mod`-declaration cycles but permits `use` cycles between
    // sibling modules. DashScript emits a flat tree of `mod` declarations
    // (never cyclic) and lowers imports to `use`, so a TS import cycle becomes
    // a use-cycle cargo accepts. Surface it as a warning rather than rejecting
    // — cargo reports any real error (e.g. a cyclic static initializer).
    if let Err(e) = detect_circular_imports(&files) {
        eprintln!("warning: {e}");
    }
    Ok(((bins, lib), deps))
}
