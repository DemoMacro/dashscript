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

/// Local binding name → its type's path (e.g. `Option<f64>`, `Vec<f64>`).
pub type Locals = HashMap<String, Path>;

/// Field rewriting active inside one `match` arm of a discriminated union:
/// within the arm body, `scrut.field` (for any `field` in `fields`) reads as the
/// destructured binding `field`. `scrut == None` disables rewriting. Names are
/// stored snake-cased to match Rust identifiers and the locals table.
#[derive(Clone, Default)]
pub struct Narrow {
    scrut: Option<String>,
    fields: HashSet<String>,
}

impl Narrow {
    /// A narrowing scope: `scrut` is the variable being matched, `fields` the
    /// data-field names of the active variant (all snake-cased).
    #[must_use]
    pub fn of(scrut: String, fields: HashSet<String>) -> Self {
        Self { scrut: Some(scrut), fields }
    }

    /// True when `scrut.field` (both snake-cased) should read as the arm's
    /// `field` binding in the current scope.
    #[must_use]
    pub fn binds(&self, scrut: &str, field: &str) -> bool {
        self.scrut.as_deref() == Some(scrut) && self.fields.contains(field)
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
        Self { locals, registry, narrow }
    }

    /// The type path of a local binding named `name`, if it is known.
    #[must_use]
    pub fn local_type(&self, name: &str) -> Option<&'a Path> {
        self.locals.get(name)
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
}

/// True when `path` is `Option<…>` (last segment is `Option`).
pub fn is_option_path(path: &Path) -> bool {
    path.segments.last().is_some_and(|s| s.ident == "Option")
}
