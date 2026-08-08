use super::*;

/// Walk the import graph from `src` and aggregate every `.ts`/`.d.ts` file's
/// optional (`?:`) field names, so each file sees imported interfaces'
/// optionals — a cross-file `opts?.field ?? d` needs to know `field` is
/// optional, but each file builds its own `TypeRegistry`. Pure-`.js` deps (no
/// type annotations) are skipped. This is the cross-file half of each file's
/// per-file registry; the union is injected via `with_extra_optionals`.
pub(crate) fn collect_package_optionals(
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
    // (path, member): `member` is the workspace member the dep lives in (`Some`
    // for a cross-package dep, `None` for the entry's own package), so `seen`
    // dedupes by the dep's mod name ([`dep_mod_name`]) and two same-stem files
    // in different packages are both visited.
    let mut worklist: VecDeque<(PathBuf, Option<String>)> = VecDeque::new();
    for imp in collector.imports(src) {
        let member = workspace_member_crate(base, &imp.source);
        if seen.insert(dep_mod_name(&imp.source, &imp.module, &member)) {
            let (dep_path, kind) = resolve_local_module(base, &imp.source)?;
            if !matches!(kind, DepKind::Js) {
                worklist.push_back((dep_path, member));
            }
        }
    }
    while let Some((path, member)) = worklist.pop_front() {
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
            let child_member = workspace_member_crate(dep_base, &imp.source).or(member.clone());
            if seen.insert(dep_mod_name(&imp.source, &imp.module, &child_member)) {
                let (dep_path, kind) = resolve_local_module(dep_base, &imp.source)?;
                if !matches!(kind, DepKind::Js) {
                    worklist.push_back((dep_path, child_member));
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
pub(crate) fn collect_package_fields(
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
    // (path, member): see [`collect_package_optionals`] — `seen` dedupes by the
    // dep's mod name ([`dep_mod_name`]) so cross-package same-stem files differ.
    let mut worklist: VecDeque<(PathBuf, Option<String>)> = VecDeque::new();
    for imp in collector.imports(src) {
        let member = workspace_member_crate(base, &imp.source);
        if seen.insert(dep_mod_name(&imp.source, &imp.module, &member)) {
            let (dep_path, kind) = resolve_local_module(base, &imp.source)?;
            if !matches!(kind, DepKind::Js) {
                worklist.push_back((dep_path, member));
            }
        }
    }
    while let Some((path, member)) = worklist.pop_front() {
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
            let child_member = workspace_member_crate(dep_base, &imp.source).or(member.clone());
            if seen.insert(dep_mod_name(&imp.source, &imp.module, &child_member)) {
                let (dep_path, kind) = resolve_local_module(dep_base, &imp.source)?;
                if !matches!(kind, DepKind::Js) {
                    worklist.push_back((dep_path, child_member));
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
pub(crate) fn collect_package_union_enums(
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
    // (path, member): see [`collect_package_optionals`] — `seen` dedupes by the
    // dep's mod name ([`dep_mod_name`]) so cross-package same-stem files differ.
    let mut worklist: VecDeque<(PathBuf, Option<String>)> = VecDeque::new();
    for imp in collector.imports(src) {
        let member = workspace_member_crate(base, &imp.source);
        if seen.insert(dep_mod_name(&imp.source, &imp.module, &member)) {
            let (dep_path, kind) = resolve_local_module(base, &imp.source)?;
            if !matches!(kind, DepKind::Js) {
                worklist.push_back((dep_path, member));
            }
        }
    }
    while let Some((path, member)) = worklist.pop_front() {
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
            let child_member = workspace_member_crate(dep_base, &imp.source).or(member.clone());
            if seen.insert(dep_mod_name(&imp.source, &imp.module, &child_member)) {
                let (dep_path, kind) = resolve_local_module(dep_base, &imp.source)?;
                if !matches!(kind, DepKind::Js) {
                    worklist.push_back((dep_path, child_member));
                }
            }
        }
    }
    Ok(shared)
}

/// Walk the entry's import graph and aggregate every file's function/const-arrow
/// signatures (name, type params, return type), the signature analogue of
/// [`collect_package_union_enums`]. A module-global factory singleton
/// (`const p = createFactory<T>(...)`) infers its type from a callee defined in
/// another file — but each file builds its own `TypeRegistry`, so the package
/// build shares them here via [`Translator::with_extra_function_signatures`].
pub(crate) fn collect_package_function_signatures(
    src: &str,
    src_path: &Path,
) -> Result<std::collections::HashMap<String, crate::translator::FnSignature>, Box<dyn Error>> {
    use std::collections::{HashMap, HashSet, VecDeque};
    let collector = Translator::new();
    let mut shared: HashMap<String, crate::translator::FnSignature> = collector
        .collect_function_signatures(src)
        .map_err(|e| format!("collect signatures {}: {e}", src_path.display()))?;
    let base = src_path.parent().unwrap_or_else(|| Path::new(""));
    let mut seen: HashSet<String> = HashSet::new();
    // (path, member_crate): member_crate is the workspace-member crate this dep
    // lives in (`Some` for a cross-package dep, `None` for the entry's own
    // package). A bare workspace specifier sets it; a relative import inherits
    // the parent's — so a factory reached through a barrel (`@scope/core` →
    // `./opc/packer`) still carries the `core` member, not the relative hop.
    let mut worklist: VecDeque<(PathBuf, Option<String>)> = VecDeque::new();
    for imp in collector.imports(src) {
        let member = workspace_member_crate(base, &imp.source);
        if seen.insert(dep_mod_name(&imp.source, &imp.module, &member)) {
            let (dep_path, kind) = resolve_local_module(base, &imp.source)?;
            if !matches!(kind, DepKind::Js) {
                worklist.push_back((dep_path, member));
            }
        }
    }
    while let Some((path, member)) = worklist.pop_front() {
        let dep_src = fs::read_to_string(&path)
            .map_err(|e| format!("cannot read import {}: {e}", path.display()))?;
        let mut dep_sigs = collector
            .collect_function_signatures(&dep_src)
            .map_err(|e| format!("collect signatures {}: {e}", path.display()))?;
        // Tag each signature with the workspace crate its file lives in, so a
        // cross-package factory's return type is prefixed at the consumer.
        for sig in dep_sigs.values_mut() {
            sig.source_crate = member.clone();
        }
        for (k, v) in dep_sigs {
            shared.entry(k).or_insert_with(|| v);
        }
        let dep_base = path.parent().unwrap_or_else(|| Path::new(""));
        for imp in collector.imports(&dep_src) {
            // A bare workspace specifier enters that member; a relative import
            // stays in the current member.
            let child_member = workspace_member_crate(dep_base, &imp.source).or(member.clone());
            if seen.insert(dep_mod_name(&imp.source, &imp.module, &child_member)) {
                let (dep_path, kind) = resolve_local_module(dep_base, &imp.source)?;
                if !matches!(kind, DepKind::Js) {
                    worklist.push_back((dep_path, child_member));
                }
            }
        }
    }
    Ok(shared)
}

/// Aggregate the lazy-static exports across the whole import graph (the
/// entry and each recursive `.ts` dep) into one accessor-name to cell-type
/// map, set on the translator before the entry translates so every consumer
/// file recognizes an imported lazy static. Mirrors
/// [`collect_package_function_signatures`]'s worklist (member tracking, `.js`
/// skip). A lone file (no `package.json`) is translated single-file and never
/// calls this, so its empty table leaves single-file translation untouched.
pub(crate) fn collect_package_lazy_statics(
    src: &str,
    src_path: &Path,
) -> Result<std::collections::HashMap<String, syn::Type>, Box<dyn Error>> {
    use std::collections::{HashSet, VecDeque};
    let collector = Translator::new();
    let mut shared = collector.collect_lazy_static_exports(src);
    let base = src_path.parent().unwrap_or_else(|| Path::new(""));
    let mut seen: HashSet<String> = HashSet::new();
    let mut worklist: VecDeque<(PathBuf, Option<String>)> = VecDeque::new();
    for imp in collector.imports(src) {
        let member = workspace_member_crate(base, &imp.source);
        if seen.insert(dep_mod_name(&imp.source, &imp.module, &member)) {
            let (dep_path, kind) = resolve_local_module(base, &imp.source)?;
            if !matches!(kind, DepKind::Js) {
                worklist.push_back((dep_path, member));
            }
        }
    }
    while let Some((path, member)) = worklist.pop_front() {
        let dep_src = fs::read_to_string(&path)
            .map_err(|e| format!("cannot read import {}: {e}", path.display()))?;
        for (k, v) in collector.collect_lazy_static_exports(&dep_src) {
            shared.entry(k).or_insert(v);
        }
        let dep_base = path.parent().unwrap_or_else(|| Path::new(""));
        for imp in collector.imports(&dep_src) {
            let child_member = workspace_member_crate(dep_base, &imp.source).or(member.clone());
            if seen.insert(dep_mod_name(&imp.source, &imp.module, &child_member)) {
                let (dep_path, kind) = resolve_local_module(dep_base, &imp.source)?;
                if !matches!(kind, DepKind::Js) {
                    worklist.push_back((dep_path, child_member));
                }
            }
        }
    }
    Ok(shared)
}

/// The workspace-member crate a bare import `source` resolves to, or `None`
/// for a relative import (same package) or a `cargo:`/npm extern. Mirrors
/// [`record_workspace_dep`]: a bare specifier that maps to a local `src/` is a
/// workspace member, whose crate name is the sanitized module ident. Used to
/// tag cross-package factory signatures so their return type is prefixed
/// `crate::<member>::…` at the consumer.
pub(crate) fn workspace_member_crate(dir: &Path, source: &str) -> Option<String> {
    if source.starts_with('.') || source.starts_with("cargo:") {
        return None;
    }
    // A bare specifier may carry a sub-path into the member crate
    // (`@office-open/core/smartart`); the cargo dep is the member crate
    // (`office-open-core`), not a phantom crate per sub-path. `module_ident`
    // would sanitize the whole specifier (`office_open_core_smartart`), so feed
    // it only the package root and let the translator's use path carry the
    // sub-path as a crate-internal module (`office_open_core::smartart`).
    let (pkg_root, _subpath) = split_package_spec(source)?;
    resolve_workspace_dep(dir, source)
        .and_then(|_| crate::translator::imports::module_ident(&pkg_root))
        .map(|i| i.to_string())
}

/// The unique Rust mod name for a dep, disambiguating cross-package same-stem
/// files. A relative import inside a workspace member carries the member prefix
/// (`member_crate` + `./types` → `member_crate_types`); a relative
/// import in the entry's own package stays bare (`./types` → `types`); a bare
/// specifier already encodes its member (`@scope/member` →
/// `member_crate`), so it is returned as-is. Without this, two packages'
/// `types.ts` both lower to `crate::types` and the second clobbers the first
/// (the cross-package stem collision between two same-stem members). The emit filename,
/// `mod` declaration, and `use` path ([`mod_use_path`]) all derive from this so
/// they agree.
pub(crate) fn dep_mod_name(source: &str, module: &str, member: &Option<String>) -> String {
    if source.starts_with('.') {
        match member {
            Some(m) => format!("{m}_{module}"),
            None => module.to_string(),
        }
    } else {
        // A bare npm dep emits under `third_party/<segment-path>` preserving
        // the specifier's directory structure (isolated namespace, no `ds_`
        // prefix), unlike a workspace member which keeps it as a real cargo
        // crate ident.
        crate::translator::imports::npm_third_party_module_path(source)
    }
}

/// Strip an `r#` raw-ident prefix from a module name for use as a *file* stem.
/// A `.ts` file named after a Rust prelude macro (`stringify.ts`) lowers to the
/// raw ident `r#stringify` so the `mod r#stringify;` declaration and
/// `crate::r#stringify::*` paths parse — but the file Rust's module system
/// looks up is `src/stringify.rs`. The `r#` is source-level escape syntax, not
/// part of the path, so only the filename drops it; the `mod` decl keeps it.
pub(crate) fn mod_file_stem(name: &str) -> &str {
    name.strip_prefix("r#").unwrap_or(name)
}
