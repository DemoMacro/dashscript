//! The `JSON` built-in: `parse`/`stringify`, mirroring `test/built-ins/JSON/`.
//!
//! Both inline a `serde_json::` call (no `__ds` helper) — the emitted
//! `serde_json::` prefix is what flags the file `needs_serde_json`, so the
//! generated crate links `serde_json`. DashScript is statically typed, so a
//! `JSON.parse` result is the dynamic `serde_json::Value`; operating on a
//! `Value` (`v["k"]`, `v.get`) has no mapping yet and fails `cargo check`
//! honestly. `JSON.stringify(x)` needs `x: Serialize` — a scalar/`Vec`/`Value`
//! works; a DashScript `struct` (no `Serialize` derive) fails `cargo check`,
//! the honest signal that full struct serialization is a later step.

use oxc_ast::ast::Argument;
use syn::{parse_quote, Expr};

use super::super::context::Ctx;
use super::super::expressions::translate_argument;

/// `JSON.parse(s)` / `JSON.stringify(x)`. Returns `None` for other names.
pub(in crate::translator) fn json_static(
    name: &str,
    args: &[Argument],
    ctx: &Ctx<'_>,
) -> Option<Expr> {
    Some(match name {
        // `JSON.parse(s)` → `serde_json::Value`. ES throws `SyntaxError` on a
        // malformed string; DashScript has no implicit per-call `catch`, so a
        // parse failure yields `Value::Null` (a `throw` needs `catch_unwind`
        // around every parse, out of scope for the inline lowering).
        "parse" => {
            let s = translate_argument(args.first()?, ctx);
            parse_quote!(serde_json::from_str::<serde_json::Value>(&(#s).to_string()).unwrap_or(serde_json::Value::Null))
        }
        // `JSON.stringify(x)` → a JSON `String`. ES returns `"null"` for
        // `undefined`/unserializable values; a `serde_json` error (a non-
        // `Serialize` receiver) maps to that same `"null"` fallback rather than
        // panicking.
        "stringify" => {
            let x = translate_argument(args.first()?, ctx);
            parse_quote!(serde_json::to_string(&#x).unwrap_or_else(|_| "null".to_string()))
        }
        _ => return None,
    })
}
