use super::*;

/// The canonical-path string for a resolved dep file. Used as the dedup key
/// in [`translate_sources`]'s worklist so a barrel (`locking/index.ts`) and a
/// same-stem definition file (`locking/locking.ts`) — two distinct files that
/// both flatten to `src/locking.rs` under the old specifier-derived key —
/// each get translated once. Two genuinely identical paths (a true import
/// cycle `a.ts ↔ b.ts`) still collapse, preserving the cycle-termination
/// guarantee. `fs::canonicalize` normalizes `..`, symlinks (a workspace-member
/// `node_modules/@scope/core` symlink resolves to the package's `src/`), and
/// case; resolution failures fall back to the raw lossy string.
pub(crate) fn canon_string(path: &Path) -> String {
    fs::canonicalize(path)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_string_lossy().into_owned())
}

/// The canonical dedup key for an import specifier. Resolves the specifier
/// against `base` (the importer's directory), then returns [`canon_string`]
/// of the result. A specifier that fails to resolve (a bare npm specifier
/// with no file to canonicalize, or a not-yet-installed dep) falls back to
/// the specifier-derived `mod_name` so dedup still terminates — the same
/// fallback the worklist takes when the dep is later popped and
/// [`resolve_local_module`] surfaces a real error.
pub(crate) fn canon_key(
    base: &Path,
    source: &str,
    mod_name: &str,
) -> Result<String, Box<dyn Error>> {
    match resolve_local_module(base, source) {
        Ok((path, _)) => Ok(canon_string(&path)),
        Err(_) => Ok(mod_name.to_string()),
    }
}

/// Walk the import graph reachable from `src` and assign each transitively
/// imported file a canonical emit name. The first file to claim a
/// `dep_mod_name`-derived emit name (in worklist discovery order: the entry's
/// direct imports first, then their transitive deps) keeps it; a later file
/// that would collide (a barrel + same-stem defn pair, or three same-stem
/// `types.ts` files in different directories flattening to `types.rs`) takes
/// the same name with `__ds_defn`, `__ds_defn_2`, … appended. Workspace-member
/// prefixing is preserved: two same-stem files in different members already
/// get distinct base names (`member_a_types` vs `member_b_types`), so the
/// collision detector never conflates them. Returns the map keyed by
/// [`canon_string`] of each reachable file's path.
pub(crate) fn compute_emit_name_map(
    translator: &Translator,
    src: &str,
    base: &Path,
) -> Result<std::collections::HashMap<String, String>, Box<dyn Error>> {
    use std::collections::{HashMap, HashSet, VecDeque};
    // BFS the import graph in the same discovery order as
    // [`translate_sources`]'s worklist, collecting `(canon_path, base_name)`
    // pairs. The worklist stores `(source_raw, importer_dir, member)`: the
    // raw import specifier (`./locking`) plus the importer's directory, so
    // the child resolves against the importer the way the main worklist does.
    // The member propagates so cross-package same-stem files stay distinct.
    let mut ordered: Vec<(String, String)> = Vec::new();
    let mut visited: HashSet<String> = HashSet::new();
    let mut queue: VecDeque<(String, PathBuf, Option<String>)> = VecDeque::new();
    for imp in translator.imports(src) {
        let member = workspace_member_crate(base, &imp.source);
        let base_name = dep_mod_name(&imp.source, &imp.module, &member);
        let canon = canon_key(base, &imp.source, &base_name)?;
        if visited.insert(canon.clone()) {
            ordered.push((canon.clone(), base_name));
            queue.push_back((imp.source, base.to_path_buf(), member));
        }
    }
    while let Some((source_raw, importer_dir, member)) = queue.pop_front() {
        // Resolve the raw specifier against the importer's directory to find
        // the dep file (same path [`translate_sources`]'s worklist takes).
        let Ok((dep_path, kind)) = resolve_local_module(&importer_dir, &source_raw) else {
            continue;
        };
        // Only `.ts`/`.js` deps emit a file and participate in filename
        // collisions; a `.d.ts`-only dep contributes types inline.
        if !matches!(kind, DepKind::Ts | DepKind::Js) {
            continue;
        }
        let dep_base = dep_path.parent().unwrap_or_else(|| Path::new(""));
        let dep_src = fs::read_to_string(&dep_path)
            .map_err(|e| format!("cannot read import {}: {e}", dep_path.display()))?;
        for imp in translator.imports(&dep_src) {
            let child_member = workspace_member_crate(dep_base, &imp.source).or(member.clone());
            let child_base = dep_mod_name(&imp.source, &imp.module, &child_member);
            let child_canon = canon_key(dep_base, &imp.source, &child_base)?;
            if visited.insert(child_canon.clone()) {
                ordered.push((child_canon.clone(), child_base));
                queue.push_back((imp.source, dep_base.to_path_buf(), child_member));
            }
        }
    }
    // Assign names greedily in discovery order. The first claimant of a base
    // name keeps it; subsequent colliders get `__ds_defn`, `__ds_defn_2`, …
    let mut map: HashMap<String, String> = HashMap::new();
    let mut used: HashSet<String> = HashSet::new();
    for (canon, base_name) in ordered {
        if map.contains_key(&canon) {
            continue;
        }
        let final_name = if used.insert(base_name.clone()) {
            base_name
        } else {
            let mut idx = 1;
            loop {
                let suffix = if idx == 1 {
                    "__ds_defn".to_string()
                } else {
                    format!("__ds_defn_{idx}")
                };
                let candidate = format!("{base_name}{suffix}");
                if used.insert(candidate.clone()) {
                    break candidate;
                }
                idx += 1;
            }
        };
        map.insert(canon, final_name);
    }
    Ok(map)
}

