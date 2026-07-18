//! `BindingPattern` / `PropertyKey` → `syn::Ident`.

use oxc_ast::ast::{BindingIdentifier, BindingPattern, PropertyKey};
use proc_macro2::Span;
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
        } else if c.is_ascii_alphanumeric() || c == '_' {
            out.push(c);
        } else {
            // JS identifiers may contain `$` (e.g. test262's `$262` harness
            // global); Rust idents may not. Map every sigil to `_` so the name
            // stays a valid ident — a translator panic here would abort the
            // whole conformance run. The sanitised name refers to a symbol
            // DashScript cannot lower, so the emitted Rust simply fails to
            // compile (a `partial`), rather than crashing translation.
            out.push('_');
        }
    }
    // An ident cannot start with a digit; prefix `_`. A `.ds` name cannot
    // start with one either, but the sigil→`_` mapping above can leave a
    // leading digit (e.g. `$2` → `_2` is fine; a hypothetical `$`-less digit
    // leader is guarded here too).
    if out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    if out.is_empty() {
        out.push('_');
    }
    // A `.ds` name that lands on a Rust keyword (`dyn`, `match`, `type`, …) is
    // emitted as a valid identifier so the generated code still parses.
    if is_rust_keyword(&out) {
        keyword_ident(&out)
    } else {
        format_ident!("{}", out)
    }
}

/// Turn a Rust keyword into a valid identifier: most become raw identifiers
/// (`r#dyn`); `self`/`crate`/`super` can't be raw, so they get a `_` suffix.
fn keyword_ident(name: &str) -> Ident {
    match name {
        "self" | "crate" | "super" => format_ident!("{}_", name),
        _ => Ident::new_raw(name, Span::call_site()),
    }
}

/// Whether `s` is a Rust strict or reserved keyword (lowercase — `snake`
/// already lowercased its input, so `Self`/`true` arrive as `self`/`true`).
fn is_rust_keyword(s: &str) -> bool {
    matches!(
        s,
        "as" | "break"
            | "const"
            | "continue"
            | "crate"
            | "dyn"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "type"
            | "unsafe"
            | "use"
            | "where"
            | "while"
            | "async"
            | "await"
            | "union"
            | "yield"
            | "try"
    )
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

/// A crate name (`serde`, `cfg-if`) → a Rust module identifier (`serde`,
/// `cfg_if`). Hyphens become underscores: Rust crate names may contain `-`, but
/// `use` paths and module idents may not.
pub fn crate_mod(name: &str) -> Ident {
    format_ident!("{}", name.replace('-', "_"))
}

/// Convert a string-literal union member (`"in_progress"`) to an `enum` variant
/// in Rust `UpperCamelCase` (`InProgress`). Non-alphanumeric chars split words.
pub fn pascal(name: &str) -> Ident {
    let mut out = String::with_capacity(name.len());
    let mut capitalize_next = true;
    for c in name.chars() {
        if c.is_alphanumeric() {
            if capitalize_next {
                out.extend(c.to_uppercase());
                capitalize_next = false;
            } else {
                out.push(c);
            }
        } else {
            capitalize_next = true;
        }
    }
    format_ident!("{}", out)
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
