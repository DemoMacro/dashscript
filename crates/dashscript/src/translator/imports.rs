//! `.ts` module imports. A relative import (`import { x } from "./other"`)
//! resolves to a local `.ts` file, so `ds build` emits one Rust module per
//! dependency (the matching `mod` declarations and `use` aliases). A `cargo:`
//! import (`import { X } from "cargo:serde"`) names a Cargo crate added via
//! `ds add`: it is not a local file (so it is excluded from module assembly
//! below) but still lowers to `use serde::X` — see [`module_ident`]. A bare
//! specifier (`lodash`) has no resolver — `check` reports it.

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    BindingIdentifier, BindingPattern, Declaration, ExportSpecifier, Function,
    ImportDeclarationSpecifier, ModuleExportName, Statement,
};
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType, Span};
use syn::{parse_quote, Ident};

use super::{bindings, semantic::SymbolKind};

thread_local! {
    /// Bare specifiers of workspace members that resolve (via a `node_modules`
    /// symlink to a local `src/`) into a `mod` of this crate. Like a relative
    /// `./m` import, they lower to `crate::mod`, not a bare `mod`: under Rust
    /// 2018 path clarity a bare `use mod::x` resolves at the crate root but not
    /// from a submodule, so the `crate::` form works everywhere. Set once by
    /// `project::translate_sources` before the entry translates, so the entry
    /// and each recursive dep emit `crate::` paths; cleared when the translate
    /// ends. Registry specifiers (`.pnpm` store, plain dirs) are not recorded.
    static WORKSPACE_DEPS: std::cell::RefCell<std::collections::HashSet<String>> =
        std::cell::RefCell::new(std::collections::HashSet::new());
}

/// Record the import graph's workspace-member specifiers (set once before the
/// entry translates), so [`mod_use_path`] routes them to `crate::<mod>`. See
/// [`WORKSPACE_DEPS`].
pub(crate) fn set_workspace_deps(deps: std::collections::HashSet<String>) {
    WORKSPACE_DEPS.with(|c| {
        let mut set = c.borrow_mut();
        set.clear();
        set.extend(deps);
    });
}

/// Clear the workspace-dep set when a `translate_sources` run ends (success or
/// error), so it does not leak into a later translate. See [`WORKSPACE_DEPS`].
pub(crate) fn clear_workspace_deps() {
    WORKSPACE_DEPS.with(|c| c.borrow_mut().clear());
}

thread_local! {
    /// Bare specifiers of workspace members translated as independent crates
    /// (cargo path deps) by `project::translate_project`. Unlike
    /// [`WORKSPACE_DEPS`] — which routes a member or `.js` stub *merged into this
    /// crate* as `crate::<mod>` (the lone-file merge model) — these are sibling
    /// crates reached through the extern prelude, so a `use` names the crate
    /// ident bare: `use ds_office_openSxml::X`. Set once per member before its
    /// files translate, so every file emits the bare-extern path; cleared when
    /// the member's translate ends.
    static WORKSPACE_MEMBER_CRATES: std::cell::RefCell<std::collections::HashSet<String>> =
        std::cell::RefCell::new(std::collections::HashSet::new());
}

/// Record the workspace members this member imports as independent extern
/// crates (set once before the member's files translate), so [`mod_use_path`]
/// routes them to a bare extern-crate `use`. See [`WORKSPACE_MEMBER_CRATES`].
pub(crate) fn set_workspace_member_crates(deps: std::collections::HashSet<String>) {
    WORKSPACE_MEMBER_CRATES.with(|c| {
        let mut set = c.borrow_mut();
        set.clear();
        set.extend(deps);
    });
}

/// Clear the member-crate set when a `translate_project` member run ends, so it
/// does not leak into a later translate. See [`WORKSPACE_MEMBER_CRATES`].
pub(crate) fn clear_workspace_member_crates() {
    WORKSPACE_MEMBER_CRATES.with(|c| c.borrow_mut().clear());
}

