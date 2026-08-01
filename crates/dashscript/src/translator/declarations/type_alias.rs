use oxc_ast::ast::{TSType, TSTypeAliasDeclaration, TSTypeParameterDeclaration};
use proc_macro2::TokenStream;
use quote::quote;
use syn::{parse_quote, Ident, Item, ItemStruct, Type};

use super::super::{bindings, types};

/// `type Point = { x: number }` → `struct`; `type Id = number` → `type Id = f64;`.
/// Returns a `Vec<Item>`: an inline-object body may introduce anonymous
/// helper structs (see [`translate_interface`]), emitted before the alias.
pub fn translate_type_alias(alias: &TSTypeAliasDeclaration) -> Vec<Item> {
    let name: &str = &alias.id.name;
    let name = bindings::type_ident(name);
    let generics = type_param_idents(&alias.type_parameters);
    match &alias.type_annotation {
        TSType::TSTypeLiteral(lit) => {
            let parent = name.to_string();
            let mut anon = Vec::new();
            let fields: Vec<TokenStream> = lit
                .members
                .iter()
                .filter_map(|sig| super::interface::struct_field(sig, &parent, &mut anon))
                .collect();
            let mut items = anon;
            // A struct a crate-root union enum references (its alias was
            // upgraded to from an inline member) must be crate-visible, or the
            // enum cannot name it across modules (E0425).
            let vis: TokenStream =
                if super::union_enum::alias_referenced_by_union(&name.to_string()) {
                    quote!(pub(crate))
                } else {
                    quote!()
                };
            let mut item: ItemStruct = parse_quote! { #[derive(Clone, Debug, PartialEq)] #vis struct #name { #(#fields)* } };
            item.generics = make_generics(&generics);
            items.push(Item::Struct(item));
            items
        }
        TSType::TSUnionType(u) => {
            // A union lowers to an `enum`; the mixed path may also yield
            // helper structs (inline-object members' `__DsAnon_<hash>`), which
            // must be emitted before the enum references them.
            if let Some((mut item, anons)) = super::union_enum::union_to_enum(&name, u) {
                item.generics = make_generics(&generics);
                let mut items: Vec<Item> = anons.into_iter().map(Item::Struct).collect();
                items.push(Item::Enum(item));
                return items;
            }
            let ty = types::translate_type_for_data(&alias.type_annotation);
            vec![alias_item(&name, &generics, &ty)]
        }
        other => {
            let ty = types::translate_type_for_data(other);
            vec![alias_item(&name, &generics, &ty)]
        }
    }
}

/// The Rust type-parameter idents of a TS `<T, U>` parameter list — reused by
/// generic type aliases (and, later, interfaces). Constraints/defaults are
/// dropped (no `where` clause); Rust monomorphizes each call site.
fn type_param_idents(
    tp: &Option<oxc_allocator::Box<'_, TSTypeParameterDeclaration>>,
) -> Vec<Ident> {
    tp.as_deref().map_or_else(Vec::new, |tp| {
        tp.params
            .iter()
            .map(|p| bindings::type_ident(&p.name.name))
            .collect()
    })
}

/// A `syn::Generics` from a list of param idents — empty for a non-generic
/// item, `<T, U>` otherwise. Built by parsing `<…>` so the `<`/`>` tokens and
/// the param list come out correct without hand-building them.
fn make_generics(gens: &[Ident]) -> syn::Generics {
    if gens.is_empty() {
        return syn::Generics::default();
    }
    parse_quote!(<#(#gens),*>)
}

/// `type Name = T;` (no generics) or `type Name<G, …> = T;`. A generic alias
/// (`type NonEmptyArray<T> = readonly [T, ...ReadonlyArray<T>]`) keeps `<T>` so
/// the body's `T` resolves — without it, `T` is an unresolved type (E0425).
fn alias_item(name: &Ident, gens: &[Ident], ty: &Type) -> Item {
    // Drop generic params the translated body never references — a generic
    // alias whose body lowered to serde_json::Value (an unmappable
    // conditional/utility type) would otherwise carry an unused param (E0392).
    let used: Vec<&Ident> = gens
        .iter()
        .filter(|g| super::super::types::type_uses_ident(ty, &g.to_string()))
        .collect();
    if used.is_empty() {
        parse_quote!(type #name = #ty;)
    } else {
        parse_quote!(type #name<#(#used),*> = #ty;)
    }
}
