//! Array methods on a `Vec` of Copy elements.

use oxc_ast::ast::{Argument, Expression, StaticMemberExpression};
use syn::{parse_quote, Expr};

use super::super::super::bindings;
use super::super::super::context::Ctx;
use super::super::{arrow_expr, translate_argument};
use super::{str_method_arg, usize_arg};

/// Array methods on a `Vec` of Copy elements. The callback methods share a
/// `xs.iter().copied().<m>(f)` core: `.map`/`.filter` collect back into a
/// `Vec`; `.find`/`.some`/`.every`/`.reduce` return a scalar. A closure that
/// receives `&Item` (`filter`/`find`) destructures its param as `|&n|`; one that
/// receives the item by value (`map`/`some`/`every`/`reduce`) uses a plain
/// `|n|`. `.slice(a, b)` → `xs[a as usize..b as usize].to_vec()`; `.join(sep)`
/// stringifies each element first; `.indexOf`/`.includes` test membership.
/// Returns `None` for an unmapped name.
pub(in crate::translator::expressions) fn array_method(
    sm: &StaticMemberExpression,
    args: &[Argument],
    ctx: &Ctx<'_>,
) -> Option<Expr> {
    let Expression::Identifier(id) = &sm.object else {
        return None;
    };
    let recv = bindings::snake(&id.name);
    let path = ctx.local_type(&recv.to_string())?;
    let is_vec = path.segments.last().is_some_and(|s| s.ident == "Vec");
    if !is_vec {
        return None;
    }
    Some(match sm.property.name.as_str() {
        "map" => {
            let cb = translate_argument(args.first()?, ctx);
            parse_quote!(#recv.iter().copied().map(#cb).collect::<Vec<_>>())
        }
        // `.flatMap(cb)` → `flat_map` then collect; `cb` returns a `Vec` per
        // element (TS `flatMap` requires an array return), flattened into one.
        "flatMap" => {
            let cb = translate_argument(args.first()?, ctx);
            parse_quote!(#recv.iter().copied().flat_map(#cb).collect::<Vec<_>>())
        }
        "filter" => {
            let Argument::ArrowFunctionExpression(arrow) = args.first()? else {
                return None;
            };
            let cb = arrow_expr(arrow, ctx, true);
            parse_quote!(#recv.iter().copied().filter(#cb).collect::<Vec<_>>())
        }
        // `.ds` indices are `f64`; cast each bound to `usize`.
        "slice" => {
            let start = args.first().map(|a| usize_arg(a, ctx));
            let end = args.get(1).map(|a| usize_arg(a, ctx));
            match (start, end) {
                (Some(s), Some(e)) => parse_quote!(#recv[#s..#e].to_vec()),
                (Some(s), None) => parse_quote!(#recv[#s..].to_vec()),
                (None, _) => parse_quote!(#recv.clone()),
            }
        }
        // `.at(i)` → element at signed index `i` (TS allows negatives to count
        // from the end). `i` is `f64`; bind it once so a side-effecting index
        // expression evaluates only once, then branch on its sign.
        "at" => {
            let idx = translate_argument(args.first()?, ctx);
            parse_quote!({
                let __at_i = #idx;
                #recv[if __at_i >= 0.0 { __at_i as usize } else { (#recv.len() as f64 + __at_i) as usize }]
            })
        }
        // `.indexOf(x)` → first index of `x`, or -1 (TS returns a `number`).
        "indexOf" => {
            let needle = translate_argument(args.first()?, ctx);
            parse_quote!(
                #recv
                    .iter()
                    .copied()
                    .position(|y| y == #needle)
                    .map(|i| i as f64)
                    .unwrap_or(-1.0)
            )
        }
        // `.lastIndexOf(x)` → last index of `x`, or -1 (`rposition`).
        "lastIndexOf" => {
            let needle = translate_argument(args.first()?, ctx);
            parse_quote!(
                #recv
                    .iter()
                    .copied()
                    .rposition(|y| y == #needle)
                    .map(|i| i as f64)
                    .unwrap_or(-1.0)
            )
        }
        // `.findIndex(cb)` → first index where cb holds, or -1. `position` takes
        // the item by value (after `.copied()`), so the param is `|n|`.
        "findIndex" => {
            let Argument::ArrowFunctionExpression(arrow) = args.first()? else {
                return None;
            };
            let cb = arrow_expr(arrow, ctx, false);
            parse_quote!(
                #recv
                    .iter()
                    .copied()
                    .position(#cb)
                    .map(|i| i as f64)
                    .unwrap_or(-1.0)
            )
        }
        // `.includes(x)` → Vec::contains (by reference).
        "includes" => {
            let needle = translate_argument(args.first()?, ctx);
            parse_quote!(#recv.contains(&#needle))
        }
        // `.find(cb)` → first match as `Option<T>` (TS `T | undefined`); the
        // closure receives `&Item`, so its param destructures as `|&n|`.
        "find" => {
            let Argument::ArrowFunctionExpression(arrow) = args.first()? else {
                return None;
            };
            let cb = arrow_expr(arrow, ctx, true);
            parse_quote!(#recv.iter().copied().find(#cb))
        }
        // `.some(cb)` → `any` (true if any element matches); `any` takes the
        // item by value (after `.copied()`), so the param is a plain `|n|`.
        "some" => {
            let Argument::ArrowFunctionExpression(arrow) = args.first()? else {
                return None;
            };
            let cb = arrow_expr(arrow, ctx, false);
            parse_quote!(#recv.iter().copied().any(#cb))
        }
        // `.every(cb)` → `all` (true if all elements match); same value param.
        "every" => {
            let Argument::ArrowFunctionExpression(arrow) = args.first()? else {
                return None;
            };
            let cb = arrow_expr(arrow, ctx, false);
            parse_quote!(#recv.iter().copied().all(#cb))
        }
        // `.forEach(cb)` → `for_each` (side-effecting; returns `()`). The
        // callback takes the item by value (after `.copied()`), so `|n|`.
        "forEach" => {
            let Argument::ArrowFunctionExpression(arrow) = args.first()? else {
                return None;
            };
            let cb = arrow_expr(arrow, ctx, false);
            parse_quote!(#recv.iter().copied().for_each(#cb))
        }
        // `.join(sep)` → `Vec<String>.join(sep)` (each element stringified first).
        "join" => {
            let sep = str_method_arg(args.first()?, ctx);
            parse_quote!(#recv.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(#sep))
        }
        // `.reduce(cb, init)` → `fold`; `.reduce(cb)` (no seed) → `reduce`,
        // which yields `Option<T>` (an empty `.ds` array has no first element).
        "reduce" => {
            let Argument::ArrowFunctionExpression(arrow) = args.first()? else {
                return None;
            };
            let cb = arrow_expr(arrow, ctx, false);
            match args.get(1) {
                Some(init) => {
                    let init = translate_argument(init, ctx);
                    parse_quote!(#recv.iter().copied().fold(#init, #cb))
                }
                None => parse_quote!(#recv.iter().copied().reduce(#cb)),
            }
        }
        // `.findLast(cb)` → last match as `Option<T>` (reverse `find`); `find`
        // takes `&Item`, so the closure param destructures as `|&n|`.
        "findLast" => {
            let Argument::ArrowFunctionExpression(arrow) = args.first()? else {
                return None;
            };
            let cb = arrow_expr(arrow, ctx, true);
            parse_quote!(#recv.iter().copied().rev().find(#cb))
        }
        // `.findLastIndex(cb)` → last index where cb holds, or -1 (`rposition`
        // searches from the end and returns the original index).
        "findLastIndex" => {
            let Argument::ArrowFunctionExpression(arrow) = args.first()? else {
                return None;
            };
            let cb = arrow_expr(arrow, ctx, false);
            parse_quote!(
                #recv
                    .iter()
                    .copied()
                    .rposition(#cb)
                    .map(|i| i as f64)
                    .unwrap_or(-1.0)
            )
        }
        // `.reduceRight(cb, init)` → reverse `fold`; `.reduceRight(cb)` → reverse
        // `reduce` (yields `Option<T>`, as the no-seed form does).
        "reduceRight" => {
            let Argument::ArrowFunctionExpression(arrow) = args.first()? else {
                return None;
            };
            let cb = arrow_expr(arrow, ctx, false);
            match args.get(1) {
                Some(init) => {
                    let init = translate_argument(init, ctx);
                    parse_quote!(#recv.iter().copied().rev().fold(#init, #cb))
                }
                None => parse_quote!(#recv.iter().copied().rev().reduce(#cb)),
            }
        }
        // `.flat()` → flatten one level (`Vec<Vec<T>>::concat` → `Vec<T>`).
        // A depth argument is unsupported.
        "flat" if args.is_empty() => parse_quote!(#recv.concat()),
        // `.concat(ys, …)` → concatenate slices into a new `Vec`. Arguments
        // are assumed to be arrays; scalar concat args are unsupported.
        "concat" => {
            let parts: Vec<Expr> = args
                .iter()
                .map(|a| {
                    let e = translate_argument(a, ctx);
                    parse_quote!(#e.as_slice())
                })
                .collect();
            parse_quote!([#recv.as_slice(), #(#parts),*].concat())
        }
        // `.fill(v)` → in-place `Vec::fill` (every element set to `v`). Mutates;
        // a start/end range is unsupported.
        "fill" if args.len() == 1 => {
            let v = translate_argument(args.first()?, ctx);
            parse_quote!(#recv.fill(#v))
        }
        // `.reverse()` → in-place `Vec::reverse`. Mutates; needs a mutable
        // (`let`) array. TS returns the same reference — DashScript uses it
        // statement-style.
        "reverse" if args.is_empty() => parse_quote!(#recv.reverse()),
        // `.shift()` → drop the first element (TS returns it; statement-style,
        // matching push/pop). Panics on an empty Vec — TS yields `undefined`.
        "shift" if args.is_empty() => parse_quote!(#recv.remove(0)),
        // `.unshift(x)` → insert `x` at the front (TS returns the new length;
        // statement-style).
        "unshift" if args.len() == 1 => {
            let v = translate_argument(args.first()?, ctx);
            parse_quote!(#recv.insert(0, #v))
        }
        // `.sort()` → in-place numeric ascending sort (TS default sort is
        // lexicographic; DashScript treats number arrays numerically). A
        // comparator argument is unsupported — it would return `Ordering`.
        "sort" if args.is_empty() => {
            // `partial_cmp` is `None` for NaN; fall back to `Equal` so a NaN
            // element never panics (TS sort never throws on NaN).
            parse_quote!(#recv.sort_by(|a, b| a.partial_cmp(b).unwrap_or(::core::cmp::Ordering::Equal)))
        }
        // `.toSorted()` → copy + numeric sort (ES2023 immutable sort; no
        // comparator arg — like `sort`, a comparator would return Ordering).
        "toSorted" if args.is_empty() => parse_quote!({
            let mut __v = #recv.clone();
            __v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(::core::cmp::Ordering::Equal));
            __v
        }),
        // `.toReversed()` → reversed copy (ES2023 immutable reverse).
        "toReversed" if args.is_empty() => {
            parse_quote!(#recv.iter().copied().rev().collect::<Vec<_>>())
        }
        // `.toSpliced(start, deleteCount, …items)` → copy + splice (ES2023).
        // `Vec::splice` replaces the range with the item iterator; the bounds
        // are bound once so a side-effecting index arg evaluates only once.
        "toSpliced" if args.len() >= 2 => {
            let start = usize_arg(args.first()?, ctx);
            let del = usize_arg(args.get(1)?, ctx);
            let items: Vec<Expr> = args
                .iter()
                .skip(2)
                .map(|a| translate_argument(a, ctx))
                .collect();
            parse_quote!({
                let mut __v = #recv.clone();
                let __start = #start;
                let __del = #del;
                __v.splice(__start..(__start + __del), [#(#items),*]);
                __v
            })
        }
        // `.with(i, v)` → copy with element `i` replaced (ES2023).
        "with" if args.len() == 2 => {
            let i = usize_arg(args.first()?, ctx);
            let v = translate_argument(args.get(1)?, ctx);
            parse_quote!({
                let mut __v = #recv.clone();
                __v[#i] = #v;
                __v
            })
        }
        _ => return None,
    })
}
