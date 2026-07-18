//! Global conversion functions called as plain identifiers (`String(x)`,
//! `parseInt(s)`, `Number(s)`, `Boolean(x)`, `isNaN`/`isFinite`). These are ES
//! globals — not a `Math`/`Object` member — so they live here rather than under
//! a named built-in file.

use oxc_ast::ast::{Argument, IdentifierReference};
use syn::{parse_quote, Expr};

use super::super::bindings;
use super::super::context::Ctx;
use super::super::expressions::{bool_expr, string_expr, translate_argument};

/// Global conversion functions called as plain identifiers: `String(x)` →
/// `format!("{}", x)`; `parseInt(s)`/`parseFloat(s)` → `s.trim().parse::<f64>()`
/// (`.ds` `number` is `f64`, so both share one parse path). Returns `None` for
/// any other name (falls through to a plain call).
pub(in crate::translator) fn global_function(
    id: &IdentifierReference,
    args: &[Argument],
    ctx: &Ctx<'_>,
) -> Option<Expr> {
    let name: &str = &id.name;
    Some(match name {
        "String" => {
            let a = translate_argument(args.first()?, ctx);
            parse_quote!(::std::format!("{}", #a))
        }
        // A malformed string yields NaN in TS, never a throw — `unwrap_or`
        // matches that without a runtime panic.
        "parseFloat" => {
            let a = translate_argument(args.first()?, ctx);
            parse_quote!(#a.trim().parse::<f64>().unwrap_or(f64::NAN))
        }
        // `parseInt(s)` → base-10 parse; `parseInt(s, radix)` →
        // `i64::from_str_radix` (an out-of-range radix yields NaN, as in TS).
        // This does not honor a `0x` prefix the way TS auto-detection does.
        "parseInt" => {
            let a = translate_argument(args.first()?, ctx);
            match args.get(1) {
                Some(radix) => {
                    let r = translate_argument(radix, ctx);
                    parse_quote!(
                        i64::from_str_radix(#a.trim(), #r as u32)
                            .map(|x| x as f64)
                            .unwrap_or(f64::NAN)
                    )
                }
                None => parse_quote!(#a.trim().parse::<f64>().unwrap_or(f64::NAN)),
            }
        }
        // `Number(s)` parses a string; `Number(n)` passes a number through.
        // ToNumber coercion: an empty or whitespace-only string is `0` (not
        // NaN — `Number("")` / `Number("  ")` are both `0`); anything else
        // parses as `f64`, NaN on a malformed string (a throw is never raised).
        "Number" => {
            let a = args.first()?;
            let e = translate_argument(a, ctx);
            if matches!(a, Argument::StringLiteral(_)) || ident_string_local(a, ctx) {
                parse_quote!({
                    // Bind the (possibly temporary) string first: `#e` may be
                    // `"x".to_string()`, and `.trim()` would borrow a value
                    // freed at the end of that expression (E0716).
                    let __s = #e;
                    let __t = __s.trim();
                    if __t.is_empty() {
                        0_f64
                    } else {
                        __t.parse::<f64>().unwrap_or(f64::NAN)
                    }
                })
            } else {
                e
            }
        }
        // `Boolean(x)` → the Rust truthiness of `x` (see `bool_cast`).
        "Boolean" => bool_cast(args.first()?, ctx),
        // `isNaN(x)` → `x.is_nan()` (DashScript's `number` is `f64`, so the TS
        // global's ToNumber coercion is already done).
        "isNaN" => {
            let a = translate_argument(args.first()?, ctx);
            parse_quote!(#a.is_nan())
        }
        // `isFinite(x)` → `x.is_finite()`.
        "isFinite" => {
            let a = translate_argument(args.first()?, ctx);
            parse_quote!(#a.is_finite())
        }
        _ => return None,
    })
}

/// True when `arg` is an identifier bound to a `string` local.
fn ident_string_local(arg: &Argument, ctx: &Ctx<'_>) -> bool {
    let Argument::Identifier(id) = arg else {
        return false;
    };
    let name = bindings::snake(&id.name).to_string();
    ctx.local_type(&name).is_some_and(|p| p.is_ident("String"))
}

/// `Boolean(x)` → the Rust truthiness of `x`. A literal folds at compile time
/// when possible: a number (`0`/`NaN` → `false`, else `true`), a string
/// (`!is_empty()`), `true`/`false` to itself. An identifier dispatches on its
/// known type: a `Vec`/`HashMap`/`String` → `!is_empty()`, an `Option` →
/// `is_some()`, a `bool` → itself, anything else (an `f64`) → `!= 0_f64`. An
/// expression of unknown type falls back to `!= 0_f64` (TS `Boolean` is most
/// often applied to numbers).
fn bool_cast(arg: &Argument, ctx: &Ctx<'_>) -> Expr {
    match arg {
        Argument::BooleanLiteral(b) => bool_expr(b.value),
        Argument::NumericLiteral(n) => bool_expr(n.value != 0_f64 && !n.value.is_nan()),
        Argument::StringLiteral(s) => {
            let e = string_expr(s);
            parse_quote!(!#e.is_empty())
        }
        Argument::Identifier(id) => {
            let name = bindings::snake(&id.name);
            let last = ctx
                .local_type(&name.to_string())
                .and_then(|p| p.segments.last())
                .map(|s| s.ident.to_string());
            match last.as_deref() {
                Some("Vec") | Some("HashMap") | Some("String") => parse_quote!(!#name.is_empty()),
                Some("Option") => parse_quote!(#name.is_some()),
                Some("bool") => parse_quote!(#name),
                _ => parse_quote!(#name != 0_f64),
            }
        }
        _ => {
            let e = translate_argument(arg, ctx);
            parse_quote!(#e != 0_f64)
        }
    }
}
