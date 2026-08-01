//! Type declarations (`interface` / `type`) → `syn` items.

mod enum_decl;
mod interface;
mod type_alias;
mod union_enum;

pub use enum_decl::translate_enum;
pub use interface::translate_interface;
pub use type_alias::translate_type_alias;
pub use union_enum::{inline_mixed_union_enum, scalar_union_enum, union_display_impl};

pub(in crate::translator) use enum_decl::eval_enum_members;
pub(in crate::translator) use interface::index_signature_type;
pub(in crate::translator) use union_enum::{
    anon_struct_for_literal, discriminant_property_value, discriminant_value, object_member_tag,
    set_shape_aliases,
};

use syn::{parse_quote, Item, Meta, Type};

/// Append `serde::Serialize`/`serde::Deserialize` to every emitted struct/enum's
/// `#[derive(...)]`. Per-function engine degradation marshals argument/return
/// values through `serde_json::Value`, so every user type crossing the
/// `call_fn` boundary needs both. Idempotent (skips a derive list already
/// carrying a serde derive) and ignores items with no `derive(...)` attribute.
pub(in crate::translator) fn add_serde_derives(items: &mut [Item]) {
    for item in items.iter_mut() {
        // A struct/enum carrying a function-pointer field (a TS callback like
        // `attribute_value_fn?: (...) => string`) still derives serde: each
        // `fn(...)` field is marked `#[serde(skip)]`, so it never serializes
        // (its `Default` — `None` for `Option<fn(...)>` — substitutes). A
        // callback cannot meaningfully cross the `call_fn` marshal boundary
        // anyway (QuickJS cannot marshal a Rust fn pointer), and the field
        // stays usable in static code (skip is serde-only).
        match item {
            Item::Struct(s) => {
                for f in s.fields.iter_mut() {
                    if type_contains_fn(&f.ty) {
                        f.attrs.push(parse_quote!(#[serde(skip)]));
                    }
                }
            }
            Item::Enum(e) => {
                for v in e.variants.iter_mut() {
                    for f in v.fields.iter_mut() {
                        if type_contains_fn(&f.ty) {
                            f.attrs.push(parse_quote!(#[serde(skip)]));
                        }
                    }
                }
            }
            _ => continue,
        }
        let attrs = match item {
            Item::Struct(s) => &mut s.attrs,
            Item::Enum(e) => &mut e.attrs,
            _ => continue,
        };
        for attr in attrs.iter_mut() {
            // The existing `#[derive(Clone, Debug, PartialEq)]` token stream.
            let prev = match &attr.meta {
                Meta::List(ml) if attr.path().is_ident("derive") => ml.tokens.clone(),
                _ => continue,
            };
            if prev.to_string().contains("Serialize") {
                continue;
            }
            *attr = parse_quote!(#[derive(#prev, serde::Serialize, serde::Deserialize)]);
        }
    }
}

/// True where a type mention contains a function pointer (`fn(...) -> ...`),
/// recursing through the wrappers a TS callback lowers into — `Option<fn(...)>`,
/// `Vec<fn(...)>`, `&fn(...)`, tuples, slices, arrays. Used to skip serde
/// derives on a struct/enum that carries a callback field.
fn type_contains_fn(ty: &Type) -> bool {
    match ty {
        Type::BareFn(_) => true,
        Type::Paren(p) => type_contains_fn(&p.elem),
        Type::Reference(r) => type_contains_fn(&r.elem),
        Type::Slice(s) => type_contains_fn(&s.elem),
        Type::Array(a) => type_contains_fn(&a.elem),
        Type::Tuple(t) => t.elems.iter().any(type_contains_fn),
        Type::Path(p) => p.path.segments.iter().any(|seg| match &seg.arguments {
            syn::PathArguments::AngleBracketed(ab) => ab.args.iter().any(|arg| match arg {
                syn::GenericArgument::Type(t) => type_contains_fn(t),
                _ => false,
            }),
            _ => false,
        }),
        _ => false,
    }
}
