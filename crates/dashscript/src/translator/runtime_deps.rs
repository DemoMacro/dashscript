//! `RuntimeDeps` — the runtime dependencies a translated file collects, so `ds
//! build` links only what the source actually uses. Extracted from
//! `translator/mod.rs`.

use std::collections::BTreeSet;

use super::append_dep;
use super::helpers::*;
use super::runtime_dep::RuntimeDep;

/// Runtime dependencies a translated file pulls in. Collected during
/// translation so `ds build` only links what the source actually uses: a file
/// that never formats a number to an ES string pulls in no `ryu_js`. Adding a
/// new runtime dep is a variant on [`RuntimeDep`] — the construction sites
/// ([`RuntimeDeps::empty`] / [`RuntimeDeps::with`] / [`RuntimeDeps::merge`]) and
/// the consumers ([`RuntimeDeps::helper_module`] /
/// [`RuntimeDeps::apply_to_cargo_toml`], …) are table-driven over
/// [`RuntimeDep::ALL`].
#[derive(Debug, Clone, Default)]
pub struct RuntimeDeps {
    deps: BTreeSet<RuntimeDep>,
    /// Build-time-resolved degraded module sources: (DsResolver specifier,
    /// source). Emitted as a `static __DS_MODULE_SOURCES` table in
    /// `__ds/engine.rs` so `source_of` reaches every degraded module without a
    /// `register_js_module` stub call — a module with no `export function`
    /// (e.g. `@scope/pkg`'s `b.js`, only `export const`/`class`) still
    /// resolves at runtime.
    js_module_sources: Vec<(String, String)>,
    /// Workspace-member crates this module imports via a bare specifier (e.g.
    /// `@office-open/xml` → `office_open_xml`). In the independent-crate model
    /// these become cargo path dependencies, not merged local modules. Kept out
    /// of the `RuntimeDep` enum so the table-driven `RuntimeDep::ALL` loop
    /// (helpers/cargo) is unaffected.
    path_deps: BTreeSet<String>,
}

impl RuntimeDeps {
    /// An empty dep set — the common case (a plain `.ts` file links nothing).
    pub fn empty() -> Self {
        Self::default()
    }

    /// Add `dep` (builder-style: returns `self` moved).
    pub fn with(mut self, dep: RuntimeDep) -> Self {
        self.deps.insert(dep);
        self
    }

    /// Add `dep` to this set in place.
    pub fn insert(&mut self, dep: RuntimeDep) {
        self.deps.insert(dep);
    }

    /// Record a workspace-member crate imported via a bare specifier — it
    /// becomes a cargo path dependency, not a merged local module. `crate_ident`
    /// is the Rust crate ident (`office_open_xml`, the use-path segment); the
    /// caller converts it to the cargo dep key (`office-open-xml`).
    pub fn add_path_dep(&mut self, crate_ident: &str) {
        self.path_deps.insert(crate_ident.to_string());
    }

    /// The workspace-member crates this project imports — for cargo path-dep
    /// emission in `to_member_toml`.
    pub fn path_deps(&self) -> &BTreeSet<String> {
        &self.path_deps
    }

    /// Record a degraded module's source under its build-time DsResolver
    /// specifier. Deduped by specifier — a module imported through several
    /// paths registers once. The engine's `source_of` falls back to this table
    /// when a specifier is not runtime-registered by a stub, so a module with
    /// no `export function` (no stub emitted) still resolves.
    pub fn add_js_module(&mut self, specifier: &str, source: &str) {
        if !self.js_module_sources.iter().any(|(s, _)| s == specifier) {
            self.js_module_sources
                .push((specifier.to_string(), source.to_string()));
        }
    }

    /// Read-only access to the build-time module source table — for tests and
    /// debugging. Each entry is `(DsResolver specifier, source)`.
    #[cfg(test)]
    pub(crate) fn js_module_sources(&self) -> &[(String, String)] {
        &self.js_module_sources
    }

    /// Whether `dep` is in the set.
    pub fn has(&self, dep: RuntimeDep) -> bool {
        self.deps.contains(&dep)
    }

    /// Readable accessors — `deps.needs_engine()` over `deps.has(RuntimeDep::Engine)`.
    pub fn needs_ryu_js(&self) -> bool {
        self.has(RuntimeDep::RyuJs)
    }
    pub fn needs_serde_json(&self) -> bool {
        self.has(RuntimeDep::SerdeJson)
    }
    pub fn needs_engine(&self) -> bool {
        self.has(RuntimeDep::Engine)
    }
    pub fn needs_array_helper(&self) -> bool {
        self.has(RuntimeDep::ArrayHelper)
    }
    pub fn needs_regress(&self) -> bool {
        self.has(RuntimeDep::Regress)
    }
    pub fn needs_temporal(&self) -> bool {
        self.has(RuntimeDep::Temporal)
    }
    /// A `new Worker(handler)` spawns a worker thread (Direction D).
    pub fn needs_worker(&self) -> bool {
        self.has(RuntimeDep::Worker)
    }

    /// Union another dep set into this one — a project links a runtime dep if
    /// any of its translated files does.
    pub fn merge(&mut self, other: &RuntimeDeps) {
        self.deps.extend(&other.deps);
        for (spec, src) in &other.js_module_sources {
            if !self.js_module_sources.iter().any(|(s, _)| s == spec) {
                self.js_module_sources.push((spec.clone(), src.clone()));
            }
        }
        self.path_deps.extend(other.path_deps.iter().cloned());
    }

