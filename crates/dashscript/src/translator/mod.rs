//! oxc AST → idiomatic Rust source, emitted through `syn` + `prettyplease`.
//!
//! Translation is one file per AST category — `declarations`, `functions`,
//! `types`, `expressions`, `bindings` — so each oxc node maps to a `syn` node
//! one-to-one. The `syn` tree is the project's hub: the translator builds it
//! (oxc → syn), `prettyplease` prints it, and the future `bindgen` parses
//! Rust crates into the same `syn` tree (syn → .d.ts) — one AST, two
//! directions. Parsing reuses `oxc_parser`; DashScript never parses itself.

mod analysis;
pub mod bindings;
mod builtins;
mod check;
mod class;
mod classify;
pub mod context;
pub mod declarations;
pub mod dts;
pub mod expressions;
mod flavor;
pub mod functions;
mod globals;
pub mod imports;
pub mod name_table;
pub mod registry;
pub use registry::{FnSignature, InterfaceField};
mod engine_js;
mod runtime_dep;
mod runtime_deps;
pub mod semantic;
pub mod types;
pub use runtime_dep::RuntimeDep;
pub use runtime_deps::RuntimeDeps;

use std::collections::{HashMap, HashSet};

use engine_js::engine_js_source;
use oxc_allocator::Allocator;
use oxc_diagnostics::OxcDiagnostic;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::SourceType;

/// Append `<pkg> = <req>` to a generated `Cargo.toml`'s `[dependencies]`,
/// creating the section if absent. A no-op when the dep is already declared —
/// the caller gates per dep (via [`RuntimeDeps::has`]) and lets this handle the
/// string edit. A string-level post-process keeps these deps out of the user's
/// `package.json` — they are DashScript-internal runtime needs.
fn append_dep(cargo_toml: &mut String, pkg: &str, req: &str) {
    let needle = format!("{pkg} =");
    if cargo_toml.contains(&needle) {
        return;
    }
    let line = format!("{pkg} = {req}\n");
    if let Some(pos) = cargo_toml.find("[dependencies]\n") {
        cargo_toml.insert_str(pos + "[dependencies]\n".len(), &line);
    } else {
        cargo_toml.push_str(&format!("\n[dependencies]\n{line}"));
    }
}

/// Stamp `Assert`/`WptAssert` (+ `Error`) when a degraded body's JS uses the
/// test262/WPT assert family. The static marker probe catches only the Rust
/// `__ds::assert_` text the static path emits; a degraded body keeps
/// `assert.sameValue`/`assert_equals` as JS (in the `__DS_MODULE_JS` const or
/// `run(src)`'s literal), so without this scan the engine never stamps the
/// Assert/WptAssert deps — `register_assert`/`register_wpt_assert` stay unwired
/// and the asserts throw `ReferenceError`, mis-flipping a real mismatch to
/// `unsupported`. `Error` is pulled alongside (mirroring the
/// `translate_program` Assert→Error rule) because `ASSERT_HELPER`'s
/// `assert_throws` routes through `catch_quiet`/`DsError` in `ERROR_HELPER`.
/// The engine's assert shims are pure JS (no `__ds::` delegate), so the static
/// assert helpers emit but stay unused on a degrade-only crate.
/// Stamp the engine-path builtins a degraded function's JS body reaches.
///
/// The static `__ds::` marker probe scans emitted Rust, but a degraded body is
/// JS (`new TextEncoder()`, `assert.sameValue`, `performance.now()`, …) — the
/// Rust probe never sees it, so `wire_web_apis` would skip the matching builtin
/// and the body would hit a `ReferenceError`. This scans the JS body the way
/// the Rust probe scans emitted Rust: the test262/WPT assert families (which
/// also pull `Error`, the shared `catch_quiet`/`DsError` machinery) and the
/// WinterTC Web APIs `wire_web_apis` registers via `engine_builtin()`
/// (`Encoding`/`HrTime`/`Base64`/`Crypto`).
fn stamp_engine_js_body_deps(deps: &mut RuntimeDeps, js: &str) {
    let had = deps.has(RuntimeDep::Assert) || deps.has(RuntimeDep::WptAssert);
    // test262 harness: `sta.js` defines `Test262Error`, `assert.js` throws it
    // via `assert.sameValue`/`notSameValue`/`throws`.
    if js.contains("assert.sameValue")
        || js.contains("assert.notSameValue")
        || js.contains("assert.throws")
        || js.contains("Test262Error")
    {
        deps.insert(RuntimeDep::Assert);
    }
    // WPT testharness: `AssertionError` + `assert_equals`/`true`/`false`/…
    // plus the async/composite entry points (`async_test`/`promise_test`/…
    // — `TESTHARNESS_REJECTED_GLOBALS`, which now degrade) and the composite
    // asserts (`assert_object_equals`/`assert_own_property`/…): a degraded
    // body calling any of these needs `register_wpt_assert` to have supplied
    // the JS shim, or it throws `ReferenceError`.
    if js.contains("assert_equals")
        || js.contains("assert_not_equals")
        || js.contains("assert_true")
        || js.contains("assert_false")
        || js.contains("assert_throws_js")
        || js.contains("assert_approx_equals")
        || js.contains("assert_array_equals")
        || js.contains("assert_object_equals")
        || js.contains("assert_own_property")
        || js.contains("assert_not_own_property")
        || js.contains("assert_inherits")
        || js.contains("assert_readonly")
        || js.contains("assert_implements")
        || js.contains("assert_less")
        || js.contains("assert_greater")
        || js.contains("assert_between")
        || js.contains("async_test")
        || js.contains("promise_test")
        || js.contains("promise_rejects")
        || js.contains("AssertionError")
        // `test(…)` / `setup(…)` / `done()` — the sync testharness entry
        // points. A degraded body using `test(…)` with no other assert keyword
        // still needs `register_wpt_assert` to define `test`/`AssertionError`.
        // Match the bare call (`test(`) rather than a specific body form so
        // `test(() => …)` / `test(function …)` / `test (…)` all land here; a
        // false positive (e.g. `.test(`) only over-registers the shim — the
        // body never calls it, so it is inert.
        || js.contains("test(")
        || js.contains("setup(")
    {
        deps.insert(RuntimeDep::WptAssert);
    }
    if (deps.has(RuntimeDep::Assert) || deps.has(RuntimeDep::WptAssert)) && !had {
        deps.insert(RuntimeDep::Error);
    }
    // WinterTC Web APIs — a degraded body calling `new TextEncoder()` /
    // `performance.now()` / `atob(…)` / `crypto.*` needs the matching engine
    // builtin (registered by `wire_web_apis`) or it throws `ReferenceError`.
    // The static Rust probe misses these (the body is JS, not `__ds::` Rust).
    if js.contains("TextEncoder") || js.contains("TextDecoder") {
        deps.insert(RuntimeDep::Encoding);
    }
    if js.contains("performance.") {
        deps.insert(RuntimeDep::HrTime);
    }
    if js.contains("atob(") || js.contains("btoa(") {
        deps.insert(RuntimeDep::Base64);
    }
    if js.contains("crypto.") {
        deps.insert(RuntimeDep::Crypto);
    }
    // `AbortController`/`AbortSignal`/`EventTarget` — a degraded body reaching
    // any of these (including `new AbortController()`, `signal.aborted`, or a
    // bare `addEventListener`/`dispatchEvent`) needs `register_abort` to have
    // supplied the JS classes, or it throws `ReferenceError`. `AbortController`
    // derives `EventTarget`, so stamping `EventTarget` registers all three.
    if js.contains("AbortController")
        || js.contains("AbortSignal")
        || js.contains("EventTarget")
        || js.contains("addEventListener")
        || js.contains("dispatchEvent")
    {
        deps.insert(RuntimeDep::EventTarget);
    }
    // `$262.agent` — a degraded atomics body needs `register_atomics_agent`
    // to have supplied the `$262` object (start/broadcast/getReport/… +
    // agent-side receiveBroadcast/report/leaving), or `$262.agent.start`
    // throws `ReferenceError`. test262's `$262` is only ever reached via
    // `.agent`, so any `$262` occurrence gates the builtin.
    if js.contains("$262") {
        deps.insert(RuntimeDep::Atomics);
    }
}

/// A `DsTruthy` impl for a user struct/enum — ES objects are always truthy, so
/// the impl is `true` regardless of the type. Emitted for every user type in a
/// file that lowered an `__ds::truthy` call, so a member access on a user-type
/// field in a condition (`if (opts.config)`) resolves instead of E0277.
fn ds_truthy_impl(name: &syn::Ident, generics: &syn::Generics) -> syn::Item {
    // A generic struct/enum's `DsTruthy` impl carries the type params (with the
    // same `Clone` bound the class impl uses) on both the impl and the self type.
    let params: Vec<syn::Ident> = generics.type_params().map(|p| p.ident.clone()).collect();
    if params.is_empty() {
        syn::parse_quote! {
            impl crate::__ds::DsTruthy for #name {
                #[inline]
                fn ds_truthy(&self) -> bool {
                    true
                }
            }
        }
    } else {
        syn::parse_quote! {
            impl<#(#params: Clone),*> crate::__ds::DsTruthy for #name<#(#params),*> {
                #[inline]
                fn ds_truthy(&self) -> bool {
                    true
                }
            }
        }
    }
}