/// Build the per-file emit-name override map for the translator. For each
/// import/re-export specifier in `dep_path`'s source, resolve it to its
/// canonical path and look up the emit name [`compute_emit_name_map`]
/// assigned. Specifiers that land on a suffixed defn (a name not equal to the
/// specifier-derived mod name) become overrides; specifiers that keep their
/// bare name are omitted (the translator's default path is correct). The map
/// is keyed by the verbatim specifier as it appears in the source, so
/// [`mod_use_path`] can match without re-running resolution.
pub(crate) fn collect_emit_overrides(
    translator: &Translator,
    dep_path: &Path,
    emit_name_map: &std::collections::HashMap<String, String>,
) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    let Ok(dep_src) = fs::read_to_string(dep_path) else {
        return out;
    };
    let dep_base = dep_path.parent().unwrap_or_else(|| Path::new(""));
    for imp in translator.imports(&dep_src) {
        if !imp.source.starts_with('.') {
            continue;
        }
        let Ok((path, _)) = resolve_local_module(dep_base, &imp.source) else {
            continue;
        };
        let canon = canon_string(&path);
        let Some(emit_name) = emit_name_map.get(&canon) else {
            continue;
        };
        // Only override when the emit name diverges from the specifier-derived
        // mod name — i.e. the resolved file was suffixed. A barrel importing
        // another barrel (no collision) keeps its bare name and needs no
        // override.
        if emit_name != &imp.module {
            // Local deps live under app/, so the override path carries the
            // app:: prefix mod_use_path expects.
            out.insert(imp.source, format!("app::{emit_name}"));
        }
    }
    out
}

/// Remove every `.rs` under `src/` (recursing subdirectories) so a prior
/// translation — now that member-crate emit preserves the source directory
/// tree — cannot leave an orphan module cargo would try to compile. Empty
/// directories are removed too; `emit_tree` recreates the ones it needs.
pub(crate) fn clean_src_dir(src: &Path) -> std::io::Result<()> {
    fn clean(dir: &Path) {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    clean(&path);
                    let _ = fs::remove_dir(&path);
                } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                    let _ = fs::remove_file(&path);
                }
            }
        }
    }
    clean(src);
    Ok(())
}

