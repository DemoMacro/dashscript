//! Runtime library mappings, organised in four layers that mirror the four
//! surfaces DashScript must translate (the same separation Deno's `ext/` uses):
//!
//! - **ECMAScript built-ins** (top level, one file per built-in) — mirror tc39
//!   test262's `test/built-ins/{Math,Array,String,Object,Number}/`, so a
//!   test262 differential failure points straight at the file here (`math.rs`).
//! - **Web API / WinterTC** (`web/`) — host-defined Web globals (Ecma TC55
//!   «Minimum Common Web API»: `console`, `TextEncoder`/`TextDecoder`, and
//!   forthcoming `URL`/`crypto`/`atob`/…), mapped to Rust crates the way Deno's
//!   `ext/web` + `ext/<api>/` back them.
//! - **Node modules** (`node/`) — `node:` imports (`node:fs`/`node:crypto`/…),
//!   parallel to Deno's `ext/node`.
//! - **Conformance test harness** (`harness/`) — orthogonal to the three
//!   runtime layers: the test262 `assert.sameValue` and WPT `test()`/
//!   `assert_equals()` APIs a conformance fixture calls, lowered to
//!   `__ds::assert_*`/`__ds::wpt_*` so the fixture runs on the static path.
//!
//! `mod.rs` re-exports each mapping in one flat namespace — `expressions` calls
//! `builtins::math_method`, `builtins::assert_call`, … — and holds the helpers
//! shared across built-ins (`map_method`, `is_ident`, `usize_arg`,
//! `str_method_arg`). Global conversion functions (`parseInt`/`String(x)`/
//! `Number(s)`/…) live in `global.rs`.

// ECMAScript built-ins (ECMA-262) — mirror tc39 test262's
// `test/built-ins/<cat>/`.
mod array;
mod collection;
mod global;
mod json;
mod math;
mod number;
mod object;
mod promise;
mod string;
mod temporal;
mod typed_array;

// Web API / WinterTC (Ecma TC55) and Node modules — host-defined globals and
// `node:` imports mapped to Rust crates.
mod node;
mod web;

// Conformance test harness (test262 `assert` + WPT `testharness`) — orthogonal.
mod harness;

#[cfg(test)]
mod drift_guard;

pub(in crate::translator) use array::{array_method, array_method_on, array_static};
pub(in crate::translator) use collection::collection_method;
pub(in crate::translator) use global::{
    es_to_string_arg, global_function, reg_exp_constructor, reg_exp_static, to_number_expr,
};
pub(in crate::translator) use json::json_static;
pub(in crate::translator) use math::{math_constant, math_method};
pub(in crate::translator) use number::{number_constant, number_method, number_static};
pub(in crate::translator) use object::object_method;
pub(in crate::translator) use promise::{
    promise_ctor, promise_executor_is_static, promise_instance_method, promise_static,
};
pub(in crate::translator) use string::{string_method, string_method_on, string_static};
pub(in crate::translator) use temporal::{
    temporal_callee_split, temporal_init_type, temporal_method, temporal_new, temporal_new_maps,
    temporal_static, temporal_static_maps, temporal_type_of_callee, TEMPORAL_TYPES,
};
pub(in crate::translator) use typed_array::typed_array_method;

pub(in crate::translator) use harness::{assert_call, assert_method, testharness_function};
pub(in crate::translator) use web::{
    abort_method, blob_ctor, blob_ctor_type, blob_method, compression_ctor_type,
    compression_method, compression_stream_ctor, console_method, crypto_method, encoding_ctor_type,
    event_init, event_target_ctor_type, event_target_method, file_ctor, file_ctor_type,
    headers_ctor, headers_ctor_type, headers_method, perf_method, readable_stream_ctor,
    streams_ctor_type, streams_method, text_decoder_method, text_encoder_method, url_ctor_type,
    url_search_params_method, url_search_params_on_url_method, url_static_method,
    urlpattern_ctor_type,
};

use oxc_ast::ast::{Argument, Expression};
use proc_macro2::Span;
use quote::format_ident;
use syn::{parse_quote, Expr, Ident};

use super::context::Ctx;
use super::expressions::translate_argument;

/// A `.ts` `number` argument cast to `usize` (e.g. for `repeat`, `slice`).
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
