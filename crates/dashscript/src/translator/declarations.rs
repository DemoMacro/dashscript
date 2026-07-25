//! Type declarations (`interface` / `type`) → `syn` items.

use oxc_ast::ast::{
    TSInterfaceDeclaration, TSLiteral, TSSignature, TSType, TSTypeAliasDeclaration, TSTypeName,
    TSUnionType,
};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{parse_quote, Arm, Ident, Item, ItemEnum, ItemImpl, ItemStruct};

use super::{bindings, types};

/// `interface Point { x: number }` → `struct Point { pub x: f64 }`.
///
/// Fields are `pub`: a TS interface describes a value's public shape, so the
/// Rust struct exposes its fields to match.
pub fn translate_interface(iface: &TSInterfaceDeclaration) -> ItemStruct {
    let name: &str = &iface.id.name;
    let name = bindings::type_ident(name);
    let fields: Vec<TokenStream> = iface.body.body.iter().filter_map(struct_field).collect();
    parse_quote! { #[derive(Clone)] struct #name { #(#fields)* } }
}

/// `type Point = { x: number }` → `struct`; `type Id = number` → `type Id = f64;`.
pub fn translate_type_alias(alias: &TSTypeAliasDeclaration) -> Option<Item> {
    let name: &str = &alias.id.name;
    let name = bindings::type_ident(name);
    match &alias.type_annotation {
        TSType::TSTypeLiteral(lit) => {
            let fields: Vec<TokenStream> = lit.members.iter().filter_map(struct_field).collect();
            Some(Item::Struct(
                parse_quote! { #[derive(Clone)] struct #name { #(#fields)* } },
            ))
        }
        TSType::TSUnionType(u) => {
            // A union of string literals or named types becomes an `enum`;
            // anything else falls back to a type alias.
            if let Some(item) = union_to_enum(&name, u) {
                return Some(Item::Enum(item));
            }
            let ty = types::translate_type(&alias.type_annotation);
            Some(parse_quote!(type #name = #ty;))
        }
        other => {
            let ty = types::translate_type(other);
            Some(parse_quote!(type #name = #ty;))
        }
    }
}

/// A union becomes an `enum`: string literals yield unit variants
/// (`"red" | "green"` → `Red, Green`), scalar keywords yield tuple variants
/// (`string | number | undefined` → `Str(String), Num(f64), Undef`), type
/// references yield tuple variants (`Circle | Square` → `Circle(Circle),
/// Square(Square)`), and object literals carrying a string-literal discriminant
/// yield named-field variants (`{ kind: "circle"; radius: number }` →
/// `Circle { radius: f64 }`). Each branch requires *every* member to map the
/// same way; a mixed union falls back to a type alias so a half-built enum
/// never reaches `cargo check`.
fn union_to_enum(name: &Ident, u: &TSUnionType) -> Option<ItemEnum> {
    let str_variants: Vec<Ident> = u.types.iter().filter_map(string_literal_variant).collect();
    if str_variants.len() == u.types.len() {
        return Some(parse_quote! { #[derive(Clone)] enum #name { #(#str_variants),* } });
    }
    let scalar_variants: Vec<syn::Variant> = u.types.iter().filter_map(scalar_variant).collect();
    if scalar_variants.len() == u.types.len() {
        return Some(parse_quote! { #[derive(Clone)] enum #name { #(#scalar_variants),* } });
    }
    let ref_variants: Vec<syn::Variant> = u.types.iter().filter_map(type_ref_variant).collect();
    if ref_variants.len() == u.types.len() {
        return Some(parse_quote! { #[derive(Clone)] enum #name { #(#ref_variants),* } });
    }
    let field_variants: Vec<syn::Variant> =
        u.types.iter().filter_map(discriminated_variant).collect();
    if !field_variants.is_empty() && field_variants.len() == u.types.len() {
        return Some(parse_quote! { #[derive(Clone)] enum #name { #(#field_variants),* } });
    }
    None
}

/// `string` / `number` / `boolean` → `Str(String)` / `Num(f64)` / `Bool(bool)` —
/// tuple variants wrapping the scalar Rust type. `undefined` / `null` → `Undef`
/// / `Null` unit variants (no value to carry). The `string | number | boolean |
/// undefined` shape is the typical XML-attribute / JSON-value union.
fn scalar_variant(ty: &TSType) -> Option<syn::Variant> {
    match ty {
        TSType::TSStringKeyword(_) => Some(parse_quote!(Str(String))),
        TSType::TSNumberKeyword(_) => Some(parse_quote!(Num(f64))),
        TSType::TSBooleanKeyword(_) => Some(parse_quote!(Bool(bool))),
        TSType::TSUndefinedKeyword(_) => Some(parse_quote!(Undef)),
        TSType::TSNullKeyword(_) => Some(parse_quote!(Null)),
        _ => None,
    }
}

/// For an all-scalar-keyword union, the `(enum name, sorted variants)` — so
/// the same union spelled in any member order (`string | number` vs `number |
/// string`) yields one shared enum. The name is `__DsUnion` + the sorted
/// member tags (e.g. `__DsUnionNumStrUndef`); the variants are sorted to
/// match, so two references to the same shape cannot produce two enum defs
/// with different variant orders. `None` when any member is not a scalar
/// keyword, so a mixed union still falls back. The single source of truth for
/// an inline union's enum name — `types::union_type` calls this to name the
/// type, and the registry pre-pass calls it to emit the definition.
pub fn scalar_union_enum(u: &TSUnionType) -> Option<(Ident, Vec<syn::Variant>)> {
    let mut tagged: Vec<(String, syn::Variant)> = u
        .types
        .iter()
        .map(|t| Some((scalar_tag(t)?.to_string(), scalar_variant(t)?)))
        .collect::<Option<_>>()?;
    tagged.sort_by(|a, b| a.0.cmp(&b.0));
    tagged.dedup_by(|a, b| a.0 == b.0);
    let stem: String = tagged.iter().map(|(t, _)| t.as_str()).collect();
    let name = format_ident!("__DsUnion{stem}");
    let variants = tagged.into_iter().map(|(_, v)| v).collect();
    Some((name, variants))
}

/// `impl Display for __DsUnion…` so a union value renders the way ES
/// `String(v)` would when interpolated into a `format!`/template literal:
/// `Str` → its inner string, `Num` → the number's string form, `Bool` →
/// `"true"`/`"false"`, `Undef` → `"undefined"`, `Null` → `"null"`. The arm
/// list mirrors the enum's variants 1:1 (variant tag names come from
/// [`scalar_variant`]), so the `match self` stays exhaustive regardless of
/// which scalar members the union spans. (`Num` uses Rust's `f64` `Display`,
/// which matches ES for the integer and simple-decimal values typical of an
/// XML attribute; the `-0` / `1e+21` edge cases diverge — a later ES-precise
/// number-to-string helper closes that gap.)
pub fn union_display_impl(item: &ItemEnum) -> ItemImpl {
    let path = &item.ident;
    let arms: Vec<Arm> = item.variants.iter().map(display_arm).collect();
    parse_quote! {
        impl ::std::fmt::Display for #path {
            fn fmt(&self, __f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                match self {
                    #(#arms)*
                }
            }
        }
    }
}

/// One `match` arm of [`union_display_impl`], keyed by the variant's tag name
/// (`Str`/`Num`/`Bool`/`Undef`/`Null`). An unexpected tag (a non-scalar variant
/// that should not occur in a `__DsUnion`) renders via `unreachable!` so the
/// impl still compiles.
fn display_arm(v: &syn::Variant) -> Arm {
    let id = &v.ident;
    match v.ident.to_string().as_str() {
        "Str" => parse_quote!(Self::Str(__s) => __f.write_str(__s.as_str()),),
        "Num" => parse_quote!(Self::Num(__n) => ::std::write!(__f, "{}", __n),),
        "Bool" => parse_quote!(Self::Bool(__b) => ::std::write!(__f, "{}", __b),),
        "Undef" => parse_quote!(Self::Undef => __f.write_str("undefined"),),
        "Null" => parse_quote!(Self::Null => __f.write_str("null"),),
        _ => parse_quote!(Self::#id => ::core::unreachable!(),),
    }
}

/// The stable tag for a scalar keyword member (`string` → `Str`), used to name
/// and order a scalar union's enum. Non-scalar members return `None`.
fn scalar_tag(t: &TSType) -> Option<&'static str> {
    match t {
        TSType::TSStringKeyword(_) => Some("Str"),
        TSType::TSNumberKeyword(_) => Some("Num"),
        TSType::TSBooleanKeyword(_) => Some("Bool"),
        TSType::TSUndefinedKeyword(_) => Some("Undef"),
        TSType::TSNullKeyword(_) => Some("Null"),
        _ => None,
    }
}

/// `"red"` → `Red` (a unit variant).
fn string_literal_variant(ty: &TSType) -> Option<Ident> {
    let TSType::TSLiteralType(lit) = ty else {
        return None;
    };
    let TSLiteral::StringLiteral(s) = &lit.literal else {
        return None;
    };
    let value: &str = &s.value;
    Some(bindings::pascal(value))
}

/// `Circle` → `Circle(Circle)` — a tuple variant wrapping the named type.
fn type_ref_variant(ty: &TSType) -> Option<syn::Variant> {
    let TSType::TSTypeReference(r) = ty else {
        return None;
    };
    let TSTypeName::IdentifierReference(id) = &r.type_name else {
        return None;
    };
    let name: &str = &id.name;
    let variant = bindings::type_ident(name);
    Some(parse_quote!(#variant(#variant)))
}

/// `{ kind: "circle"; radius: number }` → `Circle { radius: f64 }` — a
/// named-field variant of a discriminated union. The property whose *type* is
/// a string literal is the discriminant: its value names the variant and is not
/// emitted as a field. The remaining properties become the variant's named
/// fields. Returns `None` when the literal has no string-literal discriminant.
fn discriminated_variant(ty: &TSType) -> Option<syn::Variant> {
    let TSType::TSTypeLiteral(lit) = ty else {
        return None;
    };
    let mut variant_name: Option<Ident> = None;
    let mut fields: Vec<TokenStream> = Vec::new();
    for sig in &lit.members {
        let TSSignature::TSPropertySignature(ps) = sig else {
            continue;
        };
        let Some(key) = bindings::property_key_name(&ps.key) else {
            continue;
        };
        let Some(ta) = ps.type_annotation.as_ref() else {
            continue;
        };
        // A string-literal-typed property is the discriminant → variant name.
        if let TSType::TSLiteralType(lt) = &ta.type_annotation {
            if let TSLiteral::StringLiteral(s) = &lt.literal {
                variant_name = Some(bindings::pascal(&s.value));
                continue;
            }
        }
        let field_ty = types::translate_type(&ta.type_annotation);
        fields.push(quote!(#key: #field_ty));
    }
    let variant = variant_name?;
    Some(parse_quote!(#variant { #(#fields),* }))
}

/// One struct field from a property signature: `pub name: Type,`.
fn struct_field(sig: &TSSignature) -> Option<TokenStream> {
    let TSSignature::TSPropertySignature(ps) = sig else {
        return None;
    };
    let key = bindings::property_key_name(&ps.key)?;
    let ty = ps
        .type_annotation
        .as_ref()
        .map(|ta| types::translate_type(&ta.type_annotation))
        .unwrap_or_else(|| parse_quote!(_));
    // An optional (`?:`) field wraps in `Option<T>`.
    let ty = if ps.optional {
        quote!(Option<#ty>)
    } else {
        quote!(#ty)
    };
    Some(quote!(pub #key: #ty,))
}
