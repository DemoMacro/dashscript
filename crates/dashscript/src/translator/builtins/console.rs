//! `console.<m>(…)` → Rust print macros. `console` is a host object (not in
//! tc39 test262's built-ins), so it lives here alongside the ES built-ins
//! rather than mirroring a test262 directory.

use oxc_ast::ast::Expression;
use quote::format_ident;
use syn::Ident;

use super::is_ident;

/// The Rust macro for a `console.<m>(…)` call: `log` → `println!`, `warn`/
/// `error` → `eprintln!`. Returns `None` for any other member.
pub(in crate::translator) fn console_method(callee: &Expression) -> Option<Ident> {
    let Expression::StaticMemberExpression(member) = callee else {
        return None;
    };
    if !is_ident(&member.object, "console") {
        return None;
    }
    let name = match member.property.name.as_str() {
        "log" => "println",
        "warn" | "error" => "eprintln",
        _ => return None,
    };
    Some(format_ident!("{}", name))
}
