//! Type context threaded through expression translation.
//!
//! DashScript reuses `oxc` for parsing and linting, but `oxc`'s Rust layer has
//! no type checker (its type-aware linting runs in a separate Go binary). So a
//! `.ds` program's types come from the annotations the author wrote, which we
//! record as we walk declarations and statements. `Ctx` carries that record
//! into the expression layer so type-sensitive mappings — `x === null` →
//! `x.is_none()`, `a ?? b`, enum construction, array callbacks — can decide
//! without a full type checker.

use std::collections::HashMap;

use syn::Path;

/// Local binding name → its type's path (e.g. `Option<f64>`, `Vec<f64>`).
pub type Locals = HashMap<String, Path>;

/// Read-only type context for translating one function body's expressions.
pub struct Ctx<'a> {
    locals: &'a Locals,
}

impl<'a> Ctx<'a> {
    #[must_use]
    pub fn new(locals: &'a Locals) -> Self {
        Self { locals }
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
}

/// True when `path` is `Option<…>` (last segment is `Option`).
pub fn is_option_path(path: &Path) -> bool {
    path.segments.last().is_some_and(|s| s.ident == "Option")
}