/// A `DsDisplay` impl for a user type — emitted for every user struct/enum in a
/// file that lowered a `__ds::display` call, so a template-literal
/// interpolation or string concatenation of a user-type value (`${opts}`)
/// resolves instead of E0277 (`T: Display`). ES rendering: an object is
/// "[object Object]"; a union enum forwards to the translator-generated
/// `Display` impl it already carries, so the active scalar variant renders.
fn ds_display_impl(item: &syn::Item, display_types: &[String]) -> Option<syn::Item> {
    let (name, generics, body): (&syn::Ident, &syn::Generics, syn::Expr) = match item {
        syn::Item::Struct(s) => (
            &s.ident,
            &s.generics,
            syn::parse_quote!("[object Object]".to_string()),
        ),
        syn::Item::Enum(e) => {
            // A union enum carries a translator-generated `Display` impl, so it
            // forwards to `to_string` and the active scalar variant renders. A
            // user enum without `Display` (e.g. a TS union type alias lowered
            // to a named enum) falls back to ES "[object Object]".
            let body = if display_types.contains(&e.ident.to_string()) {
                syn::parse_quote!(::std::string::ToString::to_string(self))
            } else {
                syn::parse_quote!("[object Object]".to_string())
            };
            (&e.ident, &e.generics, body)
        }
        _ => return None,
    };
    let params: Vec<syn::Ident> = generics.type_params().map(|p| p.ident.clone()).collect();
    Some(if params.is_empty() {
        syn::parse_quote! {
            impl crate::__ds::DsDisplay for #name {
                #[inline]
                fn ds_display(&self) -> String {
                    #body
                }
            }
        }
    } else {
        syn::parse_quote! {
            impl<#(#params: Clone),*> crate::__ds::DsDisplay for #name<#(#params),*> {
                #[inline]
                fn ds_display(&self) -> String {
                    #body
                }
            }
        }
    })
}

mod helpers;

/// A `.ts` file's role in its project — the file-role distinction the
/// architecture's implicit-`main` design hinges on (decision point 8). `ds
/// build` sets it from the package manifest for each file it translates.
///
/// `BinEntry` (the default) lowers top-level executable statements into an
/// implicit `fn main`, the way Node runs an entry script — so a lone file (no
/// `package.json`) and a conformance fixture are always `BinEntry`. `Module`
/// lowers declarations only and rejects top-level executable statements: a
/// module declares an API, it does not run, so a `console.log` at the top of a
/// module file has no entry to land in. The `translate`/`check` entry points
/// default to `BinEntry`; `_as` variants take a role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FileRole {
    /// A bin/lib entry — top-level executable statements collect into an
    /// implicit `fn main` the translator always emits.
    #[default]
    BinEntry,
    /// A module imported by an entry — declarations only; top-level executable
    /// statements are rejected (a module declares, it does not execute).
    Module,
}

/// Translates a TypeScript-flavored `.ts` program into Rust source.
///
/// Stateless except for `extra_optionals`: optional (`?:`) field names
/// gathered from the *other* `.ts` files in a package, merged into each file's
/// per-file `TypeRegistry`. A file translating `opts?.field ?? d` must know
/// whether the imported struct's `field` is optional, but each file builds its
/// own registry — so a package build aggregates optionals once
/// ([`Self::collect_optionals`]) and injects them ([`Self::with_extra_optionals`]),
/// making cross-file optional fields visible to optional-chaining.
#[derive(Default)]
pub struct Translator {
    extra_optionals: HashMap<String, HashSet<String>>,
    /// Interface field signatures (name, translated type, optional flag) from
    /// the *other* `.ts` files in a package — the field-type analogue of
    /// `extra_optionals`. A file translating `obj.field` into the field's inner
    /// type must know the imported interface's field type, but each file builds
    /// its own registry, so a package build aggregates field signatures once
    /// ([`Self::collect_fields`]) and injects them ([`Self::with_extra_fields`]).
    extra_fields: HashMap<String, Vec<InterfaceField>>,
    /// Inline scalar-union enums (`__DsUnion…`) from the *other* `.ts` files in
    /// a package — the union-enum analogue of `extra_fields`. A file
    /// translating an imported interface's union-typed field (`element.text`:
    /// `string | number | boolean`) must recognize the union enum to coerce it
    /// (`to_string()`), but each file builds its own registry, so a package
    /// build aggregates union enums once ([`Self::collect_union_enums`]) and
    /// injects them ([`Self::with_extra_union_enums`]).
    extra_union_enums: HashMap<syn::Ident, syn::ItemEnum>,
    /// Function/const-arrow signatures from the rest of the package (see
    /// [`Self::with_extra_function_signatures`]), merged into each file's
    /// registry so a module-global factory singleton infers its type from a
    /// callee defined in another file.
    extra_function_signatures: HashMap<String, FnSignature>,
}

impl Translator {
    /// Create a translator with default options.
    #[must_use]
    pub fn new() -> Self {
        Self {
            extra_optionals: HashMap::new(),
            extra_fields: HashMap::new(),
            extra_union_enums: HashMap::new(),
            extra_function_signatures: HashMap::new(),
        }
    }

    /// Inject optional (`?:`) field names collected from the rest of the
    /// package, so a file sees imported interfaces' optional fields. Each
    /// file's own optionals are collected via [`Self::collect_optionals`];
    /// `ds build` aggregates them across the import graph and injects the
    /// union here, so `opts?.field` lowers correctly even when `opts`'s type
    /// is declared in another module.
    #[must_use]
    pub fn with_extra_optionals(mut self, optionals: HashMap<String, HashSet<String>>) -> Self {
        self.extra_optionals = optionals;
        self
    }

    /// Inject interface field signatures collected from the rest of the
    /// package, so a file sees imported interfaces' field types. The
    /// field-type analogue of [`Self::with_extra_optionals`]: `ds build`
    /// aggregates field signatures across the import graph and injects them
    /// here, so an optional-field read `f(obj.opt_field)` lowers with the
    /// imported interface's field type even when `obj`'s type is declared in
    /// another module.
    #[must_use]
    pub fn with_extra_fields(mut self, fields: HashMap<String, Vec<InterfaceField>>) -> Self {
        self.extra_fields = fields;
        self
    }

    /// Inject inline union enums collected from the rest of the package, so a
    /// file recognizes imported interfaces' union-typed fields. The union-enum
    /// analogue of [`Self::with_extra_fields`]: `ds build` aggregates union
    /// enums across the import graph and injects them here, so a union field
    /// (`element.text`) coerces correctly even when the field's type is declared
    /// in another module.
    #[must_use]
    pub fn with_extra_union_enums(mut self, unions: HashMap<syn::Ident, syn::ItemEnum>) -> Self {
        self.extra_union_enums = unions;
        self
    }

    /// Inject function/const-arrow signatures collected from the rest of the
    /// package, so a file infers a module-global factory singleton's type from
    /// a callee defined in another file (`createFactory` in a dep). The
    /// signature analogue of [`Self::with_extra_union_enums`].
    #[must_use]
    pub fn with_extra_function_signatures(
        mut self,
        signatures: HashMap<String, FnSignature>,
    ) -> Self {
        self.extra_function_signatures = signatures;
        self
    }

    /// Parse `.ts` source with oxc and translate the AST to Rust source.
    ///
    /// Convenience wrapper around [`Self::translate_with_deps`] that drops the
    /// runtime-dependency report — for callers (tests, LSP) that only want the
    /// Rust text. `ds build` uses [`Self::translate_with_deps`] so the project
    /// links only what the source uses.
    ///
    /// # Errors
    /// Returns an error string if oxc reports parse diagnostics.
    pub fn translate(&self, source: &str) -> Result<String, String> {
        Ok(self.translate_with_deps(source)?.0)
    }

    /// Parse `.ts` source, translate the AST to Rust source, and report the
    /// runtime dependencies the generated code needs. Lowers as
    /// [`FileRole::BinEntry`] — the default for a lone file (always run) and a
    /// conformance fixture.
    ///
    /// The Rust text matches [`Self::translate`]; the second return value is the
    /// set of extra crates / helper modules the translated code references, so
    /// the project emitter can add them to `Cargo.toml` and write the helper
    /// module only when needed.
    ///
    /// # Errors
    /// Returns an error string if oxc reports parse diagnostics.
    pub fn translate_with_deps(&self, source: &str) -> Result<(String, RuntimeDeps), String> {
        self.translate_with_deps_as(source, FileRole::BinEntry)
    }

