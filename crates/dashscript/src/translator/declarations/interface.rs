use oxc_ast::ast::{
    TSInterfaceDeclaration, TSMethodSignature, TSPropertySignature, TSSignature, TSType,
    TSTypeLiteral, TSTypeName, TSTypeParameterDeclaration,
};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use std::collections::HashSet;
use syn::{parse_quote, Ident, Item, Type};

use super::super::registry::TypeRegistry;
use super::super::{bindings, types};

/// `interface Point { x: number }` → `struct Point { pub x: f64 }`.
///
/// Fields are `pub`: a TS interface describes a value's public shape, so the
/// Rust struct exposes its fields to match.
pub fn translate_interface(iface: &TSInterfaceDeclaration, registry: &TypeRegistry) -> Vec<Item> {
    let name: &str = &iface.id.name;
    let name = bindings::type_ident(name);
    // A pure index-signature interface (`{ [key: string]: T }`) is a string-
    // keyed map — the same shape as `Record<string, T>` — so it lowers to a
    // `HashMap<String, T>` type alias, not a struct (it has no named fields).
    // That keeps an attribute access `attrs["k"]` on the existing HashMap
    // member path instead of an empty struct with no fields.
    if let Some(item) = index_signature_alias(&name, &iface.body.body) {
        return vec![item];
    }
    // Each inline-object field type (`declaration?: { attributes?: X }`) lowers
    // to a named `__Ds<Interface><Field>` struct emitted before this one, so
    // the field references a real type instead of `_`. A direct self-reference
    // (`parent?: Element`) wraps in `Box` so the struct is finite-sized.
    let parent = name.to_string();
    let mut anon = Vec::new();
    let own_keys: HashSet<String> = iface
        .body
        .body
        .iter()
        .filter_map(|sig| {
            let TSSignature::TSPropertySignature(ps) = sig else {
                return None;
            };
            bindings::property_key_name(&ps.key).map(|k| k.to_string())
        })
        .collect();
    let mut fields: Vec<TokenStream> = iface
        .body
        .body
        .iter()
        .filter_map(|sig| struct_field(sig, &parent, &mut anon))
        .collect();
    // Flatten `extends A, B` parents' fields into this struct (Rust has no
    // struct inheritance, so a parent's fields are merged verbatim). A field
    // the child declares wins (ES override); a diamond or cycle is safe via the
    // seen/visited sets.
    let mut seen = own_keys;
    let mut visited = HashSet::new();
    for inherited in inherited_interface_fields(&iface.id.name, registry, &mut visited) {
        if !seen.insert(inherited.name.clone()) {
            continue;
        }
        // `inherited.name` is a bindings-snaked rust name; a keyword became a
        // raw ident (`r#type`), whose string form `Ident::new` rejects — parse
        // the `r#` prefix back into `Ident::new_raw`.
        let key = match inherited.name.strip_prefix("r#") {
            Some(rest) => Ident::new_raw(rest, proc_macro2::Span::call_site()),
            None => Ident::new(&inherited.name, proc_macro2::Span::call_site()),
        };
        let inner = inherited.ty.clone();
        let ty = if inherited.optional {
            parse_quote!(Option<#inner>)
        } else {
            inner
        };
        fields.push(quote!(pub #key: #ty,));
    }
    let mut items = anon;
    let generics = interface_generics(&iface.type_parameters);
    items.push(Item::Struct(
        parse_quote! { #[derive(Clone, Debug, PartialEq)] struct #name #generics { #(#fields)* } },
    ));
    items
}

/// `<T, U = V>` for a generic interface — the param idents plus any default
/// type, so a use that omits a defaulted param still resolves
/// (`CustomDescriptor<ChartSpaceOptions>` against
/// `interface CustomDescriptor<TInput, Ctx = WriteContext, TOutput = TInput>`).
/// Empty for a non-generic interface. Constraints (`T extends X`) are dropped
/// (no `where` clause); the `#[derive(Clone, Debug, PartialEq)]` macros add
/// the matching `T:` bounds automatically.
fn interface_generics(
    tp: &Option<oxc_allocator::Box<'_, TSTypeParameterDeclaration>>,
) -> TokenStream {
    let Some(tp) = tp.as_deref() else {
        return TokenStream::new();
    };
    let params: Vec<TokenStream> = tp
        .params
        .iter()
        .map(|p| {
            let name = bindings::type_ident(&p.name.name);
            match &p.default {
                Some(default) => {
                    let default_ty = types::translate_type(default);
                    quote!(#name = #default_ty)
                }
                None => quote!(#name),
            }
        })
        .collect();
    quote!(<#(#params),*>)
}

/// Recursively collect the own fields of every interface `name` extends
/// (depth-first: a parent's parents first, then the parent itself). A `visited`
/// set breaks a cycle (`A extends B, B extends A`); the caller dedupes by name.
fn inherited_interface_fields(
    name: &str,
    registry: &TypeRegistry,
    visited: &mut HashSet<String>,
) -> Vec<super::super::registry::InterfaceField> {
    if !visited.insert(name.to_string()) {
        return Vec::new();
    }
    let mut out = Vec::new();
    if let Some(parents) = registry.interface_extends.get(name) {
        for parent in parents {
            out.extend(inherited_interface_fields(parent, registry, visited));
            if let Some(own) = registry.interface_own_fields.get(parent) {
                out.extend(own.iter().cloned());
            }
        }
    }
    out
}

/// `interface X { [key: string]: T }` (a sole index signature, no property
/// signatures) → `type X = HashMap<String, T>`. A TS index key is `string` or
/// `number` (both lower to a Rust `String` key); any other shape (mixed
/// property + index signatures, or multiple indices) returns `None` so the
/// struct path handles it.
fn index_signature_alias(name: &Ident, sigs: &[TSSignature]) -> Option<Item> {
    if sigs.len() != 1 {
        return None;
    }
    let TSSignature::TSIndexSignature(idx) = &sigs[0] else {
        return None;
    };
    let val = types::translate_type(&idx.type_annotation.type_annotation);
    let item: Item = parse_quote!(type #name = ::std::collections::HashMap<String, #val>;);
    Some(item)
}

/// An inline `{ [key: string]: T }` (sole index signature, no property
/// signatures) → `HashMap<String, T>` — the inline analogue of
/// [`index_signature_alias`] for an anonymous object literal in a type position
/// (a union member, a parameter, a `type` alias body). `None` for mixed
/// property + index signatures or multiple indices, so [`types::translate_type`]
/// keeps its `_` fallback (a struct shape stays a struct).
pub(in crate::translator) fn index_signature_type(lit: &TSTypeLiteral) -> Option<Type> {
    if lit
        .members
        .iter()
        .any(|m| matches!(m, TSSignature::TSPropertySignature(_)))
    {
        return None;
    }
    let mut idxs = lit.members.iter().filter_map(|m| match m {
        TSSignature::TSIndexSignature(idx) => Some(idx),
        _ => None,
    });
    let idx = idxs.next()?;
    if idxs.next().is_some() {
        return None;
    }
    let val = types::translate_type(&idx.type_annotation.type_annotation);
    Some(parse_quote!(::std::collections::HashMap<String, #val>))
}

/// One struct field from a property or method signature: `pub name: Type,`.
/// A property (`field: Type`) maps to a data field; a method (`m(a: A): R`)
/// maps to a function-typed field (`pub m: fn(A) -> R`) — an interface method
/// carries no body, so the field is a bare `fn` pointer a concrete object
/// supplies. An inline-object type (`declaration?: { attributes?: X }`) lowers
/// to a named `__Ds<Interface><Field>` struct pushed into `anon` (emitted
/// before the owning struct). A direct self-reference (`parent?: Element`)
/// wraps in `Box` so the struct has finite size; an array self-reference
/// (`elements?: Element[]`) does not — `Vec` is already heap-allocated.
pub(super) fn struct_field(
    sig: &TSSignature,
    parent: &str,
    anon: &mut Vec<Item>,
) -> Option<TokenStream> {
    match sig {
        TSSignature::TSPropertySignature(ps) => property_field(ps, parent, anon),
        TSSignature::TSMethodSignature(ms) => method_field(ms),
        _ => None,
    }
}

/// A property signature (`field: Type`) → `pub field: Type,` — the inline-
/// object, self-reference, and optional handling of the data-field path.
fn property_field(
    ps: &TSPropertySignature,
    parent: &str,
    anon: &mut Vec<Item>,
) -> Option<TokenStream> {
    let key = bindings::property_key_name(&ps.key)?;
    let ta = ps.type_annotation.as_ref()?;
    let field_name = key.to_string();
    let mut ty = field_type(&ta.type_annotation, parent, &field_name, anon);
    // A bare `field: <parent>` (the struct naming itself, not via `[]`) would
    // make the struct infinite-sized; `Box` indirection fixes it. The optional
    // wrap is added after, so `parent?: Element` → `Option<Box<Element>>`.
    if is_direct_self_reference(&ta.type_annotation, parent) {
        ty = parse_quote!(Box<#ty>);
    }
    let ty: Type = if ps.optional {
        parse_quote!(Option<#ty>)
    } else {
        ty
    };
    Some(quote!(pub #key: #ty,))
}

/// A method signature (`stringify(value: TInput, ctx: Ctx): string`) → a
/// function-typed field (`pub stringify: fn(TInput, Ctx) -> String`). A TS
/// interface method carries no body, so the field is a bare `fn` pointer; a
/// concrete object literal provides the callable. Param types emit in order
/// (names dropped — a Rust `fn` type names only types), so the type params the
/// method names land in the field type and the struct uses its generics.
fn method_field(ms: &TSMethodSignature) -> Option<TokenStream> {
    let key = bindings::property_key_name(&ms.key)?;
    let params: Vec<Type> = ms
        .params
        .items
        .iter()
        .filter_map(|p| {
            let ta = p.type_annotation.as_deref()?;
            Some(types::translate_type(&ta.type_annotation))
        })
        .collect();
    let ret: Type = ms
        .return_type
        .as_ref()
        .map(|t| types::translate_type(&t.type_annotation))
        .unwrap_or_else(|| parse_quote!(()));
    Some(quote!(pub #key: fn(#(#params),*) -> #ret,))
}

/// A field's value type. An inline-object (`{ ... }`) becomes a named anon
/// struct (recursive: its own inline-object fields become nested anon structs,
/// parented by the new struct's name). An array of inline objects becomes
/// `Vec<anon>`. Anything else maps through `types::translate_type`.
fn field_type(ty: &TSType, parent: &str, field: &str, anon: &mut Vec<Item>) -> Type {
    match ty {
        TSType::TSTypeLiteral(lit) => {
            let anon_name = anon_struct_name(parent, field);
            let anon_parent = anon_name.to_string();
            let fields: Vec<TokenStream> = lit
                .members
                .iter()
                .filter_map(|sig| struct_field(sig, &anon_parent, anon))
                .collect();
            anon.push(Item::Struct(
                parse_quote! { #[derive(Clone, Debug, PartialEq)] struct #anon_name { #(#fields)* } },
            ));
            parse_quote!(#anon_name)
        }
        TSType::TSArrayType(arr) => {
            let inner = field_type(&arr.element_type, parent, field, anon);
            parse_quote!(Vec<#inner>)
        }
        _ => types::translate_type_for_data(ty),
    }
}

/// `__Ds<Parent><FieldPascalCase>` — a deterministic, conflict-free name for
/// an inline-object field's anon struct (`Element.declaration` →
/// `__DsElementDeclaration`). Field names are `snake_case`; the PascalCase
/// join mirrors the surrounding Rust type-naming convention.
fn anon_struct_name(parent: &str, field: &str) -> Ident {
    let pascal: String = field
        .split('_')
        .map(|seg| {
            let mut chars = seg.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect(),
                None => String::new(),
            }
        })
        .collect();
    format_ident!("__Ds{parent}{pascal}")
}

/// Whether `ty` is a bare reference to the owning struct's name (`Element` on
/// `interface Element`) — the direct self-reference that needs `Box`. A
/// reference through `[]` (`Element[]`) or a different name is not direct.
fn is_direct_self_reference(ty: &TSType, parent: &str) -> bool {
    let TSType::TSTypeReference(r) = ty else {
        return false;
    };
    let TSTypeName::IdentifierReference(id) = &r.type_name else {
        return false;
    };
    let name: &str = &id.name;
    name == parent
}
