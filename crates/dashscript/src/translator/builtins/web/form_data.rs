//! WHATWG `FormData` API — `new FormData()` constructor + the void/bool
//! instance methods (FETCH §5.2 / XHR, a WinterTC Web API). The constructor
//! builds a `crate::__ds::DsFormData` (an ordered `(name, value)` list whose
//! value is a `string` or a `File`); `form_data_method` dispatches
//! `append`/`has`/`delete`/`set` on the receiver's resolved type. Each name
//! argument is coerced via ES `ToString` (`es_to_string_arg`, the same path
//! `Headers`/`URLSearchParams` take); the value argument routes to the `_file`
//! variant when it is a `DsFile` local and the `_str` variant otherwise. The
//! value-returning `get`/`getAll`/`entries`/`keys`/`values`/`forEach` stay
//! unmapped here — their `string | File` union result needs the union-unboxing
//! path (a separate batch); the static path lowers the mutation+query surface,
//! which is the common server shape.

use oxc_ast::ast::{Argument, Expression, StaticMemberExpression};
use syn::{parse_quote, Expr, Type};

use super::super::super::bindings::snake;
use super::super::super::context::Ctx;
use super::super::super::expressions::{is_form_data_local, translate_argument, translate_expr};
use super::super::es_to_string_arg;

/// The Rust type a WHATWG `FormData` constructor builds, if `name` is
/// `FormData`: `crate::__ds::DsFormData`. `None` otherwise (the `new` lowering
/// falls through to the generic `Foo::new` path and surfaces at `cargo check`).
pub(in crate::translator) fn form_data_ctor_type(name: &str) -> Option<Type> {
    match name {
        "FormData" => Some(parse_quote!(crate::__ds::DsFormData)),
        _ => None,
    }
}

/// `new FormData()` → `DsFormData::new()`. ES `new FormData(form)` (an HTML
/// `form` element) has no static lowering — a non-empty `args` panics the
/// `TypeError` the HTML path would surface (the WPT verdict reads the panic
/// prefix); the common server shape is the no-arg ctor.
pub(in crate::translator) fn form_data_ctor(args: &[Argument], _ctx: &Ctx<'_>) -> Expr {
    if args.is_empty() {
        parse_quote!(crate::__ds::DsFormData::new())
    } else {
        parse_quote!({
            ::core::panic!(
                "TypeError: FormData construct: the (form) arg is not statically lowered"
            )
        })
    }
}

/// A `FormData` instance method, dispatched on the receiver's resolved type.
/// Returns `None` for a non-`DsFormData` receiver or an unmapped name/arity, so
/// the call falls through to a plain method call (cargo check rejects it
/// honestly). `append`/`set` pick the `_str` or `_file` variant by inspecting
/// the value arg's resolved type (a `DsFile` local → file; else `ToString` →
/// string). The 3-arg `append(name, blob, filename)`/`set(…)` (a `Blob` + a
/// filename) is not statically lowered here (returns `None`).
pub(in crate::translator) fn form_data_method(
    sm: &StaticMemberExpression,
    args: &[Argument],
    ctx: &Ctx<'_>,
) -> Option<Expr> {
    if !is_form_data_local(&sm.object, ctx) {
        return None;
    }
    let name = sm.property.name.as_str();
    let obj = translate_expr(&sm.object, ctx);
    Some(match name {
        "has" if args.len() == 1 => {
            let k = es_to_string_arg(args.first()?, ctx);
            parse_quote!(#obj.has(#k))
        }
        "delete" if args.len() == 1 => {
            let k = es_to_string_arg(args.first()?, ctx);
            parse_quote!({
                #obj.delete(#k);
            })
        }
        "append" if args.len() == 2 => {
            let k = es_to_string_arg(args.first()?, ctx);
            if is_file_arg(args.get(1)?, ctx) {
                let v = translate_argument(args.get(1)?, ctx);
                parse_quote!({
                    #obj.append_file(#k, #v);
                })
            } else {
                let v = es_to_string_arg(args.get(1)?, ctx);
                parse_quote!({
                    #obj.append_str(#k, #v);
                })
            }
        }
        "set" if args.len() == 2 => {
            let k = es_to_string_arg(args.first()?, ctx);
            if is_file_arg(args.get(1)?, ctx) {
                let v = translate_argument(args.get(1)?, ctx);
                parse_quote!({
                    #obj.set_file(#k, #v);
                })
            } else {
                let v = es_to_string_arg(args.get(1)?, ctx);
                parse_quote!({
                    #obj.set_str(#k, #v);
                })
            }
        }
        _ => return None,
    })
}

/// Whether a `FormData` value argument is a `DsFile` local (→ the `_file`
/// variant). Mirrors the `Blob` parts identifier check: a bare identifier whose
/// resolved type's last segment is `DsFile`. A non-identifier (literal, member
/// access, call) is not statically known to be a `File`, so it falls through to
/// the `_str` ( ToString) variant.
fn is_file_arg(arg: &Argument, ctx: &Ctx<'_>) -> bool {
    let Some(Expression::Identifier(id)) = arg.as_expression() else {
        return false;
    };
    let name = snake(&id.name).to_string();
    ctx.local_type(&name)
        .is_some_and(|p| p.segments.last().is_some_and(|s| s.ident == "DsFile"))
}
