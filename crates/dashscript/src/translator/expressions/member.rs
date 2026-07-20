//! Member access: `p.x` field access, `m["k"]` HashMap / Vec index, `a?.x` chain.

use oxc_ast::ast::{ComputedMemberExpression, Expression, StaticMemberExpression};
use proc_macro2::Span;
use syn::{parse_quote, Expr};

use super::super::bindings;
use super::super::builtins;
use super::super::context::Ctx;
use super::super::types;
use super::is_hashmap;
use super::is_hashset;
use super::translate_expr;

/// Optional chaining `a?.field` → `a.as_ref().map(|__c| __c.field)`. The
/// receiver is an `Option`; the access maps over a reference and yields
/// another `Option`. Only a single optional field access is handled; indexed
/// access, optional calls, and chained `a?.b?.c` fall back to `todo!()`.
pub(super) fn chain_expr(elem: &oxc_ast::ast::ChainElement, ctx: &Ctx<'_>) -> Expr {
    use oxc_ast::ast::ChainElement;
    match elem {
        ChainElement::StaticMemberExpression(sm) => {
            let obj = translate_expr(&sm.object, ctx);
            let field = bindings::snake(&sm.property.name);
            parse_quote!(#obj.as_ref().map(|__c| __c.#field))
        }
        _ => parse_quote!(::core::todo!()),
    }
}

/// `p.x` → field access. (A `console.log` callee is intercepted earlier.)
pub(super) fn member_expr(sm: &StaticMemberExpression, ctx: &Ctx<'_>) -> Expr {
    let field_name: &str = &sm.property.name;
    // `m.size` on a Map/Set (HashMap/HashSet) → `.len()` — a property, not a
    // key lookup. Checked before the `is_hashmap_local` arm below, which would
    // otherwise lower it to `m.get("size")`. A user struct with a `size` field
    // is unaffected (its receiver is not a HashMap/HashSet local).
    if field_name == "size"
        && (is_hashmap_local(&sm.object, ctx) || is_hashset_local(&sm.object, ctx))
    {
        let obj = translate_expr(&sm.object, ctx);
        return parse_quote!((#obj.len() as f64));
    }
    // `m.index`/`m.input`/`m.length` on a `let m = s.match(/pat/)` result → the
    // `DsMatch` fields. Checked before the generic `.length` arm, which would
    // try `Option<DsMatch>::len()` (no such method). `index`/`length` are ES
    // numbers (cast to `f64`); `input` is the haystack string. `groups` is
    // left to a later phase.
    if is_match_local(&sm.object, ctx) {
        let obj = translate_expr(&sm.object, ctx);
        match field_name {
            "index" => return parse_quote!(#obj.as_ref().unwrap().index as f64),
            "input" => return parse_quote!(#obj.as_ref().unwrap().input.clone()),
            "length" => return parse_quote!(#obj.as_ref().unwrap().captures.len() as f64),
            _ => {}
        }
    }
    // `d.year`/`d.month`/`d.day`/… on a `Temporal.PlainDate` local → the matching
    // `temporal_rs::PlainDate` accessor method (Rust accessors are methods, not
    // fields; ES Temporal calendar fields are properties). Numeric fields cast to
    // `f64` (a `.ds` `number` is `f64`); `inLeapYear` is a bool, no cast.
    if is_plain_date_local(&sm.object, ctx) {
        if let Some(m) = plain_date_method(field_name) {
            let method = syn::Ident::new(m, Span::call_site());
            let obj = translate_expr(&sm.object, ctx);
            return if field_name == "inLeapYear" {
                parse_quote!(#obj.#method())
            } else {
                parse_quote!((#obj.#method() as f64))
            };
        }
    }
    // `tags.a` on a `Record`/HashMap local → `tags.get("a").copied().unwrap()`
    // (a TS `Record` static field access and `m["a"]` are the same lookup).
    if is_hashmap_local(&sm.object, ctx) {
        let obj = translate_expr(&sm.object, ctx);
        let key = syn::LitStr::new(field_name, Span::call_site());
        return parse_quote!(#obj.get(#key).copied().unwrap());
    }
    // `Math.PI` / `Math.E` → the corresponding Rust constant.
    if builtins::is_ident(&sm.object, "Math") {
        if let Some(p) = builtins::math_constant(field_name) {
            return p;
        }
    }
    // `Number.EPSILON` / `Number.MAX_VALUE` / `Number.NaN` / … → the matching
    // `f64` constant.
    if builtins::is_ident(&sm.object, "Number") {
        if let Some(p) = builtins::number_constant(field_name) {
            return p;
        }
    }
    // Inside a discriminated-union match arm, `s.field` reads as the `field`
    // binding the pattern destructured (TS narrowing).
    if let Expression::Identifier(id) = &sm.object {
        let scrut = bindings::snake(&id.name);
        let field = bindings::snake(field_name);
        if ctx.narrow_binds(&scrut.to_string(), &field.to_string()) {
            return parse_quote!(#field);
        }
    }
    let obj = translate_expr(&sm.object, ctx);
    // `.length` on a Vec/String maps to Rust's `.len()` (a method, not a field).
    // TS `.length` is always a `number` → `f64`; `len()` returns `usize`, so cast.
    // Index/repeat sites that need `usize` cast the whole expression again.
    if field_name == "length" {
        return parse_quote!((#obj.len() as f64));
    }
    let field = bindings::snake(field_name);
    parse_quote!(#obj.#field)
}

/// `arr[i]` → `arr[i as usize]`; `m["k"]` on a `HashMap` →
/// `m.get("k").copied().unwrap()`. A `.ds` index is `f64`; Rust indexes by
/// `usize`, so the Vec/array index is cast. A HashMap key is looked up with
/// `.get` (typed: the key is assumed present, so `unwrap` panics if absent —
/// matching the non-optional type).
pub(super) fn computed_member(cm: &ComputedMemberExpression, ctx: &Ctx<'_>) -> Expr {
    let obj = translate_expr(&cm.object, ctx);
    if is_hashmap_local(&cm.object, ctx) {
        let key = index_key(&cm.expression, ctx);
        return parse_quote!(#obj.get(#key).copied().unwrap());
    }
    // `m[i]` on a `let m = s.match(/pat/)` result → `captures[i]` (the whole
    // match at 0, the capture groups at 1..). Out-of-range or a non-
    // participating group yields the string `"undefined"` (ES `undefined`, but
    // `console.log` renders both identically). `as_ref` borrows so `m` survives
    // repeat reads.
    if is_match_local(&cm.object, ctx) {
        let idx = index_expr(&cm.expression, ctx);
        return parse_quote!(
            #obj.as_ref().unwrap().captures.get(#idx).cloned().flatten()
                .unwrap_or_else(|| "undefined".to_string())
        );
    }
    let idx = index_expr(&cm.expression, ctx);
    // `s[i]` on a string → the i-th char. Rust's `str` has no `Index<usize>`,
    // so a string index lowers to `chars().nth(i)` (the char as a `String`, or
    // "" if out of range — TS returns undefined). ASCII matches; non-BMP
    // UTF-16 vs Rust `char` diverge (a lone surrogate can't occur in UTF-8).
    if is_string_receiver(&cm.object, ctx) {
        return parse_quote!(#obj.chars().nth(#idx).map(|c| c.to_string()).unwrap_or_default());
    }
    let indexed = parse_quote!(#obj[#idx]);
    // `let x = arr[i]` moves the element out of `arr`; if `arr` is read again
    // later (use count > 1) and the element is not `Copy`, clone it so those
    // reads still see a value. A scalar element copies on index — no clone.
    if index_needs_clone(&cm.object, ctx) {
        parse_quote!(#indexed.clone())
    } else {
        indexed
    }
}

/// A `usize`-typed index for `arr[expr]`. A bitwise index (`i & mask`) emits
/// its masked result directly to `usize`, skipping the `f64` round-trip the
/// number model adds between `bitwise_expr` and the index cast. That hop both
/// costs a conversion per access and — worse — obscures the `& mask` range
/// from LLVM, defeating bounds-check elision on the `Vec` index (V8 elides it;
/// the `f64` intermediate was the gap). Any other index (counter, arithmetic)
/// translates normally and casts to `usize`.
fn index_expr(expr: &Expression, ctx: &Ctx<'_>) -> Expr {
    if let Expression::BinaryExpression(bin) = expr {
        if let Some(int_idx) = super::binary::bitwise_expr_to(bin, ctx, parse_quote!(usize)) {
            return int_idx;
        }
    }
    let e = translate_expr(expr, ctx);
    Expr::Cast(syn::ExprCast {
        attrs: Vec::new(),
        expr: Box::new(e),
        as_token: syn::Token![as](Span::call_site()),
        ty: Box::new(parse_quote!(usize)),
    })
}

/// Whether `expr` is a string receiver for `s[i]` indexing: a string literal
/// or a local whose type is `String`/`str`. Rust's `str` cannot be indexed by
/// `usize`, so such an index lowers to `chars().nth(i)`.
fn is_string_receiver(expr: &Expression, ctx: &Ctx<'_>) -> bool {
    if matches!(expr, Expression::StringLiteral(_)) {
        return true;
    }
    let Expression::Identifier(id) = expr else {
        return false;
    };
    let name = bindings::snake(&id.name).to_string();
    ctx.local_type(&name).is_some_and(|ty| {
        ty.segments
            .last()
            .is_some_and(|s| s.ident == "String" || s.ident == "str")
    })
}

/// Whether `expr` is a `let m = s.match(/pat/)` result: a local whose recorded
/// type is `Option<DsMatch>` (the last path segment is `DsMatch`). Lets
/// `m[0]`/`m.index`/`m.input`/`m.length` route to the `DsMatch` accessors
/// instead of failing on `Option`'s missing `Index`/`len`.
fn is_match_local(expr: &Expression, ctx: &Ctx<'_>) -> bool {
    let Expression::Identifier(id) = expr else {
        return false;
    };
    let name = bindings::snake(&id.name).to_string();
    ctx.local_type(&name).is_some_and(is_option_ds_match)
}

/// Whether `expr` is a `Temporal.PlainDate` local (a
/// `let d = Temporal.PlainDate.from(…)` result), so `d.year`/`d.month`/…
/// route to `temporal_rs::PlainDate`'s accessor methods instead of failing on a
/// missing struct field (E0609 — PlainDate's slots are private).
fn is_plain_date_local(expr: &Expression, ctx: &Ctx<'_>) -> bool {
    let Expression::Identifier(id) = expr else {
        return false;
    };
    let name = bindings::snake(&id.name).to_string();
    ctx.local_type(&name)
        .is_some_and(|ty| ty.segments.last().is_some_and(|s| s.ident == "PlainDate"))
}

/// The `temporal_rs::PlainDate` accessor method for an ES Temporal calendar
/// field, if any (`dayOfYear` → `day_of_year`, …). Returns `None` for a field
/// that is not a calendar accessor (e.g. `calendar`, or a user-added field).
fn plain_date_method(name: &str) -> Option<&'static str> {
    match name {
        "year" => Some("year"),
        "month" => Some("month"),
        "day" => Some("day"),
        "dayOfWeek" => Some("day_of_week"),
        "dayOfYear" => Some("day_of_year"),
        "daysInWeek" => Some("days_in_week"),
        "daysInMonth" => Some("days_in_month"),
        "daysInYear" => Some("days_in_year"),
        "monthsInYear" => Some("months_in_year"),
        "inLeapYear" => Some("in_leap_year"),
        _ => None,
    }
}

/// Whether a recorded type path is `Option<…DsMatch>` (a `.match` result). The
/// last segment is `Option`; `DsMatch` sits in its generic argument, so a plain
/// last-segment check (like `is_string_receiver`) misses it.
fn is_option_ds_match(path: &syn::Path) -> bool {
    let Some(seg) = path.segments.last() else {
        return false;
    };
    if seg.ident != "Option" {
        return false;
    }
    let syn::PathArguments::AngleBracketed(args) = &seg.arguments else {
        return false;
    };
    args.args.iter().any(|arg| match arg {
        syn::GenericArgument::Type(syn::Type::Path(tp)) => tp
            .path
            .segments
            .last()
            .is_some_and(|s| s.ident == "DsMatch"),
        _ => false,
    })
}

/// Whether indexing `expr` (a `Vec` local) into a binding needs `.clone()`:
/// the local is read more than once (a move would break later reads), and the
/// element is not `Copy` (or its type is unknown — clone to be safe). A scalar
/// element copies on index, so no clone.
fn index_needs_clone(expr: &Expression, ctx: &Ctx<'_>) -> bool {
    let Expression::Identifier(id) = expr else {
        return false;
    };
    let name = bindings::snake(&id.name).to_string();
    if ctx.use_count(&name) <= 1 {
        return false;
    }
    match ctx.local_type(&name) {
        None => true,
        Some(ty) => !element_is_copy(ty),
    }
}

/// Whether the element type of a `Vec<T>` is `Copy` (so indexing copies rather
/// than moves). A non-`Vec` or non-generic type is treated as non-`Copy`.
fn element_is_copy(path: &syn::Path) -> bool {
    let Some(seg) = path.segments.last() else {
        return false;
    };
    if seg.ident != "Vec" {
        return false;
    }
    let syn::PathArguments::AngleBracketed(args) = &seg.arguments else {
        return false;
    };
    let Some(syn::GenericArgument::Type(elem)) = args.args.first() else {
        return false;
    };
    types::type_path(elem).is_some_and(types::is_copy_path)
}

/// True when `expr` is a local whose type is a `HashMap`.
pub(in crate::translator) fn is_hashmap_local(expr: &Expression, ctx: &Ctx<'_>) -> bool {
    let Expression::Identifier(id) = expr else {
        return false;
    };
    let name = bindings::snake(&id.name).to_string();
    ctx.local_type(&name).is_some_and(is_hashmap)
}

/// True when `expr` is a local whose type is a `HashSet` (an ES `Set`).
pub(in crate::translator) fn is_hashset_local(expr: &Expression, ctx: &Ctx<'_>) -> bool {
    let Expression::Identifier(id) = expr else {
        return false;
    };
    let name = bindings::snake(&id.name).to_string();
    ctx.local_type(&name).is_some_and(is_hashset)
}

/// A HashMap key: a string literal stays bare (a `&str` for `HashMap::get`);
/// any other expression gets `.as_str()`.
fn index_key(expr: &Expression, ctx: &Ctx<'_>) -> Expr {
    if let Expression::StringLiteral(s) = expr {
        let lit = syn::LitStr::new(s.value.as_str(), Span::call_site());
        return parse_quote!(#lit);
    }
    let e = translate_expr(expr, ctx);
    parse_quote!(#e.as_str())
}
