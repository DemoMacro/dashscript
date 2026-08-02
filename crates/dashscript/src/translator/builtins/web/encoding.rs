//! WHATWG Encoding API constructor types — `new TextEncoder()` / `new
//! TextDecoder()` (a WinterTC Web API). The constructors are stateless unit
//! structs injected by the `Encoding` runtime dep (`__ds::TextEncoder` /
//! `__ds::TextDecoder`, see `RuntimeDep::Encoding`), so this module carries
//! only the name → type mapping the static translator needs in two places:
//! `new` lowering (emit `crate::__ds::TextEncoder::new()`) and module-global
//! singleton inference (a `OnceLock<crate::__ds::TextEncoder>`). Keeping the
//! mapping here means the two call sites cannot drift on the Rust path.

use syn::{parse_quote, Type};

/// The Rust type a WHATWG Encoding API constructor builds, if `name` is one:
/// `TextEncoder` → `crate::__ds::TextEncoder`, `TextDecoder` →
/// `crate::__ds::TextDecoder`. `None` for any other name. Both are stateless,
/// so a module-global singleton (`const encoder = new TextEncoder()`) is sound
/// behind a `OnceLock`.
pub(in crate::translator) fn encoding_ctor_type(name: &str) -> Option<Type> {
    match name {
        "TextEncoder" => Some(parse_quote!(crate::__ds::TextEncoder)),
        "TextDecoder" => Some(parse_quote!(crate::__ds::TextDecoder)),
        _ => None,
    }
}
