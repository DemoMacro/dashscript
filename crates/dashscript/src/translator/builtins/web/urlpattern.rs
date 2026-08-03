//! WHATWG URLPattern API — `new URLPattern(input[, baseURL])` constructor (a
//! WinterTC Web API). `urlpattern_ctor_type` carries the name → type mapping the
//! `new` lowering needs; argument dispatch (string | URL | undefined) lives in
//! `expressions/new.rs::urlpattern_ctor`. The constructor builds a stateful
//! `crate::__ds::DsURLPattern` (a `urlpattern::UrlPattern` wrapper) injected by
//! the `URLPattern` runtime dep. A pattern that fails to compile panics a
//! `TypeError` (the ES URLPattern constructor's error class). Instance methods
//! (`test`/`exec`) are not yet lowered — they fall through to a plain method
//! call (cargo check rejects honestly).

use syn::{parse_quote, Type};

/// The Rust type a WHATWG URLPattern constructor builds, if `name` is one:
/// `URLPattern` → `crate::__ds::DsURLPattern`. `None` for any other name (the
/// `new` lowering falls through to the generic `Foo::new` path and surfaces at
/// `cargo check`).
pub(in crate::translator) fn urlpattern_ctor_type(name: &str) -> Option<Type> {
    match name {
        "URLPattern" => Some(parse_quote!(crate::__ds::DsURLPattern)),
        _ => None,
    }
}
