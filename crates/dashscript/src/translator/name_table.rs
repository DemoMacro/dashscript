//! `SymbolId` → Rust name assignment, with scope-aware disambiguation.
//!
//! This replaces the lossy `bindings::snake(name)` string conversion at the
//! binding boundary. Two `.ts` bindings `N` and `n` are *distinct* `SymbolId`s
//! to oxc (they are different declarations in the same scope), but both
//! snake-fold to `n` — producing a silent same-scope shadow in the emitted
//! Rust. By keying on `SymbolId` we can give them distinct Rust names.
//!
//! `build` assigns each symbol `snake(name)`, disambiguating same-scope
//! collisions: two bindings in one oxc scope share a Rust block (a function
//! body is one flat block; `for (let …)` is a nested block; `for (var …)` is
//! function-scoped, the way Rust sees it flattened), so a snake-name collision
//! there (`N` and `n` both → `n`) would shadow silently. The second and later
//! collisions in a scope get `_2`/`_3`. Bindings in different scopes are in
//! different Rust blocks, where shadowing is legal, so they keep their base
//! name.

use std::collections::{HashMap, HashSet};

use oxc_ast::ast::{BindingIdentifier, BindingPattern, IdentifierReference};
use oxc_semantic::{ScopeId, Scoping, SymbolId};
use syn::Ident;

use super::bindings;

/// Per-file map from `SymbolId` to the Rust identifier emitted for it. Borrows
/// the semantic `Scoping` (which lives as long as the parse arena) so
/// `of_reference` can resolve `reference_id` → `SymbolId`.
pub struct NameTable<'scoping> {
    scoping: &'scoping Scoping,
    map: HashMap<SymbolId, Ident>,
    /// The `SymbolId`s bound by a namespace import (`import * as ns`). A
    /// reference to such a binding is a *module path prefix*, not a value:
    /// `ns.foo` lowers to `ns::foo`. Tracked per-symbol (scope-aware) so two
    /// same-named bindings in different scopes stay distinct, the way `map`
    /// does. Populated after `build` from the program's import specifiers.
    namespaces: HashSet<SymbolId>,
    /// The `SymbolId`s of top-level `const` bindings promoted to crate-level
    /// `const` items (escape promotion, A3) — a `const` number literal
    /// referenced from a top-level function. A reference to one is an `f64`
    /// value, so number→string emit must route it through
    /// `__ds::number_to_string` the way it does a numeric local. Tracked
    /// per-symbol so the promotion is visible in every body (`fn main` and each
    /// function), not just the one that would have declared it as a local.
    number_consts: HashSet<SymbolId>,
    /// The `SymbolId`s of module-level `const` bindings lowered to lazy statics
    /// (OnceLock + accessor fn) — a non-const-expr initializer (an object, a
    /// regex, a `Map`/`Set`) constructs its value once at first use, not at
    /// compile time. A reference to one emits the accessor call (`name()`),
    /// which returns `&'static T`. Tracked per-symbol so the lowering is visible
    /// in every body, like `number_consts`.
    lazy_statics: HashSet<SymbolId>,
    /// The `SymbolId`s of mutable module-global `let` bindings lowered to
    /// thread-local `RefCell`s (B3-2): a top-level `let` rebound or
    /// member-mutated from a function cannot live in `fn main` (a Rust fn item
    /// cannot close over a `main` local), so it hoists behind a `RefCell` with a
    /// get/set accessor pair. The value is `(set-accessor ident, optional)`:
    /// `set_x` drives the reassignment rewrite, and `optional` flags a B3-2c
    /// delayed-binding binding (declared without an initializer —
    /// `let x: T | undefined;` → `RefCell<Option<T>>` seeded `None`, set later
    /// via `set_x(v)`), whose read rewrite differs (a call `x(a)` →
    /// `x().expect(..)(a)`, truthiness `if (x)` → `x().is_some()`). Tracked
    /// per-symbol, like `lazy_statics`.
    mutable_statics: HashMap<SymbolId, (Ident, bool)>,
}

impl<'a> NameTable<'a> {
    /// The Rust name for a *binding* occurrence (a declaration): reads the
    /// `symbol_id` cell `SemanticBuilder` filled. Symbols oxc did not bind
    /// (some pattern positions) fall back to `snake(name)`.
    pub fn of_binding(&self, id: &BindingIdentifier) -> Ident {
        match id.symbol_id.get() {
            Some(sid) => self.map.get(&sid).cloned(),
            None => None,
        }
        .unwrap_or_else(|| bindings::snake(&id.name))
    }

    /// The Rust name for a binding *pattern*: a `BindingIdentifier` resolves via
    /// [`NameTable::of_binding`]; a destructuring pattern has no single symbol,
    /// so it falls back to `bindings::binding_name` (the sub-bindings are walked
    /// separately in `destructure`).
    pub fn of_pattern(&self, pat: &BindingPattern) -> Ident {
        match pat {
            BindingPattern::BindingIdentifier(id) => self.of_binding(id),
            _ => bindings::binding_name(pat),
        }
    }

