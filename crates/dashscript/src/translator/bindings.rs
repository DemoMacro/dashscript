//! `BindingPattern` / `PropertyKey` → `syn::Ident`.

use oxc_ast::ast::{BindingIdentifier, BindingPattern, PropertyKey};
use quote::format_ident;
use syn::Ident;

/// Convert a DashScript identifier to idiomatic Rust `snake_case`.
///
/// DashScript inherits TypeScript's `camelCase`; Rust warns on anything but
/// `snake_case`. Converting at the binding boundary — applied to function,
/// variable, parameter, and field names alike — keeps the generated code
/// warning-free and consistent across definition, reference, and field access.
pub fn snake(name: &str) -> Ident {
    let mut out = String::with_capacity(name.len() + 4);
    for (i, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i != 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    format_ident!("{}", out)
}

/// Identifier name from a `BindingIdentifier`.
pub fn ident_of(ident: &BindingIdentifier) -> Ident {
    let name: &str = &ident.name;
    snake(name)
}

/// Identifier for a *type* (interface / type-alias name). Type names keep their
/// original form: Rust requires `UpperCamelCase` types, unlike the `snake_case`
/// value identifiers [`snake`] produces. TS type names are conventionally
/// already PascalCase, so we pass them through unchanged.
pub fn type_ident(name: &str) -> Ident {
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
            Some(snake(name))
        }
        _ => None,
    }
}