/// Build the per-importer emit-path override map for one file: each relative
/// import resolved to its target, mapped to the target's crate-local path (the
/// emit rel-path with `/` → `::`). The translator's [`mod_use_path`] reads this
/// so a nested import emits `crate::drawingml::locking::…` rather than the flat
/// `crate::locking`. `./locking` resolves to the barrel from a sibling directory
/// but to the defn from inside it — the per-file map carries that distinction.
/// Imports whose target is not in the emit set (a bare npm specifier, an
/// unresolved import, or a `.d.ts`) are omitted, taking the default path.
pub(crate) fn collect_member_overrides(
    translator: &Translator,
    ds: &Path,
    emit_rel: &std::collections::HashMap<String, (String, bool)>,
) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    let Ok(src) = fs::read_to_string(ds) else {
        return out;
    };
    let base = ds.parent().unwrap_or_else(|| Path::new(""));
    for imp in translator.imports(&src) {
        if !imp.source.starts_with('.') {
            continue;
        }
        let Ok((target, kind)) = resolve_local_module(base, &imp.source) else {
            continue;
        };
        if !matches!(kind, DepKind::Ts | DepKind::Js) {
            continue;
        }
        let canon = canon_string(&target);
        let Some((rel, _)) = emit_rel.get(&canon) else {
            continue;
        };
        out.insert(imp.source, rel.replace('/', "::"));
    }
    out
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
                // `lib_entry = None`: translate_project falls back to `main`
                // when it is a source file (has_ts_entry guarantees a `.ts`
                // entry); a lone-file build has no tsconfig `paths` mapping.
                let ((bins, lib), deps) = translate_project(&root, &package, project_dir, None)?;
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
    // An entry is a crate-root file, so its own stem is the emit path —
    // matching `src_to_rust_path` and `rel_emit_path` (a `src/index.ts` lib
    // entry emits at `src/index.rs`, not the barrel-flattened `src/src.rs`
    // `stem_of` would produce for a subdirectory barrel).
    let stem = |p: &str| {
        Path::new(p)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("dash")
            .to_string()
    };
    let mut stems: Vec<String> = bins.iter().map(|(_, ds_path)| stem(ds_path)).collect();
    if let Some(lib_path) = lib {
        stems.push(stem(lib_path));
    }
    stems
}

/// Write the DashScript runtime module under `src/__ds/` and declare it at each
/// crate root, when the translated sources reference it. Both runtime parts
/// live under one `__ds/` directory — keeping a crate's `src/` split by code
/// origin (runtime in `__ds/`, local code in `app/`, third-party deps
/// in `third_party/`):
/// - `__ds/mod.rs` — static helper items (`number_to_string`, `array_set`,
///   `DsError`, …) a lowering reaches as `crate::__ds::X`.
/// - `__ds/engine.rs` — the `rquickjs` compat engine, only when a fixture
///   degrades; its entry points are `crate::__ds::engine::{run, call_fn,
///   call_module_fn}`. A no-op when no runtime dep is set.
pub fn apply_runtime_deps(
    project_dir: &Path,
    deps: &RuntimeDeps,
    root_stems: &[String],
) -> Result<(), Box<dyn Error>> {
    let helper = deps.helper_module();
    let engine = deps.engine_helper_module();
    if helper.is_none() && engine.is_none() {
        return Ok(());
    }
    let runtime_dir = project_dir.join("src").join("__ds");
    fs::create_dir_all(&runtime_dir)?;
    // `__ds/mod.rs` holds the static helpers; the engine is its child module,
    // declared only when present (an engine-only crate has no static helpers).
    let mut mod_src = helper.unwrap_or_default();
    if let Some(engine_src) = engine {
        mod_src.push_str("\npub mod engine;\n");
        fs::write(runtime_dir.join("engine.rs"), engine_src)?;
    }
    fs::write(runtime_dir.join("mod.rs"), mod_src)?;
    // Declare `mod __ds;` once at each crate root (a root already declaring it
    // is left untouched). Both the static items (`crate::__ds::X`) and the
    // engine (`crate::__ds::engine::X`) reach through this one declaration.
    let decl = "mod __ds;";
    for stem in root_stems {
        let path = project_dir.join("src").join(format!("{stem}.rs"));
        let Ok(body) = fs::read_to_string(&path) else {
            continue;
        };
        if body.contains(decl) {
            continue;
        }
        fs::write(&path, format!("{decl}\n{body}"))?;
    }
    Ok(())
}

