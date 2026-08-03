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
use super::super::super::expressions::{is_event_target_local, translate_argument, translate_expr};
use super::super::es_to_string_arg;

/// The Rust type a WHATWG EventTarget API constructor builds, if `name` is one:
/// `EventTarget` → `crate::__ds::DsEventTarget`, `Event` → `crate::__ds::DsEvent`.
/// `None` for any other name (the `new` lowering falls through to the generic
/// `Foo::new` path and surfaces at `cargo check`).
pub(in crate::translator) fn event_target_ctor_type(name: &str) -> Option<Type> {
    match name {
        "EventTarget" => Some(parse_quote!(crate::__ds::DsEventTarget)),
        "Event" => Some(parse_quote!(crate::__ds::DsEvent)),
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
    if !is_event_target_local(&sm.object, ctx) {
        return None;
    }
    let name = sm.property.name.as_str();
    let obj = translate_expr(&sm.object, ctx);
    Some(match name {
        // `et.addEventListener(type, cb[, useCapture|options])` →
        // `et.add_event_listener(type, Box::new(..))`. A `null`/`undefined`
        // callback (ES ignores it) returns `None` so the call surfaces honestly
        // rather than boxing a non-callable.
        "addEventListener" => {
            let type_ = es_to_string_arg(args.first()?, ctx);
            let cb = event_listener_callback(args.get(1)?, ctx)?;
            parse_quote!({
                #obj.add_event_listener(#type_, #cb);
            })
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
    Some(parse_quote!(
        ::std::boxed::Box::new(move |__ds_evt: &crate::__ds::DsEvent| {
            let _ = (#cb)(__ds_evt);
        })
    ))
}
