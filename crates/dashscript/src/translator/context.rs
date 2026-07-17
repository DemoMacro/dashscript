//! Type context threaded through expression translation.
//!
//! DashScript reuses `oxc` for parsing and linting, but `oxc`'s Rust layer has
//! no type checker (its type-aware linting runs in a separate Go binary). So a
//! `.ds` program's types come from the annotations the author wrote, which we
//! record as we walk declarations and statements. `Ctx` carries that record
//! into the expression layer so type-sensitive mappings — `x === null` →
//! `x.is_none()`, `a ?? b`, enum construction, array callbacks — can decide
//! without a full type checker.

use std::collections::{HashMap, HashSet};

use syn::Path;

use super::registry::{TypeRegistry, VariantShape};

/// A function body's locals: their declared types, plus the set of names that
/// are mutated (assigned / updated / mutator-method receiver) — so a `.ds`
/// `let` only becomes `let mut` when the binding is actually changed.
pub struct Locals {
    types: HashMap<String, Path>,
    pub mutated: HashSet<String>,
    pub use_counts: HashMap<String, u32>,
}

impl Locals {
    #[must_use]
    pub fn new() -> Self {
        Self {
            types: HashMap::new(),
            mutated: HashSet::new(),
            use_counts: HashMap::new(),
        }
    }

    /// The type path of a local binding named `name`, if known.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Path> {
        self.types.get(name)
    }

    /// Record a local's type path.
    pub fn insert(&mut self, name: String, path: Path) {
        self.types.insert(name, path);
    }
}

impl Default for Locals {
    fn default() -> Self {
        Self::new()
    }
}

/// Field rewriting active inside one `match` arm of a discriminated union:
/// within the arm body, `scrut.field` (for any `field` in `fields`) reads as the
/// destructured binding `field`. `scrut == None` disables rewriting. Names are
/// stored snake-cased to match Rust identifiers and the locals table.
#[derive(Clone, Default)]
pub struct Narrow {
    scrut: Option<String>,
    fields: HashSet<String>,
    option_some: HashSet<String>,
}

impl Narrow {
    /// A narrowing scope: `scrut` is the variable being matched, `fields` the
    /// data-field names of the active variant (all snake-cased).
    #[must_use]
    pub fn of(scrut: String, fields: HashSet<String>) -> Self {
        Self {
            scrut: Some(scrut),
            fields,
            option_some: HashSet::new(),
        }
    }

    /// True when `scrut.field` (both snake-cased) should read as the arm's
    /// `field` binding in the current scope.
    #[must_use]
    pub fn binds(&self, scrut: &str, field: &str) -> bool {
        self.scrut.as_deref() == Some(scrut) && self.fields.contains(field)
    }

    /// A child scope that also narrows `name` (snake-cased) from `Option<T>` to
    /// `T`, matching an `if let Some(name) = name` branch.
    #[must_use]
    pub fn with_option_some(&self, name: String) -> Self {
        let mut next = self.clone();
        next.option_some.insert(name);
        next
    }

    /// True when `name` (snake-cased) is narrowed from `Option` to its inner
    /// value in this scope.
    #[must_use]
    pub fn is_option_some(&self, name: &str) -> bool {
        self.option_some.contains(name)
    }
}

/// Read-only type context for translating one function body's expressions.
pub struct Ctx<'a> {
    locals: &'a Locals,
    registry: &'a TypeRegistry,
    narrow: &'a Narrow,
}

impl<'a> Ctx<'a> {
    #[must_use]
    pub fn new(locals: &'a Locals, registry: &'a TypeRegistry, narrow: &'a Narrow) -> Self {
        Self {
            locals,
            registry,
            narrow,
        }
    }

    /// The type path of a local binding named `name`, if it is known.
    #[must_use]
    pub fn local_type(&self, name: &str) -> Option<&'a Path> {
        self.locals.get(name)
    }

    /// How often local `name` (snake-cased) is read in this body. A non-`Copy`
    /// local read more than once must be cloned when passed by value — its
    /// first move would break a later read. `0` when unknown.
    #[must_use]
    pub fn use_count(&self, name: &str) -> u32 {
        self.locals.use_counts.get(name).copied().unwrap_or(0)
    }

    /// True when `name` is a local of an `Option<…>` type.
    #[must_use]
    pub fn is_option(&self, name: &str) -> bool {
        self.locals.get(name).is_some_and(is_option_path)
    }

    /// The variant shape for a `kind` value of a discriminated-union enum named
    /// `type_name`, if that type is a registered discriminated union.
    #[must_use]
    pub fn variant(&self, type_name: &str, kind: &str) -> Option<&'a VariantShape> {
        self.registry.unions.get(type_name)?.get(kind)
    }

    /// The parameter type paths declared by the function named `name` (original
    /// `.ds` spelling), if any. Each entry is `None` for an unannotated param.
    #[must_use]
    pub fn function_params(&self, name: &str) -> Option<&'a [Option<Path>]> {
        self.registry.functions.get(name).map(Vec::as_slice)
    }

    /// Per-parameter "has a default initializer?" flags for the function named
    /// `name` (original `.ds` spelling), if any.
    #[must_use]
    pub fn function_defaults(&self, name: &str) -> Option<&'a [bool]> {
        self.registry.function_defaults.get(name).map(Vec::as_slice)
    }

    /// The optional (`?:`) field names of the struct/interface named
    /// `type_name`, when it has any. Lets a literal that omits an optional
    /// field fill in `None`.
    #[must_use]
    pub fn struct_optionals(&self, type_name: &str) -> Option<&'a HashSet<String>> {
        self.registry.structs.get(type_name)
    }

    /// True when `scrut.field` (both snake-cased) is narrowed to an arm binding
    /// in the current scope.
    #[must_use]
    pub fn narrow_binds(&self, scrut: &str, field: &str) -> bool {
        self.narrow.binds(scrut, field)
    }

    /// True when `name` (snake-cased) is narrowed from `Option<T>` to `T` in the
    /// current scope — an `if (name)` truthiness branch on a `Copy` inner type,
    /// so `name!`/`name` read the bound inner value, not `Option::unwrap`.
    #[must_use]
    pub fn is_narrowed_some(&self, name: &str) -> bool {
        self.narrow.is_option_some(name)
    }
}

/// True when `path` is `Option<…>` (last segment is `Option`).
pub fn is_option_path(path: &Path) -> bool {
    path.segments.last().is_some_and(|s| s.ident == "Option")
}
