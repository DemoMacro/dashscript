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

/// True when `dep_path` lives under a `node_modules` directory — an npm
/// package's `.js`. Such modules are pure ECMAScript implementations (classes
/// that `extends`, `BigInt`, prototype reflection, …) the static translator
/// cannot lower correctly, so they degrade wholesale to the engine rather than
/// transpile per-feature. A workspace `.js` (under `packages/`) keeps the
/// transpile-first path: it is a first-party source the translator may lower.
fn is_npm_js(dep_path: &Path) -> bool {
    dep_path
        .components()
        .any(|c| c.as_os_str() == std::ffi::OsStr::new("node_modules"))
}

/// Build-time mirror of the engine's runtime `DsResolver` join
/// (`__ds_engine.rs`): a bare specifier stays as-is (already a resolved
/// `node_modules` package path); a relative specifier joins onto the base
/// module's directory. The result is the key a degraded module is registered
/// under, so the runtime resolver — which applies the identical join — finds
/// it. Bare and relative must agree between build time and runtime, or a
/// transitive `import "./dep.js"` resolves to a key the loader never stored.
fn ds_resolve_specifier(base: &str, name: &str) -> String {
    if !name.starts_with('.') {
        name.to_string()
    } else {
        let base_dir = base.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("");
        let rel = name.strip_prefix("./").unwrap_or(name);
        if base_dir.is_empty() {
            rel.to_string()
        } else {
            format!("{base_dir}/{rel}")
        }
    }
}

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
    import_specifier: &str,
    member: Option<String>,
    deps: &mut RuntimeDeps,
) -> Result<String, Box<dyn Error>> {
    // The workspace member this dep lives in, so its relative imports carry the
    // member prefix (`./types` → `crate::<member>_types`) and do not collide
    // with a same-stem file in another package. Cleared on return (success or
    // error) so it does not leak into the next dep's translate.
    crate::translator::imports::set_current_member(member);
    struct MemberGuard;
    impl Drop for MemberGuard {
        fn drop(&mut self) {
            crate::translator::imports::clear_current_member();
        }
    }
    let _member_guard = MemberGuard;
    // The import specifier this dep is reached under, so a per-function-
    // degraded `.ts` module whose annotation-stripped JS still carries ESM
    // imports routes its degraded bodies to `call_module_fn` keyed by this
    // specifier (the loader resolves the imports). Cleared on return like the
    // member guard.
    crate::translator::imports::set_current_module_specifier(Some(import_specifier.to_string()));
    struct SpecifierGuard;
    impl Drop for SpecifierGuard {
        fn drop(&mut self) {
            crate::translator::imports::clear_current_module_specifier();
        }
    }
    let _specifier_guard = SpecifierGuard;
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
            // A `.js` module whose class `extends` another (e.g. a crypto
            // package's `class _A extends B`) cannot lower statically,
            // so it degrades wholesale to the engine — stub fns forward to
            // QuickJS instead of the static translator emitting a
            // `compile_error!`. `.ts` deps keep the per-function/whole-program
            // degradation path (their classes may extend too, but a `.ts` dep
            // is a workspace source, not an npm package).
            if matches!(kind, DepKind::Js)
                && (is_npm_js(dep_path) || translator.js_module_needs_engine(&dep_src))
            {
                return degrade_js_module(
                    translator,
                    dep_path,
                    &dep_src,
                    None,
                    import_specifier,
                    deps,
                );
            }
            // B6-5c: if this dep directly imports a degraded `.js` module, its
            // functions depend on engine-only exports — degrade the whole
            // module (every function under `call_module_fn`, the loader
            // resolves the imports). Per-file: cleared on return so it does not
            // leak into the next dep.
            let dep_base = dep_path.parent().unwrap_or_else(|| Path::new(""));
            Translator::set_whole_module_degrade(src_imports_degraded_js(&dep_src, dep_base));
            struct DegradeGuard;
            impl Drop for DegradeGuard {
                fn drop(&mut self) {
                    Translator::set_whole_module_degrade(false);
                }
            }
            let _degrade_guard = DegradeGuard;
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
            // declarations; the `.js` is the implementation. Transpile the
            // `.js` (batch C path — untyped params default to `f64`) and prepend
            // the `.d.ts`'s type items so cross-module type imports resolve.
            let js_src = fs::read_to_string(&js_path)
                .map_err(|e| format!("cannot read import {}: {e}", js_path.display()))?;
            let dts_src = fs::read_to_string(&dts_path)
                .map_err(|e| format!("cannot read import {}: {e}", dts_path.display()))?;
            // Same wholesale degradation as a bare `.js`: a class `extends`
            // lowers to engine stubs. The sibling `.d.ts`'s `declare function`
            // signatures specialize each stub (a marshal-safe signature like
            // `bytesToHex(bytes: Uint8Array): string` keeps its concrete Rust
            // types); a signature with an unmappable type falls back to `Value`.
            if is_npm_js(&js_path) || translator.js_module_needs_engine(&js_src) {
                return degrade_js_module(
                    translator,
                    &js_path,
                    &js_src,
                    Some(&dts_src),
                    import_specifier,
                    deps,
                );
            }
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

/// The marshal-safe Rust types a degraded stub can specialize to. A type
/// outside this set (a user `struct`, an `Option<T>`, a bare type-alias
/// reference the degraded module did not emit) returns `None` from
/// [`marshal_kind`], so the stub falls back to a `Value` signature.
#[derive(Clone, Copy, PartialEq, Debug)]
enum MarshalKind {
    /// `String` — a JS `string`.
    Str,
    /// `f64` — a JS `number`.
    Num,
    /// `bool` — a JS `boolean`.
    Bool,
    /// `Vec<u8>` — a `Uint8Array`/`ArrayBuffer` (a crypto byte buffer).
    Bytes,
    /// `serde_json::Value` — the universal marshal type (already marshaled).
    Json,
}

/// The [`MarshalKind`] of a Rust type, or `None` when it is not one of the
/// marshal-safe scalars. The degraded stub emitter specializes a stub fn's
/// signature only when every param and the return type yield `Some`.
fn marshal_kind(ty: &syn::Type) -> Option<MarshalKind> {
    let path = match ty {
        syn::Type::Path(p) if p.qself.is_none() => &p.path,
        _ => return None,
    };
    let last = path.segments.last()?;
    Some(match last.ident.to_string().as_str() {
        "String" => MarshalKind::Str,
        "f64" => MarshalKind::Num,
        "bool" => MarshalKind::Bool,
        "Value" => {
            // Only `serde_json::Value` is marshal-safe (a bare `Value` from
            // another path is not — it would be an unresolved name).
            path.segments
                .iter()
                .any(|s| s.ident == "serde_json")
                .then_some(MarshalKind::Json)?
        }
        "Vec" => {
            // Only `Vec<u8>` (a `Uint8Array`) is marshal-safe — its JSON shape
            // is a number array serde deserializes back to bytes.
            let syn::PathArguments::AngleBracketed(args) = &last.arguments else {
                return None;
            };
            let syn::GenericArgument::Type(inner) = args.args.first()? else {
                return None;
            };
            let inner_last = match inner {
                syn::Type::Path(p) if p.qself.is_none() => p.path.segments.last()?,
                _ => return None,
            };
            (inner_last.ident == "u8").then_some(MarshalKind::Bytes)?
        }
        _ => return None,
    })
}

/// Render a `syn::Type` back to source text for the emitted stub, collapsing
/// the spaced punctuation `quote!` emits (`Vec < u8 >`, `serde_json :: Value`)
/// so the stub stays `cargo fmt`-clean. The marshal-safe set is finite
/// (`String`/`f64`/`bool`/`Vec<u8>`/`serde_json::Value`/`Option<…>`), so the
/// collapses cover every type the stub emitter renders.
fn render_type(ty: &syn::Type) -> String {
    use quote::ToTokens;
    ty.to_token_stream()
        .to_string()
        .replace(" < ", "<")
        .replace(" >", ">")
        .replace(" :: ", "::")
        .replace("< ", "<")
        .replace("> ", ">")
}

/// Walk a degraded module's ESM import graph and record every transitive
/// `.js`/`.ts` source in `deps.js_module_sources` under its DsResolver
/// specifier. A `.js` package like `@scope/pkg` is a multi-file graph
/// (`a.js` → `b.js` → `c.js`); the runtime `DsLoader` resolves each
/// transitive `import` via `source_of`, so each one must be registered at build
/// time — not just the directly-imported module. A module with no
/// `export function` (only `export const`/`class`, e.g. `b.js`) emits no stub
/// and is never runtime-registered, so it relies on this table alone.
fn register_js_module_graph(
    translator: &Translator,
    js_path: &Path,
    js_src: &str,
    specifier: &str,
    deps: &mut RuntimeDeps,
) {
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut worklist: std::collections::VecDeque<(PathBuf, String, String)> =
        std::collections::VecDeque::new();
    worklist.push_back((
        js_path.to_path_buf(),
        js_src.to_string(),
        specifier.to_string(),
    ));
    while let Some((path, src, spec)) = worklist.pop_front() {
        if !seen.insert(spec.clone()) {
            continue;
        }
        deps.add_js_module(&spec, &src);
        let base = path.parent().unwrap_or_else(|| Path::new(""));
        for imp in translator.imports(&src) {
            let child_spec = ds_resolve_specifier(&spec, &imp.source);
            if seen.contains(&child_spec) {
                continue;
            }
            let Ok((child_path, child_kind)) = resolve_local_module(base, &imp.source) else {
                continue;
            };
            let child_js = match &child_kind {
                DepKind::Js | DepKind::Ts => fs::read_to_string(&child_path).ok(),
                DepKind::DtsWithJs { js_path, .. } => fs::read_to_string(js_path).ok(),
                DepKind::DtsOnly => None,
            };
            let Some(child_js) = child_js else {
                continue;
            };
            worklist.push_back((child_path, child_js, child_spec));
        }
    }
}

/// Lower a `.js`/`.mjs`/`.cjs` module that degrades wholesale to the engine —
/// it declares a class `extends` the static translator cannot lower. Flag the
/// engine runtime dep (so `__ds_engine.rs` is emitted), register this module
/// and its whole transitive import graph under their DsResolver specifiers
/// ([`register_js_module_graph`]), and emit one stub `fn` per
/// `export function`: each forwards to `__ds_engine::call_module_fn`. When a
/// sibling `.d.ts` carries the function's signature and every param/return type
/// is marshal-safe, the stub specializes to those concrete types (marshaling
/// via `serde_json::{to,from}_value`) so a static call site stays type-correct;
/// otherwise it marshals `Value` end to end.
fn degrade_js_module(
    translator: &Translator,
    js_path: &Path,
    js_src: &str,
    dts_src: Option<&str>,
    import_specifier: &str,
    deps: &mut RuntimeDeps,
) -> Result<String, Box<dyn Error>> {
    deps.insert(crate::translator::RuntimeDep::Engine);
    // Register this module and every transitive `.js` it imports, each under
    // the specifier the runtime `DsResolver` resolves its imports to (bare
    // verbatim, relative joined onto the base) — not its filesystem path, which
    // a `package.json` `exports` map may diverge from.
    register_js_module_graph(translator, js_path, js_src, import_specifier, deps);
    let specifier = import_specifier;
    let path_lit = format!("{specifier:?}");
    // Inline the `.js` source at build time as a Rust string literal so the
    // emitted crate is self-contained — the engine's `Loader` reads it from the
    // `JS_MODULES` table (or the build-time `__DS_MODULE_SOURCES` table) at
    // runtime, never from the filesystem.
    let js_source_lit = format!("{js_src:?}");
    // Index the sibling `.d.ts`'s declared signatures by name+arity so each
    // stub can specialize when its whole signature is marshal-safe.
    let sigs = dts_src
        .map(|s| translator.dts_fn_signatures(s))
        .unwrap_or_default();
    let mut out = String::from(
        "//! Degraded to the embedded QuickJS engine: a class `extends` here has \
         no static lowering. Each exported function forwards to the engine; when \
         its `.d.ts` signature is fully marshal-safe the stub keeps that concrete \
         type so a static call site stays type-correct.\n\n",
    );
    for (name, nparams) in translator.js_export_fns(js_src) {
        let fn_lit = format!("{name:?}");
        // Find the matching `.d.ts` signature and specialize only when every
        // param and the return type is marshal-safe (and the arity matches).
        let specialized = sigs
            .iter()
            .find(|s| s.name == name && s.params.len() == nparams)
            .and_then(|s| {
                let ret = s.ret.as_ref()?;
                let _param_kinds: Vec<MarshalKind> = s
                    .params
                    .iter()
                    .map(marshal_kind)
                    .collect::<Option<Vec<_>>>()?;
                let _ret_kind = marshal_kind(ret)?;
                Some(s)
            });
        let stub = match specialized {
            Some(sig) => {
                // Concrete signature: marshal each arg via
                // `serde_json::to_value` and the return via `from_value`. Every
                // type here is `Serialize + Deserialize` (the marshal-safe set).
                let params = sig
                    .params
                    .iter()
                    .enumerate()
                    .map(|(i, ty)| format!("__ds_p{i}: {}", render_type(ty)))
                    .collect::<Vec<_>>()
                    .join(", ");
                let ret_str = render_type(sig.ret.as_ref().expect("checked Some"));
                let args = (0..sig.params.len())
                    .map(|i| {
                        format!("serde_json::to_value(&__ds_p{i}).expect(\"marshal {name} arg\")")
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "pub fn {name}({params}) -> {ret_str} {{\n    \
                     crate::__ds_engine::register_js_module({path_lit}, {js_source_lit});\n    \
                     let __ds_ret = crate::__ds_engine::call_module_fn({path_lit}, {fn_lit}, \
                     &[{args}]);\n    \
                     serde_json::from_value(__ds_ret).expect(\"unmarshal {name} return\")\n}}\n\n",
                )
            }
            None => {
                // Value stub: no sibling `.d.ts`, an arity mismatch, or a
                // param/return type that is not marshal-safe.
                let params = (0..nparams)
                    .map(|i| format!("__ds_p{i}: serde_json::Value"))
                    .collect::<Vec<_>>()
                    .join(", ");
                let args = (0..nparams)
                    .map(|i| format!("__ds_p{i}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "pub fn {name}({params}) -> serde_json::Value {{\n    \
                     crate::__ds_engine::register_js_module({path_lit}, {js_source_lit});\n    \
                     crate::__ds_engine::call_module_fn({path_lit}, {fn_lit}, &[{args}])\n}}\n\n",
                )
            }
        };
        out.push_str(&stub);
    }
    Ok(out)
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
fn collect_package_function_signatures(
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
fn collect_package_lazy_statics(
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
fn workspace_member_crate(dir: &Path, source: &str) -> Option<String> {
    if source.starts_with('.') || source.starts_with("cargo:") {
        return None;
    }
    resolve_workspace_dep(dir, source)
        .and_then(|_| crate::translator::imports::module_ident(source))
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
fn dep_mod_name(source: &str, module: &str, member: &Option<String>) -> String {
    if source.starts_with('.') {
        match member {
            Some(m) => format!("{m}_{module}"),
            None => module.to_string(),
        }
    } else {
        module.to_string()
    }
}

/// Strip an `r#` raw-ident prefix from a module name for use as a *file* stem.
/// A `.ts` file named after a Rust prelude macro (`stringify.ts`) lowers to the
/// raw ident `r#stringify` so the `mod r#stringify;` declaration and
/// `crate::r#stringify::*` paths parse — but the file Rust's module system
/// looks up is `src/stringify.rs`. The `r#` is source-level escape syntax, not
/// part of the path, so only the filename drops it; the `mod` decl keeps it.
fn mod_file_stem(name: &str) -> &str {
    name.strip_prefix("r#").unwrap_or(name)
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

    // Bare workspace-member specifiers (`@scope/core`) resolve (via a
    // node_modules symlink) to a local `src/`, so `ds build` translates them
    // into a `mod` of this crate. Collect the import graph's such specifiers
    // before the entry translates, so the entry and each recursive dep lower
    // their `@scope/…` imports to `crate::mod` — not a bare `mod`, which Rust
    // 2018 path clarity rejects from a submodule. Cleared on translate end.
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
    // Worklist of `(module, source_spec, base_dir, specifier, member)` — each
    // popped dep is translated once, then its own imports extend the worklist.
    // A cycle (a.ts ↔ b.ts) terminates: `seen` dedupes by the dep's mod name.
    // `specifier` is the dep's DsResolver specifier (bare verbatim, relative
    // joined onto the importer's), so a degraded `.js` registers under the key
    // the runtime resolver finds it under. `member` is the workspace member the
    // dep lives in (`Some("member_crate")` for a file reached through a
    // workspace-member barrel; `None` for the entry's own package): the emit
    // filename and `mod` decl use [`dep_mod_name`] so a relative import inside a
    // member carries the member prefix and does not collide with a same-stem
    // file in another package. The entry's own specifier is its file stem.
    let entry_spec = src_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    let mut worklist: std::collections::VecDeque<(
        String,
        String,
        PathBuf,
        String,
        Option<String>,
    )> = std::collections::VecDeque::new();
    for imp in translator.imports(src) {
        let member = workspace_member_crate(base, &imp.source);
        if seen.insert(dep_mod_name(&imp.source, &imp.module, &member)) {
            let spec = ds_resolve_specifier(&entry_spec, &imp.source);
            worklist.push_back((imp.module, imp.source, base.to_path_buf(), spec, member));
        }
    }
    while let Some((module, source, dir, spec, member)) = worklist.pop_front() {
        let (dep_path, kind) = resolve_local_module(&dir, &source)?;
        let dep_rust = translate_dep(
            &translator,
            &dep_path,
            kind,
            &spec,
            member.clone(),
            &mut deps,
        )?;
        let emit_name = dep_mod_name(&source, &module, &member);
        fs::write(
            project_dir
                .join("src")
                .join(format!("{}.rs", mod_file_stem(&emit_name))),
            dep_rust,
        )?;
        mod_decls.push_str(&format!("mod {emit_name};\n"));
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
            if seen.insert(dep_mod_name(&imp.source, &imp.module, &child_member)) {
                let child_spec = ds_resolve_specifier(&spec, &imp.source);
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
    let (rust, mut deps) = translator
        .translate_with_deps_as(&src, role)
        .map_err(|e| format!("translate {}: {e}", ds.display()))?;
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut mod_decls = String::new();
    for imp in translator.imports(&src) {
        if !seen.insert(imp.module.clone()) {
            continue;
        }
        mod_decls.push_str(&format!("mod {};\n", imp.module));
        // A relative `.ts` dep is scanned + translated by `walk_ts`; everything
        // else (a bare npm import, or a relative `.js`/`.d.ts` dep `walk_ts`
        // skips) is translated here so its `mod` decl resolves. An unresolved
        // import is left for `walk_ts` or cargo to surface.
        let (dep_path, kind) = match resolve_local_module(base, &imp.source) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let scanned_by_walk = imp.source.starts_with('.') && matches!(kind, DepKind::Ts);
        if !scanned_by_walk {
            let spec = ds_resolve_specifier(&entry_spec, &imp.source);
            let dep_rust = translate_dep(&translator, &dep_path, kind, &spec, None, &mut deps)?;
            fs::write(
                project_dir
                    .join("src")
                    .join(format!("{}.rs", mod_file_stem(&imp.module))),
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
    // not affect the decision. Canonicalize failures (symlink loop, missing
    // privilege) fall back to the joined path on BOTH sides — the per-file
    // check below uses the same fallback — so the comparison stays symmetric
    // instead of silently dropping an entry whose canonicalization failed.
    let entry_paths: std::collections::HashSet<PathBuf> = bins
        .iter()
        .map(|(_, p)| {
            let full = root.join(p);
            full.canonicalize().unwrap_or(full)
        })
        .chain(lib.as_ref().map(|p| {
            let full = root.join(p);
            full.canonicalize().unwrap_or(full)
        }))
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
        inject_helper_module(project_dir, "__ds_engine", &engine, root_stems)?;
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
/// a resolved `.js` is ESM or CommonJS, used when wiring the engine. tsconfig
/// path aliases (`@alias/*`-style) are auto-discovered — the caller resolves
/// via `resolve_file`, which walks up from the importer to find `tsconfig.json`;
/// `symlinks: true` follows pnpm/npm `node_modules` links.
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
    fn translate_dep_degrades_js_class_extends() {
        // A `.js` module whose class `extends` another cannot lower statically,
        // so translate_dep emits engine-forwarding stub fns and flags the
        // engine runtime dep — instead of the static translator's
        // `compile_error!`.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(
            root,
            "dep.js",
            "class A extends B {}\nexport function f(x) { return x; }",
        );
        let translator = Translator::new();
        let mut deps = RuntimeDeps::empty();
        let rust = translate_dep(
            &translator,
            &root.join("dep.js"),
            DepKind::Js,
            "dep.js",
            None,
            &mut deps,
        )
        .expect("a class-extends .js degrades");
        assert!(rust.contains("pub fn f"), "stub fn emitted: {rust}");
        assert!(
            rust.contains("call_module_fn"),
            "stub forwards to the engine: {rust}"
        );
        assert!(
            !rust.contains("compile_error"),
            "no static compile_error leaked: {rust}"
        );
        assert!(deps.needs_engine(), "engine runtime dep flagged");
    }

    #[test]
    fn translate_dep_keeps_static_js_without_extends() {
        // A `.js` module without `extends` stays on the static path (no stub).
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(
            root,
            "dep.js",
            "export function add(a, b) { return a + b; }",
        );
        let translator = Translator::new();
        let mut deps = RuntimeDeps::empty();
        let rust = translate_dep(
            &translator,
            &root.join("dep.js"),
            DepKind::Js,
            "dep.js",
            None,
            &mut deps,
        )
        .expect("a plain .js transpiles");
        assert!(
            !rust.contains("call_module_fn"),
            "no engine stub for a static .js: {rust}"
        );
        assert!(!deps.needs_engine());
    }

    #[test]
    fn degrade_registers_under_import_specifier_not_path() {
        // A degraded `.js` registers under its import specifier (the runtime
        // `DsResolver` finds bare specifiers verbatim), not its filesystem path —
        // a `package.json` `exports` map may diverge the two.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(
            root,
            "dep.js",
            "class A extends B {}\nexport function f(x) { return x; }",
        );
        let translator = Translator::new();
        let mut deps = RuntimeDeps::empty();
        let rust = translate_dep(
            &translator,
            &root.join("dep.js"),
            DepKind::Js,
            "@scope/pkg/dep.js",
            None,
            &mut deps,
        )
        .expect("a class-extends .js degrades");
        assert!(
            rust.contains("\"@scope/pkg/dep.js\""),
            "stub registers under the import specifier: {rust}"
        );
        assert!(
            !rust.contains(&root.display().to_string().replace('\\', "/")),
            "stub does not use the filesystem path: {rust}"
        );
    }

    #[test]
    fn degrade_registers_transitive_js_under_resolver_specifier() {
        // `a.js` (class extends → degrades) imports `b.js` (only `export const`,
        // no `export function`). Both must land in the build-time source table
        // under their DsResolver specifiers — `a.js` under the bare import
        // specifier, `b.js` under the joined specifier (`pkg/` + `b.js`) — so the
        // runtime loader resolves the transitive import even though `b.js` emits
        // no stub fn and is never runtime-registered.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(
            root,
            "a.js",
            "import { x } from \"./b.js\";\nclass A extends B {}\nexport function f() { return x; }",
        );
        write(root, "b.js", "export const x = 42;");
        let translator = Translator::new();
        let mut deps = RuntimeDeps::empty();
        translate_dep(
            &translator,
            &root.join("a.js"),
            DepKind::Js,
            "pkg/a.js",
            None,
            &mut deps,
        )
        .expect("a.js degrades");
        let sources = deps.js_module_sources();
        assert!(
            sources.iter().any(|(s, _)| s == "pkg/a.js"),
            "a.js registered under its specifier: {sources:?}"
        );
        assert!(
            sources.iter().any(|(s, _)| s == "pkg/b.js"),
            "b.js registered under the joined specifier (pkg/b.js), not \"./b.js\" or a path: {sources:?}"
        );
    }

    #[test]
    fn record_workspace_dep_marks_node_modules_js_as_local() {
        // A bare specifier resolving to a node_modules `.js` degrades to an
        // in-crate mod stub, so it must be recorded as local — the consumer's
        // `use` path is `crate::…`, not a bare `mod` (Rust 2018 path clarity
        // rejects the bare form from a sibling module).
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let pkg = root.join("node_modules").join("my-pkg");
        fs::create_dir_all(&pkg).unwrap();
        write(
            &pkg,
            "package.json",
            r#"{ "name": "my-pkg", "main": "index.js" }"#,
        );
        write(&pkg, "index.js", "export function f(x) { return x; }");
        let mut deps = std::collections::HashSet::new();
        record_workspace_dep("my-pkg", root, &mut deps);
        assert!(
            deps.contains("my-pkg"),
            "node_modules .js should be recorded as a local mod: {deps:?}"
        );
    }

    #[test]
    fn emit_cargo_project_degrades_js_dep_with_class_extends() {
        // End-to-end: an entry importing a `.js` dep with a class `extends`
        // emits engine-forwarding stubs for that dep and the `__ds_engine`
        // helper module (the stubs call into it), flagged via the engine
        // runtime dep on the emitted Cargo.toml.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(
            root,
            "package.json",
            r#"{ "name": "probe", "bin": "main.ts" }"#,
        );
        write(
            root,
            "main.ts",
            "import { f } from \"./dep.js\";\nfunction main() { console.log(f(3)); }",
        );
        write(
            root,
            "dep.js",
            "class A extends B {}\nexport function f(x) { return x + 1; }",
        );
        let out = tmp.path().join("out");
        let main_path = root.join("main.ts");
        let src = fs::read_to_string(&main_path).unwrap();
        emit_cargo_project(&src, &main_path, &out).unwrap();
        let mut stub = String::new();
        for entry in fs::read_dir(out.join("src")).unwrap() {
            let p = entry.unwrap().path();
            if p.extension().and_then(|e| e.to_str()) == Some("rs") {
                let body = fs::read_to_string(&p).unwrap();
                if body.contains("crate::__ds_engine::call_module_fn") {
                    stub = body;
                }
            }
        }
        assert!(stub.contains("pub fn f"), "stub fn emitted: {stub}");
        assert!(
            stub.contains("register_js_module"),
            "stub registers its module: {stub}"
        );
        assert!(
            out.join("src").join("__ds_engine.rs").exists(),
            "__ds_engine helper emitted"
        );
        let cargo = fs::read_to_string(out.join("Cargo.toml")).unwrap();
        assert!(
            cargo.contains("rquickjs") || cargo.contains("rquickjs-sys"),
            "engine crate dep on Cargo.toml: {cargo}"
        );
    }

    #[test]
    fn emit_cargo_project_specializes_stub_from_marshal_safe_dts() {
        // A `.js` dep with a class `extends` degrades to engine stubs, and a
        // sibling `.d.ts` with a marshal-safe `declare function` signature
        // specializes the stub: `bytesToHex(b: Uint8Array): string` becomes
        // `pub fn bytesToHex(__ds_p0: Vec<u8>) -> String`, marshaling via
        // serde_json rather than `Value` end to end. (The entry only imports
        // it; whether the call site stays type-correct is a separate concern.)
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(
            root,
            "package.json",
            r#"{ "name": "probe", "bin": "main.ts" }"#,
        );
        write(
            root,
            "main.ts",
            "import { bytesToHex } from \"./dep.js\";\nfunction main() {}",
        );
        write(
            root,
            "dep.js",
            "class A extends B {}\nexport function bytesToHex(b) { return b; }",
        );
        write(
            root,
            "dep.d.ts",
            "declare function bytesToHex(b: Uint8Array): string;",
        );
        let out = tmp.path().join("out");
        let main_path = root.join("main.ts");
        let src = fs::read_to_string(&main_path).unwrap();
        emit_cargo_project(&src, &main_path, &out).unwrap();
        let mut stub = String::new();
        for entry in fs::read_dir(out.join("src")).unwrap() {
            let p = entry.unwrap().path();
            if p.extension().and_then(|e| e.to_str()) == Some("rs") {
                let body = fs::read_to_string(&p).unwrap();
                if body.contains("crate::__ds_engine::call_module_fn") {
                    stub = body;
                }
            }
        }
        assert!(stub.contains("pub fn bytesToHex"), "stub emitted: {stub}");
        assert!(
            stub.contains("Vec<u8>") && stub.contains("-> String"),
            "Uint8Array param + string return specialized: {stub}"
        );
        assert!(
            stub.contains("serde_json::from_value") && stub.contains("serde_json::to_value"),
            "marshal via serde_json: {stub}"
        );
    }

    #[test]
    fn emit_cargo_project_falls_back_to_value_for_unmappable_dts() {
        // A `.d.ts` signature with a non-marshal-safe type (a `string | number`
        // union lowers to a generated enum, not a JSON scalar) does not
        // specialize the stub — it stays `serde_json::Value` end to end.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(
            root,
            "package.json",
            r#"{ "name": "probe", "bin": "main.ts" }"#,
        );
        write(
            root,
            "main.ts",
            "import { pick } from \"./dep.js\";\nfunction main() {}",
        );
        write(
            root,
            "dep.js",
            "class A extends B {}\nexport function pick(x) { return x; }",
        );
        write(
            root,
            "dep.d.ts",
            "declare function pick(x: string | number): string;",
        );
        let out = tmp.path().join("out");
        let main_path = root.join("main.ts");
        let src = fs::read_to_string(&main_path).unwrap();
        emit_cargo_project(&src, &main_path, &out).unwrap();
        let mut stub = String::new();
        for entry in fs::read_dir(out.join("src")).unwrap() {
            let p = entry.unwrap().path();
            if p.extension().and_then(|e| e.to_str()) == Some("rs") {
                let body = fs::read_to_string(&p).unwrap();
                if body.contains("crate::__ds_engine::call_module_fn") {
                    stub = body;
                }
            }
        }
        assert!(
            stub.contains("pub fn pick(__ds_p0: serde_json::Value) -> serde_json::Value"),
            "Value stub for an unmappable signature: {stub}"
        );
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
    fn translate_sources_disambiguates_cross_package_same_stem() {
        // Two workspace packages both ship a `types.ts`. Without the member
        // prefix both lower to `crate::types` and the second clobbers the first
        // (the cross-package stem collision between two same-stem members); with it, package
        // `b`'s file becomes `crate::scope_b_types` while the entry's own stays
        // `crate::types`. The emit filename, `mod` decl, and `use` path all
        // derive from `dep_mod_name` so they agree.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let pkg_a = root.join("packages").join("a");
        let pkg_b = root.join("packages").join("b");
        fs::create_dir_all(pkg_a.join("src")).unwrap();
        fs::create_dir_all(pkg_b.join("src")).unwrap();
        write(&pkg_a, "package.json", r#"{ "name": "@scope/a" }"#);
        write(&pkg_b, "package.json", r#"{ "name": "@scope/b" }"#);
        write(
            &pkg_a.join("src"),
            "index.ts",
            "import { X } from \"./types\";\nimport { Y } from \"@scope/b\";\nfunction main() {}",
        );
        write(
            &pkg_a.join("src"),
            "types.ts",
            "export interface X { a: number }",
        );
        write(&pkg_b.join("src"), "index.ts", "export * from \"./types\";");
        write(
            &pkg_b.join("src"),
            "types.ts",
            "export interface Y { b: number }",
        );
        let scope_nm = root.join("node_modules").join("@scope");
        fs::create_dir_all(&scope_nm).unwrap();
        if make_symlink(&pkg_b, &scope_nm.join("b")).is_err() {
            eprintln!(
                "translate_sources_disambiguates_cross_package_same_stem: skipped (no symlink privilege)"
            );
            return;
        }
        let out = tmp.path().join("out");
        fs::create_dir_all(out.join("src")).unwrap();
        let entry = pkg_a.join("src").join("index.ts");
        let src = fs::read_to_string(&entry).unwrap();
        translate_sources(&src, &entry, &out).expect("translate");
        let dir = out.join("src");
        // Entry's own types.ts → src/types.rs (no prefix).
        assert!(dir.join("types.rs").exists(), "entry types.rs missing");
        // Package b's types.ts → src/scope_b_types.rs (member prefix).
        assert!(
            dir.join("scope_b_types.rs").exists(),
            "cross-package types.rs should be prefixed; src has: {:?}",
            fs::read_dir(&dir)
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
        );
        let main = fs::read_to_string(dir.join("main.rs")).unwrap();
        assert!(main.contains("mod types;"), "entry mod decl: {main}");
        assert!(
            main.contains("mod scope_b_types;"),
            "prefixed mod decl: {main}"
        );
        // The barrel (b/index.ts) reaches its own `./types` via the prefixed
        // path too — `use crate::scope_b_types::*`, not `crate::types::*`.
        let barrel = fs::read_to_string(dir.join("scope_b.rs")).unwrap();
        assert!(
            barrel.contains("crate::scope_b_types"),
            "barrel uses the member-prefixed use path: {barrel}"
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