    /// The `__ds` helper module source — assembled from whichever helper slices
    /// this dep set flagged (`number_to_string` for `RyuJs`, `array_set` for
    /// `ArrayHelper`), in [`RuntimeDep::ALL`] order. `None` when neither is
    /// needed, so the caller writes nothing and the default build pulls no
    /// `ryu_js`.
    pub fn helper_module(&self) -> Option<String> {
        let mut src = String::from(
            "//! DashScript runtime helpers: ES-compat shims a bare Rust lowering\n//! would get wrong (Number::toString, Array auto-grow).\n\n",
        );
        let mut any = false;
        for d in RuntimeDep::ALL {
            if self.has(d) {
                if let Some(slice) = d.helper() {
                    src.push_str(slice);
                    any = true;
                }
            }
        }
        // The `serde_json::Value` `DsSameValue` impl needs both the trait
        // (`ASSERT_HELPER`, emitted by `Assert`) and `serde_json` (flagged by
        // `SerdeJson`); emit it only when both are present, so a non-JSON assert
        // fixture (no serde_json dep) never references `serde_json::Value`.
        if self.has(RuntimeDep::Assert) && self.has(RuntimeDep::SerdeJson) {
            src.push_str(ASSERT_VALUE_HELPER);
            any = true;
        }
        // `DsResponse::headers` returns a `DsHeaders` (defined in
        // `HEADERS_HELPER`), so a fetch-using fixture needs the Headers slice
        // even when it does not itself lower a `new Headers()` — otherwise the
        // `DsHeaders` return type is undefined (E0433). A `Headers`-only
        // fixture already emits `HEADERS_HELPER` via its own marker; this only
        // fills the gap when `Fetch` is present without `Headers`.
        if self.has(RuntimeDep::Fetch) && !self.has(RuntimeDep::Headers) {
            src.push_str(HEADERS_HELPER);
            any = true;
        }
        // `TextDecoder`'s fatal-mode decode path panics a `DsError` (defined
        // in `ERROR_HELPER`), so an encoding-using fixture needs the Error
        // slice even when it does not itself lower a `throw`/`new Error()` —
        // otherwise the `DsError` reference inside `ENCODING_HELPER` is
        // undefined (E0433). An `Error`-already-active fixture emits
        // `ERROR_HELPER` via its own marker; this only fills the gap when
        // `Encoding` is present without `Error`. Surfaced by the Engine ∧
        // Encoding integration test (a per-function degrade fixture + a
        // static `TextEncoder` function) — the engine builtin wires
        // `register_text_encoding`, but the `DsError` gap is in the static
        // `__ds.rs` either path writes.
        if self.has(RuntimeDep::Encoding) && !self.has(RuntimeDep::Error) {
            src.push_str(ERROR_HELPER);
            any = true;
        }
        any.then_some(src)
    }

    /// The `__ds::engine` compat module source — runs a `.ts` source under an
    /// embedded QuickJS engine — when this dep set flags `Engine`. `None`
    /// otherwise, so the caller writes nothing and pulls no engine dependency.
    /// The build-time module source table (`__DS_MODULE_SOURCES`) is appended
    /// so the runtime `source_of` reaches every degraded module — including
    /// ones with no `export function` (no stub, never runtime-registered).
    pub fn engine_helper_module(&self) -> Option<String> {
        self.needs_engine().then(|| {
            let mut src = ENGINE_HELPER_MODULE.to_string();
            src.push_str("\nstatic __DS_MODULE_SOURCES: &[(&str, &str)] = &[");
            for (spec, source) in &self.js_module_sources {
                src.push_str(&format!("({spec:?}, {source:?}),"));
            }
            src.push_str("];\n");
            // Stamp `wire_web_apis`'s body with one `register_<api>(ctx)?;` call
            // per active Web API RuntimeDep that has an engine builtin, and
            // append each builtin's `fn register_<api>(ctx)` (the Javy pattern:
            // JS shim + native fn delegating to the same `__ds::` impl the
            // static path lowers to). Only APIs the static path already pulled
            // in are registered, so the engine never references a `__ds::` type
            // the crate lacks. The placeholder stays (empty body) when no Web
            // API dep is active — `wire_web_apis` is a no-op then.
            let mut wire_body = String::new();
            let mut builtin_fns = String::new();
            for d in RuntimeDep::ALL {
                if self.has(d) {
                    if let Some((call, fn_src)) = d.engine_builtin() {
                        wire_body.push_str(&format!("    {call}?;\n"));
                        builtin_fns.push_str(fn_src);
                    }
                }
            }
            src = src.replace("/* __DS_WIRE_WEB_APIS_BODY__ */", &wire_body);
            src.push_str(&builtin_fns);
            // engine.rs lives at src/__ds/engine.rs — a child of the `__ds`
            // runtime module — so its builtins (which delegate to
            // `crate::__ds::X` static impls) need the parent module in scope.
            // Only when a builtin is present, so an engine-only crate (no Web
            // API builtins) gets no unused import.
            if !builtin_fns.is_empty() {
                src = src.replacen(
                    "use rquickjs::context::EvalOptions;",
                    "use crate::__ds;\nuse rquickjs::context::EvalOptions;",
                    1,
                );
            }
            src
        })
    }

    /// Append each flagged cargo dep to a generated `Cargo.toml`, creating the
    /// `[dependencies]` section if absent. A no-op for a dep already declared
    /// (e.g. the project declared `cargo:ryu_js`) — so a consumer can call this
    /// unconditionally and let the dep set gate it. A string-level post-process
    /// keeps the dep out of the user's `package.json` — it is a DashScript-
    /// internal runtime need, not a declared project dependency.
    pub fn apply_to_cargo_toml(&self, cargo_toml: &mut String) {
        for d in RuntimeDep::ALL {
            if self.has(d) {
                if let Some(deps) = d.cargo() {
                    for &(pkg, req) in deps {
                        append_dep(cargo_toml, pkg, req);
                    }
                }
            }
        }
    }
}