    /// The Rust name for a *reference* occurrence (a read/write): resolves
    /// `reference_id` → `SymbolId` via the scoping, then looks up the table.
    /// Unresolved references (host globals like test262's `$262`, cross-module
    /// imports oxc did not resolve) fall back to `snake(name)`.
    pub fn of_reference(&self, id: &IdentifierReference) -> Ident {
        let sid = id
            .reference_id
            .get()
            .and_then(|rid| self.scoping.get_reference(rid).symbol_id());
        sid.and_then(|s| self.map.get(&s).cloned())
            .unwrap_or_else(|| bindings::snake(&id.name))
    }

    /// The `SymbolId` a reference resolves to, if any (used by type queries to
    /// key `Locals` by symbol rather than by snake-name string).
    pub fn symbol_of_reference(&self, id: &IdentifierReference) -> Option<SymbolId> {
        let rid = id.reference_id.get()?;
        self.scoping.get_reference(rid).symbol_id()
    }

    /// The `SymbolId` a binding pattern declares, if any. A `BindingIdentifier`
    /// resolves via its `symbol_id` cell; a destructuring pattern has no single
    /// symbol, so it returns `None` (the sub-bindings are walked separately).
    /// Used by flavor inference to key the dep set scope-aware — two `i` loops
    /// in different scopes are distinct `SymbolId`s even though their Rust name
    /// is the same (`i`) in both.
    pub fn symbol_of_pattern(&self, pat: &BindingPattern) -> Option<SymbolId> {
        match pat {
            BindingPattern::BindingIdentifier(id) => id.symbol_id.get(),
            _ => None,
        }
    }

    /// Whether `id` resolves to a namespace-import binding (`import * as ns`).
    /// A reference to such a binding is a module-path prefix: `ns.foo` lowers to
    /// `ns::foo`, not a field access. Returns `false` for any unresolved
    /// reference (a host global, or a binding oxc did not bind).
    #[must_use]
    pub fn is_namespace(&self, id: &IdentifierReference) -> bool {
        let Some(sid) = self.symbol_of_reference(id) else {
            return false;
        };
        self.namespaces.contains(&sid)
    }

    /// Record the namespace-import bindings in a program (`import * as ns`).
    /// Walked from the parsed statements after `build` (the import specifiers
    /// carry the `BindingIdentifier` whose `symbol_id` cell `SemanticBuilder`
    /// filled). A namespace import is module-scoped, so its binding has a
    /// `SymbolId` like any other declaration.
    pub fn register_namespaces(&mut self, body: &[oxc_ast::ast::Statement]) {
        use oxc_ast::ast::{ImportDeclarationSpecifier, Statement};
        for stmt in body {
            let Statement::ImportDeclaration(imp) = stmt else {
                continue;
            };
            let Some(specs) = imp.specifiers.as_ref() else {
                continue;
            };
            for spec in specs {
                if let ImportDeclarationSpecifier::ImportNamespaceSpecifier(ns) = spec {
                    if let Some(sid) = ns.local.symbol_id.get() {
                        self.namespaces.insert(sid);
                    }
                }
            }
        }
    }

    /// Record a promoted top-level `const` binding (escape promotion, A3) so a
    /// reference to it is recognized as an `f64` value — see [`Self::is_number_const`].
    pub fn register_number_const(&mut self, sym: SymbolId) {
        self.number_consts.insert(sym);
    }

    /// Whether `id` resolves to a top-level `const` promoted to a crate-level
    /// `const` item (escape promotion, A3). Such a reference is an `f64` value
    /// living in a `const` item, not a `fn main` local, so number→string emit
    /// routes it through `__ds::number_to_string` (see `is_number_local`).
    /// Returns `false` for any unresolved reference.
    #[must_use]
    pub fn is_number_const(&self, id: &IdentifierReference) -> bool {
        let Some(sid) = self.symbol_of_reference(id) else {
            return false;
        };
        self.number_consts.contains(&sid)
    }

    /// Record a module-level `const` binding lowered to a lazy static (OnceLock
    /// + accessor fn) — see [`Self::is_lazy_static`].
    pub fn register_lazy_static(&mut self, sym: SymbolId) {
        self.lazy_statics.insert(sym);
    }

    /// Whether `id` resolves to a module-level `const` lowered to a lazy static
    /// (OnceLock + accessor fn). Such a reference emits the accessor call
    /// (`name()`) rather than a bare identifier — the value lives behind a
    /// `OnceLock`, not as a `const` item. Returns `false` for any unresolved
    /// reference.
    #[must_use]
    pub fn is_lazy_static(&self, id: &IdentifierReference) -> bool {
        let Some(sid) = self.symbol_of_reference(id) else {
            return false;
        };
        self.lazy_statics.contains(&sid)
    }

