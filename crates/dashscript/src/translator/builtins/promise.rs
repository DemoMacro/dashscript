//! ES `Promise` static combinators — `Promise.resolve(x)`/`Promise.all([...])`.
//!
//! These are the only `Promise` forms with a static lowering (T3 stage 2a);
//! every other `Promise` usage (a bare value reference, `new Promise`,
//! `.then`/`.catch`/`.race`/`.allSettled`/`Symbol.species`/thenable `await`)
//! degrades to the engine (test262) or is `unsupported` (WinterTC). A
//! `Promise<T>` lowers to a boxed, single-threaded `Future<Output = T>`
//! (`DsPromise<T>` — the `Promise` runtime dep's `DS_PROMISE_HELPER` slice),
//! so every Promise site shares one Rust type (each `futures` combinator has a
//! distinct anonymous type; boxing unifies them).
//!
//! `Promise.resolve(x)` → `__ds::ds_promise_resolve(x)`;
//! `Promise.all([p1, p2, …])` → `__ds::ds_promise_all(vec![e1, e2, …])`, where
//! each `ei` lowers to a `DsPromise<T>` (the call site is responsible for
//! wrapping a non-Promise element; ES `Promise.all` coerces each via
//! `Promise.resolve`). Returns `None` for any other property — `classify` only
//! pulls `resolve`/`all` out of the engine degrade, so a different property
//! that slipped past surfaces honestly as a compile error.

use oxc_ast::ast::{Argument, Expression};
use syn::{parse_quote, Expr};

use super::super::context::Ctx;
use super::super::expressions::{translate_argument, translate_expr};

/// `Promise.resolve(x)` / `Promise.all([...])` → a `DsPromise<T>` expression.
/// Returns `None` for any other property name (falls through to a plain call,
/// surfacing honestly at cargo check).
pub(in crate::translator) fn promise_static(
    property: &str,
    args: &[Argument],
    ctx: &Ctx<'_>,
) -> Option<Expr> {
    Some(match property {
        // `Promise.resolve(x)` → `__ds::ds_promise_resolve(x)`. The value may
        // already be a `DsPromise` (ES `Promise.resolve(p)` returns `p` for a
        // same-constructor thenable — approximated; the common fixture shape
        // is `Promise.resolve(value)` over a plain value).
        "resolve" => {
            let x = translate_argument(args.first()?, ctx);
            parse_quote!(crate::__ds::ds_promise_resolve(#x))
        }
        // `Promise.all([p1, p2, …])` → `__ds::ds_promise_all(vec![e1, e2, …])`.
        // Each element must lower to a `DsPromise<T>`; an empty array fulfills
        // with `[]` (an empty `vec!`). A spread element or a non-array argument
        // (an arbitrary iterable) has no static lowering — `None`.
        "all" => {
            let arr = match args.first()?.as_expression()? {
                Expression::ArrayExpression(a) => a,
                _ => return None,
            };
            let elems: Vec<Expr> = arr
                .elements
                .iter()
                .map(|el| Some(translate_expr(el.as_expression()?, ctx)))
                .collect::<Option<_>>()?;
            parse_quote!(crate::__ds::ds_promise_all(vec![#(#elems),*]))
        }
        _ => return None,
    })
}
