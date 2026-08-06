//! WHATWG EventTarget/Event API — the WinterTC (Ecma TC55) DOM Events surface
//! (`new EventTarget()`, `new Event(type, init)`, `addEventListener`/
//! `removeEventListener`/`dispatchEvent`, `event.type`/`.bubbles`/…). The
//! constructors build a `crate::__ds::DsEventTarget` / `crate::__ds::DsEvent`
//! injected by the `EventTarget` runtime dep (a pure-`std` pub/sub — never
//! degraded). `event_target_ctor_type` carries the name → type mapping the
//! `new` lowering needs; `event_init` destructures the `{ bubbles, cancelable }`
//! init object; `event_target_method` dispatches the instance methods on a
//! `DsEventTarget` receiver. The event properties (`event.type`/`.bubbles`/…)
//! are dispatched in `member.rs`.
//!
//! A listener callback (`addEventListener`'s second arg) is wrapped in a
//! discard-return adapter so any callback shape — a named function reference
//! (`et.addEventListener("x", listener)`), an arrow (`(e) => { … }`), or a
//! function expression — type-checks against `Box<dyn FnMut(&DsEvent)>`:
//! `Box::new(move |__ds_evt: &DsEvent| { let _ = <cb>(__ds_evt); })`. The
//! adapter's parameter is annotated `&DsEvent`, so a named-function listener's
//! `evt` parameter is inferred as `&DsEvent` by the per-body parameter scan in
//! `analysis.rs` (the way `flavor::infer` pins `number` flavors).

use oxc_ast::ast::{
    Argument, Expression, ObjectExpression, ObjectPropertyKind, PropertyKey, StaticMemberExpression,
};
use syn::{parse_quote, Expr, Type};

use super::super::super::context::Ctx;
use super::super::super::expressions::{
    is_abort_controller_receiver, is_abort_signal_receiver, is_event_target_local,
    translate_argument, translate_expr,
};
use super::super::es_to_string_arg;

/// True when `expr` is the WinterTC global-scope identifier `self` or
/// `globalThis` — the global object, which is itself an `EventTarget`. A member
/// call on such a receiver (`self.addEventListener(…)`) routes to the shared
/// `__ds::wpt_self()` target instead of a phantom `self_`/`global_this` binding
/// (which would fail with E0425).
pub(in crate::translator) fn is_self_or_global_this(expr: &Expression) -> bool {
    matches!(
        expr,
        Expression::Identifier(id) if matches!(id.name.as_str(), "self" | "globalThis")
    )
}

/// The Rust type a WHATWG EventTarget API constructor builds, if `name` is one:
/// `EventTarget` → `crate::__ds::DsEventTarget`, `Event` → `crate::__ds::DsEvent`.
/// `None` for any other name (the `new` lowering falls through to the generic
/// `Foo::new` path and surfaces at `cargo check`).
pub(in crate::translator) fn event_target_ctor_type(name: &str) -> Option<Type> {
    match name {
        "EventTarget" => Some(parse_quote!(crate::__ds::DsEventTarget)),
        "Event" => Some(parse_quote!(crate::__ds::DsEvent)),
        // `DsCustomEvent` (no `<T>` — the chunk type is inferred at the call
        // site from the `detail` value, the way `DsReadableStream`'s `T` is).
        // The member dispatch keys off the last path segment, so the bare
        // `DsCustomEvent` registration routes `ev.detail`/`.type`/… correctly.
        "CustomEvent" => Some(parse_quote!(crate::__ds::DsCustomEvent)),
        "AbortController" => Some(parse_quote!(crate::__ds::DsAbortController)),
        _ => None,
    }
}

