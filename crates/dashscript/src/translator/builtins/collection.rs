//! ES `Map` / `Set` instance methods on a DashScript `HashMap` / `HashSet`.
//!
//! `Map<K, V>` → `HashMap<K, V>` and `Set<T>` → `HashSet<T>` (see `types`);
//! these methods dispatch on the receiver's resolved type. A `Map`'s insertion
//! order is not preserved (`HashMap` is unordered — an `IndexMap` would keep
//! it, a later dep). `m.get(k)` returns `Option<V>` (ES `V | undefined`),
//! matching DashScript's nullable→`Option` mapping — so a `console.log` of it
//! prints `Some(…)`/`None`, not the ES `…`/`undefined` (a general nullable
//! display limit, not Map-specific).
//!
//! A `Set<number>` / `Map<number, _>` lowers to `HashSet<DsF64Key>` /
//! `HashMap<DsF64Key, V>` — `f64` lacks `Eq`/`Hash`, so each key wraps in
//! `DsF64Key` (SameValueZero). The `f64key` flag below threads that wrap from
//! the receiver's resolved type into each `insert`/`contains`/`get`/`remove`.

use oxc_ast::ast::{Argument, StaticMemberExpression};
use syn::{parse_quote, Expr};

use super::super::context::Ctx;
use super::super::expressions::{
    hashmap_uses_f64_key, hashset_uses_f64_key, is_hashmap_local, is_hashset_local, is_number_arg,
    translate_argument, translate_expr,
};

/// Wrap a key/value expression in `DsF64Key(…)` when the receiver is a
/// number-keyed collection (a `Set<number>`/`Map<number, _>`).
fn keyed(f64key: bool, e: Expr) -> Expr {
    if f64key {
        parse_quote!(crate::__ds::DsF64Key(#e))
    } else {
        e
    }
}

/// A `Map` / `Set` instance method, dispatched on the receiver's type. Returns
/// `None` for a non-collection receiver or an unmapped name (falls through to a
/// plain call → `cargo check` rejects it honestly).
pub(in crate::translator) fn collection_method(
    sm: &StaticMemberExpression,
    args: &[Argument],
    ctx: &Ctx<'_>,
) -> Option<Expr> {
    let name = sm.property.name.as_str();
    let obj = translate_expr(&sm.object, ctx);
    if is_hashmap_local(&sm.object, ctx) {
        // `f64` lacks `Eq`/`Hash`: an annotated `Map<number, _>` wraps each key
        // in `DsF64Key` (detected via the receiver type), and an unannotated
        // `new Map()` whose first key argument is a number wraps too — the
        // inferred `HashMap<K, V>` would otherwise fail to compile.
        let f64key = hashmap_uses_f64_key(&sm.object, ctx)
            || args.first().is_some_and(|a| is_number_arg(a, ctx));
        Some(match name {
            // `m.set(k, v)` → `m.insert(k, v)`. ES returns the map for chaining;
            // the insert's `Option<V>` is dropped (chaining is not yet mapped),
            // so the call lowers to a statement block.
            "set" => {
                let k = keyed(f64key, translate_argument(args.first()?, ctx));
                let v = translate_argument(args.get(1)?, ctx);
                parse_quote!({ #obj.insert(#k, #v); })
            }
            // `m.get(k)` → `Option<V>` (ES returns `V | undefined`).
            "get" => {
                let k = keyed(f64key, translate_argument(args.first()?, ctx));
                parse_quote!(#obj.get(&#k).cloned())
            }
            "has" => {
                let k = keyed(f64key, translate_argument(args.first()?, ctx));
                parse_quote!(#obj.contains_key(&#k))
            }
            // `m.delete(k)` → `bool` (whether a value was removed).
            "delete" => {
                let k = keyed(f64key, translate_argument(args.first()?, ctx));
                parse_quote!(#obj.remove(&#k).is_some())
            }
            "clear" if args.is_empty() => parse_quote!(#obj.clear()),
            _ => return None,
        })
    } else if is_hashset_local(&sm.object, ctx) {
        // As above: an annotated `Set<number>` wraps via the receiver type, and
        // an unannotated `new Set()` whose first element is a number wraps too.
        let f64key = hashset_uses_f64_key(&sm.object, ctx)
            || args.first().is_some_and(|a| is_number_arg(a, ctx));
        Some(match name {
            // `s.add(v)` → `s.insert(v)` (statement; ES chaining unmapped).
            "add" => {
                let v = keyed(f64key, translate_argument(args.first()?, ctx));
                parse_quote!({ #obj.insert(#v); })
            }
            "has" => {
                let v = keyed(f64key, translate_argument(args.first()?, ctx));
                parse_quote!(#obj.contains(&#v))
            }
            // `s.delete(v)` → `bool` (`HashSet::remove` returns bool directly).
            "delete" => {
                let v = keyed(f64key, translate_argument(args.first()?, ctx));
                parse_quote!(#obj.remove(&#v))
            }
            "clear" if args.is_empty() => parse_quote!(#obj.clear()),
            _ => return None,
        })
    } else {
        None
    }
}
