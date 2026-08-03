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
            let a = super::assert_operand(args.first()?, ctx);
            let b = super::assert_operand(args.get(1)?, ctx);
            parse_quote!(crate::__ds::wpt_assert_equals(&(#a), &(#b)))
        }
        "assert_not_equals" => {
            let a = super::assert_operand(args.first()?, ctx);
            let b = super::assert_operand(args.get(1)?, ctx);
            parse_quote!(crate::__ds::wpt_assert_not_equals(&(#a), &(#b)))
        }
        // `assert_array_equals(actual, expected[, msg])` — length + per-element
        // SameValue. The operands deref from `&Vec<T>`/`&[T]`; the optional `msg`
        // (arg 2) is dropped. Different element types across actual/expected
        // fail inference (E0308) — an honest partial the static path leaves to
        // the fixture's other asserts.
        "assert_array_equals" => {
            let a = super::assert_operand(args.first()?, ctx);
            let b = super::assert_operand(args.get(1)?, ctx);
            parse_quote!(crate::__ds::wpt_assert_array_equals(&(#a), &(#b)))
        }
        // `assert_approx_equals(actual, expected, epsilon[, msg])` — numeric
        // approximation: pass iff `|actual - expected| <= epsilon`. Each
        // operand is a `number` cast to `f64` at the call site (so an `i64`
        // flavor local type-checks); the optional `msg` (arg 3) is dropped. A
        // non-numeric operand fails inference (E0308) — honest partial.
        "assert_approx_equals" => {
            let a = translate_argument(args.first()?, ctx);
            let b = translate_argument(args.get(1)?, ctx);
            let eps = translate_argument(args.get(2)?, ctx);
            parse_quote!(
                crate::__ds::wpt_assert_approx_equals((#a) as f64, (#b) as f64, (#eps) as f64)
            )
        }
        // `assert_true(x)` / `assert_false(x)` — WPT requires `actual === true`
        // (resp. `=== false`) strictly, which is SameValue against the boolean.
        // Routing through `wpt_assert_equals` accepts any `DsSameValue` operand
        // (a number/string projects via `DsCmp`; a non-bool projecting to
        // `Num`/`Str` mismatches `Bool` → fail, exactly the strict semantics).
        "assert_true" => {
            let a = super::assert_operand(args.first()?, ctx);
            parse_quote!(crate::__ds::wpt_assert_equals(&(#a), &true))
        }
        "assert_false" => {
            let a = super::assert_operand(args.first()?, ctx);
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
        // dropped. An assert failure inside `fn` propagates (fail-fast). WPT
        // fixtures write the callback as `function () { … }` or `() => { … }`;
        // both lower to a closure (see [`test_callback_closure`]). Any other
        // shape — or an async/generator callback — returns `None`, so the call
        // surfaces as a plain E0425 (honestly unsupported on the static path).
        "test" => match test_callback_closure(args.first()?, ctx) {
            Some(f) => parse_quote!((#f)()),
            None => return None,
        },
        // `promise_test(async fn, name)` → `wpt_promise_test(name, future).await`,
        // where `future` is the callback's `Future` — an inline async
        // arrow/function-expression lowers to `async move { body }` (its body
        // via `body_block`), or a named async-function reference lowers to
        // `named_fn()` (slice 2c). The `.await` makes the entry's `main` async
        // (Stage 1 detects it). The callback's `t` parameter is dropped — a
        // body that names it surfaces as a compile error (honest partial); most
        // `promise_test` bodies use the global `assert_*` and do not name `t`.
        // A non-async/non-expression callback, or a non-literal name, returns
        // `None` (honest unsupported).
        "promise_test" => match promise_test_callback(args, ctx) {
            Some((name, fut)) => parse_quote!(crate::__ds::wpt_promise_test(#name, #fut).await),
            None => return None,
        },
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

/// Lower the `test()` callback argument to a closure. WPT fixtures write it as
/// a `FunctionExpression` (`test(function () { … })`) or an
/// `ArrowFunctionExpression` (`test(() => { … })`); both lower to a Rust closure
/// (a `FunctionExpression` shares the block-body arrow's `FormalParameters` +
/// `FunctionBody` shape). Returns `None` for any other shape, or an
/// async/generator callback (a runtime the static path lacks) — the call then
/// surfaces as a plain E0425, honestly unsupported.
fn test_callback_closure(arg: &Argument, ctx: &Ctx<'_>) -> Option<Expr> {
    match arg {
        Argument::ArrowFunctionExpression(arrow) => Some(
            super::super::super::expressions::arrow_expr(arrow, ctx, false),
        ),
        Argument::FunctionExpression(f) => {
            super::super::super::expressions::function_expr_to_closure(f, ctx)
        }
        _ => None,
    }
}

/// `promise_test(async fn, name)` → the `(name, future)` pair: `name` is a
/// string literal, and `future` is the callback lowered to a Rust expression
/// that evaluates to a `Future` — `async move { … }` for an inline async
/// arrow/function-expression (its body via [`expressions::body_block`]), or
/// `named_fn()` for a reference to an async function declaration (the fn item
/// that [`translate_argument`] lowers, called, yields its `Future`). Returns
/// `None` if arg0 is none of those, or arg1 is not a string literal — the call
/// then surfaces as a plain E0425, honestly unsupported.
fn promise_test_callback(args: &[Argument], ctx: &Ctx<'_>) -> Option<(syn::LitStr, syn::Expr)> {
    let name = match args.get(1)?.as_expression()? {
        Expression::StringLiteral(s) => syn::LitStr::new(s.value.as_str(), Span::call_site()),
        _ => return None,
    };
    let fut: syn::Expr = match args.first()?.as_expression()? {
        Expression::ArrowFunctionExpression(arrow) if arrow.r#async => {
            let block =
                super::super::super::expressions::body_block(&arrow.params, &arrow.body, ctx);
            parse_quote!(async move #block)
        }
        Expression::FunctionExpression(f) if f.r#async => {
            let block =
                super::super::super::expressions::body_block(&f.params, f.body.as_deref()?, ctx);
            parse_quote!(async move #block)
        }
        // `promise_test(namedFn, name)` — a reference to an async function
        // declaration. `translate_argument` lowers the identifier to the Rust
        // fn-item path; calling it `()` yields its `Future` (a Rust async fn
        // item is `fn() -> impl Future`). A callback that names the `t`
        // parameter has no static test-object model — its signature then
        // mismatches the `()` call and surfaces as an honest compile error.
        Expression::Identifier(_) => {
            let callee = translate_argument(args.first()?, ctx);
            parse_quote!((#callee)())
        }
        _ => return None,
    };
    Some((name, fut))
}