    /// True when the source uses a construct that degrades to the engine — a
    /// per-function site or a top-level dynamic construct. A project emitter
    /// probes each file once (cheap parse + classify, no translation) to decide
    /// whether the whole project needs cross-file serde derives. A parse error
    /// reads as `false` (the later translate reports it properly).
    #[must_use]
    pub fn uses_engine(&self, source: &str) -> bool {
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, source, SourceType::ts()).parse();
        if !ret.diagnostics.is_empty() {
            return false;
        }
        let program = allocator.alloc(ret.program);
        check::program_uses_engine(program)
    }

    /// True when `source` (a `.js`/`.mjs`/`.cjs` module) declares a class the
    /// static translator cannot lower — a class with a `super_class`
    /// (`extends`). Such a module degrades wholesale to the engine: its
    /// exports become stub fns that call into QuickJS, which runs the real
    /// prototype chain. Only top-level classes are scanned (an npm package's
    /// classes are top-level `export`s); a parse error reads as `false` (the
    /// later translate reports it properly).
    #[must_use]
    pub fn js_module_needs_engine(&self, source: &str) -> bool {
        use oxc_ast::ast::{Declaration, ExportDefaultDeclarationKind, Statement};
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, source, SourceType::ts()).parse();
        if !ret.diagnostics.is_empty() {
            return false;
        }
        let program = allocator.alloc(ret.program);
        let needs_engine = |class: &oxc_ast::ast::Class<'_>| {
            matches!(
                classify::classify_class(class),
                classify::Mapping::DegradeEngine(_)
            )
        };
        program.body.iter().any(|stmt| match stmt {
            Statement::ClassDeclaration(class) => needs_engine(class),
            Statement::ExportNamedDeclaration(exp) => matches!(
                &exp.declaration,
                Some(Declaration::ClassDeclaration(class)) if needs_engine(class)
            ),
            Statement::ExportDefaultDeclaration(exp) => matches!(
                &exp.declaration,
                ExportDefaultDeclarationKind::ClassDeclaration(class) if needs_engine(class)
            ),
            _ => false,
        })
    }

    /// The `(name, param_count)` of each `export function` in a `.js`/`.mjs`/
    /// `.cjs` module — the surface a degraded module's stub fns expose. A
    /// degraded module (one with a class `extends`) emits one stub per exported
    /// function, each forwarding to the engine. A parse error yields none.
    #[must_use]
    pub fn js_export_fns(&self, source: &str) -> Vec<(String, usize)> {
        use oxc_ast::ast::{Declaration, Statement};
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, source, SourceType::ts()).parse();
        if !ret.diagnostics.is_empty() {
            return Vec::new();
        }
        let program = allocator.alloc(ret.program);
        let mut out = Vec::new();
        for stmt in &program.body {
            if let Statement::ExportNamedDeclaration(exp) = stmt {
                if let Some(Declaration::FunctionDeclaration(f)) = &exp.declaration {
                    if let Some(id) = &f.id {
                        out.push((id.name.to_string(), f.params.items.len()));
                    }
                }
            }
        }
        out
    }

    /// Set whether the whole project has an engine-degradation site, so every
    /// translated file derives `Serialize`/`Deserialize` — a type defined in a
    /// non-degraded file may cross a degraded function's marshal boundary in
    /// another file, so its derives are needed project-wide. Set once by the
    /// project emitter before translating any file.
    pub fn set_force_serde_derive(b: bool) {
        functions::set_force_serde_derive(b);
    }

    /// Set whether the next `.ts` file translated degrades wholesale to the
    /// engine — every top-level function runs under `call_module_fn`. Set
    /// per-file by the project emitter when the file transitively imports a
    /// degraded module (a `.js` the static table cannot lower), so its
    /// functions — which depend on engine-only exports — run under the engine.
    pub fn set_whole_module_degrade(b: bool) {
        functions::set_whole_module_degrade(b);
    }

    /// Parse `.ts` source, translate the AST to Rust source, report the runtime
    /// dependencies, and lower according to `role`. [`FileRole::BinEntry`] emits
    /// an implicit `fn main` collecting top-level executable statements;
    /// [`FileRole::Module`] emits declarations only and rejects top-level
    /// executable statements (a module declares an API, it does not run). `ds
    /// build` passes `Module` for a file that is not a package entry.
    ///
    /// # Errors
    /// Returns an error string if oxc reports parse diagnostics, or if `role`
    /// is [`FileRole::Module`] and the file has top-level executable statements.
    pub fn translate_with_deps_as(
        &self,
        source: &str,
        role: FileRole,
    ) -> Result<(String, RuntimeDeps), String> {
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, source, SourceType::ts()).parse();

        if !ret.diagnostics.is_empty() {
            return Err(format!(
                "dashscript: oxc reported {} parse diagnostic(s)",
                ret.diagnostics.len()
            ));
        }

        // Move the program into the arena so the arena, the program, and the
        // semantic analysis all share one lifetime `'a` (the same trick
        // `semantic::analyze_symbols` uses). `with_build_nodes(true)` fills the
        // `symbol_id` / `reference_id` cells on each `BindingIdentifier` /
        // `IdentifierReference` so the translator can resolve any identifier to
        // its `SymbolId` — the identity `NameTable` keys on, replacing the lossy
        // `snake(name)` string fold.
        let program = allocator.alloc(ret.program);
        let sret = SemanticBuilder::new().with_build_nodes(true).build(program);
        let scoping = sret.semantic.into_scoping();
        let mut names = name_table::build(&scoping);

        // Engine-gated compat path. A source using ES dynamic reflection the
        // static translator cannot lower degrades to an embedded QuickJS engine
        // instead of failing `cargo check`. Two granularities:
        //   * a construct at top level (outside any function) has no function
        //     boundary to rewrite, so the whole program runs under the engine
        //     (`run(js_source)`) — the conformance-oracle path;
        //   * a construct inside a top-level `function` degrades only that
        //     function: its body becomes `__ds::engine::call_fn("name", …)`,
        //     keeping the Rust signature, while every other function stays
        //     native Rust. The rest of the file is lowered normally.
        // `program_engine_sites` is the same `collect_unsupported` walk that
        // flags these `unsupported` in `ds lint` — one source of truth for what
        // the engine covers. Default `ds build` output stays pure Rust; only a
        // program that actually uses such a construct pulls the `rquickjs` dep.
        // A module file whose top-level executable statement cannot hoist (a
        // `for` loop filling a `Map` at load time, a `let` holding a function
        // value) has no Rust home — a module declares, it does not run. Degrade
        // the whole module to the engine rather than rejecting
        // (degrade-over-reject; arch decision point 8). This reuses the
        // transitive whole-module-degrade path: every top-level function routes
        // to `call_module_fn`, and the module source carries these top-level
        // statements for the engine to run. The guard clears the thread-local
        // on return so a later translate in the same thread is unaffected.
        let module_exec_unhoistable = if matches!(role, FileRole::Module) {
            let probe_reg = registry::build_registry(&program.body, &names);
            let probe_mt = functions::mutable_top_level_names(&program.body, &names, &probe_reg);
            functions::module_has_unhoistable_exec(&program.body, &names, &probe_mt)
        } else {
            false
        };
        let _degrade_guard =
            (module_exec_unhoistable && !functions::whole_module_degrade()).then(|| {
                functions::set_whole_module_degrade(true);
                functions::WholeModuleDegradeGuard
            });
        let sites = check::program_engine_sites(program);
        if sites.top_level_dynamic {
            // Whole-program `run`. The engine evaluates ECMAScript, so strip the
            // TS type annotations — QuickJS parses JS, not TS. `engine_js_source`
            // (shared with `engine_source`) lets the conformance harness run the
            // exact bytes embedded here without compiling a throwaway project.
            let js_source = engine_js_source(program, &allocator, scoping);
            let src_lit = syn::LitStr::new(&js_source, proc_macro2::Span::call_site());
            let main_item: syn::Item = syn::parse_quote! {
                fn main() {
                    crate::__ds::engine::run(#src_lit);
                }
            };
            let rust = prettyplease::unparse(&syn::File {
                shebang: None,
                attrs: Vec::new(),
                items: vec![main_item],
            });
            let mut deps = RuntimeDeps::empty();
            deps.insert(RuntimeDep::Engine);
            stamp_engine_js_body_deps(&mut deps, &js_source);
            return Ok((rust, deps));
        }
        // Per-function degradation: publish the dynamic-function set so
        // `translate_function` swaps just those bodies for `call_fn` (every
        // other function stays native Rust). The `__DS_MODULE_JS` const and the
        // serde derives are emitted below, after the items are collected.
        let mut dynamic_fns = sites.dynamic_fns;
        // B6-5c: whole-module degrade (transitive — this .ts imports a degraded
        // module, so its functions depend on engine-only exports) → every
        // top-level function runs under the engine. Add all top-level function
        // names so they swap to `call_module_fn`; their JS (carrying the ESM
        // imports) is loaded by the module loader. This is the path around a
        // generic-callable export (e.g. an npm package's `export const sha512 =
        // createHasher(…)`) the translator cannot specialize into a stub: the
        // function's body runs in the engine, which resolves the import itself.
        if functions::whole_module_degrade() {
            for stmt in &program.body {
                if let Some(tlf) = check::top_level_function(stmt) {
                    if let Some(name) = tlf.ts_name() {
                        dynamic_fns.insert(name.to_string());
                    }
                }
            }
        }
        let per_function = !dynamic_fns.is_empty();
        // Always (re)set the thread-local dynamic-function set — even when
        // empty. The conformance harness translates many fixtures in one
        // thread; without a reset, a fixture that degrades its `main` leaves a
        // stale set that rewrites the next fixture's `main` as an engine call,
        // emitting `__ds::engine` references while `needs_engine` stays false.
        functions::set_dynamic_fns(dynamic_fns);
        // B6-5b: a per-function-degraded module whose annotation-stripped JS
        // still carries ESM `import`/`export` cannot run under `call_fn`'s
        // script-mode `eval` (ESM syntax is rejected in script mode). When such
        // a module also has a known import specifier (it is a dep reached via
        // that specifier), its degraded bodies route to `call_module_fn` keyed
        // by the specifier — the module loader resolves the imports — and the
        // module source lands in the runtime dep's static table. B6-5c extends
        // the loader gate to whole-module degrade (its functions call
        // engine-only exports through the module JS, which carries imports).
        let has_esm_binding = program
            .body
            .iter()
            .any(|s| s.as_module_declaration().is_some());
        let needs_loader = has_esm_binding || functions::whole_module_degrade();
        functions::set_module_mode(
            per_function && needs_loader && imports::current_module_specifier().is_some(),
        );

        // Record the file's namespace-import bindings (`import * as ns`) so a
        // reference to `ns` is recognized as a module-path prefix (`ns.foo` →
        // `ns::foo`) rather than a field access. The engine path returns above,
        // so this only runs for the statically-lowered Rust path.
        names.register_namespaces(&program.body);

        // First pass: collect discriminated-union enum shapes so later
        // expression translation can build variant constructors.
        let mut registry = registry::build_registry(&program.body, &names);
        // Merge optional (`?:`) field names gathered from the rest of the
        // package, so a file sees imported interfaces' optionals — a
        // cross-file `opts?.field ?? d` needs to know `field` is optional.
        for (name, fields) in &self.extra_optionals {
            registry
                .structs
                .entry(name.clone())
                .or_insert_with(|| fields.clone());
        }
        // Merge interface field signatures from the rest of the package, so a
        // file sees imported interfaces' field types — a cross-file
        // `f(obj.opt_field)` into the field's inner type needs the field's type,
        // but each file builds its own registry.
        for (name, fields) in &self.extra_fields {
            registry
                .interface_own_fields
                .entry(name.clone())
                .or_insert_with(|| fields.clone());
        }
        // Merge inline union enums from the rest of the package, so a file
        // recognizes imported interfaces' union-typed fields — a cross-file
        // `return element.text` (a union) into a `String` coerces via the
        // union's `Display` impl, but each file builds its own registry.
        for (name, item) in &self.extra_union_enums {
            registry
                .union_enums
                .entry(name.clone())
                .or_insert_with(|| item.clone());
        }
        // Merge function/const-arrow signatures from the rest of the package,
        // so a module-global factory singleton (`const p = createFactory<T>(...)`)
        // infers its type from a callee defined in another file.
        for (name, sig) in &self.extra_function_signatures {
            registry
                .function_signatures
                .entry(name.clone())
                .or_insert_with(|| sig.clone());
        }
        // Escape promotion (A3): a top-level `const` number/boolean literal
        // referenced from a top-level `function` cannot stay in `fn main` (a
        // Rust fn item cannot close over a `main` local), so it is hoisted to a
        // crate-level `const` item. Register the numeric ones in the name table
        // BEFORE any body is translated, so a function that appears before the
        // const in source order still sees it as an `f64` value for number→
        // string routing (Rust items are hoisted; ES top-level bindings are
        // order-independent at the module level).
        //
        // A module file has no `fn main`, so *every* const-expr `const` must
        // promote to a crate item (there is no escape set to compute); the
        // entry path promotes only the ones a function closes over.
        // Top-level `let` bindings mutated from a top-level function cannot
        // lower to an immutable `OnceLock` (they need a `thread_local!`
        // `RefCell`, B3-2) — both the const-item and lazy-static candidate
        // checks exclude them. Computed first because the promoted set below
        // and the lazy-static pre-pass both consult it.
        let mutable_top_level =
            functions::mutable_top_level_names(&program.body, &names, &registry);
        // Entry-file lazy-static hoist set (B3-1b): the non-const-expr
        // `const`/non-mutated `let` candidates a top-level function references —
        // these hoist to a `static OnceLock` + accessor so the function can see
        // them (an unreferenced one stays an `fn main` local). A module hoists
        // every candidate regardless, so it ignores this set.
        let escaped_lazy = functions::escaped_lazy_static_names(
            &program.body,
            &names,
            &registry,
            &mutable_top_level,
        );
        // Entry-file mutable-static hoist set (B3-2): the mutable value `let`
        // candidates a top-level function references — these hoist to a
        // thread-local `RefCell` so the function can see them. A module hoists
        // every candidate (no `fn main`), so it ignores this set.
        let escaped_mutable = functions::escaped_mutable_static_names(
            &program.body,
            &names,
            &registry,
            &mutable_top_level,
        );
        let promoted = if matches!(role, FileRole::Module) {
            functions::all_promotable_const_names(&program.body, &names, &mutable_top_level)
        } else {
            functions::promoted_const_names(&program.body, &names, &registry, &mutable_top_level)
        };
        for s in &program.body {
            if let oxc_ast::ast::Statement::VariableDeclaration(v) = s {
                if let Some((sym, name, kind)) =
                    functions::promotable_const_info(v, &names, &mutable_top_level)
                {
                    if kind.is_number() && promoted.contains(&name) {
                        names.register_number_const(sym);
                    }
                }
            }
        }
        // Lazy static pre-pass: register hoisted non-const-expr `const`/let
        // bindings (an object, a regex, …) so a reference before the definition
        // in source order still emits the accessor call. A module hoists every
        // candidate (no `fn main`); an entry hoists only the ones a function
        // references (B3-1b) — the rest stay `fn main` locals.
        for s in &program.body {
            if !functions::lazy_static_candidate(s, &mutable_top_level, &names) {
                continue;
            }
            let Some(sym) = functions::lazy_static_sym(s, &names) else {
                continue;
            };
            let hoist = matches!(role, FileRole::Module)
                || functions::decl_name(s, &names).is_some_and(|n| escaped_lazy.contains(&n));
            if hoist {
                names.register_lazy_static(sym);
                // Record the cell type so a same-file `n["k"]` index on a
                // file-local lazy static (e.g. an alias `const n = m;`) routes
                // to `n().get(k)` — its cell type is not in the cross-file
                // export table.
                if let Some((_, ty)) =
                    functions::lazy_static_export_info(s, &names, &registry, &mutable_top_level)
                {
                    names.register_lazy_static_cell_type(sym, ty);
                }
            }
        }
        // Register imported lazy-static exports (a cross-file `const`/`let`
        // lowered to an accessor fn in another module) so a reference emits
        // the accessor call (`name()`) rather than a bare identifier. The
        // export table is populated by `project::translate_sources` before the
        // entry translates; a lone-file translate (empty table) registers
        // nothing.
        imports::register_imported_lazy_statics(&program.body, &mut names);
        // Record this file's re-exports (`export { X } from "./m"`) so a
        // sibling `import { X } from "./m"` drops the redundant `use` (the
        // `pub use` already binds X locally) instead of emitting both, which
        // Rust rejects as E0252 (the name defined multiple times).
        imports::register_re_exports(&program.body);
        struct ReExportGuard;
        impl Drop for ReExportGuard {
            fn drop(&mut self) {
                imports::clear_re_exports();
            }
        }
        let _re_export_guard = ReExportGuard;
        // Pure-TS execution semantics: a top-level statement that *runs* in
        // source order (a `const`, an expression, control flow, a throw) does
        // not map to a Rust item — it belongs inside the entry point, the way
        // Node runs a script's top-level statements immediately. Declarations
        // (`function` / `class` / `interface` / `type` / `import` / `export`)
        // still lower to Rust items. Split the body: declarations → items;
        // executable statements → one implicit `fn main` body (or an empty
        // `fn main {}` when there are none — a Rust binary needs an entry).
        let mut items: Vec<syn::Item> = Vec::new();
        // Inline scalar-union enums (`__DsUnion…`) discovered by the registry
        // pre-pass are emitted first, before any item that names them. A
        // `FileRole::Module` skips emission: the entry emits the enum at the
        // crate root, and every reference is `crate::`-prefixed (`types` /
        // `binary` / `object` / `unary`) so a module resolves to that one
        // definition instead of its own (a per-module `enum __DsUnion…` would
        // be a distinct nominal type → E0308 at any cross-module call).
        if !matches!(role, FileRole::Module) {
            let mut union_enum_names: Vec<&syn::Ident> = registry.union_enums.keys().collect();
            union_enum_names.sort();
            items.extend(union_enum_names.into_iter().flat_map(|name| {
                let e = &registry.union_enums[name];
                [
                    syn::Item::Enum(e.clone()),
                    syn::Item::Impl(declarations::union_display_impl(e)),
                ]
            }));
            let mut anon_names: Vec<&syn::Ident> = registry.anon_structs.keys().collect();
            anon_names.sort();
            items.extend(
                anon_names
                    .into_iter()
                    .map(|name| syn::Item::Struct(registry.anon_structs[name].clone())),
            );
        }
        // Fn-alias consts (`const g = f`, `f` a same-file const-arrow fn) lower
        // to `use f as g;` items — a fn value alias renames the fn, no runtime
        // binding (the fn is a static item, so no `OnceLock`). Collected once so
        // a forward alias (alias before the fn in source order) resolves too.
        let const_arrow_names = functions::const_arrow_fn_names(&program.body, &names);
        let mut exec_stmts: Vec<&oxc_ast::ast::Statement> = Vec::new();
        for s in &program.body {
            // whole_module_degrade: every value binding and executable runs
            // under the engine (the module source carries them), so skip the
            // static hoist paths — a `const M = new Map()` must not be emitted
            // as a half-initialized OnceLock alongside the engine's filled map.
            // Declarations (function/class/interface/type/import/export) still
            // lower to items; their function bodies route to `call_module_fn`.
            if functions::whole_module_degrade() && functions::is_executable_top_level(s) {
                exec_stmts.push(s);
                continue;
            }
            // A promoted const-expr `const` lowers to a crate-level `const`
            // item here (escape promotion, A3) — NOT collected into `fn main`,
            // so a top-level function reading it resolves to the item, not a
            // `main` local it cannot see.
            if let Some(item) = functions::promoted_const_item(s, &promoted, &names) {
                items.push(item);
                continue;
            }
            // A module-level non-const-expr `const`/non-mutated `let` (an
            // object, a regex, …) lowers to a lazy static (OnceLock + accessor
            // fn) — see `lazy_static_items`. A module hoists every candidate;
            // an entry hoists only the ones a function references (B3-1b) — the
            // rest stay `fn main` locals (source-order, zero-cost).
            let hoist_lazy = matches!(role, FileRole::Module)
                || functions::decl_name(s, &names).is_some_and(|n| escaped_lazy.contains(&n));
            if hoist_lazy {
                if let Some(lazy_items) =
                    functions::lazy_static_items(s, &names, &registry, &mutable_top_level)
                {
                    items.extend(lazy_items);
                    continue;
                }
            }
            // A mutable module-global value `let` (B3-2) hoists to a thread-local
            // `RefCell` + get/set accessors — a module hoists every candidate, an
            // entry hoists only the ones a function references.
            let hoist_mutable = matches!(role, FileRole::Module)
                || functions::decl_name(s, &names).is_some_and(|n| escaped_mutable.contains(&n));
            if hoist_mutable {
                if let Some((ms_items, setter, optional)) =
                    functions::mutable_static_items(s, &names, &registry, &mutable_top_level)
                {
                    if let Some(sym) = functions::lazy_static_sym(s, &names) {
                        names.register_mutable_static(sym, setter, optional);
                    }
                    items.extend(ms_items);
                    continue;
                }
            }
            // A fn alias (`const g = f`, `f` a same-file const-arrow fn) →
            // `use f as g;` item — a declaration (a name rename), not an
            // executable statement, so it never enters the implicit `fn main`
            // (and never trips the module "may only declare" reject).
            if let Some(alias_item) = functions::fn_alias_use_item(s, &const_arrow_names, &names) {
                items.push(alias_item);
                continue;
            }
            if functions::is_executable_top_level(s) {
                exec_stmts.push(s);
            } else {
                items.extend(functions::translate_statement(s, &registry, &names));
            }
        }
        // The implicit entry analyzes the top-level executable statements the
        // same way a function body is analyzed (mutations, member mutations, use
        // counts, number flavor). Declaration statements are no-ops in the
        // walk, so passing the full `program.body` slice is equivalent to the
        // executable subset. `return_path` is `None` — a top-level `return
        // expr;` cannot yield a value (binary `main` returns `()`); `check`
        // flags it unsupported.
        match role {
            FileRole::BinEntry => {
                let main_item: syn::Item = {
                    let mut locals = context::Locals::new();
                    let analysis = analysis::analyze(
                        &program.body,
                        &names,
                        &registry.mut_methods,
                        &registry.ref_params,
                    );
                    locals.mutated = analysis.mutated;
                    locals.member_mutated = analysis.member_mutated;
                    locals.use_counts = analysis.use_counts;
                    locals.number_flavors = flavor::infer(&program.body, &names);
                    let mut out: Vec<syn::Stmt> = exec_stmts
                        .into_iter()
                        .flat_map(|s| {
                            functions::translate_stmt(
                                s,
                                &mut locals,
                                &registry,
                                &context::Narrow::default(),
                                None,
                                &names,
                            )
                        })
                        .collect();
                    functions::drop_trailing_return(&mut out);
                    // A `setTimeout`/`setInterval`/`queueMicrotask` registers a
                    // callback on the event loop's task (timer) or microtask
                    // queue; ES drains both once main returns (the call stack is
                    // empty, the way a browser drains between tasks). Emit the
                    // drain as the entry's last statement — but only when either
                    // queue was actually referenced. A WPT fixture's call sits
                    // inside the `function main` body (a fn item, not a top-level
                    // exec stmt), so the scan covers both the entry statements
                    // (`out`) and the emitted items (fn bodies), looking for the
                    // `wpt_set_timeout`/`wpt_set_interval`/`wpt_queue_microtask`
                    // the dispatch emits. The microtask checkpoint runs first
                    // (`wpt_drain_microtasks`), then the timer queue
                    // (`wpt_run_timers`, a no-op when no timer was registered);
                    // `wpt_run_timers` itself re-drains microtasks after every
                    // timer fire, so a timer callback that queues a microtask
                    // runs it before the next timer. WPT timer fixtures clamp
                    // every delay to 0, so the timer drain is a deterministic
                    // CPU loop, not a real wait.
                    let needs_event_loop_drain = out
                        .iter()
                        .map(|s| quote::ToTokens::to_token_stream(s).to_string())
                        .chain(
                            items
                                .iter()
                                .map(|i| quote::ToTokens::to_token_stream(i).to_string()),
                        )
                        .any(|t| {
                            t.contains("wpt_set_timeout")
                                || t.contains("wpt_set_interval")
                                || t.contains("wpt_queue_microtask")
                        });
                    if needs_event_loop_drain {
                        out.push(syn::parse_quote!(crate::__ds::wpt_drain_microtasks();));
                        out.push(syn::parse_quote!(crate::__ds::wpt_run_timers();));
                    }
                    // An `async function main(): Promise<void>` lowers to an
                    // `async fn __ds_main` item; the implicit `fn main` must
                    // `.await` its call sites (a top-level `main()` is the
                    // explicit way to run it under pure-TS semantics) and run
                    // under `#[tokio::main]` so the returned future resolves.
                    // Rewriting the calls drops `.await` into the entry block,
                    // so `is_async_entry` below fires on its own; the
                    // `main_is_async` disjunct is a fallback for an `async
                    // function main` declared but never called at the top level.
                    let main_is_async = program.body.iter().any(|s| {
                        matches!(s,
                            oxc_ast::ast::Statement::FunctionDeclaration(f)
                            if f.r#async
                                && f.id.as_ref().is_some_and(|id| id.name.as_str() == "main")
                        )
                    });
                    if main_is_async {
                        for stmt in &mut out {
                            functions::await_main_calls(stmt);
                        }
                    }
                    let block: syn::Block = syn::parse_quote!({ #(#out)* });
                    // A top-level `await` (or a top-level call to an async fn
                    // that awaits) needs an async entry —
                    // `#[tokio::main(flavor = "current_thread")] async fn main`
                    // (single-thread, matching JS's event loop, no `Send` bound
                    // on futures) — so the `.await` resolves under a runtime.
                    // The attribute string MUST match `RuntimeDep::Tokio`'s
                    // marker exactly: the dep scan keys off it to pull `tokio`,
                    // and a mismatch silently drops the crate (E0433) which then
                    // leaves `async fn main` unattributed (E0752). Detected by
                    // scanning the entry block's tokens for `await`; a nested
                    // `async fn` item in the block also contains `await`, which
                    // over-triggers harmlessly (the runtime starts; the nested
                    // fn is simply not awaited).
                    let is_async_entry = main_is_async
                        || quote::ToTokens::to_token_stream(&block)
                            .to_string()
                            .contains("await");
                    if is_async_entry {
                        syn::parse_quote! {
                            #[tokio::main(flavor = "current_thread")]
                            async fn main() #block
                        }
                    } else {
                        syn::parse_quote! {
                            fn main() #block
                        }
                    }
                };
                items.push(main_item);
            }
            FileRole::Module => {
                // Module semantics (arch decision point 8): a module only
                // declares, never executes. Top-level executable statements have
                // no `fn main` to run in (a Node module only exports; it does
                // not run top-level statements unless it is an entry) — reject,
                // rather than silently dropping their side effects.
                if !exec_stmts.is_empty() && !functions::whole_module_degrade() {
                    return Err(
                        "a module file may only declare (function / class / interface / \
                         type / import / export) — top-level executable statements have no \
                         entry to run in; move them into a function, or make this file a \
                         bin entry"
                            .into(),
                    );
                }
                // whole_module_degrade: the top-level executables (collected in
                // `exec_stmts`) run under the engine via the module source, so
                // they are not dropped — a degraded module keeps its side effects.
                // declarations-only: a crate-internal module (src/<stem>.rs)
                // with no `fn main`, brought in by the entry via `mod <stem>;`.
            }
        }
        // Per-function degradation: emit the file's annotation-stripped JS.
        // Module mode carries an ESM `import`/`export`, so it cannot live in a
        // `__DS_MODULE_JS` const eval'd by `call_fn`'s script mode — instead the
        // source is collected into the runtime dep's static table (read by the
        // loader at runtime), and degraded bodies call `call_module_fn`.
        // Script-eval mode keeps the `__DS_MODULE_JS` const that `call_fn`
        // evals before each degraded invocation so the function's helpers are
        // in scope. `engine_js_source` strips the TS annotations (QuickJS parses
        // JS); it consumes `scoping`/`program` (the static pass above is done),
        // so it runs last among the item-emitting passes.
        let mut module_source: Option<String> = None;
        if per_function {
            let module_js = engine_js_source(program, &allocator, scoping);
            if functions::module_mode() {
                module_source = Some(module_js);
            } else {
                let js_lit = syn::LitStr::new(&module_js, proc_macro2::Span::call_site());
                items.push(syn::parse_quote! {
                    /// The whole module's annotation-stripped JS — `__ds::engine::call_fn`
                    /// evals this before each degraded-function invocation so the
                    /// function's helper dependencies are in scope.
                    const __DS_MODULE_JS: &str = #js_lit;
                });
            }
        }
        let mut file = syn::File {
            shebang: None,
            attrs: Vec::new(),
            items,
        };
        if per_function || functions::force_serde_derive() {
            // Every emitted struct/enum derives `Serialize`/`Deserialize` so the
            // `call_fn` argument/return values marshal across the QuickJS
            // boundary. The project-level flag covers a file that is not itself
            // degraded but whose types a degraded function in another file
            // marshals — its types need the derives too.
            declarations::add_serde_derives(&mut file.items);
        }
        // An emit point that routes an `f64` through the ES NumberToString
        // helper writes a `crate::__ds::number_to_string` call into the Rust
        // text; a `JSON.parse`/`JSON.stringify` call inlines `serde_json::`.
        // Either prefix means the generated crate needs the matching crate (and
        // the `__ds` helper module, for ryu_js). Scanning the emitted text
        // (rather than threading a `RefCell<RuntimeDeps>` through every
        // expression) keeps the dep report a pure function of the output — the
        // `__ds::` prefix is a DashScript-reserved namespace a `.ts` source
        // cannot produce any other way, and `serde_json::` likewise only
        // appears via the `JSON` builtin.
        // Probe the unparsed text for runtime-dep markers (a `.ts` source
        // cannot emit `__ds::` / `serde_json::` any other way).
        let probe = prettyplease::unparse(&file);
        let mut deps = RuntimeDeps::empty();
        for d in RuntimeDep::ALL {
            if d.marker().is_some_and(|m| probe.contains(m)) {
                deps.insert(d);
            }
        }
        // ASSERT_HELPER is emitted as one slice for any `assert.*` use (the
        // `__ds::assert_` marker), and that slice carries `assert_throws`, which
        // catch_unwinds via `catch_quiet` and panics a `DsError` on a class
        // mismatch. Both live in ERROR_HELPER — so every assert-bearing fixture
        // pulls ERROR_HELPER alongside, even a `sameValue`-only one (whose
        // `assert_throws` is unused but still must type-check).
        // WPT asserts (`wpt_assert_throws`) share the same `catch_quiet`/
        // `DsError` machinery, so a WPT-only fixture pulls ERROR_HELPER too.
        if deps.has(RuntimeDep::Assert)
            || deps.has(RuntimeDep::WptAssert)
            || deps.has(RuntimeDep::URLPattern)
        {
            deps.insert(RuntimeDep::Error);
        }
        // DS_ABORT_HELPER's `DsAbortSignal` embeds a `DsEventTarget` and its
        // methods take `&DsEvent` callbacks (an `AbortSignal` extends
        // `EventTarget`), so any AbortController-bearing fixture must also pull
        // EVENT_TARGET_HELPER, or `DsEventTarget`/`DsEvent`/`DsEventInit` are
        // E0433. The marker probe catches `__ds::DsAbort` (the controller/signal
        // emit) but not the transitive EventTarget use inside the helper source.
        if deps.has(RuntimeDep::AbortController) {
            deps.insert(RuntimeDep::EventTarget);
            // DS_ABORT_HELPER's `reason()`/`throw_if_aborted()` carry a
            // `DsError` (the default `AbortError` DOMException), so an
            // AbortController-bearing fixture must also pull ERROR_HELPER, or
            // `DsError` is E0433. The marker probe sees the struct emit but
            // not this transitive use inside the helper source.
            deps.insert(RuntimeDep::Error);
        }
        // FILE_HELPER's `DsFile` wraps a `DsBlob` (a `File` extends `Blob`), so
        // any File-bearing fixture must also pull BLOB_HELPER, or `DsBlob` is
        // E0433 inside the DsFile methods. The marker probe catches
        // `__ds::DsFile` but not the transitive `DsBlob` use inside the helper.
        if deps.has(RuntimeDep::File) {
            deps.insert(RuntimeDep::Blob);
        }
        // FORM_DATA_HELPER's `DsFormEntryValue` enum carries a `DsFile` (a
        // `FormData` value is a `string` or a `File`), so any FormData-bearing
        // fixture must also pull FILE_HELPER (→ BLOB_HELPER transitively), or
        // `DsFile`/`DsBlob` are E0433 inside the enum/methods. The marker probe
        // catches `__ds::DsFormData` but not the transitive `DsFile` use.
        if deps.has(RuntimeDep::FormData) {
            deps.insert(RuntimeDep::File);
        }
        // DS_FETCH_HELPER's `DsRequest`/`DsResponse` live in the Fetch helper
        // slice alongside `ds_fetch`, so any Request- or Response-bearing fixture
        // must pull DS_FETCH_HELPER + `reqwest`, or those types are E0433. The
        // marker probe catches `__ds::ds_fetch` (`fetch(url)`/`fetch(request)`
        // lowers to `ds_fetch`/`ds_fetch_request`) but not a `new Request(…)`/
        // `new Response(…)`-only fixture whose sole emit is `DsRequest::new`/
        // `DsResponse::new`. Insert `Fetch` when either appears, the way `File`
        // pulls `Blob` and `FormData` pulls `File`.
        if probe.contains("__ds::DsRequest") || probe.contains("__ds::DsResponse") {
            deps.insert(RuntimeDep::Fetch);
        }
        // `DsPromise<T>` (a `Promise<T>` type annotation — `let p: Promise<T>`
        // or a non-async fn returning `Promise<T>`) lives in DS_PROMISE_HELPER
        // alongside `ds_promise_resolve`/`ds_promise_all`, so a fixture whose
        // emit references the type but not the value-layer `ds_promise_*`
        // functions must pull the slice + `futures`, or `DsPromise` is E0433.
        // The marker probe catches `__ds::ds_promise_` (the value functions) but
        // not a type-annotation-only fixture whose sole emit is `DsPromise<T>` —
        // insert `Promise` the way a `new Request(…)`-only fixture inserts Fetch.
        if probe.contains("__ds::DsPromise") {
            deps.insert(RuntimeDep::Promise);
        }
        // `DsCustomEvent` lives in EVENT_TARGET_HELPER alongside `DsEvent`/
        // `DsEventTarget`/`DsEventInit`, so any CustomEvent-bearing fixture must
        // pull EVENT_TARGET_HELPER, or `DsCustomEvent` is E0433. The marker
        // probe catches `__ds::DsEvent` (a common prefix of the three Event
        // types) but NOT `__ds::DsCustomEvent` (it starts `DsC`, not `DsEvent`)
        // — so a `new CustomEvent(…)`-only fixture injects EventTarget
        // explicitly, the way a `new Request(…)`-only fixture injects Fetch.
        if probe.contains("__ds::DsCustomEvent") {
            deps.insert(RuntimeDep::EventTarget);
        }
        // `self.<method>(…)` / `globalThis.<method>(…)` on the WinterTC global
        // EventTarget lowers to `__ds::wpt_self().<method>(…)`. `wpt_self` lives
        // in EVENT_TARGET_HELPER, but the emit references neither `DsEvent` nor
        // `DsEventTarget`, so the marker probe misses it — a fixture whose only
        // EventTarget use is via `self`/`globalThis` would see `wpt_self` as
        // E0425. Pull the slice the way a `new CustomEvent(…)`-only fixture does.
        if probe.contains("__ds::wpt_self") {
            deps.insert(RuntimeDep::EventTarget);
        }
        // `reportError(e)` lowers to `__ds::ds_report_error` (HTML §5 — dispatch
        // an `"error"` event on the global EventTarget). `ds_report_error` lives
        // in EVENT_TARGET_HELPER alongside `DsEvent`/`wpt_self`, but the emit
        // references neither, so the marker probe misses it — a fixture whose
        // only EventTarget use is `reportError(…)` would see `ds_report_error`
        // as E0425. Pull the slice the way a `wpt_self`-only fixture does.
        if probe.contains("__ds::ds_report_error") {
            deps.insert(RuntimeDep::EventTarget);
        }
        // `done()` lowers to `__ds::wpt_done` (sets the timer drain's DONE
        // flag). `wpt_done` lives in TIMERS_HELPER alongside the queue/drain,
        // so any fixture that calls `done()` — timer or not — pulls the slice
        // (the queue/drain are dead code on a non-timer fixture, but `wpt_done`
        // must resolve). A timer fixture already pulls it via `wpt_set_*`.
        if probe.contains("__ds::wpt_done") {
            deps.insert(RuntimeDep::Timers);
        }
        // Per-function degradation pulls the engine runtime (`rquickjs` + the
        // serde marshal layer) plus `serde` with `derive` (every struct/enum is
        // `Serialize`/`Deserialize` in this mode). The marker probe does not
        // catch this (no `serde_json::`/`__ds::` text is emitted by the static
        // path), so it is inserted explicitly when the route is per-function.
        if per_function {
            deps.insert(RuntimeDep::Engine);
            // A degraded function's JS body (in the `__DS_MODULE_JS` const,
            // inlined into `probe`) may use `assert.sameValue`/`assert_equals`,
            // which the Rust marker probe does not catch — scan it so the engine
            // registers the matching pure-JS assert shim.
            stamp_engine_js_body_deps(&mut deps, &probe);
        }
        // Module mode: the per-function-degraded module's source goes into the
        // static table the engine loader reads at runtime (no `__DS_MODULE_JS`
        // const is emitted in this mode), keyed by the module's import
        // specifier so the loader's `DsResolver` reaches it.
        if let Some(js) = module_source {
            let spec = imports::current_module_specifier().unwrap_or_default();
            deps.add_js_module(&spec, &js);
        }
        // A file that defines a user struct/enum forces the `Truthy` dep even
        // if this file never tests truthiness itself: another module may call
        // `__ds::truthy(&element)` on a type defined here (`types::Element`),
        // and that call resolves only if this file ships the trait definition
        // (in `__ds.rs`) and the per-type impl. `Truthy` is pure-std (no cargo
        // dep), so forcing it adds no external dependency — at worst an unused
        // impl warning on a struct no caller ever tests.
        let has_user_type = file
            .items
            .iter()
            .any(|item| matches!(item, syn::Item::Struct(_) | syn::Item::Enum(_)));
        if has_user_type {
            deps.insert(RuntimeDep::Truthy);
            // A type defined here may be stringified (`__ds::display`) by a
            // cross-module caller too — the same case as `DsTruthy` above.
            // Forcing `Display` ships the per-type `DsDisplay` impl in this
            // file (the trait lives in this crate's `__ds.rs`); pure-std, no
            // cargo dep, so at worst an unused impl on a type no caller
            // stringifies.
            deps.insert(RuntimeDep::Display);
        }
        // If `__ds::truthy` is used, every user struct/enum in this file needs a
        // `DsTruthy` impl — a member access on a user-type field
        // (`if (opts.config)`) would otherwise be E0277. ES objects are always
        // truthy, so the impl is `true`. Gated on the marker so a file that
        // never tests truthiness adds no impls (and pulls no `__ds` module).
        let mut file = file;
        if deps.has(RuntimeDep::Truthy) {
            let impls: Vec<syn::Item> = file
                .items
                .iter()
                .filter_map(|item| match item {
                    syn::Item::Struct(s) => Some(ds_truthy_impl(&s.ident, &s.generics)),
                    syn::Item::Enum(e) => Some(ds_truthy_impl(&e.ident, &e.generics)),
                    _ => None,
                })
                .collect();
            file.items.extend(impls);
        }
        // If `__ds::display` is used (a non-number template interpolation or a
        // string concatenation of a non-string), every user struct/enum in this
        // file needs a `DsDisplay` impl — `${opts}` on a user-type field would
        // otherwise be E0277. Gated on the marker so a file that never
        // stringifies a user type adds no impls.
        if deps.has(RuntimeDep::Display) {
            // `to_string`-forwarding below needs the enum to carry `Display`, so
            // collect which types this file already gives a `Display` impl (the
            // translator emits one for every union enum) and pass the set in:
            // those forward to `to_string`, anything else renders as
            // "[object Object]".
            let display_types: Vec<String> = file
                .items
                .iter()
                .filter_map(|i| match i {
                    syn::Item::Impl(imp) => {
                        let (_, path, _) = imp.trait_.as_ref()?;
                        let last = path.segments.last()?;
                        if last.ident != "Display" {
                            return None;
                        }
                        match imp.self_ty.as_ref() {
                            syn::Type::Path(p) => Some(p.path.segments.last()?.ident.to_string()),
                            _ => None,
                        }
                    }
                    _ => None,
                })
                .collect();
            let impls: Vec<syn::Item> = file
                .items
                .iter()
                .filter_map(|item| ds_display_impl(item, &display_types))
                .collect();
            file.items.extend(impls);
        }
        let rust = prettyplease::unparse(&file);
        Ok((rust, deps))
    }

    /// Check `.ts` source for translatability without emitting Rust.
    ///
    /// Returns syntax errors from `oxc_parser` plus one diagnostic per
    /// top-level statement the translator cannot map. An empty `Vec` means the
    /// file is translatable to valid Rust (as far as DashScript can tell).
    #[must_use]
    pub fn check(&self, source: &str) -> Vec<OxcDiagnostic> {
        check::check(source)
    }

    /// Role-aware translatability check — see [`Self::check`]. [`FileRole::Module`]
    /// additionally flags top-level executable statements (a module declares,
    /// it does not run).
    #[must_use]
    pub fn check_as(&self, source: &str, role: FileRole) -> Vec<OxcDiagnostic> {
        check::check_as(source, role)
    }

    /// Like [`Self::check_as`] but drops `DegradeEngine` (and runtime-`typeof`)
    /// diagnostics — for the conformance harness, which runs degraded fixtures
    /// through the compile path (the production binary's embedded QuickJS)
    /// rather than short-circuiting them as `unsupported`. A degrade is a
    /// translatability *fallback*, not a failure: the function still lowers,
    /// via the engine. Only a hard `Reject` short-circuits.
    #[must_use]
    pub fn check_reject_only(&self, source: &str, role: FileRole) -> Vec<OxcDiagnostic> {
        check::check_reject_only(source, role)
    }

    /// The annotation-stripped ECMAScript the engine compat path runs under
    /// QuickJS. The conformance harness uses this both for `needs_engine`
    /// fixtures (ES reflection the static translator cannot lower) and as the
    /// `cargo check` failure fallback — running the JS directly under QuickJS
    /// rather than reporting a static-only partial. Returns `None` only when
    /// oxc reports parse diagnostics (invalid source); a valid program always
    /// yields JS, mirroring the exact bytes `translate_with_deps` embeds in
    /// `__ds::engine::run`. Whether a fixture routes to the engine at all is
    /// decided by `RuntimeDeps::needs_engine`, not here.
    #[must_use]
    pub fn engine_source(&self, source: &str) -> Option<String> {
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, source, SourceType::ts()).parse();
        if !ret.diagnostics.is_empty() {
            return None;
        }
        let program = allocator.alloc(ret.program);
        let sret = SemanticBuilder::new().with_build_nodes(true).build(program);
        let scoping = sret.semantic.into_scoping();
        Some(engine_js_source(program, &allocator, scoping))
    }

    /// The local `.ts` modules this file imports (`import { x } from "./other"`
    /// → `other`), for `ds build` to assemble one Rust module per dependency.
    #[must_use]
    pub fn imports(&self, source: &str) -> Vec<imports::ImportRef> {
        imports::collect_imports(source)
    }

    /// Collect this file's interface/type-alias optional (`?:`) field names,
    /// for cross-file sharing via [`Self::with_extra_optionals`]. A package
    /// build calls this on every `.ts` in the import graph, aggregates the
    /// results, and injects the union — so a file translating `opts?.field`
    /// sees an imported interface's optional fields even though each file
    /// builds its own `TypeRegistry`.
    ///
    /// # Errors
    /// Returns an error string if oxc reports parse diagnostics.
    pub fn collect_optionals(
        &self,
        source: &str,
    ) -> Result<HashMap<String, HashSet<String>>, String> {
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, source, SourceType::ts()).parse();
        if !ret.diagnostics.is_empty() {
            return Err(format!(
                "dashscript: oxc reported {} parse diagnostic(s)",
                ret.diagnostics.len()
            ));
        }
        let program = allocator.alloc(ret.program);
        let sret = SemanticBuilder::new().with_build_nodes(true).build(program);
        let names = name_table::build(sret.semantic.scoping());
        let registry = registry::build_registry(&program.body, &names);
        Ok(registry.structs.into_iter().collect())
    }

    /// Collect this file's interface field signatures (name, translated type,
    /// optional flag), for cross-file sharing via [`Self::with_extra_fields`].
    /// The field-type analogue of [`Self::collect_optionals`]: a package build
    /// aggregates them across the import graph and injects the union, so a file
    /// translating `obj.field` into the field's inner type sees an imported
    /// interface's field type even though each file builds its own
    /// `TypeRegistry`.
    ///
    /// # Errors
    /// Returns an error string if oxc reports parse diagnostics.
    pub fn collect_fields(
        &self,
        source: &str,
    ) -> Result<HashMap<String, Vec<InterfaceField>>, String> {
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, source, SourceType::ts()).parse();
        if !ret.diagnostics.is_empty() {
            return Err(format!(
                "dashscript: oxc reported {} parse diagnostic(s)",
                ret.diagnostics.len()
            ));
        }
        let program = allocator.alloc(ret.program);
        let sret = SemanticBuilder::new().with_build_nodes(true).build(program);
        let names = name_table::build(sret.semantic.scoping());
        let registry = registry::build_registry(&program.body, &names);
        Ok(registry.interface_own_fields.into_iter().collect())
    }

    /// Collect this file's inline scalar-union enums (`__DsUnion…`), for
    /// cross-file sharing via [`Self::with_extra_union_enums`]. The union-enum
    /// analogue of [`Self::collect_fields`]: a package build aggregates them
    /// across the import graph and injects them, so a file translating an
    /// imported interface's union-typed field recognizes the union even though
    /// each file builds its own `TypeRegistry`.
    ///
    /// # Errors
    /// Returns an error string if oxc reports parse diagnostics.
    pub fn collect_union_enums(
        &self,
        source: &str,
    ) -> Result<HashMap<syn::Ident, syn::ItemEnum>, String> {
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, source, SourceType::ts()).parse();
        if !ret.diagnostics.is_empty() {
            return Err(format!(
                "dashscript: oxc reported {} parse diagnostic(s)",
                ret.diagnostics.len()
            ));
        }
        let program = allocator.alloc(ret.program);
        let sret = SemanticBuilder::new().with_build_nodes(true).build(program);
        let names = name_table::build(sret.semantic.scoping());
        let registry = registry::build_registry(&program.body, &names);
        Ok(registry.union_enums)
    }

    /// Collect this file's function/const-arrow signatures (name, type params,
    /// return type), for cross-file sharing via
    /// [`Self::with_extra_function_signatures`]. The signature analogue of
    /// [`Self::collect_union_enums`]: a package build aggregates them across
    /// the import graph and injects them, so a module-global factory singleton
    /// (`const p = createFactory<T>(...)`) infers its type from a callee in a
    /// dep even though each file builds its own registry.
    ///
    /// # Errors
    /// Returns an error string if oxc reports parse diagnostics.
    pub fn collect_function_signatures(
        &self,
        source: &str,
    ) -> Result<HashMap<String, FnSignature>, String> {
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, source, SourceType::ts()).parse();
        if !ret.diagnostics.is_empty() {
            return Err(format!(
                "dashscript: oxc reported {} parse diagnostic(s)",
                ret.diagnostics.len()
            ));
        }
        let program = allocator.alloc(ret.program);
        let sret = SemanticBuilder::new().with_build_nodes(true).build(program);
        let names = name_table::build(sret.semantic.scoping());
        let registry = registry::build_registry(&program.body, &names);
        Ok(registry.function_signatures)
    }

    /// The lazy-static exports of a `.ts` source — each export's accessor name
    /// (`snake(TS export name)`) mapped to its `OnceLock` cell value type (the
    /// `T` in `OnceLock<T>`). `project::translate_sources` aggregates these
    /// across the import graph so a consumer file recognizes an imported lazy
    /// static (the `use` path, the accessor call, a `HashMap` index). A parse
    /// error yields an empty map.
    #[must_use]
    pub fn collect_lazy_static_exports(&self, source: &str) -> HashMap<String, syn::Type> {
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, source, SourceType::ts()).parse();
        if !ret.diagnostics.is_empty() {
            return HashMap::new();
        }
        let program = allocator.alloc(ret.program);
        let sret = SemanticBuilder::new().with_build_nodes(true).build(program);
        let names = name_table::build(sret.semantic.scoping());
        let registry = registry::build_registry(&program.body, &names);
        let mutable_top_level =
            functions::mutable_top_level_names(&program.body, &names, &registry);
        let mut out = HashMap::new();
        for stmt in &program.body {
            if let Some((name, ty)) =
                functions::lazy_static_export_info(stmt, &names, &registry, &mutable_top_level)
            {
                out.insert(name, ty);
            }
        }
        out
    }

    /// Translate a `.d.ts` declaration source to a Rust module body — each
    /// `interface`/`type` becomes a `pub` struct/alias. A pure `.d.ts` (an
    /// `@types/*` package with no sibling `.js`) carries types only, so a
    /// value import surfaces as a `cargo check` "cannot find function"
    /// honestly. Used by `ds build` when a dependency resolves to a `.d.ts`.
    #[must_use]
    pub fn translate_dts(&self, source: &str) -> String {
        dts::translate_dts(source)
    }

    /// The `declare function` signatures in a `.d.ts` source — the (name, param
    /// types, return type) of each declared function, with unmappable TS types
    /// degraded to `serde_json::Value`. A degraded `.js` module's stub emitter
    /// specializes a stub fn's signature from its sibling `.d.ts` when every
    /// type is marshal-safe, so a static call site stays type-correct. A parse
    /// error yields none.
    #[must_use]
    pub fn dts_fn_signatures(&self, source: &str) -> Vec<dts::DtsFnSig> {
        dts::dts_fn_signatures(source)
    }

    /// The inline scalar-union enums (`__DsUnion…`) a `.ts` file's type
    /// positions introduce, each as `(name, rust_text)` where `rust_text` is
    /// the enum plus its `Display` impl. `ds build` collects these across the
    /// entry and every dependency so a multi-file package emits each enum once
    /// at the crate root — a dependency's union that the entry never names
    /// directly still resolves, rather than leaving the module pointing at a
    /// missing `crate::__DsUnion…`.
    #[must_use]
    pub fn union_enum_items(&self, source: &str) -> Vec<(String, String)> {
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, source, SourceType::ts()).parse();
        let program = allocator.alloc(ret.program);
        let sret = SemanticBuilder::new().with_build_nodes(true).build(program);
        let names = name_table::build(sret.semantic.scoping());
        let mut registry = registry::build_registry(&program.body, &names);
        // Merge inline union enums from the rest of the package, so this
        // returns the same union set the entry actually emits at its crate root
        // (`translate_with_deps` merges `extra_union_enums` into its registry
        // too). Without this, a package build's dedup set would miss a dep's
        // unions and re-prepend them, defining each twice (E0428).
        for (name, item) in &self.extra_union_enums {
            registry
                .union_enums
                .entry(name.clone())
                .or_insert_with(|| item.clone());
        }
        // When the project has an engine-degradation site, the union enums and
        // anon structs hoisted here to the crate root (lone-file mode) may cross
        // a degraded function's `call_fn` marshal boundary, so they need serde
        // derives — the same project-wide flag `translate_with_deps` honors for
        // a file's own items.
        let force = functions::force_serde_derive();
        let mut items: Vec<(String, String)> = registry
            .union_enums
            .into_values()
            .map(|e| {
                let name = e.ident.to_string();
                let display = declarations::union_display_impl(&e);
                // A union enum lives at the crate root, where a cross-module
                // caller may stringify it (`__ds::display(&union_value)`). It
                // already carries a `Display` impl (above), so its `DsDisplay`
                // forwards to `to_string` and the active scalar variant renders.
                let ds_display: syn::ItemImpl = {
                    let ident = &e.ident;
                    syn::parse_quote! {
                        impl crate::__ds::DsDisplay for #ident {
                            #[inline]
                            fn ds_display(&self) -> String {
                                ::std::string::ToString::to_string(self)
                            }
                        }
                    }
                };
                let mut ui = vec![
                    syn::Item::Enum(e),
                    syn::Item::Impl(display),
                    syn::Item::Impl(ds_display),
                ];
                if force {
                    declarations::add_serde_derives(&mut ui);
                }
                let text = prettyplease::unparse(&syn::File {
                    shebang: None,
                    attrs: Vec::new(),
                    items: ui,
                });
                (name, text)
            })
            .collect();
        items.extend(registry.anon_structs.into_values().map(|s| {
            let name = s.ident.to_string();
            let mut si = vec![syn::Item::Struct(s)];
            if force {
                declarations::add_serde_derives(&mut si);
            }
            let text = prettyplease::unparse(&syn::File {
                shebang: None,
                attrs: Vec::new(),
                items: si,
            });
            (name, text)
        }));
        items.sort_by(|a, b| a.0.cmp(&b.0));
        items
    }

    /// The bare-crate imports in a `.ts` file (`import { X } from "crate"`),
    /// each with its `.ts` byte span. Used by `ds lsp` to resolve
    /// go-to-definition on an import specifier to the crate's `~/.cargo` source.
    #[must_use]
    pub fn crate_imports(&self, source: &str) -> Vec<imports::CrateImport> {
        imports::collect_crate_imports(source)
    }

    /// The locally declarable names in a `.ts` file (`function`, `interface`,
    /// `type`, `export`, `import`), each with its binding byte span. Used by
    /// `ds lsp` for in-file go-to-definition (everything but crate imports).
    #[must_use]
    pub fn declarations(&self, source: &str) -> Vec<imports::LocalSymbol> {
        imports::collect_declarations(source)
    }

    /// Whether the `.ts` source declares a top-level `function main()`.
    ///
    /// Under pure-TS execution semantics, `function main` is an ordinary
    /// declaration (renamed `__ds_main`); the translator always emits an
    /// implicit `fn main`. So this reports only whether a binding named `main`
    /// was declared — it no longer gates the binary entry. AST-level (not a
    /// substring scan), so `main_loop` or a `"fn main"` string literal cannot
    /// trip it.
    #[must_use]
    pub fn has_main(&self, source: &str) -> bool {
        imports::has_main(source)
    }

    /// Symbol-level analysis for one `.ts` file: every declaration's span,
    /// kind, and resolved references (read/write). Powers LSP find-references /
    /// rename with **symbol-level precision** — two same-named bindings in
    /// different scopes are distinct symbols, so renaming one never touches the
    /// other. Returns an owned snapshot that borrows nothing (the parse arena is
    /// released). An empty table means the file failed to parse.
    #[must_use]
    pub fn symbols(&self, source: &str) -> semantic::SymbolTable {
        semantic::analyze_symbols(source)
    }
}

#[cfg(test)]
mod tests;