    /// Record a mutable module-global `let` lowered to a thread-local `RefCell`
    /// (B3-2) — `setter` is the set-accessor ident (`set_x`), and `optional`
    /// flags a B3-2c delayed-binding binding (`RefCell<Option<T>>`, no
    /// initializer, seeded `None`).
    pub fn register_mutable_static(&mut self, sym: SymbolId, setter: Ident, optional: bool) {
        self.mutable_statics.insert(sym, (setter, optional));
    }

    /// Whether `id` resolves to a mutable module-global `RefCell` (B3-2). A read
    /// emits the get accessor (`x()`), the same shape as a lazy static; this mark
    /// drives the reassignment/update rewrite (`set_x(v)`), since the get accessor
    /// returns a clone, not an lvalue. Returns `false` for any unresolved ref.
    #[must_use]
    pub fn is_mutable_static(&self, id: &IdentifierReference) -> bool {
        let Some(sid) = self.symbol_of_reference(id) else {
            return false;
        };
        self.mutable_statics.contains_key(&sid)
    }

    /// The set-accessor ident for a mutable module-global `RefCell` (B3-2), so a
    /// reassignment `x = v` lowers to `set_x(v)`. `None` for any non-mutable ref.
    #[must_use]
    pub fn mutable_static_setter(&self, id: &IdentifierReference) -> Option<Ident> {
        let sid = self.symbol_of_reference(id)?;
        self.mutable_statics
            .get(&sid)
            .map(|(setter, _)| setter.clone())
    }

    /// Whether `id` is a B3-2c delayed-binding mutable static — declared with no
    /// initializer (`let x: T | undefined;` → `RefCell<Option<T>>`), whose read
    /// rewrite differs from a value-type mutable static (a call wraps in
    /// `.expect(..)`, truthiness maps to `is_some()`/`is_none()`). `false` for
    /// any non-mutable or value-typed ref.
    #[must_use]
    pub fn is_optional_mutable_static(&self, id: &IdentifierReference) -> bool {
        let Some(sid) = self.symbol_of_reference(id) else {
            return false;
        };
        self.mutable_statics.get(&sid).is_some_and(|(_, opt)| *opt)
    }
}

/// Build a name table for one file's symbols — see the module doc for the
/// same-scope disambiguation rule.
pub fn build(scoping: &Scoping) -> NameTable<'_> {
    // Group symbols by their declaring scope, keyed by the snake-folded name so
    // `N` and `n` (both → `n`) land in the same collision group.
    let mut by_scope: HashMap<ScopeId, HashMap<String, Vec<SymbolId>>> = HashMap::new();
    for sid in scoping.symbol_ids() {
        let scope = scoping.symbol_scope_id(sid);
        let base = bindings::snake(scoping.symbol_name(sid)).to_string();
        by_scope
            .entry(scope)
            .or_default()
            .entry(base)
            .or_default()
            .push(sid);
    }
    let mut map = HashMap::new();
    for group in by_scope.into_values() {
        for (base, mut sids) in group {
            // Stable order: `SymbolId` is assigned in declaration order, so the
            // first-declared binding keeps the base name and later ones suffix.
            sids.sort_unstable();
            for (i, sid) in sids.into_iter().enumerate() {
                let ident = if i == 0 {
                    bindings::snake(scoping.symbol_name(sid))
                } else {
                    // `_{i+1}` on the snake base. Strip an `r#` raw-ident prefix
                    // first so a keyword binding disambiguates as `type_2`, not
                    // `r#type_2`; the suffixed name is never a keyword, so a
                    // plain ident suffices.
                    let stripped = base.strip_prefix("r#").unwrap_or(&base);
                    let disambiguated = format!("{}_{}", stripped, i + 1);
                    syn::Ident::new(&disambiguated, proc_macro2::Span::call_site())
                };
                map.insert(sid, ident);
            }
        }
    }
    // A top-level `function main` would collide with the implicit cargo
    // `fn main` the translator emits (pure-TS execution semantics: a
    // function declaration is not an entry point). Rename the root scope's
    // `main` binding to `__ds_main` — the definition and every call site
    // pick this up via `of_binding` / `of_reference`. Nested `main` (a
    // local function inside another function) is left untouched.
    let root = scoping.root_scope_id();
    for sid in scoping.symbol_ids() {
        if scoping.symbol_scope_id(sid) == root && scoping.symbol_name(sid) == "main" {
            map.insert(sid, Ident::new("__ds_main", proc_macro2::Span::call_site()));
        }
    }
    NameTable {
        scoping,
        map,
        namespaces: HashSet::new(),
        number_consts: HashSet::new(),
        lazy_statics: HashSet::new(),
        mutable_statics: HashMap::new(),
    }
}
