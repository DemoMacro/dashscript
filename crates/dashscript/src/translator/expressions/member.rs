//! Member access: `p.x` field access, `m["k"]` HashMap / Vec index, `a?.x` chain.

use oxc_ast::ast::{ComputedMemberExpression, Expression, StaticMemberExpression};
use proc_macro2::Span;
use syn::{parse_quote, Expr, Ident};

use super::super::bindings;
use super::super::builtins;
use super::super::context::Ctx;
use super::super::types;
use super::is_hashset;
use super::translate_expr;
use super::{is_hashmap, is_vec_u8};

/// Optional chaining `a?.field` → `a.as_ref().map(|__c| __c.field)`. The
/// receiver is an `Option`; the access maps over a reference and yields
/// another `Option`. When the field is itself optional (`?:` → `Option<T>`),
/// `map` would nest (`Option<Option<T>>`) and a following `?? default`
/// (`unwrap_or_else`) would mistype — `and_then` flattens the nesting so
/// `opt?.field ?? d` lowers to `opt.and_then(|c| c.field.clone()).unwrap_or(d)`.
/// Only a single optional field access is handled; indexed access, optional
/// calls, and chained `a?.b?.c` fall back to `todo!()`.
pub(super) fn chain_expr(elem: &oxc_ast::ast::ChainElement, ctx: &Ctx<'_>) -> Expr {
    use oxc_ast::ast::ChainElement;
    match elem {
        ChainElement::StaticMemberExpression(sm) => {
            let obj = translate_expr(&sm.object, ctx);
            let field = bindings::snake(&sm.property.name);
            // `.length` on a Vec/String maps to Rust's `.len()` (a method, not a
            // field), mirroring the non-chain path; `len()` returns `usize` so
            // cast to `f64` (TS `.length` is always a `number`).
            if field == "length" {
                let c = Ident::new("__c", Span::call_site());
                return parse_quote!(#obj.as_ref().map(|#c| (#c.len() as f64)));
            }
            if receiver_field_is_optional(&sm.object, &field, ctx) {
                parse_quote!(#obj.as_ref().and_then(|__c| __c.#field.clone()))
            } else {
                parse_quote!(#obj.as_ref().map(|__c| __c.#field))
            }
        }
        _ => parse_quote!(::core::todo!()),
    }
}

/// Whether `obj.field` (snake-cased `field`) reads an optional (`?:`) field of
/// the struct inside `obj`'s `Option<…>` type — the case where `?.field` needs
/// `and_then` rather than `map` to avoid a nested `Option`. `obj` must be a
/// plain identifier whose recorded local type is `Option<Struct>`, and `Struct`
/// must register `field` as optional.
fn receiver_field_is_optional(obj: &Expression, field: &Ident, ctx: &Ctx<'_>) -> bool {
    let Expression::Identifier(id) = obj else {
        return false;
    };
    let name = bindings::snake(&id.name).to_string();
    let Some(ty) = ctx.local_type(&name) else {
        return false;
    };
    let Some(inner) = option_inner_last_ident(ty) else {
        return false;
    };
    ctx.struct_optionals(&inner)
        .is_some_and(|s| s.contains(field.to_string().as_str()))
}

/// Whether `obj.field` is a direct read of an optional (`?:`) field — `obj` is
/// a struct-typed local/param and `field` is registered optional for that
/// struct. Unlike [`receiver_field_is_optional`] (a field nested inside an
/// `Option<Struct>`, reached via `obj?.field`), here `obj` itself is the
/// struct, so `field`'s Rust type is `Option<T>`: a store wraps `Some(..)`, a
/// read yields `Option<T>`. Used by assignment (`obj.opt = v` →
/// `obj.opt = Some(v)`) and by detecting an RHS that already yields `Option<T>`.
pub(super) fn static_member_is_optional_field(
    obj: &Expression,
    field: &Ident,
    ctx: &Ctx<'_>,
) -> bool {
    let Expression::Identifier(id) = obj else {
        return false;
    };
    let obj_name = bindings::snake(&id.name).to_string();
    let Some(ty) = ctx.local_type(&obj_name) else {
        return false;
    };
    let Some(seg) = ty.segments.last() else {
        return false;
    };
    ctx.struct_optionals(&seg.ident.to_string())
        .is_some_and(|s| s.contains(field.to_string().as_str()))
}

/// The last path segment inside an `Option<…>` type path (`Option<X>` → `X`),
/// when the path is a single `Option` segment with one generic type argument.
fn option_inner_last_ident(path: &syn::Path) -> Option<String> {
    let seg = path.segments.last()?;
    if seg.ident != "Option" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(args) = &seg.arguments else {
        return None;
    };
    args.args.iter().find_map(|arg| match arg {
        syn::GenericArgument::Type(syn::Type::Path(tp)) => {
            tp.path.segments.last().map(|s| s.ident.to_string())
        }
        _ => None,
    })
}

/// `p.x` → field access. (A `console.log` callee is intercepted earlier.)
pub(super) fn member_expr(sm: &StaticMemberExpression, ctx: &Ctx<'_>) -> Expr {
    let field_name: &str = &sm.property.name;
    // `ns.foo` where `ns` is a namespace import (`import * as ns from "./m"`)
    // → the module path `ns::foo`, not a field access. A namespace alias is a
    // Rust `use m as ns;`, so its members are path segments. Checked first so no
    // later `.length`/`.size`/HashMap/Math arm intercepts a path prefix.
    if let Expression::Identifier(id) = &sm.object {
        if ctx.names().is_namespace(id) {
            let ns = ctx.names().of_reference(id);
            let field = bindings::snake(field_name);
            return parse_quote!(#ns::#field);
        }
    }
    // `Color.Red` where `Color` is a TS enum → `Color::Red`, a path constant
    // in the enum's module (an ES enum lowers to `mod Color { pub const Red:
    // i64 = 0; … }`). Read right after the namespace-import arm and before the
    // HashMap/struct arms so an enum member access does not fall through to a
    // struct field. The member keeps its TS spelling (`Red`), matching the
    // const name `translate_enum` emitted.
    if let Expression::Identifier(id) = &sm.object {
        let obj_name = id.name.to_string();
        if ctx.registry().enums.contains(&obj_name) {
            let ns = bindings::type_ident(&obj_name);
            let member = bindings::type_ident(field_name);
            return parse_quote!(#ns::#member);
        }
    }
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
    // `/pat/gi.flags` / `.source` / `.global` / `.ignoreCase` / … on a regex
    // literal → the static property (known at translate time). Only a literal
    // receiver; a regex local has lost its source flags.
    if let Expression::RegExpLiteral(re) = &sm.object {
        if let Some(e) = super::regex_literal_property(re, field_name) {
            return e;
        }
    }
    // `d.year`/`d.month`/`d.day`/… on a `Temporal.PlainDate` local → the matching
    // `temporal_rs::PlainDate` accessor method (Rust accessors are methods, not
    // fields; ES Temporal calendar fields are properties). Numeric fields cast to
    // `f64` (a `.ts` `number` is `f64`); `inLeapYear` is a bool, no cast.
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
    // `tags.a` on a `Record`/HashMap local → `tags.get("a").<copied|cloned>().unwrap()`
    // (a TS `Record` static field access and `m["a"]` are the same lookup). A
    // `Copy` value (f64/bool) copies out of the borrow; a non-`Copy` value (a
    // union enum carrying a String) clones — `.copied()`'s `T: Copy` bound
    // would otherwise fail.
    if is_hashmap_local(&sm.object, ctx) {
        let obj = translate_expr(&sm.object, ctx);
        let key = syn::LitStr::new(field_name, Span::call_site());
        let access = hashmap_access_method(&sm.object, ctx);
        return parse_quote!(#obj.get(#key).#access().unwrap());
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
    // An `Option` local (non-narrowed) read by field — the author asserted
    // non-null (a TS `if (opt)` guard, or an optional-parameter promise), so the
    // receiver lowers to `name.as_ref().unwrap()` and the field sees the inner
    // value. `Option` has no public fields, so an Option-typed receiver at a
    // field access is always an inner-value read.
    let obj =
        option_unwrap_object(&sm.object, ctx).unwrap_or_else(|| translate_expr(&sm.object, ctx));
    // `.length` on a Vec/String maps to Rust's `.len()` (a method, not a field).
    // TS `.length` is always a `number` → `f64`; `len()` returns `usize`, so cast.
    // Index/repeat sites that need `usize` cast the whole expression again.
    if field_name == "length" {
        // `.length` on an optional Vec field (`parent.elements.length`) — the
        // field is `Option<Vec<..>>`, and `Option::len()` is private (nightly);
        // map the inner `Vec`'s length, defaulting to `0` when `None` (the ES
        // code path that reaches here has already asserted the field non-null).
        if let Expression::StaticMemberExpression(inner) = &sm.object {
            let inner_field = bindings::snake(&inner.property.name);
            if static_member_is_optional_field(&inner.object, &inner_field, ctx) {
                let inner_obj = translate_expr(&inner.object, ctx);
                return parse_quote!(
                    (#inner_obj.#inner_field.as_ref().map(|__c| __c.len() as f64).unwrap_or(0_f64))
                );
            }
        }
        return parse_quote!((#obj.len() as f64));
    }
    let field = bindings::snake(field_name);
    parse_quote!(#obj.#field)
}

/// If `e` is a bare identifier bound to an `Option<T>` local that is not
/// narrowed to its inner value, emit `name.as_ref().unwrap()` — the author
/// asserted non-null (a TS `if (opt)` guard or an optional-parameter promise),
/// so a field or method access on the receiver reads the inner value. `None`
/// for any other shape (so a plain receiver is translated normally).
pub(in crate::translator) fn option_unwrap_object(e: &Expression, ctx: &Ctx<'_>) -> Option<Expr> {
    let Expression::Identifier(id) = e else {
        return None;
    };
    let name = bindings::snake(&id.name).to_string();
    if !ctx.is_option(&name) || ctx.is_narrowed_some(&name) {
        return None;
    }
    let ident = Ident::new(&name, Span::call_site());
    Some(parse_quote!(#ident.as_ref().unwrap()))
}

/// `arr[i]` → `arr[i as usize]`; `m["k"]` on a `HashMap` →
/// `m.get("k").copied().unwrap()`. A `.ts` index is `f64`; Rust indexes by
/// `usize`, so the Vec/array index is cast. A HashMap key is looked up with
/// `.get` (typed: the key is assumed present, so `unwrap` panics if absent —
/// matching the non-optional type).
pub(super) fn computed_member(cm: &ComputedMemberExpression, ctx: &Ctx<'_>) -> Expr {
    let obj = translate_expr(&cm.object, ctx);
    if is_hashmap_local(&cm.object, ctx) {
        let key = index_key(&cm.expression, ctx);
        let access = hashmap_access_method(&cm.object, ctx);
        return parse_quote!(#obj.get(#key).#access().unwrap());
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
pub(in crate::translator) fn is_option_ds_match(path: &syn::Path) -> bool {
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

/// Whether the value type of a `HashMap<K, V>` is `Copy` (so a `.get` yields
/// `Option<&V>` that can be `.copied()` rather than `.cloned()`). A non-
/// `HashMap` or a non-`Copy` `V` (e.g. an inline scalar-union enum, which
/// carries a `String`) returns `false`, selecting `.cloned()`.
fn hashmap_value_is_copy(path: &syn::Path) -> bool {
    let Some(seg) = path.segments.last() else {
        return false;
    };
    if seg.ident != "HashMap" {
        return false;
    }
    let syn::PathArguments::AngleBracketed(args) = &seg.arguments else {
        return false;
    };
    let Some(syn::GenericArgument::Type(v)) = args.args.iter().nth(1) else {
        return false;
    };
    types::type_path(v).is_some_and(types::is_copy_path)
}

/// `copied` when `expr` is a `HashMap` local whose value is `Copy`, else
/// `cloned` — the method name for `Option<&V>` → `Option<V>` after a `.get`.
fn hashmap_access_method(expr: &Expression, ctx: &Ctx<'_>) -> syn::Ident {
    let copy = match expr {
        Expression::Identifier(id) => {
            let name = bindings::snake(&id.name).to_string();
            ctx.local_type(&name).is_some_and(hashmap_value_is_copy)
        }
        _ => false,
    };
    syn::Ident::new(if copy { "copied" } else { "cloned" }, Span::call_site())
}

/// True when `expr` is a local whose type is a `HashMap`.
pub(in crate::translator) fn is_hashmap_local(expr: &Expression, ctx: &Ctx<'_>) -> bool {
    let Expression::Identifier(id) = expr else {
        return false;
    };
    let name = bindings::snake(&id.name).to_string();
    if ctx.local_type(&name).is_some_and(is_hashmap) {
        return true;
    }
    // An imported (or same-module aliased) lazy static whose `OnceLock` cell
    // holds a `HashMap` — `m["k"]` lowers to `m().get(k)`. The cell type comes
    // from the cross-file lazy-static export table, since a `use`-imported
    // accessor fn has no entry in this file's `local_type`.
    super::super::imports::lazy_static_export_type(&name)
        .map(|ty| matches!(ty, syn::Type::Path(ref tp) if is_hashmap(&tp.path)))
        .unwrap_or(false)
}

/// True when `expr` is a local whose type is a `HashSet` (an ES `Set`).
pub(in crate::translator) fn is_hashset_local(expr: &Expression, ctx: &Ctx<'_>) -> bool {
    let Expression::Identifier(id) = expr else {
        return false;
    };
    let name = bindings::snake(&id.name).to_string();
    ctx.local_type(&name).is_some_and(is_hashset)
}

/// True when `expr` is a local whose type is `Vec<u8>` (a `Uint8Array` byte
/// buffer).
pub(in crate::translator) fn is_vec_u8_local(expr: &Expression, ctx: &Ctx<'_>) -> bool {
    let Expression::Identifier(id) = expr else {
        return false;
    };
    let name = bindings::snake(&id.name).to_string();
    ctx.local_type(&name).is_some_and(is_vec_u8)
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
