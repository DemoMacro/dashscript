//! `Object.<m>(record)` static methods on a `Record` (a `HashMap`). Mirrors
//! `test/built-ins/Object/`.

use oxc_ast::ast::Argument;
use syn::{parse_quote, Expr};

use super::super::context::Ctx;
use super::super::expressions::{is_number_arg, translate_argument};
use super::str_method_arg;

/// `Object.<m>(record)` on a `Record` (a `HashMap`): `keys` → the map's keys
/// as `Vec<String>`, `values` → its values (cloned, so Copy and Clone both
/// work), `entries` → `(K, V)` pairs. `is`/`hasOwn`/`getOwnPropertyNames`/
/// `assign`/`fromEntries` round out the static set DashScript maps on a
/// `Record`. Returns `None` for any other member.
pub(in crate::translator) fn object_method(
    name: &str,
    args: &[Argument],
    ctx: &Ctx<'_>,
) -> Option<Expr> {
    let r = translate_argument(args.first()?, ctx);
    Some(match name {
        "keys" => parse_quote!(#r.keys().map(|k| k.to_string()).collect::<Vec<String>>()),
        "values" => parse_quote!(#r.values().cloned().collect::<Vec<_>>()),
        "entries" => {
            parse_quote!(#r.iter().map(|(k, v)| (k.clone(), v.clone())).collect::<Vec<_>>())
        }
        // `Object.is(a, b)` → value identity: equal, or both NaN (TS `Object.is`
        // treats `NaN === NaN`, unlike `===`). The NaN arm is emitted only when
        // both operands are numeric — `.is_nan()` exists solely on `f64`, so a
        // blanket emit fails to compile for `Object.is(true, false)` /
        // `Object.is("a", "b")` / Record operands. `+0`/`-0` differ in TS but
        // not under Rust `==` — that edge is not honored.
        "is" if args.len() == 2 => {
            let a = args.first()?;
            let b_arg = args.get(1)?;
            let b = translate_argument(b_arg, ctx);
            if is_number_arg(a, ctx) && is_number_arg(b_arg, ctx) {
                parse_quote!((#r == #b) || (#r.is_nan() && #b.is_nan()))
            } else {
                parse_quote!(#r == #b)
            }
        }
        // `Object.hasOwn(m, key)` → `HashMap::contains_key` (a Record owns its
        // keys). `key` is a `&str` (a literal stays a bare pattern).
        "hasOwn" if args.len() == 2 => {
            let k = str_method_arg(args.get(1)?, ctx);
            parse_quote!(#r.contains_key(#k))
        }
        // `Object.getOwnPropertyNames(m)` ≡ `Object.keys(m)` for a Record (a
        // HashMap's keys are its own string property names).
        "getOwnPropertyNames" => {
            parse_quote!(#r.keys().map(|k| k.to_string()).collect::<Vec<String>>())
        }
        // `Object.assign(target, …srcs)` → a cloned target with each source
        // merged in (Record = HashMap, so `extend` merges by key).
        "assign" => {
            let srcs: Vec<Expr> = args
                .iter()
                .skip(1)
                .map(|a| translate_argument(a, ctx))
                .collect();
            parse_quote!({
                let mut __m = #r.clone();
                #(__m.extend(#srcs.clone());)*
                __m
            })
        }
        // `Object.fromEntries(entries)` → collect `(K, V)` pairs into a HashMap.
        "fromEntries" => {
            parse_quote!(#r.into_iter().collect::<::std::collections::HashMap<String, f64>>())
        }
        // `Object.freeze`/`seal`/`preventExtensions` are no-ops returning the
        // value unchanged — Rust has no runtime immutability to enforce, and a
        // DashScript `Record` is already as strict as it gets at compile time.
        // `.clone()` because the value is owned (`Record` is not `Copy`): a
        // bare `#r` would move it, breaking `Object.freeze(m); …m…`.
        "freeze" | "seal" | "preventExtensions" => parse_quote!(#r.clone()),
        // `Object.isFrozen`/`isSealed` → `false`: DashScript never freezes a
        // Record, so it is always mutable. `isExtensible` → `true` (likewise).
        "isFrozen" | "isSealed" => parse_quote!(false),
        "isExtensible" => parse_quote!(true),
        _ => return None,
    })
}
