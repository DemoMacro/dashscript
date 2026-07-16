//! Method-call mappings for `.ds` expressions, grouped by receiver kind:
//! `array` (`Vec` callbacks), `string`, `number`, plus shared helpers for
//! name-only renames, global conversion functions, `console`, and truthiness.

mod array;
mod number;
mod string;

pub(super) use array::array_method;
pub(super) use number::number_method;
pub(super) use string::string_method;

use oxc_ast::ast::{Argument, Expression, IdentifierReference};
use proc_macro2::Span;
use quote::format_ident;
use syn::{parse_quote, Expr, Ident};

use super::super::bindings;
use super::super::context::Ctx;
use super::{bool_expr, string_expr, translate_argument};

/// A `.ds` `number` argument cast to `usize` (e.g. for `repeat`).
fn usize_arg(arg: &Argument, ctx: &Ctx<'_>) -> Expr {
    let e = translate_argument(arg, ctx);
    parse_quote!(#e as usize)
}

/// A string-method argument as a `&str`: a string literal stays a bare literal
/// (a perfect `Pattern`); any other expression (a `String` var or call) gets
/// `.as_str()` so it satisfies Rust's `&str`-typed string APIs.
fn str_method_arg(arg: &Argument, ctx: &Ctx<'_>) -> Expr {
    if let Argument::StringLiteral(s) = arg {
        let lit = syn::LitStr::new(s.value.as_str(), Span::call_site());
        return parse_quote!(#lit);
    }
    let e = translate_argument(arg, ctx);
    parse_quote!(#e.as_str())
}

/// A handful of TS method names map to a different Rust method name; the
/// receiver and arguments are passed through unchanged. Unmapped methods fall
/// through to a plain call on the receiver expression.
pub(super) fn map_method(name: &str) -> Option<Ident> {
    let mapped = match name {
        "toUpperCase" => "to_uppercase",
        "toLowerCase" => "to_lowercase",
        "trim" => "trim",
        "trimStart" => "trim_start",
        "trimEnd" => "trim_end",
        "push" => "push",
        "pop" => "pop",
        // `.toString()` → `.to_string()` (Rust's `Display`). A numeric receiver
        // with a radix (`(255).toString(16)`) is handled in `number_method`.
        "toString" => "to_string",
        _ => return None,
    };
    Some(format_ident!("{}", mapped))
}

/// `Object.<m>(record)` on a `Record` (a `HashMap`): `keys` → the map's keys
/// as `Vec<String>`, `values` → its values (cloned, so Copy and Clone both
/// work), `entries` → `(K, V)` pairs. Returns `None` for any other member.
pub(super) fn object_method(name: &str, args: &[Argument], ctx: &Ctx<'_>) -> Option<Expr> {
    let r = translate_argument(args.first()?, ctx);
    Some(match name {
        "keys" => parse_quote!(#r.keys().map(|k| k.to_string()).collect::<Vec<_>>()),
        "values" => parse_quote!(#r.values().cloned().collect::<Vec<_>>()),
        "entries" => {
            parse_quote!(#r.iter().map(|(k, v)| (k.clone(), v.clone())).collect::<Vec<_>>())
        }
        _ => return None,
    })
}

/// `String.<m>(…)`: `fromCharCode(n)` → a one-char `String` from the code
/// point (or `""` if `n` isn't a valid `char`). Returns `None` otherwise.
pub(super) fn string_static(name: &str, args: &[Argument], ctx: &Ctx<'_>) -> Option<Expr> {
    let n = translate_argument(args.first()?, ctx);
    Some(match name {
        "fromCharCode" => {
            parse_quote!(char::from_u32(#n as u32).map(|c| c.to_string()).unwrap_or_default())
        }
        _ => return None,
    })
}

/// Global conversion functions called as plain identifiers: `String(x)` →
/// `format!("{}", x)`; `parseInt(s)`/`parseFloat(s)` → `s.trim().parse::<f64>()`
/// (`.ds` `number` is `f64`, so both share one parse path). Returns `None` for
/// any other name (falls through to a plain call).
pub(super) fn global_function(
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
        "Number" => {
            let a = args.first()?;
            let e = translate_argument(a, ctx);
            if matches!(a, Argument::StringLiteral(_)) || ident_string_local(a, ctx) {
                parse_quote!(#e.trim().parse::<f64>().unwrap_or(f64::NAN))
            } else {
                e
            }
        }
        // `Boolean(x)` → the Rust truthiness of `x` (see `bool_cast`).
        "Boolean" => bool_cast(args.first()?, ctx),
        _ => return None,
    })
}

/// True when `arg` is an identifier bound to a `string` local.
fn ident_string_local(arg: &Argument, ctx: &Ctx<'_>) -> bool {
    let Argument::Identifier(id) = arg else { return false };
    let name = bindings::snake(&id.name).to_string();
    ctx.local_type(&name).is_some_and(|p| p.is_ident("String"))
}

/// `Boolean(x)` → the Rust truthiness of `x`. A literal folds at compile time
/// when possible: a number (`0`/`NaN` → `false`, else `true`), a string
/// (`!is_empty()`), `true`/`false` to itself. An identifier dispatches on its
/// known type: a `Vec`/`HashMap`/`String` → `!is_empty()`, an `Option` →
/// `is_some()`, a `bool` → itself, anything else (an `f64`) → `!= 0.0`. An
/// expression of unknown type falls back to `!= 0.0` (TS `Boolean` is most
/// often applied to numbers).
fn bool_cast(arg: &Argument, ctx: &Ctx<'_>) -> Expr {
    match arg {
        Argument::BooleanLiteral(b) => bool_expr(b.value),
        Argument::NumericLiteral(n) => bool_expr(n.value != 0.0 && !n.value.is_nan()),
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
                _ => parse_quote!(#name != 0.0),
            }
        }
        _ => {
            let e = translate_argument(arg, ctx);
            parse_quote!(#e != 0.0)
        }
    }
}

/// A truthiness test for the block-local `__l`, picking the check by the
/// original left operand's type — used by `||`/`&&` value semantics. Mirrors
/// `bool_cast` but references `__l` rather than re-evaluating the operand.
pub(super) fn truthy_cond(left: &Expression, ctx: &Ctx<'_>) -> Expr {
    let l: Ident = format_ident!("__l");
    match left {
        Expression::StringLiteral(_) => parse_quote!(!#l.is_empty()),
        Expression::Identifier(id) => {
            let name = bindings::snake(&id.name).to_string();
            let last = ctx
                .local_type(&name)
                .and_then(|p| p.segments.last())
                .map(|s| s.ident.to_string());
            match last.as_deref() {
                Some("Vec") | Some("HashMap") | Some("String") => parse_quote!(!#l.is_empty()),
                Some("Option") => parse_quote!(#l.is_some()),
                Some("bool") => parse_quote!(#l),
                _ => parse_quote!(#l != 0.0),
            }
        }
        Expression::NumericLiteral(n) => bool_expr(n.value != 0.0 && !n.value.is_nan()),
        Expression::BooleanLiteral(b) => bool_expr(b.value),
        _ => parse_quote!(#l != 0.0),
    }
}

/// True when `expr` is a `bool` operand (a `BooleanLiteral`, or a local
/// annotated `boolean`) — those short-circuit as Rust `&&`/`||`.
pub(super) fn expr_is_bool(expr: &Expression, ctx: &Ctx<'_>) -> bool {
    match expr {
        Expression::BooleanLiteral(_) => true,
        Expression::Identifier(id) => {
            let name = bindings::snake(&id.name).to_string();
            ctx.local_type(&name)
                .and_then(|p| p.segments.last())
                .is_some_and(|s| s.ident == "bool")
        }
        _ => false,
    }
}

/// The Rust macro for a `console.<m>(…)` call: `log` → `println!`, `warn`/
/// `error` → `eprintln!`. Returns `None` for any other member.
pub(super) fn console_method(callee: &Expression) -> Option<Ident> {
    let Expression::StaticMemberExpression(member) = callee else {
        return None;
    };
    if !is_ident(&member.object, "console") {
        return None;
    }
    let name = match member.property.name.as_str() {
        "log" => "println",
        "warn" | "error" => "eprintln",
        _ => return None,
    };
    Some(format_ident!("{}", name))
}

/// True when `expr` is an `Identifier` whose name equals `expected`.
pub(super) fn is_ident(expr: &Expression, expected: &str) -> bool {
    let Expression::Identifier(ident) = expr else {
        return false;
    };
    let name: &str = &ident.name;
    name == expected
}