/// The crate-local emit path for a translated source file under member-crate
/// tree emit: the file's path relative to `root` with its extension dropped, a
/// leading `src/` stripped (source lives under `src/`, but the crate's own
/// `src/` is the emit root), and a trailing `/index` dropped for a subdirectory
/// barrel (`chart/index.ts` → `chart`, emitted at `src/chart/mod.rs`). A root
/// `index.ts` keeps `index` — it is the entry, not a barrel. Backslashes
/// (Windows) normalize to `/` so segments split uniformly.
pub(crate) fn rel_emit_path(file: &Path, root: &Path) -> String {
    let rel = file.strip_prefix(root).unwrap_or(file);
    let mut s = rel.with_extension("").to_string_lossy().replace('\\', "/");
    if let Some(rest) = s.strip_prefix("src/") {
        s = rest.to_string();
    }
    if s.ends_with("/index") {
        s.truncate(s.len() - "/index".len());
    }
    s
}

/// Whether `file` is a subdirectory barrel — an `index.ts` whose module is the
/// parent directory (`drawingml/locking/index.ts` → `crate::…::locking`),
/// emitted at `src/{dir}/mod.rs`. A root `index.ts` (directly under `root` or
/// `root/src`) is the package entry, not a barrel — it keeps its own file.
pub(crate) fn is_barrel_index(file: &Path, root: &Path) -> bool {
    if file.file_name().and_then(|n| n.to_str()) != Some("index.ts") {
        return false;
    }
    let parent = file.parent().unwrap_or(Path::new(""));
    parent != root && parent != root.join("src")
}

/// Split `path` into `(parent, last_segment)` at its final `/`. `None` when
/// `path` has no `/` (a top-level segment).
pub(crate) fn split_last_seg(path: &str) -> Option<(String, String)> {
    let idx = path.rfind('/')?;
    Some((path[..idx].to_string(), path[idx + 1..].to_string()))
}

/// Resolve each walk-TS file to its effective emit rel-path. The source
/// directory tree is preserved (`drawingml/color/solid-fill.ts` →
/// `app/drawingml/color/solid_fill`); the only collision a tree cannot
/// disambiguate is a file whose stem equals its directory's name when that
/// directory also has a barrel (`drawingml/locking/locking.ts` +
/// `drawingml/locking/index.ts` would both occupy `src/app/drawingml/locking/`
/// — a `.rs` file beside a `mod.rs` Rust refuses, E0761), so the file takes an
/// `__ds_defn` suffix. Local non-entry code is rooted under `app/` (three-way
/// `src/` isolation: runtime `__ds/`, third-party deps `third_party/`, local
/// code `app/`); a `bin`/`lib` root entry stays at the `src/` root — its
/// `[[bin]]`/`[lib]` path and `apply_runtime_deps`'s `mod __ds;` injection both
/// assume `src/{stem}.rs`. Returns `(file, effective_rel_path, is_barrel)` per
/// input file, in input order.
pub(crate) fn resolve_emit_paths(
    files: &[PathBuf],
    root: &Path,
    root_entries: &std::collections::HashSet<PathBuf>,
) -> Vec<(PathBuf, String, bool)> {
    let mut out: Vec<(PathBuf, String, bool)> = files
        .iter()
        .map(|f| {
            let mut rel = rel_emit_path(f, root);
            // Root entry stays at the src/ root; every other local file goes
            // under app/. Canonicalize with the same fallback as the entry-set
            // builder so a canonicalize failure stays symmetric on both sides.
            let canon = f.canonicalize().unwrap_or_else(|_| f.clone());
            if !root_entries.contains(&canon) {
                rel = format!("app/{rel}");
            }
            (f.clone(), rel, is_barrel_index(f, root))
        })
        .collect();
    let barrel_dirs: std::collections::HashSet<String> = out
        .iter()
        .filter(|(_, _, b)| *b)
        .map(|(_, r, _)| r.clone())
        .collect();
    for (_, rel, barrel) in out.iter_mut() {
        if *barrel {
            continue;
        }
        // A file `dir/stem` collides with `dir`'s barrel when `stem` equals the
        // directory's own name (the barrel's mod.rs is `dir/mod.rs`; the file
        // would be `dir/<dirname>.rs`, which Rust cannot place beside it).
        if let Some((dir, stem)) = split_last_seg(rel) {
            if barrel_dirs.contains(&dir) && split_last_seg(&dir).is_some_and(|(_, d)| d == stem) {
                *rel = format!("{dir}/{stem}__ds_defn");
            }
        }
    }
    out
}