thread_local! {
    /// Cross-file lazy-static exports visible to the file being translated:
    /// each export's accessor name (`snake(TS export name)` — the name the
    /// `OnceLock` accessor fn takes) mapped to its cell value type (the `T` in
    /// `OnceLock<T>`). Set once by `project::translate_sources`, aggregated
    /// across the whole import graph before the entry translates, so a consumer
    /// file recognizes an imported lazy static instead of treating the export
    /// name as a type or a plain `const` item: the `use` path takes the snake
    /// accessor name (`use crate::a::m`, not `use crate::a::M`), a reference
    /// emits the accessor call, and a `HashMap` index lowers to `.get(…)`.
    /// Cleared when the translate ends. A lone file (no `package.json`) never
    /// sets this, so single-file translation is unaffected.
    static LAZY_STATIC_EXPORTS: std::cell::RefCell<std::collections::HashMap<String, syn::Type>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// Record the import graph's lazy-static exports (set once before the entry
/// translates), so [`named_use_tree`] routes a value import to its accessor fn
/// and [`is_hashmap_local`] recognizes an imported `HashMap` cell. See
/// [`LAZY_STATIC_EXPORTS`].
pub(crate) fn set_lazy_static_exports(map: std::collections::HashMap<String, syn::Type>) {
    LAZY_STATIC_EXPORTS.with(|c| {
        let mut m = c.borrow_mut();
        m.clear();
        m.extend(map);
    });
}

/// Clear the lazy-static export map when a `translate_sources` run ends
/// (success or error), so it does not leak into a later translate. See
/// [`LAZY_STATIC_EXPORTS`].
pub(crate) fn clear_lazy_static_exports() {
    LAZY_STATIC_EXPORTS.with(|c| c.borrow_mut().clear());
}

/// Whether `accessor_name` (the snake-folded export name) names a cross-file
/// lazy-static export visible to the current file. The accessor fn a
/// `use crate::…::name;` imports is named `snake(TS export name)`, so the
/// caller passes that folded form.
pub(crate) fn is_lazy_static_export(accessor_name: &str) -> bool {
    LAZY_STATIC_EXPORTS.with(|c| c.borrow().contains_key(accessor_name))
}

/// The cell value type (`T` in `OnceLock<T>`) of a cross-file lazy-static
/// export, so a consumer can lower `m["k"]` to `m().get(k)` when the cell holds
/// a `HashMap`, or annotate a same-module alias `const x = m;` with the cell
/// type. `None` for any name that is not a lazy-static export.
pub(crate) fn lazy_static_export_type(accessor_name: &str) -> Option<syn::Type> {
    LAZY_STATIC_EXPORTS.with(|c| c.borrow().get(accessor_name).cloned())
}

/// Register each imported lazy-static export's local binding as a lazy static,
/// so a reference emits the accessor call (`name()`) rather than a bare
/// identifier. Walked from the parsed statements after `build` (each import
/// specifier's `local` carries the `BindingIdentifier` whose `symbol_id` cell
/// `SemanticBuilder` filled) — mirrors `NameTable::register_namespaces`. A
/// lone-file translate (empty export table) registers nothing.
pub(crate) fn register_imported_lazy_statics(
    body: &[Statement],
    names: &mut super::name_table::NameTable<'_>,
) {
    use oxc_ast::ast::ImportDeclarationSpecifier;
    let map = LAZY_STATIC_EXPORTS.with(|c| c.borrow().clone());
    if map.is_empty() {
        return;
    }
    for stmt in body {
        let Statement::ImportDeclaration(imp) = stmt else {
            continue;
        };
        let Some(specs) = imp.specifiers.as_ref() else {
            continue;
        };
        for spec in specs {
            let ImportDeclarationSpecifier::ImportSpecifier(s) = spec else {
                continue;
            };
            let imported = module_export_name_str(&s.imported);
            if map.contains_key(bindings::snake(&imported).to_string().as_str()) {
                if let Some(sym) = s.local.symbol_id.get() {
                    names.register_lazy_static(sym);
                }
            }
        }
    }
}

thread_local! {
    /// The workspace member the file currently being translated lives in
    /// (`Some("member_crate")` while translating a file reached through a
    /// workspace-member barrel; `None` for the entry's own package). A
    /// relative import (`./types`) inside a member carries the member prefix so
    /// two same-stem files in different members lower to distinct mods
    /// (`crate::member_crate_types` vs the entry's own `crate::types`), the
    /// cross-package stem-collision fix. A bare workspace specifier already
    /// encodes its member, so it is unaffected. Set by
    /// `project::translate_dep` around each dep's translate; cleared after.
    static CURRENT_MEMBER: std::cell::RefCell<Option<String>> =
        const { std::cell::RefCell::new(None) };
}

/// Set the workspace member the next dep translate runs under. See
/// [`CURRENT_MEMBER`].
pub(crate) fn set_current_member(member: Option<String>) {
    CURRENT_MEMBER.with(|c| *c.borrow_mut() = member);
}

/// Clear the member when a dep translate ends (success or error), so it does
/// not leak into a later translate. See [`CURRENT_MEMBER`].
pub(crate) fn clear_current_member() {
    CURRENT_MEMBER.with(|c| *c.borrow_mut() = None);
}

/// The workspace member the file currently being translated lives in, or
/// `None` for the entry's own package. See [`CURRENT_MEMBER`].
pub(crate) fn current_member() -> Option<String> {
    CURRENT_MEMBER.with(|c| c.borrow().clone())
}

thread_local! {
    /// The DsResolver specifier the file currently being translated is imported
    /// under (e.g. `Some("@scope/pkg")` while translating a dep reached via that
    /// import specifier). A per-function-degraded `.ts` module whose
    /// annotation-stripped JS still carries ESM `import`/`export … from` cannot
    /// run under `call_fn`'s script-mode `ctx.eval` (ESM imports are not parsed
    /// in script mode), so the translator switches its degraded bodies to
    /// `call_module_fn` keyed by this specifier — the module loader resolves the
    /// imports. Set by `project::translate_dep` around each dep's translate;
    /// cleared after.
    static CURRENT_MODULE_SPECIFIER: std::cell::RefCell<Option<String>> =
        const { std::cell::RefCell::new(None) };
}

/// Set the import specifier the next dep translate runs under. See
/// [`CURRENT_MODULE_SPECIFIER`].
pub(crate) fn set_current_module_specifier(spec: Option<String>) {
    CURRENT_MODULE_SPECIFIER.with(|c| *c.borrow_mut() = spec);
}

/// Clear the specifier when a dep translate ends (success or error), so it
/// does not leak into a later translate. See [`CURRENT_MODULE_SPECIFIER`].
pub(crate) fn clear_current_module_specifier() {
    CURRENT_MODULE_SPECIFIER.with(|c| *c.borrow_mut() = None);
}

/// The import specifier the file currently being translated is reached under,
/// or `None` when it has none (an entry, or a translate outside `ds build`).
/// See [`CURRENT_MODULE_SPECIFIER`].
pub(crate) fn current_module_specifier() -> Option<String> {
    CURRENT_MODULE_SPECIFIER.with(|c| c.borrow().clone())
}

thread_local! {
    /// Per-file emit-name overrides for relative import specifiers, set by
    /// `project::translate_dep` around each dep's translate. A barrel
    /// (`locking/index.ts`) and a same-stem definition file
    /// (`locking/locking.ts`) both flatten to `src/locking.rs` if deduped by
    /// specifier-derived mod name, so the defn is suffixed
    /// (`locking__ds_defn.rs`) and the barrel's `pub use crate::locking::X`
    /// (a self-reference otherwise) is rerouted to `crate::locking__ds_defn::X`.
    /// The map key is the verbatim import source as it appears in the file
    /// being translated (`./locking`), so [`mod_use_path`] can resolve it
    /// without re-running module resolution; the value is the suffixed emit
    /// name. The member-prefix logic in [`mod_use_path`] still applies on
    /// top, so a defn reached inside a workspace member lowers to
    /// `crate::<member>_locking__ds_defn`. Cleared when the dep translate
    /// ends. Empty for a barrel-free package (no collisions), so single-file
    /// and no-barrel translation is unaffected.
    static EMIT_NAME_OVERRIDES: std::cell::RefCell<std::collections::HashMap<String, String>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// Set the per-file emit-name overrides for the next dep translate. See
/// [`EMIT_NAME_OVERRIDES`].
pub(crate) fn set_emit_name_overrides(map: std::collections::HashMap<String, String>) {
    EMIT_NAME_OVERRIDES.with(|c| {
        let mut m = c.borrow_mut();
        m.clear();
        m.extend(map);
    });
}

/// Clear the overrides when a dep translate ends (success or error), so it
/// does not leak into a later translate. See [`EMIT_NAME_OVERRIDES`].
pub(crate) fn clear_emit_name_overrides() {
    EMIT_NAME_OVERRIDES.with(|c| c.borrow_mut().clear());
}

/// The suffixed emit name for an import source, when the source resolves to
/// a definition file that shares its stem with a barrel (the
/// `locking/locking.ts` + `locking/index.ts` collision). `None` for sources
/// that keep their specifier-derived name (the common case). See
/// [`EMIT_NAME_OVERRIDES`].
pub(crate) fn emit_name_override(source: &str) -> Option<String> {
    EMIT_NAME_OVERRIDES.with(|c| c.borrow().get(source).cloned())
}

/// A `.ts` import of a local module: the Rust module name (`other`) and the
/// original source string (`"./other"`).
#[derive(Debug, Clone)]
pub struct ImportRef {
    /// Snake-cased Rust module name, derived from the source's file stem.
    pub module: String,
    /// The verbatim import source (`"./other"`).
    pub source: String,
    /// `import type` is erased at compile time (no Rust `use`), so it is no
    /// runtime module dependency — emit still records the dep (the type may be
    /// used in a value position like a function signature, so the module must
    /// still be on the crate), but cycle detection ignores it.
    pub is_type_only: bool,
}

/// The local modules a `.ts` file imports, in source order. Used by `ds build`
/// to emit one `src/<module>.rs` per dependency. A re-export (`export * from
/// "./m"` / `export { x } from "./m"`) names the same dependency an `import`
/// would, so it is collected too — otherwise a barrel `index.ts` that only
/// re-exports would emit `pub use crate::m::*` with no `mod m;` and no
/// `src/m.rs`, leaving every re-export unresolved (E0432).
pub(crate) fn collect_imports(source: &str) -> Vec<ImportRef> {
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, SourceType::ts()).parse();
    ret.program
        .body
        .iter()
        .filter_map(|stmt| {
            // A relative import (`./other`) resolves to a local `.ts` file; a
            // bare specifier (`lodash`) resolves to `node_modules/<pkg>` — both
            // are assembled into `mod` decls. A `cargo:` import names a Rust
            // crate (no assembled module), so it is excluded.
            let (src, is_type_only): (&str, bool) = match stmt {
                Statement::ImportDeclaration(imp) => {
                    (&imp.source.value, !imp.import_kind.is_value())
                }
                Statement::ExportAllDeclaration(exp) => (&exp.source.value, false),
                Statement::ExportNamedDeclaration(exp) => {
                    // `export { x } from "./m"` (a re-export) carries a source;
                    // a local `export { x }` does not — only the former is a dep.
                    (&exp.source.as_ref()?.value, false)
                }
                _ => return None,
            };
            if src.starts_with("cargo:") {
                return None;
            }
            let module = module_ident(src)?.to_string();
            Some(ImportRef {
                module,
                source: src.to_string(),
                is_type_only,
            })
        })
        .collect()
}

/// The Rust module name for an import source. Three families, aligned with
/// Deno's `npm:`/`jsr:`/`node:` markers (`.temp/architecture-proposal.md`):
/// - `cargo:adler` → the crate's module ident (`adler`; `cfg-if` → `cfg_if` —
///   a `use` path may not contain a hyphen).
/// - `./other` → the local file stem (`other`).
/// - a bare specifier (`lodash`, `my-pkg`, `@scope/pkg`) → an npm module ident
///   (`lodash`, `my_pkg`, `scope_pkg`). The translator emits
///   `use <ident>::foo;`; `ds build` resolves it to `node_modules/<pkg>` — a
///   `.ts` entry translates as a module, a `.js` entry errors honestly (engine
///   integration is deferred). Resolution is a build-pipeline concern, so
///   `check` (pure, no filesystem) passes a bare import rather than flagging
///   it: whether it lowers to valid Rust depends on the target, the third
///   layer of the correctness chain.
pub(crate) fn module_ident(source: &str) -> Option<Ident> {
    if let Some(rest) = source.strip_prefix("cargo:") {
        Some(bindings::crate_mod(rest))
    } else if source.starts_with('.') {
        let stem = source.rsplit(['/', '\\']).next()?;
        let stem = stem.trim_end_matches(".ts");
        if stem.is_empty() || stem == "." || stem == ".." {
            return None;
        }
        Some(bindings::snake_module(stem))
    } else {
        Some(bare_module_ident(source))
    }
}

/// The `use` path for an import/export source, split by code origin: a local
/// relative import → `crate::app::<mod>`; a workspace-member crate (a sibling
/// crate built independently by `translate_project`) → bare `<crate_ident>`
/// (extern prelude); a merged in-crate dep (a `.js` stub a lone file pulls in)
/// → `crate::<mod>`; a `cargo:` crate → bare `<mod>` (extern prelude); a bare
/// npm specifier → `crate::third_party::<mod>`.
pub(crate) fn mod_use_path(source: &str, mod_ident: &Ident) -> syn::Path {
    if source.starts_with('.') {
        // Local relative import → rooted under `app/`. The override carries
        // the target's full crate-local path (with `app::` baked in by
        // collect_member_overrides / collect_emit_overrides), so a suffixed
        // defn behind a barrel reroutes correctly and a multi-segment tree path
        // resolves — used verbatim to avoid doubling a member prefix. The
        // member branch (translate_sources merge model) and the fallback both
        // root under `app/` too.
        if let Some(path) = emit_name_override(source) {
            return syn::parse_str(&format!("crate::{path}"))
                .unwrap_or_else(|_| parse_quote!(crate::app::#mod_ident));
        }
        if let Some(member) = current_member() {
            let prefixed = Ident::new(&format!("{member}_{}", mod_ident), mod_ident.span());
            return parse_quote!(crate::app::#prefixed);
        }
        parse_quote!(crate::app::#mod_ident)
    } else if WORKSPACE_MEMBER_CRATES.with(|c| c.borrow().contains(source)) {
        // A bare specifier resolving to a sibling workspace member translated
        // as an independent crate (a cargo path dep built by
        // `translate_project`) — reached bare through the extern prelude, so
        // the `use` names the crate ident verbatim (`ds_office_openSxml`),
        // matching the crate name / path-dep key / member cache dir.
        parse_quote!(#mod_ident)
    } else if WORKSPACE_DEPS.with(|c| c.borrow().contains(source)) {
        // A bare specifier merged *into this crate* as a `mod` (the lone-file
        // path: a registry `.js` package degrades to a stub `mod`), reached as
        // `crate::<mod>` from any submodule. Distinct from a workspace member
        // crate above — that is a sibling crate, not a mod of this one.
        parse_quote!(crate::#mod_ident)
    } else if source.starts_with("cargo:") {
        // An extern crate (`cargo:serde`) — bare use via the extern prelude.
        parse_quote!(#mod_ident)
    } else {
        // A bare npm specifier lives under `third_party/<segment-path>`,
        // preserving the specifier's directory structure (the scope→name and
        // pkg→subpath `/` boundaries). The use path mirrors that tree (relative,
        // like the original single-segment form): `@noble/hashes/sha2.js` →
        // `third_party::noble::hashes::sha2Djs`.
        let path = format!(
            "third_party::{}",
            npm_third_party_module_path(source).replace('/', "::")
        );
        syn::parse_str(&path).unwrap_or_else(|_| parse_quote!(third_party))
    }
}

/// An npm package name → a DashScript crate ident that is **injective** over
/// npm's package-name charset, so two distinct npm names never collapse to one
/// Rust ident. A flat `-`/`_`/`.` → `_` map is lossy: `office-open-xml`,
/// `office_open_xml`, and `@office-open/xml` would all become `office_open_xml`,
/// colliding with one another and with a cargo-native crate of that name. The
/// `ds_` prefix separates npm-origin crates from cargo-native crates
/// (`cargo:serde` → `serde`, no prefix — see [`module_ident`]'s `cargo:` arm),
/// so the npm and cargo ecosystems cannot shadow one another.
///
/// npm forbids uppercase letters in package names (registry rule), so uppercase
/// escape markers are unforgeable by any legal name and the map is injective:
///   - `@` (leading scope marker) — dropped
///   - `[a-z0-9]` — as-is
///   - `-` → `_`   (npm's common separator)
///   - `_` → `U`   (legal but rare mid-name)
///   - `.` → `D`   (legal but rare mid-name)
///   - `/` → `S`   (scope→name separator)
///   - any other char (uppercase in a legacy pkg, unicode, a non-leading `@`)
///     → `X{6-hex}` — fixed-width, `X` unused elsewhere, so still injective.
///
/// `@office-open/xml` → `ds_office_openSxml`; `office-open-xml` →
/// `ds_office_open_xml`; `@office_open/xml` → `ds_officeUopenSxml`;
/// `@types/node` → `ds_typesSnode`. The result is used verbatim as the cargo
/// `[package].name`, the path-dep key, the member cache dir, and the `use` path
/// segment — they all agree because the name is already a legal ident (no `-`,
/// so cargo does no `-`→`_` munging).
pub(crate) fn npm_to_ds_ident(name: &str) -> String {
    let stripped = name.trim_start_matches('@');
    let mut out = String::with_capacity(stripped.len() + 4);
    out.push_str("ds_");
    for c in stripped.chars() {
        match c {
            '-' => out.push('_'),
            '_' => out.push('U'),
            '.' => out.push('D'),
            '/' => out.push('S'),
            c if c.is_ascii_lowercase() || c.is_ascii_digit() => out.push(c),
            // Outside npm's legal charset: hex-escape so the map stays injective
            // even for legacy uppercase names. `X` is unused by the rules above
            // and the 6-digit field is fixed-width, so this cannot collide.
            c => out.push_str(&format!("X{:06x}", u32::from(c))),
        }
    }
    out
}

/// A bare npm specifier → a `/`-joined multi-segment emit path under
/// `third_party/`, preserving the specifier's directory structure (the
/// scope→name and pkg→subpath boundaries). Each segment is escaped with the
/// same injective rules as [`npm_to_ds_ident`] but WITHOUT flattening `/` (the
/// `/` is the directory boundary, not escaped), and the file extension is
/// preserved via `.`→`D` (`sha2.js`→`sha2Djs`) so `cache.mjs`/`.cjs`/`.js`
/// coexisting as siblings stay distinct. A digit-leading segment (a subpath
/// filename, never a package name) is prefixed `N` so it stays a legal ident
/// without colliding with `-`→`_`. Unlike [`npm_to_ds_ident`] (which a workspace
/// member keeps `ds_`-prefixed as a real cargo crate), a transpiled non-member
/// dep lives in the isolated `third_party/` namespace and needs no prefix.
pub(crate) fn npm_third_party_module_path(source: &str) -> String {
    source
        .trim_start_matches('@')
        .split('/')
        .map(escape_npm_segment)
        .collect::<Vec<_>>()
        .join("/")
}

/// Escape one path segment of an npm specifier to a legal, injective Rust ident
/// fragment. The rules mirror [`npm_to_ds_ident`]'s per-character map but
/// without the `ds_` prefix or `/`→`S` (the caller splits on `/`); a digit
/// leading the segment gets an `N` prefix so the fragment is a valid ident and
/// cannot collide with a `-`→`_` escape.
fn escape_npm_segment(seg: &str) -> String {
    let mut out = String::with_capacity(seg.len() + 1);
    let chars: Vec<char> = seg.chars().collect();
    if chars.first().is_some_and(|c| c.is_ascii_digit()) {
        out.push('N');
    }
    for c in chars {
        match c {
            '-' => out.push('_'),
            '_' => out.push('U'),
            '.' => out.push('D'),
            c if c.is_ascii_lowercase() || c.is_ascii_digit() => out.push(c),
            c => out.push_str(&format!("X{:06x}", u32::from(c))),
        }
    }
    out
}

/// A bare npm specifier (`lodash`, `@scope/pkg`, `@scope/pkg/sub`) → one valid
/// Rust module ident via [`npm_to_ds_ident`] (injective, `ds_`-prefixed). The
/// result is the crate ident a `use` path names, and it matches the cargo
/// `[package].name` / path-dep key / member cache dir exactly.
fn bare_module_ident(source: &str) -> Ident {
    bindings::crate_mod(&npm_to_ds_ident(source))
}

/// The local binding of a named or default import — `import { foo }` and
/// `import foo` — in the form the imported item has in its module: a binding
/// starting uppercase names a type (interface/type alias, kept PascalCase);
/// otherwise it names a value (function, snake_cased). A namespace import
/// (`import * as ns`) is excluded — it needs its own lowering, tracked
/// separately.
pub(crate) fn named_local(spec: &ImportDeclarationSpecifier) -> Option<Ident> {
    let local = match spec {
        ImportDeclarationSpecifier::ImportSpecifier(s) => &s.local,
        ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => &s.local,
        ImportDeclarationSpecifier::ImportNamespaceSpecifier(_) => return None,
    };
    Some(casing_ident(&local.name))
}

/// Type-vs-value casing for an import name: an uppercase-first name (an
/// interface/type alias) keeps PascalCase; otherwise snake_cased (a function or
/// value). The same rule applies to both the imported name (what the source
/// module exports) and the local binding — a Rust `use` of a type is a type, a
/// `use` of a value is a value.
fn casing_ident(name: &str) -> Ident {
    if name.chars().next().is_some_and(char::is_uppercase) {
        bindings::type_ident(name)
    } else {
        bindings::snake(name)
    }
}

/// One `use` tree for a named or default import — a bare `foo`, or
/// `foo as fooA` when the local binding renames the imported item. A namespace
/// import (`import * as ns`) returns `None` here: it has no in-group form and
/// is emitted as its own `use mod as ns;` item. The path segment is the
/// imported name (what the source module exports); the alias is the local
/// binding. When they match (no `as`), a bare name keeps the rendered output
/// brace-free (`use other::foo`, not `use other::{foo as foo}`).
pub(crate) fn named_use_tree(spec: &ImportDeclarationSpecifier) -> Option<syn::UseTree> {
    match spec {
        ImportDeclarationSpecifier::ImportNamespaceSpecifier(_) => None,
        ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
            // A default import has no separate imported name — the local binding
            // names the crate item directly, so a bare tree (path == alias).
            Some(use_tree_from(&s.local.name, &s.local.name))
        }
        ImportDeclarationSpecifier::ImportSpecifier(s) => {
            let imported = module_export_name_str(&s.imported);
            // An imported lazy static's accessor fn is `snake(imported)`, not
            // the type-cased export name — `use crate::a::m`, not
            // `use crate::a::M`. The local binding snake-folds too: it names a
            // value (the accessor fn), not a type, so the body reference
            // (`of_reference` → snake) resolves to the accessor call site.
            if is_lazy_static_export(&bindings::snake(&imported).to_string()) {
                Some(snake_use_tree(&imported, &s.local.name))
            } else {
                Some(use_tree_from(&imported, &s.local.name))
            }
        }
    }
}

/// One `use` tree for a named export specifier — `export { foo }` (bare) or
/// `export { foo as bar }` (rename). The path segment is the `local` name (in
/// the source module, or the local binding when there is no `from`); the alias
/// is the `exported` name exposed to importers — the mirror of an import's
/// `imported` → `local` pair.
pub(crate) fn export_use_tree(spec: &ExportSpecifier) -> syn::UseTree {
    let local = module_export_name_str(&spec.local);
    let exported = module_export_name_str(&spec.exported);
    // A re-exported lazy static's accessor fn is `snake(local)`, mirroring
    // [`named_use_tree`]'s import-side guard — a barrel `export { MY_CONST }`
    // lowers to `pub use crate::m::my_const`, not `::MY_CONST`. The import
    // graph's lazy-static exports are set once before the entry translates, so
    // the table is already complete when a barrel's re-export lowers.
    if is_lazy_static_export(&bindings::snake(&local).to_string()) {
        snake_use_tree(&local, &exported)
    } else {
        use_tree_from(&local, &exported)
    }
}

/// The Rust alias ident for an `export * as <name>` namespace re-export — the
/// `name` a `pub use mod as <name>;` exposes, with the type-vs-value casing.
pub(crate) fn export_alias_ident(name: &ModuleExportName) -> Ident {
    casing_ident(&module_export_name_str(name))
}

/// A `ModuleExportName` as a plain string — the three oxc forms (an identifier
/// name, an identifier reference, or a string literal) all carry a `.name` /
/// `.value`. Shared by import and export specifier lowering.
fn module_export_name_str(name: &ModuleExportName) -> String {
    match name {
        ModuleExportName::IdentifierName(id) => id.name.to_string(),
        ModuleExportName::IdentifierReference(id) => id.name.to_string(),
        ModuleExportName::StringLiteral(s) => s.value.to_string(),
    }
}

/// A `use` tree from a (path, alias) name pair: a bare `name` when they match,
/// else `path as alias`. Shared by import (`imported` → `local`) and export
/// (`local` → `exported`) lowering — both names take the type-vs-value casing.
fn use_tree_from(path: &str, alias: &str) -> syn::UseTree {
    use syn::UseTree;
    let path_ident = casing_ident(path);
    let alias_ident = casing_ident(alias);
    if path_ident == alias_ident {
        UseTree::Name(syn::UseName { ident: alias_ident })
    } else {
        UseTree::Rename(syn::UseRename {
            ident: path_ident,
            as_token: Default::default(),
            rename: alias_ident,
        })
    }
}

/// A `use` tree for an imported lazy-static accessor — both the path (the
/// accessor fn name) and the alias (the local binding) are *value* names, so
/// both snake-fold. Unlike [`use_tree_from`] (type-vs-value casing), a
/// lazy-static export name like `M` lowers to the accessor `m`, and the local
/// binding — also a value, not a type — snake-folds too, so the body reference
/// (`of_reference` → snake) resolves to the accessor call site.
fn snake_use_tree(path: &str, alias: &str) -> syn::UseTree {
    use syn::UseTree;
    let path_ident = bindings::snake(path);
    let alias_ident = bindings::snake(alias);
    if path_ident == alias_ident {
        UseTree::Name(syn::UseName { ident: alias_ident })
    } else {
        UseTree::Rename(syn::UseRename {
            ident: path_ident,
            as_token: Default::default(),
            rename: alias_ident,
        })
    }
}

/// The local alias of a namespace import (`import * as ns`) — snake_cased, the
/// name the body uses as a module-path prefix (`ns.foo` → `ns::foo`). `None`
/// when the specifiers hold no namespace import.
pub(crate) fn namespace_local(specs: &[ImportDeclarationSpecifier]) -> Option<Ident> {
    specs.iter().find_map(|spec| {
        if let ImportDeclarationSpecifier::ImportNamespaceSpecifier(ns) = spec {
            Some(bindings::snake(&ns.local.name))
        } else {
            None
        }
    })
}

/// One symbol brought in by a `cargo:` import (`import { X } from "cargo:crate"`),
/// in the form the translator emits in the Rust `use` clause, plus the byte
/// span of the local binding in the `.ts` source — so the language server can
/// map a cursor position onto the symbol.
#[derive(Debug, Clone)]
pub struct CrateImportSymbol {
    /// The symbol name as it appears in the emitted `use crate::NAME;`
    /// (PascalCase types kept; values snake_cased — same rule as `named_local`).
    pub name: String,
    /// The `.ts` byte span of the local binding, for cursor hit-testing.
    pub span: Span,
}

/// A `cargo:` import (`import { X } from "cargo:serde"`) — not a local `.ts`
/// file but a crate fetched via `ds add`. The module ident is hyphen-normalized
/// (`cfg-if` → `cfg_if`); each symbol name matches what the translator writes
/// in the `use` clause.
#[derive(Debug, Clone)]
pub struct CrateImport {
    /// The crate module ident (`serde`, `cfg_if`) used as the `use` path.
    pub module: String,
    /// The symbols imported from this crate, with their `.ts` byte spans.
    pub symbols: Vec<CrateImportSymbol>,
    /// The `.ts` byte span of the import source string (`"cargo:adler"`), for
    /// cursor hit-testing on the crate name (go-to-definition → crate root).
    pub source_span: Span,
}

/// The `cargo:` imports in a `.ts` file (`import { X } from "cargo:crate"`),
/// with each symbol's `.ts` byte span. Used by `ds lsp` to resolve a
/// go-to-definition request on an import specifier to the crate's source.
pub(crate) fn collect_crate_imports(source: &str) -> Vec<CrateImport> {
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, SourceType::ts()).parse();
    ret.program
        .body
        .iter()
        .filter_map(|stmt| {
            let Statement::ImportDeclaration(imp) = stmt else {
                return None;
            };
            // Only `cargo:` imports are crate imports — a bare specifier is an
            // unsupported npm import, a relative import is a local `.ts` module.
            imp.source.value.strip_prefix("cargo:")?;
            let module = module_ident(&imp.source.value)?.to_string();
            let symbols = imp
                .specifiers
                .as_ref()?
                .iter()
                .filter_map(|spec| {
                    let local = match spec {
                        ImportDeclarationSpecifier::ImportSpecifier(s) => &s.local,
                        ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => &s.local,
                        ImportDeclarationSpecifier::ImportNamespaceSpecifier(_) => return None,
                    };
                    let name = named_local(spec)?.to_string();
                    Some(CrateImportSymbol {
                        name,
                        span: local.span,
                    })
                })
                .collect();
            Some(CrateImport {
                module,
                symbols,
                source_span: imp.source.span,
            })
        })
        .collect()
}

/// A locally declarable name — `function`, `interface`, `type`, an `export`ed
/// form, or an `import` binding — with the byte span of its binding. Used by
/// `ds lsp` for in-file go-to-definition (the rust-analyzer backend handles
/// crate imports; this handles everything declared inside the `.ts` file).
#[derive(Debug, Clone)]
pub struct LocalSymbol {
    /// The bound name as written in `.ts` (e.g. `foo`, `Point`).
    pub name: String,
    /// The `.ts` byte span of the binding identifier.
    pub span: Span,
    /// What the symbol declares — drives the document-symbol icon and hover.
    pub kind: SymbolKind,
    /// A function's parameter list and return type (source slices), for
    /// signature help and hover. `None` for non-functions.
    pub signature: Option<Signature>,
    /// The full declaration span (`interface Point { … }`, `type Id = …`),
    /// for hover to show the complete type. `None` when the hover is a
    /// signature or header (functions, imports).
    pub decl_span: Option<Span>,
}

/// A function's signature as written in `.ts` — parameter names, their type
/// annotation (verbatim source slice, e.g. `number`, `string[]`), and the
/// return type. Powers LSP signature help and hover for user functions.
#[derive(Debug, Clone)]
pub struct Signature {
    pub params: Vec<ParamInfo>,
    pub return_type: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ParamInfo {
    pub name: String,
    pub type_text: Option<String>,
    pub optional: bool,
}

impl Signature {
    /// `(name: type, opt?: type): return` — the one-line signature used by
    /// document-symbol detail, hover, and signature-help labels. An untyped
    /// parameter renders as `any`; a missing return type renders as `void`.
    pub fn label(&self) -> String {
        let params: Vec<String> = self.params.iter().map(render_param).collect();
        let ret = self
            .return_type
            .clone()
            .unwrap_or_else(|| "void".to_string());
        format!("({}): {}", params.join(", "), ret)
    }
}

/// One parameter rendered as `name: type` (or `name?: type`, `name: any`).
fn render_param(p: &ParamInfo) -> String {
    let ty = p.type_text.clone().unwrap_or_else(|| "any".to_string());
    if p.optional {
        format!("{}?: {}", p.name, ty)
    } else {
        format!("{}: {}", p.name, ty)
    }
}

/// Whether the `.ts` source declares a top-level `function main()`.
///
/// Under pure-TS execution semantics, `function main` is an ordinary
/// declaration — it is renamed `__ds_main` and does **not** itself become the
/// cargo entry. The translator always emits an implicit `fn main` that collects
/// the file's top-level executable statements (empty for a declarations-only
/// file). This predicate therefore no longer decides whether a binary entry
/// exists; it only reports whether a binding literally named `main` was
/// declared, for callers that still want that signal. AST-level, so a
/// `main_loop` helper or a `"fn main"` string literal never trips a match.
pub(crate) fn has_main(source: &str) -> bool {
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, SourceType::ts()).parse();
    ret.program.body.iter().any(has_main_stmt)
}

/// One statement declares `function main` (bare, or `export function main`).
fn has_main_stmt(stmt: &Statement) -> bool {
    match stmt {
        Statement::FunctionDeclaration(f) => is_named_main(&f.id),
        Statement::ExportNamedDeclaration(exp) => matches!(
            &exp.declaration,
            Some(Declaration::FunctionDeclaration(f)) if is_named_main(&f.id)
        ),
        _ => false,
    }
}

fn is_named_main(id: &Option<BindingIdentifier>) -> bool {
    id.as_ref().is_some_and(|id| id.name.as_str() == "main")
}

/// Every declarable name in a `.ts` file with its binding span, kind, and (for
/// functions) signature. Used by `ds lsp` for in-file go-to-definition,
/// document symbols, hover, and signature help.
pub(crate) fn collect_declarations(source: &str) -> Vec<LocalSymbol> {
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, SourceType::ts()).parse();
    let mut out = Vec::new();
    for stmt in &ret.program.body {
        collect_from_statement(stmt, source, &mut out);
    }
    out
}

fn collect_from_statement(stmt: &Statement, source: &str, out: &mut Vec<LocalSymbol>) {
    match stmt {
        Statement::FunctionDeclaration(f) => extend_binding(
            &f.id,
            SymbolKind::Function,
            function_signature(f, source),
            out,
        ),
        Statement::TSInterfaceDeclaration(i) => {
            out.push(symbol_decl(&i.id, SymbolKind::Interface, i.span()))
        }
        Statement::TSTypeAliasDeclaration(t) => {
            out.push(symbol_decl(&t.id, SymbolKind::TypeAlias, t.span()))
        }
        Statement::ImportDeclaration(imp) => {
            if let Some(specs) = &imp.specifiers {
                for spec in specs {
                    let local = match spec {
                        ImportDeclarationSpecifier::ImportSpecifier(s) => Some(&s.local),
                        ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => Some(&s.local),
                        ImportDeclarationSpecifier::ImportNamespaceSpecifier(_) => None,
                    };
                    if let Some(local) = local {
                        out.push(LocalSymbol {
                            name: local.name.to_string(),
                            span: local.span,
                            kind: SymbolKind::Other,
                            signature: None,
                            decl_span: None,
                        });
                    }
                }
            }
        }
        Statement::ExportNamedDeclaration(exp) => {
            if let Some(decl) = &exp.declaration {
                collect_from_declaration(decl, source, out);
            }
        }
        _ => {}
    }
}

fn collect_from_declaration(decl: &Declaration, source: &str, out: &mut Vec<LocalSymbol>) {
    match decl {
        Declaration::FunctionDeclaration(f) => extend_binding(
            &f.id,
            SymbolKind::Function,
            function_signature(f, source),
            out,
        ),
        Declaration::TSInterfaceDeclaration(i) => {
            out.push(symbol_decl(&i.id, SymbolKind::Interface, i.span()))
        }
        Declaration::TSTypeAliasDeclaration(t) => {
            out.push(symbol_decl(&t.id, SymbolKind::TypeAlias, t.span()))
        }
        _ => {}
    }
}

fn extend_binding(
    id: &Option<BindingIdentifier>,
    kind: SymbolKind,
    signature: Option<Signature>,
    out: &mut Vec<LocalSymbol>,
) {
    if let Some(id) = id {
        out.push(symbol_with(id, kind, signature));
    }
}

fn symbol_with(
    id: &BindingIdentifier,
    kind: SymbolKind,
    signature: Option<Signature>,
) -> LocalSymbol {
    LocalSymbol {
        name: id.name.to_string(),
        span: id.span,
        kind,
        signature,
        decl_span: None,
    }
}

/// A symbol with a full declaration span — interface/type aliases, so hover
/// can show the complete definition (`interface Point { x: number }`).
fn symbol_decl(id: &BindingIdentifier, kind: SymbolKind, decl_span: Span) -> LocalSymbol {
    LocalSymbol {
        name: id.name.to_string(),
        span: id.span,
        kind,
        signature: None,
        decl_span: Some(decl_span),
    }
}

/// A function's signature from its AST: parameter names, their type annotation
/// (verbatim source slice, e.g. `number`, `string[]`), and the return type.
/// Destructuring parameters (`{ x }`) show as `_`. Slices the source by the
/// type's span so the text matches what the developer wrote.
fn function_signature(f: &Function, source: &str) -> Option<Signature> {
    let params = f
        .params
        .items
        .iter()
        .map(|fp| {
            let name = match &fp.pattern {
                BindingPattern::BindingIdentifier(id) => id.name.to_string(),
                _ => "_".to_string(),
            };
            ParamInfo {
                name,
                type_text: fp
                    .type_annotation
                    .as_ref()
                    .map(|ta| source[ta.type_annotation.span()].to_string()),
                optional: fp.optional,
            }
        })
        .collect();
    Some(Signature {
        params,
        return_type: f
            .return_type
            .as_ref()
            .map(|ta| source[ta.type_annotation.span()].to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn function_signature_with_params_and_return() {
        let src = "function greet(name: string, times?: number): string { return name; }";
        let decls = collect_declarations(src);
        let greet = decls.iter().find(|d| d.name == "greet").expect("greet");
        assert_eq!(greet.kind, SymbolKind::Function);
        let sig = greet.signature.as_ref().expect("signature");
        assert_eq!(sig.params.len(), 2);
        assert_eq!(sig.params[0].name, "name");
        assert_eq!(sig.params[0].type_text.as_deref(), Some("string"));
        assert_eq!(sig.params[1].name, "times");
        assert!(sig.params[1].optional, "times is optional");
        assert_eq!(sig.return_type.as_deref(), Some("string"));
    }

    #[test]
    fn interface_and_type_alias_kinds_no_signature() {
        let src = "interface Point { x: number } type Id = number;";
        let decls = collect_declarations(src);
        let p = decls.iter().find(|d| d.name == "Point").expect("Point");
        assert_eq!(p.kind, SymbolKind::Interface);
        assert!(p.signature.is_none());
        let id = decls.iter().find(|d| d.name == "Id").expect("Id");
        assert_eq!(id.kind, SymbolKind::TypeAlias);
        assert!(id.signature.is_none());
    }

    #[test]
    fn import_binding_is_other() {
        let src = "import { foo } from \"./other\";";
        let decls = collect_declarations(src);
        let foo = decls.iter().find(|d| d.name == "foo").expect("foo");
        assert_eq!(foo.kind, SymbolKind::Other);
        assert!(foo.signature.is_none());
    }

    #[test]
    fn function_without_return_type() {
        let src = "function f(x: number) { return x; }";
        let decls = collect_declarations(src);
        let f = decls.iter().find(|d| d.name == "f").expect("f");
        let sig = f.signature.as_ref().expect("sig");
        assert!(sig.return_type.is_none());
        assert_eq!(sig.params[0].type_text.as_deref(), Some("number"));
    }

    #[test]
    fn signature_label_renders_params_and_return() {
        let src = "function greet(name: string, times?: number): string { return name; }";
        let decls = collect_declarations(src);
        let greet = decls.iter().find(|d| d.name == "greet").expect("greet");
        let sig = greet.signature.as_ref().expect("sig");
        assert_eq!(sig.label(), "(name: string, times?: number): string");
    }

    #[test]
    fn signature_label_void_when_no_return() {
        let src = "function f() {}";
        let decls = collect_declarations(src);
        let f = decls.iter().find(|d| d.name == "f").expect("f");
        assert_eq!(f.signature.as_ref().expect("sig").label(), "(): void");
    }

    #[test]
    fn workspace_dep_resolves_as_local_crate_module() {
        clear_workspace_deps();
        let mut deps = std::collections::HashSet::new();
        deps.insert("@scope/b".to_string());
        set_workspace_deps(deps);
        // A recorded workspace member is a cargo path dep, reached as
        // crate::<ident>; an unrecorded bare specifier is a transpiled npm dep
        // under third_party/.
        let ident = bare_module_ident("@scope/b");
        let path = mod_use_path("@scope/b", &ident);
        assert_eq!(
            path.segments.first().expect("segments").ident.to_string(),
            "crate"
        );
        clear_workspace_deps();
        // After clear, the specifier is no longer a workspace dep — it routes
        // as a bare npm dep under third_party/.
        let path = mod_use_path("@scope/b", &ident);
        assert_eq!(
            path.segments.first().expect("segments").ident.to_string(),
            "third_party"
        );
    }

    #[test]
    fn workspace_member_crate_resolves_as_bare_extern() {
        clear_workspace_deps();
        clear_workspace_member_crates();
        let mut deps = std::collections::HashSet::new();
        deps.insert("@scope/b".to_string());
        set_workspace_member_crates(deps);
        // A workspace member built as an independent crate (translate_project)
        // is reached bare through the extern prelude — `ds_scopeSb`, not under
        // crate:: or third_party::.
        let ident = bare_module_ident("@scope/b");
        assert_eq!(ident.to_string(), "ds_scopeSb");
        let path = mod_use_path("@scope/b", &ident);
        assert_eq!(path.segments.len(), 1);
        assert_eq!(
            path.segments.first().expect("segments").ident.to_string(),
            "ds_scopeSb"
        );
        clear_workspace_member_crates();
        // After clear, the specifier is no longer a member crate — routes as a
        // bare npm dep under third_party/.
        let path = mod_use_path("@scope/b", &ident);
        assert_eq!(
            path.segments.first().expect("segments").ident.to_string(),
            "third_party"
        );
    }

    #[test]
    fn npm_to_ds_ident_is_injective_and_prefixed() {
        // Documented examples.
        assert_eq!(npm_to_ds_ident("@office-open/xml"), "ds_office_openSxml");
        assert_eq!(npm_to_ds_ident("office-open-xml"), "ds_office_open_xml");
        assert_eq!(npm_to_ds_ident("@office_open/xml"), "ds_officeUopenSxml");
        assert_eq!(npm_to_ds_ident("@types/node"), "ds_typesSnode");
        assert_eq!(npm_to_ds_ident("lodash"), "ds_lodash");

        // The flat-map failure mode: distinct npm names that a naive
        // `-`/`_`/`.` → `_` map would collapse to one ident must stay distinct.
        let mut seen = std::collections::HashSet::new();
        for ident in [
            npm_to_ds_ident("office-open-xml"),
            npm_to_ds_ident("office_open_xml"),
            npm_to_ds_ident("@office-open/xml"),
            npm_to_ds_ident("office.open.xml"),
        ] {
            assert!(
                seen.insert(ident.clone()),
                "collision: {ident} produced by two distinct npm names"
            );
        }

        // Always a legal Rust ident: `ds_` prefix guarantees it never starts
        // with a digit, every char is `[a-z0-9_UDSX]`.
        for nasty in ["@x", "a.b/c-d_e", "1", "@scope/a@b"] {
            let ident = npm_to_ds_ident(nasty);
            assert!(
                ident.starts_with("ds_"),
                "{nasty:?} -> {ident:?} lost the prefix"
            );
        }

        // A legacy uppercase name (npm forbids these today) still maps
        // injectively via the hex fallback and never collides with its
        // lowercase peer.
        assert_ne!(npm_to_ds_ident("jQuery"), npm_to_ds_ident("jquery"));
    }

    #[test]
    fn npm_third_party_module_path_preserves_tree_and_extension() {
        // The specifier's `/` boundaries are real directories (scope→name,
        // pkg→subpath), preserved as path segments; the `@` scope marker is
        // dropped; the file extension survives as `.`→`D` so sibling
        // cache.{m,c,}js stay distinct.
        assert_eq!(
            npm_third_party_module_path("@noble/hashes/sha2.js"),
            "noble/hashes/sha2Djs"
        );
        assert_eq!(npm_third_party_module_path("lodash"), "lodash");
        assert_eq!(
            npm_third_party_module_path("@scope/pkg-name"),
            "scope/pkg_name"
        );
        // Extension preservation: three siblings sharing a stem but differing
        // in extension never collapse.
        assert_eq!(npm_third_party_module_path("p/cache.mjs"), "p/cacheDmjs");
        assert_eq!(npm_third_party_module_path("p/cache.cjs"), "p/cacheDcjs");
        assert_eq!(npm_third_party_module_path("p/cache.js"), "p/cacheDjs");

        // Injective per segment: distinct specifiers never share one path.
        let mut seen = std::collections::HashSet::new();
        for path in [
            npm_third_party_module_path("@noble/hashes"),
            npm_third_party_module_path("noble-hashes"),
            npm_third_party_module_path("@noble_hashes"),
            npm_third_party_module_path("noble.hashes"),
        ] {
            assert!(
                seen.insert(path.clone()),
                "collision: {path} from two distinct specifiers"
            );
        }

        // A digit-leading subpath segment (never a package name) gets an `N`
        // prefix so it is a legal ident and cannot collide with `-`→`_`.
        assert_eq!(npm_third_party_module_path("pkg/3rd"), "pkg/N3rd");
        assert_ne!(
            npm_third_party_module_path("pkg/3rd"),
            npm_third_party_module_path("pkg/-3rd")
        );
    }
}
