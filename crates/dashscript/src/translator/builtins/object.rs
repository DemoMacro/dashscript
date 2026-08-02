//! `Object.<m>(record)` static methods on a `Record` (a `HashMap`). Mirrors
//! `test/built-ins/Object/`.

use oxc_ast::ast::Argument;
use syn::{parse_quote, Expr};

use super::super::context::Ctx;
use super::super::expressions::{is_number_arg, translate_argument, translate_number_to};
use super::super::flavor::NumberFlavor;
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
        // The `+0`/`-0` edge (`Object.is(0, -0) === false`) is honored via a
        // sign check — Rust `==` treats `+0 == -0`, so an explicit
        // `is_sign_negative` comparison distinguishes them.
        "is" if args.len() == 2 => {
            let a = args.first()?;
            let b_arg = args.get(1)?;
            if is_number_arg(a, ctx) && is_number_arg(b_arg, ctx) {
                // Re-translate both operands at `f64` so `.is_nan()` compiles
                // for a flavor-promoted `i64` argument (the function-level `r`
                // may be `i64`; `i64 → f64` is exact below 2^53).
                let r = translate_number_to(a.as_expression()?, NumberFlavor::F64, ctx);
                let b = translate_number_to(b_arg.as_expression()?, NumberFlavor::F64, ctx);
                parse_quote!((#r == #b && (#r != 0.0 || #r.is_sign_negative() == #b.is_sign_negative())) || (#r.is_nan() && #b.is_nan()))
            } else {
                let b = translate_argument(b_arg, ctx);
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
        // `Object.freeze`/`seal`/`preventExtensions`/`isFrozen`/`isSealed`/
        // `isExtensible` are intercepted by `classify_call`, which degrades the
        // enclosing function to the engine — a `Record` carries no runtime
        // [[Extensible]]/attribute flag, so a static no-op (`freeze` → clone)
        // or hardcoded (`isExtensible` → true) emit would mis-report a
        // freeze-then-query fixture. They never reach this arm.
        _ => return None,
    })
}
