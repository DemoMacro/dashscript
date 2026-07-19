//! ES built-in library mappings — one file per built-in, mirroring tc39
//! test262's `test/built-ins/{Math,Array,String,Object,Number}/`. A test262
//! differential failure (e.g. `test/built-ins/Math/round/…`) points straight
//! at the matching file here (`math.rs`), so coverage gaps and the code that
//! closes them stay co-located.
//!
//! `mod.rs` re-exports each built-in's mapping functions in one flat namespace
//! — `expressions` calls `builtins::math_method`, `builtins::array_static`, …
//! — and holds the helpers shared across built-ins (`map_method`, `is_ident`,
//! `usize_arg`, `str_method_arg`). Global conversion functions
//! (`parseInt`/`String(x)`/`Number(s)`/…) live in `global.rs`; `console` in
//! `console.rs`. Future Node standard libraries (`node:crypto`/`node:zlib`/
//! `node:fs`) will live under `node/`, parallel to the ES built-ins.

mod array;
mod collection;
mod console;
mod global;
mod json;
mod math;
mod node;
mod number;
mod object;
mod string;

pub(in crate::translator) use array::{array_method, array_method_on, array_static};
pub(in crate::translator) use collection::collection_method;
pub(in crate::translator) use console::console_method;
pub(in crate::translator) use global::{global_function, to_number_expr};
pub(in crate::translator) use json::json_static;
pub(in crate::translator) use math::{math_constant, math_method};
pub(in crate::translator) use number::{number_constant, number_method, number_static};
pub(in crate::translator) use object::object_method;
pub(in crate::translator) use string::{string_method, string_method_on, string_static};

use oxc_ast::ast::{Argument, Expression};
use proc_macro2::Span;
use quote::format_ident;
use syn::{parse_quote, Expr, Ident};

use super::context::Ctx;
use super::expressions::translate_argument;

/// A `.ds` `number` argument cast to `usize` (e.g. for `repeat`, `slice`).
pub(in crate::translator) fn usize_arg(arg: &Argument, ctx: &Ctx<'_>) -> Expr {
    let e = translate_argument(arg, ctx);
    parse_quote!(#e as usize)
}

/// A string-method argument as a `&str`: a string literal stays a bare literal
/// (a perfect `Pattern`); any other expression (a `String` var or call) gets
/// `.as_str()` so it satisfies Rust's `&str`-typed string APIs.
pub(in crate::translator) fn str_method_arg(arg: &Argument, ctx: &Ctx<'_>) -> Expr {
    if let Argument::StringLiteral(s) = arg {
        let lit = syn::LitStr::new(s.value.as_str(), Span::call_site());
        return parse_quote!(#lit);
    }
    let e = translate_argument(arg, ctx);
    parse_quote!(#e.as_str())
}

/// A handful of TS method names map to a different Rust method name; the
/// receiver and arguments are passed through unchanged. Unmapped methods fall
/// through to a plain call on the receiver expression.
pub(in crate::translator) fn map_method(name: &str) -> Option<Ident> {
    let mapped = match name {
        "toUpperCase" => "to_uppercase",
        "toLowerCase" => "to_lowercase",
        // `toLocaleUpperCase()`/`toLocaleLowerCase()` with NO locale argument
        // lower to the locale-independent Rust methods — per ECMA-262 §22.1.3 a
        // locale-less `toLocale*` is equivalent to `toUpperCase`/`toLowerCase`.
        // The locale-bearing form is intercepted by `check` (no ICU locale
        // table), so only the locale-less form reaches here. ASCII/most BMP
        // chars match; SpecialCasing conditionals (final-sigma) diverge from a
        // locale-aware Node — the same limit `toUpperCase` → `to_uppercase` has.
        "toLocaleUpperCase" => "to_uppercase",
        "toLocaleLowerCase" => "to_lowercase",
        "trim" => "trim",
        "trimStart" => "trim_start",
        "trimEnd" => "trim_end",
        "push" => "push",
        "pop" => "pop",
        // `.toString()` → `.to_string()` (Rust's `Display`). A numeric receiver
        // with a radix (`(255).toString(16)`) is handled in `number_method`.
        "toString" => "to_string",
        _ => return None,
    };
    Some(format_ident!("{}", mapped))
}

/// True when `expr` is an `Identifier` whose name equals `expected`.
pub(in crate::translator) fn is_ident(expr: &Expression, expected: &str) -> bool {
    let Expression::Identifier(ident) = expr else {
        return false;
    };
    let name: &str = &ident.name;
    name == expected
}
