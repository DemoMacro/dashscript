//! `BindingPattern` / `PropertyKey` → `syn::Ident`.

use oxc_ast::ast::{BindingIdentifier, BindingPattern, PropertyKey};
use quote::format_ident;
use syn::Ident;

/// Identifier name from a `BindingIdentifier`.
pub fn ident_of(ident: &BindingIdentifier) -> Ident {
    let name: &str = &ident.name;
    format_ident!("{}", name)
}

/// Identifier name from a `BindingPattern` (parameter / variable binding).
///
/// Destructuring patterns (`ObjectPattern` / `ArrayPattern`) are unsupported
/// yet and fall back to `_`.
pub fn binding_name(pattern: &BindingPattern) -> Ident {
    match pattern {
        BindingPattern::BindingIdentifier(id) => ident_of(id),
        _ => format_ident!("_"),
    }
}

/// Identifier name from a static property key — `x` in `{ x: 1 }` or
/// `interface { x: number }`. Computed keys are unsupported yet.
pub fn property_key_name(key: &PropertyKey) -> Option<Ident> {
    match key {
        PropertyKey::StaticIdentifier(id) => {
            let name: &str = &id.name;
            Some(format_ident!("{}", name))
        }
        _ => None,
    }
}
