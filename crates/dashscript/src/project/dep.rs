use super::*;

/// True when `dep_path` lives under a `node_modules` directory — an npm
/// package's `.js`. Such modules are pure ECMAScript implementations (classes
/// that `extends`, `BigInt`, prototype reflection, …) the static translator
/// cannot lower correctly, so they degrade wholesale to the engine rather than
/// transpile per-feature. A workspace `.js` (under `packages/`) keeps the
/// transpile-first path: it is a first-party source the translator may lower.
pub(crate) fn is_npm_js(dep_path: &Path) -> bool {
    dep_path
        .components()
        .any(|c| c.as_os_str() == std::ffi::OsStr::new("node_modules"))
}

/// Build-time mirror of the engine's runtime `DsResolver` join
/// (`__ds/engine.rs`): a bare specifier stays as-is (already a resolved
/// `node_modules` package path); a relative specifier joins onto the base
/// module's directory. The result is the key a degraded module is registered
/// under, so the runtime resolver — which applies the identical join — finds
/// it. Bare and relative must agree between build time and runtime, or a
/// transitive `import "./dep.js"` resolves to a key the loader never stored.
pub(crate) fn ds_resolve_specifier(base: &str, name: &str) -> String {
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
pub(crate) fn translate_dep(
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
pub(crate) enum MarshalKind {
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
pub(crate) fn marshal_kind(ty: &syn::Type) -> Option<MarshalKind> {
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
pub(crate) fn render_type(ty: &syn::Type) -> String {
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
pub(crate) fn register_js_module_graph(
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
/// engine runtime dep (so `__ds/engine.rs` is emitted), register this module
/// and its whole transitive import graph under their DsResolver specifiers
/// ([`register_js_module_graph`]), and emit one stub `fn` per
/// `export function`: each forwards to `__ds::engine::call_module_fn`. When a
/// sibling `.d.ts` carries the function's signature and every param/return type
/// is marshal-safe, the stub specializes to those concrete types (marshaling
/// via `serde_json::{to,from}_value`) so a static call site stays type-correct;
/// otherwise it marshals `Value` end to end.
pub(crate) fn degrade_js_module(
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
    // The `.js` source was registered above (transitive graph) and is embedded
    // in the build-time `__DS_MODULE_SOURCES` table; the engine's `Loader`
    // reads it via `source_of` at runtime. A stub must NOT re-inline the source
    // — doing so once per exported function copies the whole module N times (a
    // 1.8 MB stub for a 20-export module) and rustc chokes parsing the literal.
    // One copy, in the build-time table, is the source of truth.
    // Index the sibling `.d.ts`'s declared signatures by name+arity so each
    // stub can specialize when its whole signature is marshal-safe.
    let sigs = dts_src
        .map(|s| translator.dts_fn_signatures(s))
        .unwrap_or_default();
    let export_fns = translator.js_export_fns(js_src);
    if export_fns.is_empty() {
        // No `export function` (e.g. the module only has `export class extends`
        // or `export const`): there is nothing for a Rust stub to forward to.
        // The module's source is still registered above and embedded in
        // `__DS_MODULE_SOURCES`, so the engine's `Loader` resolves it when
        // another degraded module imports it — but no `.rs` stub is emitted, so
        // the caller skips writing the file and its `mod` declaration.
        return Ok(String::new());
    }
    let mut out = String::from(
        "//! Degraded to the embedded QuickJS engine: a class `extends` here has \
         no static lowering. Each exported function forwards to the engine; when \
         its `.d.ts` signature is fully marshal-safe the stub keeps that concrete \
         type so a static call site stays type-correct.\n\n",
    );
    for (name, nparams) in export_fns {
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
                     let __ds_ret = crate::__ds::engine::call_module_fn({path_lit}, {fn_lit}, \
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
                     crate::__ds::engine::call_module_fn({path_lit}, {fn_lit}, &[{args}])\n}}\n\n",
                )
            }
        };
        out.push_str(&stub);
    }
    Ok(out)
}
