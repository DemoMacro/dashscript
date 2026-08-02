//! The WPT (web-platform-tests) testharness API — `test()`/`assert_equals`/
//! `assert_throws_dom`/…. These are *not* ES built-ins; they are the
//! host-defined test harness WPT fixtures call (the web-platform analogue of
//! test262's `assert.sameValue`). WinterTC conformance runs WPT fixtures on the
//! **static path** (translate → cargo → run), so the harness API lowers to Rust
//! helpers (`__ds::wpt_*`) — never to the embedded engine — matching the
//! "WinterTC is pure-Rust, no degradation" contract. Composite asserts
//! (`assert_array_equals`/`assert_object_equals`/…) and the async forms
//! (`async_test`/`promise_test`) stay unmapped (`classify` rejects them
//! honestly — the async forms need a runtime this static path does not ship).
//!
//! Sibling to `assert.rs` (the test262 harness); both lower to `ASSERT_HELPER`.

use oxc_ast::ast::{Argument, Expression, IdentifierReference};
use proc_macro2::Span;
use syn::{parse_quote, Expr};

use super::super::super::context::Ctx;
use super::super::super::expressions::translate_argument;

/// WPT testharness global functions called as plain identifiers: `test(fn,
/// name)`, `assert_equals(a, b)`, `assert_true(x)`, `assert_throws_dom(name,
/// fn)`, …. Returns `None` for any other name (falls through to a plain call,
/// surfacing honestly as E0425).
///
/// `test(fn, name)` lowers to an immediate closure invocation — the
/// assert-driven conformance verdict (any failure = partial) does not need the
/// browser-facing per-test aggregation, so `name` is dropped and an assert
/// failure inside `fn` propagates as a panic (fail-fast, matching test262's own
/// assert semantics). `setup`/`done` are no-ops (synchronous fixtures run their
/// `test()` calls in source order; there is no aggregation step on the static
/// path).
pub(in crate::translator) fn testharness_function(
    id: &IdentifierReference,
    args: &[Argument],
    ctx: &Ctx<'_>,
) -> Option<Expr> {
    let name: &str = &id.name;
    Some(match name {
        // `assert_equals(a, b[, msg])` → SameValue check; the optional `msg`
        // (arg 2) is dropped — the verdict keys off the `AssertionError:`
        // prefix only.
        "assert_equals" => {
            let a = translate_argument(args.first()?, ctx);
            let b = translate_argument(args.get(1)?, ctx);
            parse_quote!(crate::__ds::wpt_assert_equals(&(#a), &(#b)))
        }
        "assert_not_equals" => {
            let a = translate_argument(args.first()?, ctx);
            let b = translate_argument(args.get(1)?, ctx);
            parse_quote!(crate::__ds::wpt_assert_not_equals(&(#a), &(#b)))
        }
        // `assert_true(x)` / `assert_false(x)` — WPT requires `actual === true`
        // (resp. `=== false`) strictly, which is SameValue against the boolean.
        // Routing through `wpt_assert_equals` accepts any `DsSameValue` operand
        // (a number/string projects via `DsCmp`; a non-bool projecting to
        // `Num`/`Str` mismatches `Bool` → fail, exactly the strict semantics).
        "assert_true" => {
            let a = translate_argument(args.first()?, ctx);
            parse_quote!(crate::__ds::wpt_assert_equals(&(#a), &true))
        }
        "assert_false" => {
            let a = translate_argument(args.first()?, ctx);
            parse_quote!(crate::__ds::wpt_assert_equals(&(#a), &false))
        }
        // `assert_throws_dom(name, fn)` / `assert_throws_js(ctor, fn)` — see
        // [`wpt_assert_throws_expr`]. `return`d (not wrapped in the outer
        // `Some(...)`) since the helper returns `Option<Expr>` itself.
        "assert_throws_dom" | "assert_throws_js" => return wpt_assert_throws_expr(args, ctx),
        // `assert_unreached([msg])` — always panics. The optional message is
        // dropped (the helper takes no args; the verdict keys off the prefix).
        "assert_unreached" => parse_quote!(crate::__ds::wpt_assert_unreached()),
        // `test(fn, name[, props])` → invoke `fn` immediately; `name`/`props`
        // dropped. An assert failure inside `fn` propagates (fail-fast).
        "test" => {
            let f = translate_argument(args.first()?, ctx);
            parse_quote!((#f)())
        }
        // `setup(fn_or_props)` / `done()` — no-ops on the static path.
        "setup" | "done" => parse_quote!(()),
        _ => return None,
    })
}

/// `assert_throws_dom(name, fn)` / `assert_throws_js(ctor, fn)` →
/// `__ds::wpt_assert_throws("name", || body)`. The expected name is a string
/// literal (`"NetworkError"`) or an Identifier (a JS constructor like
/// `TypeError`); either emits a `&str` literal the helper matches against
/// `DsError.name`. The body lowers to a `FnOnce() -> R` closure (arrow params
/// dropped by `arrow_expr`). Returns `None` if arg0 is neither a string nor an
/// Identifier (a dynamic expected value is reflection the static path cannot
/// express).
fn wpt_assert_throws_expr(args: &[Argument], ctx: &Ctx<'_>) -> Option<Expr> {
    let expected = match args.first()?.as_expression()? {
        Expression::StringLiteral(s) => syn::LitStr::new(s.value.as_str(), Span::call_site()),
        Expression::Identifier(id) => syn::LitStr::new(id.name.as_str(), Span::call_site()),
        _ => return None,
    };
    let f = translate_argument(args.get(1)?, ctx);
    Some(parse_quote!(crate::__ds::wpt_assert_throws(#expected, #f)))
}
