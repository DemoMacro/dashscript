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
    }
}
