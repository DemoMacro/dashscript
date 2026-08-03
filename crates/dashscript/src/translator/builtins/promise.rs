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

use oxc_ast::ast::{
    Argument, BindingPattern, Expression, FormalParameters, FunctionBody, StaticMemberExpression,
};
use quote::format_ident;
use syn::{parse_quote, Expr};

use super::super::bindings;
use super::super::context::Ctx;
use super::super::expressions::{body_block, translate_argument, translate_expr};

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

/// `p.then(onFulfilled)` (and future instance combinators) on a `DsPromise<T>`
/// receiver — a `Promise` instance method (T3 stage 2b). Returns `None` for any
/// other property name or a non-Promise receiver (falls through to a plain
/// call, surfacing honestly at cargo check). `then` lowers to
/// `ds_promise_then(p, onFul)`; `onFul` (arg 0) is an arrow/function expression
/// lowering to a closure whose parameter type Rust infers from the receiver's
/// `DsPromise<T>`. `onRejected` (arg 1), thenable flattening, and `.catch`/
/// `.finally` are not yet modelled — honest partials.
pub(in crate::translator) fn promise_instance_method(
    sm: &StaticMemberExpression,
    args: &[Argument],
    ctx: &Ctx<'_>,
) -> Option<Expr> {
    if !receiver_is_promise(&sm.object, ctx) {
        return None;
    }
    Some(match sm.property.name.as_str() {
        "then" => {
            // ES `then(onFulfilled, onRejected)` — only the single-callback
            // form lowers statically; an `onRejected` (arg 1) needs engine-side
            // error propagation, so it falls through (an honest partial).
            if args.len() > 1 {
                return None;
            }
            let on_ful_arg = args.first()?;
            // `onFulfilled` must be a function expression (arrow or `function`);
            // a non-function value (ES skips it) has no static lowering.
            if !matches!(
                on_ful_arg.as_expression(),
                Some(Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_))
            ) {
                return None;
            }
            let p = translate_expr(&sm.object, ctx);
            let on_ful = translate_argument(on_ful_arg, ctx);
            parse_quote!(crate::__ds::ds_promise_then(#p, #on_ful))
        }
        _ => return None,
    })
}

/// True when `expr` evaluates to a `DsPromise<T>` — either a local whose
/// resolved type is `DsPromise` (`const p = Promise.resolve(x); p.then(..)`), or
/// a `Promise.resolve(..)`/`Promise.all([..])` call (the only two static
/// combinators with a native emit), so a chained `.then` lands on a Promise
/// value. A `fetch(..)`/thenable-`await` receiver is not yet covered — those
/// stay honest partials.
fn receiver_is_promise(expr: &Expression, ctx: &Ctx<'_>) -> bool {
    if is_ds_promise_local(expr, ctx) {
        return true;
    }
    if let Expression::CallExpression(c) = expr {
        if let Expression::StaticMemberExpression(sm) = &c.callee {
            if let Expression::Identifier(id) = &sm.object {
                if id.name.as_str() == "Promise"
                    && matches!(sm.property.name.as_str(), "resolve" | "all")
                {
                    return true;
                }
            }
        }
    }
    // `new Promise(executor)` yields a `DsPromise<T>` (T3 stage 2c), so a
    // chained `.then`/`.catch` on the constructor result dispatches here.
    if let Expression::NewExpression(n) = expr {
        if let Expression::Identifier(id) = &n.callee {
            return id.name.as_str() == "Promise";
        }
    }
    false
}

/// True when `expr` is a local whose resolved type's last path segment is
/// `DsPromise` (the boxed-Future alias), so `p.then(..)` lowers to the
/// combinator rather than a phantom method binding.
fn is_ds_promise_local(expr: &Expression, ctx: &Ctx<'_>) -> bool {
    let Expression::Identifier(id) = expr else {
        return false;
    };
    let name = bindings::snake(&id.name).to_string();
    ctx.local_type(&name)
        .is_some_and(|p| p.segments.last().is_some_and(|s| s.ident == "DsPromise"))
}

/// True when `new Promise(executor)` has a static lowering — the executor
/// (arg 0) is a non-`async` arrow or a non-`async` non-generator `function`
/// expression, exactly the shapes [`promise_ctor`] accepts. `classify` queries
/// this so its `NewExpression` verdict stays in lockstep with the emit (the
/// classify↔emit drift metatest enforces it); a diverging shape would either
/// degrade a ctor that maps or map one that phantoms.
pub(in crate::translator) fn promise_executor_is_static(args: &[Argument]) -> bool {
    let Some(executor) = args.first().and_then(Argument::as_expression) else {
        return false;
    };
    match executor {
        // An async executor drops an `await` into the sync wrapper closure.
        Expression::ArrowFunctionExpression(a) => !a.r#async,
        Expression::FunctionExpression(f) => !f.r#async && !f.generator,
        _ => false,
    }
}

/// `new Promise((resolve, reject) => { … })` → `__ds::ds_promise_new(closure)`.
/// The executor runs synchronously under a clonable `DsResolver`; the closure
/// the translator emits takes a single synthesized `__ds_res: DsResolver<_>`
/// and a prelude binds the JS `resolve`/`reject` names to resolver-method
/// closures (`move |v| __ds_res.resolve(v)` / `.reject(v)`). Cloning the
/// resolver (a cheap `Arc`) lets it escape into a deferred callback
/// (`setTimeout(() => resolve(x), …)`). The value type `T` is inferred from
/// the `resolve(value)` call site — Rust propagates it through the resolver
/// method to `ds_promise_new`'s return.
///
/// Returns `None` for a non-function executor, an async/generator executor (a
/// Promise executor is always called synchronously — an `async` arrow would
/// drop an `await` into the sync wrapper), or a body-less `FunctionExpression`,
/// so those fall through to the generic `new` path and surface honestly.
pub(in crate::translator) fn promise_ctor(args: &[Argument], ctx: &Ctx<'_>) -> Option<Expr> {
    let executor = args.first()?.as_expression()?;
    let (params, body): (&FormalParameters, &FunctionBody) = match executor {
        // An async/generator executor is not a synchronous Promise executor —
        // its `await`/`yield` have no place in the sync wrapper closure.
        Expression::ArrowFunctionExpression(a) if !a.r#async => (&a.params, &a.body),
        Expression::FunctionExpression(f) if !f.r#async && !f.generator => {
            (&f.params, f.body.as_deref()?)
        }
        _ => return None,
    };
    // Translate the executor body with its params registered (so mutation/
    // flavor analysis sees `resolve`/`reject`), but NOT emitted as Rust params
    // — the wrapper takes a single `__ds_res`, and the prelude provides the JS
    // names. `body_block` is the shared body translator (same one a block-body
    // arrow uses), so the executor body and any other closure body never diverge.
    let mut block = body_block(params, body, ctx);
    // Prelude: `let <jsName> = { let __ds_res = __ds_res.clone(); move |v| __ds_res.<m>(v) };`
    // for each of the first two params (resolve, then reject). Only a plain
    // identifier param binds (a destructured executor param is vanishingly rare
    // and has no static lowering here).
    let mut prelude: Vec<syn::Stmt> = Vec::new();
    for (i, fp) in params.items.iter().enumerate().take(2) {
        if !matches!(fp.pattern, BindingPattern::BindingIdentifier(_)) {
            continue;
        }
        let name = bindings::binding_name(&fp.pattern);
        let method = if i == 0 {
            format_ident!("resolve")
        } else {
            format_ident!("reject")
        };
        prelude.push(parse_quote! {
            let #name = {
                let __ds_res = __ds_res.clone();
                move |v| __ds_res.#method(v)
            };
        });
    }
    // The prelude must run before the executor body (it binds `resolve`/
    // `reject` the body references); prepend it to the translated block.
    prelude.append(&mut block.stmts);
    block.stmts = prelude;
    Some(parse_quote!(
        crate::__ds::ds_promise_new(|__ds_res: crate::__ds::DsResolver<_>| #block)
    ))
}
