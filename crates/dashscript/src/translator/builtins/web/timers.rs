//! WHATWG `setTimeout`/`setInterval`/`clearTimeout`/`clearInterval` (HTML §8.6
//! timers, a WinterTC Web API) — the event loop's task queue. ES `setTimeout`
//! queues a callback to fire later; the static path models that queue as a
//! `thread_local` drain run at the entry's end (the moment ES itself drains —
//! main returned, call stack empty). `timer_function` dispatches the four
//! globals to `__ds::wpt_*` helpers in the `Timers` runtime dep slice; the
//! callback argument is wrapped in a discard-return thunk so any callback shape
//! type-checks against `Box<dyn FnMut()>`. See `TIMERS_HELPER`.
//!
//! Sibling to the other WinterTC Web API modules (`url`/`crypto`/…); the
//! globals are dispatched in `global.rs` (they are bare-identifier calls, not
//! instance methods on a receiver).

use oxc_ast::ast::{Argument, Expression, IdentifierReference};
use syn::{parse_quote, Expr};

use super::super::super::context::Ctx;
use super::super::super::expressions::translate_argument;

/// `setTimeout`/`setInterval`/`clearTimeout`/`clearInterval` called as plain
/// identifiers. Returns `None` for any other name (falls through to a plain
/// call, surfacing honestly as E0425). `setTimeout`/`setInterval` register the
/// callback on the queue; `clearTimeout`/`clearInterval` cancel a handle (ES
/// keeps both handle kinds in one id space, so one clear covers both).
pub(in crate::translator) fn timer_function(
    id: &IdentifierReference,
    args: &[Argument],
    ctx: &Ctx<'_>,
) -> Option<Expr> {
    let name: &str = &id.name;
    Some(match name {
        "setTimeout" => {
            let cb = timer_callback_thunk(args.first()?, ctx);
            let delay = timer_delay(args.get(1), ctx);
            parse_quote!(crate::__ds::wpt_set_timeout(#cb, #delay))
        }
        "setInterval" => {
            let cb = timer_callback_thunk(args.first()?, ctx);
            let delay = timer_delay(args.get(1), ctx);
            parse_quote!(crate::__ds::wpt_set_interval(#cb, #delay))
        }
        "clearTimeout" | "clearInterval" => {
            let handle = translate_argument(args.first()?, ctx);
            parse_quote!(crate::__ds::wpt_clear_timer(#handle))
        }
        // `queueMicrotask(cb)` — HTML's microtask queue (a WinterTC §5.2 global).
        // The callback is the same `Box<dyn FnMut()>` thunk shape as a timer
        // callback; the queue drains at every task boundary inside
        // `wpt_run_timers` (after each fire) and once at the entry's end before
        // the timer queue runs (see `wpt_drain_microtasks`). A non-function
        // argument (`undefined`/`null`/`0`) fails to type-check against
        // `FnMut()` → cargo-check-fail → honestly `unsupported`, matching ES
        // throwing `TypeError` for a non-callback (surfaced as a build failure
        // rather than a runtime throw on the static path).
        "queueMicrotask" => {
            let cb = timer_callback_thunk(args.first()?, ctx);
            parse_quote!(crate::__ds::wpt_queue_microtask(#cb))
        }
        _ => return None,
    })
}

/// The delay (ms) — `setTimeout`/`setInterval`'s 2nd arg. Missing → 0 (ES: a
/// missing delay is 0). Cast to `f64` so an `i64` flavor literal (`Math.pow(…)`)
/// type-checks against the helper's `f64`; the clamp then wraps it as HTML's
/// `long` (so `Math.pow(2, 32)` → 0).
fn timer_delay(arg: Option<&Argument>, ctx: &Ctx<'_>) -> Expr {
    match arg {
        Some(a) => {
            let d = translate_argument(a, ctx);
            parse_quote!((#d) as f64)
        }
        None => parse_quote!(0.0f64),
    }
}

/// Lower a timer callback argument to a `Box<dyn FnMut()>` thunk that fires it
/// (return discarded). Any callback shape works: a WPT harness global
/// (`done`/`assert_unreached`) lowers to the matching helper; a named function
/// reference / arrow / function-expression lowers to a closure the thunk calls.
/// `done` is special-cased to `wpt_done()` — the drain keys its stop off the
/// DONE flag, so a queued `done` callback must set it (the same flag a
/// `done()` call sets inside an interval body like `next`).
fn timer_callback_thunk(arg: &Argument, ctx: &Ctx<'_>) -> Expr {
    let body: Expr = match arg.as_expression() {
        Some(Expression::Identifier(id)) => match id.name.as_str() {
            "done" => parse_quote!({
                crate::__ds::wpt_done();
            }),
            // `setup` as a callback is a no-op (the harness setup phase).
            "setup" => parse_quote!({}),
            "assert_unreached" => parse_quote!({
                crate::__ds::wpt_assert_unreached();
            }),
            _ => {
                let name = translate_argument(arg, ctx);
                parse_quote!({ let _ = (#name)(); })
            }
        },
        _ => {
            let cb = translate_argument(arg, ctx);
            parse_quote!({ let _ = (#cb)(); })
        }
    };
    parse_quote!(::std::boxed::Box::new(move || #body))
}
