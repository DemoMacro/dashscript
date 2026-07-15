//! Type declarations (`interface` / `type`) → `syn` items.

use oxc_ast::ast::{TSInterfaceDeclaration, TSSignature, TSType, TSTypeAliasDeclaration};
use proc_macro2::TokenStream;
use quote::quote;
use syn::{parse_quote, Item, ItemStruct};

use super::{bindings, types};

/// `interface Point { x: number }` → `struct Point { pub x: f64 }`.
///
/// Fields are `pub`: a TS interface describes a value's public shape, so the
/// Rust struct exposes its fields to match.
pub fn translate_interface(iface: &TSInterfaceDeclaration) -> ItemStruct {
    let name = bindings::ident_of(&iface.id);
    let fields: Vec<TokenStream> = iface.body.body.iter().filter_map(struct_field).collect();
    parse_quote! { struct #name { #(#fields)* } }
}

/// `type Point = { x: number }` → `struct`; `type Id = number` → `type Id = f64;`.
pub fn translate_type_alias(alias: &TSTypeAliasDeclaration) -> Option<Item> {
    let name = bindings::ident_of(&alias.id);
    match &alias.type_annotation {
        TSType::TSTypeLiteral(lit) => {
            let fields: Vec<TokenStream> = lit.members.iter().filter_map(struct_field).collect();
            Some(Item::Struct(parse_quote! { struct #name { #(#fields)* } }))
        }
        other => {
            let ty = types::translate_type(other);
            Some(parse_quote!(type #name = #ty;))
        }
    }
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
