//! Type declarations (`interface` / `type`) → `syn` items.

use oxc_ast::ast::{
    TSLiteral, TSInterfaceDeclaration, TSSignature, TSType, TSTypeAliasDeclaration, TSTypeName,
    TSUnionType,
};
use proc_macro2::TokenStream;
use quote::quote;
use syn::{parse_quote, Ident, Item, ItemEnum, ItemStruct};

use super::{bindings, types};

/// `interface Point { x: number }` → `struct Point { pub x: f64 }`.
///
/// Fields are `pub`: a TS interface describes a value's public shape, so the
/// Rust struct exposes its fields to match.
pub fn translate_interface(iface: &TSInterfaceDeclaration) -> ItemStruct {
    let name: &str = &iface.id.name;
    let name = bindings::type_ident(name);
    let fields: Vec<TokenStream> = iface.body.body.iter().filter_map(struct_field).collect();
    parse_quote! { struct #name { #(#fields)* } }
}

/// `type Point = { x: number }` → `struct`; `type Id = number` → `type Id = f64;`.
pub fn translate_type_alias(alias: &TSTypeAliasDeclaration) -> Option<Item> {
    let name: &str = &alias.id.name;
    let name = bindings::type_ident(name);
    match &alias.type_annotation {
        TSType::TSTypeLiteral(lit) => {
            let fields: Vec<TokenStream> = lit.members.iter().filter_map(struct_field).collect();
            Some(Item::Struct(parse_quote! { struct #name { #(#fields)* } }))
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
/// (`"red" | "green"` → `Red, Green`), type references yield tuple variants
/// (`Circle | Square` → `Circle(Circle), Square(Square)`). Mixed unions or
/// those with other members fall back to a type alias.
fn union_to_enum(name: &Ident, u: &TSUnionType) -> Option<ItemEnum> {
    let str_variants: Vec<Ident> = u.types.iter().filter_map(string_literal_variant).collect();
    if str_variants.len() == u.types.len() {
        return Some(parse_quote! { enum #name { #(#str_variants),* } });
    }
    let ref_variants: Vec<syn::Variant> = u.types.iter().filter_map(type_ref_variant).collect();
    if ref_variants.len() == u.types.len() {
        return Some(parse_quote! { enum #name { #(#ref_variants),* } });
    }
    None
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
    Some(quote!(pub #key: #ty,))
}
