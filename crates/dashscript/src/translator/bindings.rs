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
    let chars: Vec<char> = name.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if c.is_ascii_uppercase() {
            // Insert a word boundary before this uppercase unless it continues
            // an acronym: `ENTITY_MAP` → `entity_map` (not `e_n_t_i_t_y_…`),
            // `HTMLElement` → `html_element`. A boundary is needed when the
            // previous char is lowercase/digit (camelCase: `defaultIndent`), or
            // when an acronym tail meets a lowercase tail (`HTMLParser` →
            // `html_parser`: the `L`→`P` edge splits because `a` follows).
            let prev = if i > 0 { Some(chars[i - 1]) } else { None };
            let next = chars.get(i + 1).copied();
            let boundary = match (prev, next) {
                (None, _) | (Some('_'), _) => false,
                (Some(p), _) if p.is_ascii_lowercase() || p.is_ascii_digit() => true,
                (Some(p), Some(nx)) if p.is_ascii_uppercase() && nx.is_ascii_lowercase() => true,
                _ => false,
            };
            if boundary {
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
    // An ident cannot start with a digit; prefix `_`. A `.ts` name cannot
    // start with one either, but the sigil→`_` mapping above can leave a
    // leading digit (e.g. `$2` → `_2` is fine; a hypothetical `$`-less digit
    // leader is guarded here too).
    if out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    if out.is_empty() {
        out.push('_');
    }
    // A `.ts` name that lands on a Rust keyword (`dyn`, `match`, `type`, …) or
    // a prelude macro name (`format`, `vec`, `panic`, …) is emitted as a valid
    // identifier so the generated code still parses — a bare `format` in value
    // position is otherwise parsed as the `format!` macro.
    if is_rust_keyword(&out) || is_rust_prelude_macro(&out) {
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
            | "abstract"
            | "become"
            | "box"
            | "do"
            | "final"
            | "macro"
            | "override"
            | "priv"
            | "typeof"
            | "unsized"
            | "virtual"
    )
}

/// Whether `s` is a Rust *prelude macro* name that collides with a value
/// binding in generated code. A bare `format` / `vec` / `panic` / … in value
/// position parses as the macro invocation `format!` / `vec!` / …
/// (`expected value, found macro `format``), so a `.ts` binding named `format`
/// (e.g. `for (const format of formats)` in the WPT compression fixtures) is
/// emitted as `r#format`. Macro *calls* (`format!(…)`) are unaffected — only a
/// same-named binding collides. The set is the standard-prelude macros that are
/// also common English-word identifiers; rare ones (`module_path`, `option_env`)
/// are omitted until a fixture hits them.
fn is_rust_prelude_macro(s: &str) -> bool {
    matches!(
        s,
        "assert"
            | "cfg"
            | "concat"
            | "dbg"
            | "env"
            | "eprint"
            | "eprintln"
            | "file"
            | "format"
            | "include"
            | "include_bytes"
            | "include_str"
            | "line"
            | "matches"
            | "panic"
            | "print"
            | "println"
            | "stringify"
            | "todo"
            | "unimplemented"
            | "unreachable"
            | "vec"
            | "write"
            | "writeln"
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
    // A TS type name that lands on a Rust keyword (`type`, `macro`) emits a raw
    // ident so `struct r#type {}` parses — `format_ident!` would emit a bare
    // keyword and fail. PascalCase type names (`Match`) are not Rust keywords,
    // so they pass through unchanged.
    if is_rust_keyword(name) {
        keyword_ident(name)
    } else {
        format_ident!("{}", name)
    }
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
    // A literal whose only alphanumerics start with a digit (`"3d"`, `"4th"`)
    // would yield an ident that cannot start with one; a leading `_` keeps it
    // valid — mirrors `snake`. An all-symbol/empty literal lands on the `Empty`
    // fallback. PascalCase output is never a lowercase keyword, but `Self` is
    // reserved — suffix `_`.
    if out.is_empty() {
        out.push_str("Empty");
    }
    if out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    if out == "Self" {
        out.push('_');
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

#[cfg(test)]
mod tests {
    use super::{pascal, snake};

    #[test]
    fn pascal_digit_prefix_gets_underscore() {
        // A leading digit is not a valid ident start; a `_` prefix keeps it valid.
        assert_eq!(pascal("3d").to_string(), "_3d");
        assert_eq!(pascal("4th").to_string(), "_4th");
    }

    #[test]
    fn pascal_empty_or_symbols_fall_back() {
        assert_eq!(pascal("").to_string(), "Empty");
        assert_eq!(pascal("!!!").to_string(), "Empty");
    }

    #[test]
    fn pascal_self_keyword_is_suffixed() {
        // `Self` is reserved; `pascal("self")` lands on it.
        assert_eq!(pascal("self").to_string(), "Self_");
    }

    #[test]
    fn pascal_normal_still_pascalcase() {
        assert_eq!(pascal("in_progress").to_string(), "InProgress");
        assert_eq!(pascal("red").to_string(), "Red");
    }

    #[test]
    fn snake_prelude_macro_name_is_raw_ident() {
        // `format` is a Rust prelude macro; a binding named `format` (WPT
        // compression fixtures: `for (const format of formats)`) emits `r#format`
        // so it isn't parsed as the `format!` macro in value position.
        assert_eq!(snake("format").to_string(), "r#format");
        assert_eq!(snake("vec").to_string(), "r#vec");
        assert_eq!(snake("Format").to_string(), "r#format");
    }

    #[test]
    fn snake_non_macro_name_unchanged() {
        // A name that merely *contains* the macro substring is unaffected.
        assert_eq!(snake("formatter").to_string(), "formatter");
        assert_eq!(snake("vector").to_string(), "vector");
    }
}