/// `new Event(type, init?)`'s `init` object — `{ bubbles, cancelable }`, both
/// defaulting to `false`. Only BooleanLiteral field values lower statically (the
/// common fixture shape); a non-literal value or absent field defaults to
/// `false` (the ES default). Other `init` fields (`composed`/`detail`/…) are
/// dropped — the `DsEventInit` model carries only the two the dispatch contract
/// reads.
pub(in crate::translator) fn event_init(obj: &ObjectExpression) -> Expr {
    let mut bubbles: Expr = parse_quote!(false);
    let mut cancelable: Expr = parse_quote!(false);
    for kind in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(p) = kind else {
            continue;
        };
        let name = match &p.key {
            PropertyKey::StaticIdentifier(id) => id.name.as_str(),
            PropertyKey::StringLiteral(s) => s.value.as_str(),
            _ => continue,
        };
        let value = match &p.value {
            Expression::BooleanLiteral(b) => b.value,
            _ => continue,
        };
        let lit = syn::LitBool::new(value, proc_macro2::Span::call_site());
        match name {
            "bubbles" => bubbles = parse_quote!(#lit),
            "cancelable" => cancelable = parse_quote!(#lit),
            _ => {}
        }
    }
    parse_quote!(crate::__ds::DsEventInit {
        bubbles: #bubbles,
        cancelable: #cancelable,
    })
}

/// `new CustomEvent(type, init?)`'s `init` object — `{ detail, bubbles,
/// cancelable }`. Returns the three as separate exprs (the `DsCustomEvent::new`
/// signature takes them positionally). `detail` is `None` when absent or not a
/// plain expression (ES `undefined`); `bubbles`/`cancelable` default to `false`
/// and only BooleanLiteral fields lower statically (the common fixture shape —
/// mirroring [`event_init`]). Other `init` fields (`sweet`/`composed`/…) are
/// dropped — ES ignores unknown fields on the init record, and a property read
/// like `ev.sweet` lowers to a plain field access that surfaces honestly.
pub(in crate::translator) fn custom_event_init(
    obj: &ObjectExpression,
    ctx: &Ctx<'_>,
) -> (Option<Expr>, Expr, Expr) {
    let mut detail: Option<Expr> = None;
    let mut bubbles: Expr = parse_quote!(false);
    let mut cancelable: Expr = parse_quote!(false);
    for kind in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(p) = kind else {
            continue;
        };
        let name = match &p.key {
            PropertyKey::StaticIdentifier(id) => id.name.as_str(),
            PropertyKey::StringLiteral(s) => s.value.as_str(),
            _ => continue,
        };
        match name {
            "detail" => detail = Some(translate_expr(&p.value, ctx)),
            "bubbles" => {
                if let Expression::BooleanLiteral(b) = &p.value {
                    let lit = syn::LitBool::new(b.value, proc_macro2::Span::call_site());
                    bubbles = parse_quote!(#lit);
                }
            }
            "cancelable" => {
                if let Expression::BooleanLiteral(b) = &p.value {
                    let lit = syn::LitBool::new(b.value, proc_macro2::Span::call_site());
                    cancelable = parse_quote!(#lit);
                }
            }
            _ => {}
        }
    }
    (detail, bubbles, cancelable)
}

/// An `EventTarget` instance method, dispatched on the receiver's resolved
/// type. Returns `None` for a non-`DsEventTarget` receiver or an unmapped name,
/// so the call falls through to a plain method call (cargo check rejects it
/// honestly). The `useCapture`/`options` third arg of `addEventListener`/
/// `removeEventListener` is dropped (single-threaded, no capture phase); the
/// `removeEventListener` callback arg is dropped too (the helper removes every
/// listener for `type` — see `DsEventTarget::remove_event_listener`).
pub(in crate::translator) fn event_target_method(
    sm: &StaticMemberExpression,
    args: &[Argument],
    ctx: &Ctx<'_>,
) -> Option<Expr> {
    // WinterTC `self` / `globalThis` is the global scope, which is itself an
    // `EventTarget` — route `self.addEventListener(…)` / `globalThis.
    // removeEventListener(…)` to the shared `__ds::wpt_self()` target, the same
    // way a `new EventTarget()` local routes to its own listener set.
    let self_scope = is_self_or_global_this(&sm.object);
    if !is_event_target_local(&sm.object, ctx) && !self_scope {
        return None;
    }
    let name = sm.property.name.as_str();
    let obj: Expr = if self_scope {
        parse_quote!(crate::__ds::wpt_self())
    } else {
        translate_expr(&sm.object, ctx)
    };
    Some(match name {
        // `et.addEventListener(type, cb[, useCapture|options])` →
        // `et.add_event_listener(type, Box::new(..))`. A `null`/`undefined`
        // (or absent) callback is a no-op per ES — emit an empty block rather
        // than boxing a non-callable or falling back to an unmapped call.
        "addEventListener" => {
            let type_ = es_to_string_arg(args.first()?, ctx);
            match args.get(1).and_then(|a| event_listener_callback(a, ctx)) {
                Some(cb) => parse_quote!({
                    #obj.add_event_listener(#type_, #cb);
                }),
                None => parse_quote!({}),
            }
        }
        // `et.removeEventListener(type, cb[, options])` → drop `cb`/`options`.
        "removeEventListener" => {
            let type_ = es_to_string_arg(args.first()?, ctx);
            parse_quote!({
                #obj.remove_event_listener(#type_);
            })
        }
        // `et.dispatchEvent(event)` → `et.dispatch_event(&event)` (the ES
        // return value: `false` iff cancelable + `preventDefault` was called).
        "dispatchEvent" => {
            let event = translate_argument(args.first()?, ctx);
            parse_quote!(#obj.dispatch_event(&#event))
        }
        _ => return None,
    })
}

/// AbortController/AbortSignal instance methods, dispatched on the receiver's
/// resolved type. `controller.abort()` flips the shared flag; `signal.
/// addEventListener`/`removeEventListener`/`dispatchEvent` route to the signal's
/// embedded EventTarget (an `AbortSignal` extends `EventTarget`). Returns `None`
/// for any other receiver/name (the call falls through to a plain method call →
/// cargo check honestly). The ES `reason` arg of `abort` and the `useCapture`/
/// `options` arg of `addEventListener`/`removeEventListener` are dropped
/// (single-threaded, no capture phase — same simplification as `DsEventTarget`).
pub(in crate::translator) fn abort_method(
    sm: &StaticMemberExpression,
    args: &[Argument],
    ctx: &Ctx<'_>,
) -> Option<Expr> {
    let name = sm.property.name.as_str();
    if is_abort_controller_receiver(&sm.object, ctx) {
        let obj = translate_expr(&sm.object, ctx);
        return match name {
            // `controller.abort([reason])` — flip `aborted` + fire "abort". The
            // ES `reason` arg is dropped (the common WPT shape does not read it).
            "abort" => Some(parse_quote!({ #obj.abort(); })),
            _ => None,
        };
    }
    if is_abort_signal_receiver(&sm.object, ctx) {
        let obj = translate_expr(&sm.object, ctx);
        return match name {
            // `signal.addEventListener(type, cb)` → the embedded EventTarget,
            // via the same discard-return adapter as `DsEventTarget`. A `null`/
            // `undefined` (or absent) callback is a no-op per ES.
            "addEventListener" => {
                let type_ = es_to_string_arg(args.first()?, ctx);
                match args.get(1).and_then(|a| event_listener_callback(a, ctx)) {
                    Some(cb) => Some(parse_quote!({ #obj.add_event_listener(#type_, #cb); })),
                    None => Some(parse_quote!({})),
                }
            }
            "removeEventListener" => {
                let type_ = es_to_string_arg(args.first()?, ctx);
                Some(parse_quote!({ #obj.remove_event_listener(#type_); }))
            }
            "dispatchEvent" => {
                let event = translate_argument(args.first()?, ctx);
                Some(parse_quote!(#obj.dispatch_event(&#event)))
            }
            // `signal.throwIfAborted()` — if aborted, panic the reason (a
            // `DsError`); else a no-op.
            "throwIfAborted" if args.is_empty() => Some(parse_quote!({ #obj.throw_if_aborted(); })),
            _ => None,
        };
    }
    None
}

/// `AbortSignal` static methods, dispatched when the receiver is the
/// `AbortSignal` constructor itself (a global-scope identifier, not a
/// `DsAbortSignal` value). `AbortSignal.abort()` → a fresh aborted signal (the
/// default `AbortError` reason); mirrors `url_static_method`'s `URL` guard.
/// Returns `None` for any other receiver/name (the call falls through to a
/// plain method call → cargo check honestly). `AbortSignal.any()` (multi-signal
/// aggregation) and `.timeout()` (a timer) are out of scope (honest partials).
pub(in crate::translator) fn abort_static(
    sm: &StaticMemberExpression,
    args: &[Argument],
) -> Option<Expr> {
    match &sm.object {
        Expression::Identifier(id) if id.name.as_str() == "AbortSignal" => {}
        _ => return None,
    }
    let name = sm.property.name.as_str();
    Some(match name {
        "abort" if args.is_empty() => {
            parse_quote!(crate::__ds::DsAbortSignal::aborted_signal())
        }
        _ => return None,
    })
}

/// Lower an `addEventListener` callback argument to a `Box<dyn FnMut(&DsEvent)>`
/// via a discard-return adapter. Any callback shape works: a named function
/// reference lowers to its fn-item path, an arrow/function-expression to a
/// closure — the adapter calls it and drops the return, so a `listener` that
/// returns `bool` and an arrow that returns `()` both satisfy `FnMut(&DsEvent)
/// -> ()`. The adapter's `__ds_evt: &DsEvent` annotation drives the per-body
/// parameter inference (`analysis.rs`) that pins a named listener's `evt`
/// parameter to `&DsEvent`. Returns `None` for a `null`/`undefined` callback
/// (ES ignores it; the call surfaces honestly).
fn event_listener_callback(arg: &Argument, ctx: &Ctx<'_>) -> Option<Expr> {
    match arg.as_expression()? {
        Expression::NullLiteral(_) => return None,
        Expression::Identifier(id) if id.name.as_str() == "undefined" => return None,
        _ => {}
    }
    let cb = translate_argument(arg, ctx);
    // A 0-arg listener (`function handler() { … }` / `() => { … }`) lowers to a
    // Rust fn item / closure taking no args, so forwarding `__ds_evt` would be
    // E0057. Detect a 0-arg callback and emit a discard-arg adapter that drops
    // the event; otherwise forward the event to the callback.
    if callback_is_zero_arg(arg, ctx) {
        Some(parse_quote!(
            ::std::boxed::Box::new(move |__ds_evt: &crate::__ds::DsEvent| {
                let _ = __ds_evt;
                let _ = (#cb)();
            })
        ))
    } else {
        Some(parse_quote!(
            ::std::boxed::Box::new(move |__ds_evt: &crate::__ds::DsEvent| {
                let _ = (#cb)(__ds_evt);
            })
        ))
    }
}

/// Whether an `addEventListener` callback is a 0-arg callable — a listener
/// that does not take the event, so the discard-arg adapter is safe. An inline
/// function/arrow expression with no parameters is 0-arg; a named reference is
/// 0-arg only when it resolves to a registered top-level fn with no params (the
/// registry does not recurse into fn bodies, so a nested fn or a closure local
/// reads as unknown — forward the event rather than guess, since assuming 0-arg
/// would mis-forward a 1-arg listener with E0061). Any other shape forwards.
fn callback_is_zero_arg(arg: &Argument, ctx: &Ctx<'_>) -> bool {
    match arg.as_expression() {
        Some(Expression::FunctionExpression(f)) => f.params.items.is_empty(),
        Some(Expression::ArrowFunctionExpression(a)) => a.params.items.is_empty(),
        Some(Expression::Identifier(id)) => matches!(
            ctx.function_params(id.name.as_str()),
            Some(params) if params.is_empty()
        ),
        _ => false,
    }
}
