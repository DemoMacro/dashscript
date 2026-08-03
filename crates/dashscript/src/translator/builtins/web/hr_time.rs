//! High Resolution Time API — `performance.now()` (a WinterTC Web API, W3C
//! hr-time). Returns a monotonic DOMHighResTimeStamp (milliseconds since the
//! process timeOrigin). The receiver is the global `performance` object;
//! WinterTC's `self` is the global-object alias (`self === globalThis`), so
//! both `performance.now()` and `self.performance.now()` lower to the same
//! `__ds::perf_now()` helper (injected by the `HrTime` runtime dep — a
//! function-local `static OnceLock<Instant>`, pure `std`). Only `now()` is
//! mapped today; `timeOrigin`/`PerformanceObserver`/EventTarget are not Tier 1.

use oxc_ast::ast::{Expression, StaticMemberExpression};
use syn::{parse_quote, Expr};

/// `performance.now()` → `__ds::perf_now()`. Returns `None` unless the callee
/// is `<performance>.now` (the bare global, or via the WinterTC `self`
/// global-object alias), so any other receiver/name falls through to a plain
/// method call (cargo check rejects it honestly).
pub(in crate::translator) fn perf_method(sm: &StaticMemberExpression) -> Option<Expr> {
    if sm.property.name.as_str() != "now" {
        return None;
    }
    is_performance_receiver(&sm.object)?;
    Some(parse_quote!(crate::__ds::perf_now()))
}

/// Whether `expr` is the global `performance` object: the bare identifier, or
/// `self.performance` (the WinterTC `self` global-object alias).
fn is_performance_receiver(expr: &Expression) -> Option<()> {
    match expr {
        Expression::Identifier(id) if id.name.as_str() == "performance" => Some(()),
        Expression::StaticMemberExpression(sm) if sm.property.name.as_str() == "performance" => {
            matches!(&sm.object, Expression::Identifier(id) if id.name.as_str() == "self")
                .then_some(())
        }
        _ => None,
    }
}
