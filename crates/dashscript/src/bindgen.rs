//! Rust crate → `.ds` type declaration.
//!
//! Powers `ds add rust:<crate>`: inspect a crate's public surface and emit a
//! `.ds` declaration so importing the crate yields editor completion and
//! type checking — the cross-language analogue of `@types` / DefinitelyTyped.

/// Generates a `.ds` type declaration for a Rust crate.
#[derive(Default)]
pub struct Bindgen {
    // Options land here: visibility filters, rename rules, feature flags, ...
}

impl Bindgen {
    /// Create a bindgen with default options.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Generate a `.ds` declaration for the named crate.
    ///
    /// Maps Rust constructs (`struct`, `enum`, `fn`, `trait`) to their `.ds`
    /// equivalents so editor types stay correct.
    pub fn generate(&self, crate_name: &str) -> String {
        let _ = crate_name;
        // TODO: inspect the crate's public surface (via `syn`) and emit .ds.
        todo!("Rust crate → .ds type declaration")
    }
}