/// A translated file's emit target under member-crate tree emit.
pub(crate) struct EmitFile {
    /// Effective crate-local path (`drawingml/locking/locking__ds_defn`, or
    /// `drawingml/locking` for a barrel).
    pub(crate) rel_path: String,
    /// Translated Rust source (no directory `mod` declarations — those are
    /// synthesized by [`emit_tree`] from the path tree).
    pub(crate) content: String,
    /// A subdirectory barrel, emitted at `src/{rel_path}/mod.rs`.
    pub(crate) is_barrel: bool,
    /// The crate-root entry (a `bin`/`lib` target). It is not declared as a
    /// child of itself, but it carries the top-level `mod` declarations
    /// (`children[""]`) so the crate root assembles the module tree.
    pub(crate) is_root_entry: bool,
}

/// Write translated files preserving the source directory tree. Each file lands
/// at `src/{rel_path}.rs` (a barrel at `src/{rel_path}/mod.rs`). Every interior
/// directory gets a `mod.rs` declaring its direct children — a barrel's
/// `mod.rs` is its translated body with the child declarations prepended; a
/// barrel-less intermediate directory gets a synthesized `mod.rs` of `mod
/// <child>;` lines. Children are derived from the emit set itself (every file's
/// path contributes its segments), so the Rust module tree is complete
/// regardless of how the source imports across directory levels.
pub(crate) fn emit_tree(src_dir: &Path, files: &[EmitFile]) -> Result<(), Box<dyn Error>> {
    use std::collections::{BTreeMap, BTreeSet};
    let barrel_dirs: std::collections::HashSet<&str> = files
        .iter()
        .filter(|f| f.is_barrel)
        .map(|f| f.rel_path.as_str())
        .collect();
    // dir (rel path; "" = crate root) → direct child mod idents. A root entry
    // contributes nothing (it is the crate root, not a child); every other
    // file's path segments register each level's child (file or subdirectory).
    let mut children: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for f in files {
        if f.is_root_entry {
            continue;
        }
        let segs: Vec<&str> = f.rel_path.split('/').collect();
        for i in 0..segs.len() {
            let dir = if i == 0 {
                String::new()
            } else {
                segs[..i].join("/")
            };
            children.entry(dir).or_default().insert(segs[i].to_string());
        }
    }
    let decls_for = |dir: &str| -> String {
        // `pub mod` so a cross-layer `use` (e.g. the entry reaching
        // `third_party::noble::hashes::sha2Djs`) sees every interior module —
        // a private `mod` would hide each level from its grandparent (E0603).
        children
            .get(dir)
            .map(|kids| kids.iter().map(|c| format!("pub mod {c};\n")).collect())
            .unwrap_or_default()
    };
    // Write each translated file. A barrel prepends its directory's children;
    // the root entry prepends the top-level children; an ordinary file prepends
    // nothing (its siblings are declared by the parent mod.rs).
    for f in files {
        let decls = if f.is_barrel {
            decls_for(&f.rel_path)
        } else if f.is_root_entry {
            decls_for("")
        } else {
            String::new()
        };
        let body = if decls.is_empty() {
            f.content.clone()
        } else {
            format!("{decls}\n{}", f.content)
        };
        let path = if f.is_barrel {
            src_dir.join(format!("{}/mod.rs", f.rel_path))
        } else {
            src_dir.join(format!("{}.rs", f.rel_path))
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, body)?;
    }
    // Synthesize a mod.rs for each barrel-less interior directory.
    for dir in children.keys() {
        if dir.is_empty() || barrel_dirs.contains(dir.as_str()) {
            continue;
        }
        let decls = decls_for(dir);
        if decls.is_empty() {
            continue;
        }
        let path = src_dir.join(format!("{dir}/mod.rs"));
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, decls)?;
    }
    Ok(())
}
