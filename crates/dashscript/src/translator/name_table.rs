//! `SymbolId` → Rust name assignment, with scope-aware disambiguation.
//!
//! This replaces the lossy `bindings::snake(name)` string conversion at the
//! binding boundary. Two `.ds` bindings `N` and `n` are *distinct* `SymbolId`s
//! to oxc (they are different declarations in the same scope), but both
//! snake-fold to `n` — producing a silent same-scope shadow in the emitted
//! Rust. By keying on `SymbolId` we can give them distinct Rust names.
//!
//! Stage 1.1 (this file): `build` assigns every symbol `snake(name)` with **no
//! disambiguation**, so output is byte-identical to the pre-`NameTable` code —
//! it only establishes the plumbing (`of_binding` / `of_reference` read the
//! semantic cells that `SemanticBuilder` fills). Stage 1.3 adds `rust_scope`
//! grouping + `_2`-suffix disambiguation for same-scope collisions.

use std::collections::HashMap;

use oxc_ast::ast::{BindingIdentifier, BindingPattern, IdentifierReference};
use oxc_semantic::{Scoping, SymbolId};
use syn::Ident;

use super::bindings;

/// Per-file map from `SymbolId` to the Rust identifier emitted for it. Borrows
/// the semantic `Scoping` (which lives as long as the parse arena) so
/// `of_reference` can resolve `reference_id` → `SymbolId`.
pub struct NameTable<'scoping> {
    scoping: &'scoping Scoping,
    map: HashMap<SymbolId, Ident>,
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
}

/// Build a name table for one file's symbols. Stage 1.1: every symbol keeps
/// `snake(name)` (no disambiguation) — see the module doc for the staging plan.
pub fn build(scoping: &Scoping) -> NameTable<'_> {
    let mut map = HashMap::new();
    for sid in scoping.symbol_ids() {
        let name = scoping.symbol_name(sid);
        map.insert(sid, bindings::snake(name));
    }
    NameTable { scoping, map }
}
