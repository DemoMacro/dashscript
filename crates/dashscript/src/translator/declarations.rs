//! Type declarations (`interface` / `type`) → `syn` items.

use oxc_ast::ast::{
    TSInterfaceDeclaration, TSLiteral, TSSignature, TSType, TSTypeAliasDeclaration, TSTypeName,
    TSTypeParameterDeclaration, TSUnionType,
};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{parse_quote, Arm, Ident, Item, ItemEnum, ItemImpl, ItemStruct, Type};

use super::{bindings, types};

/// `interface Point { x: number }` → `struct Point { pub x: f64 }`.
///
/// Fields are `pub`: a TS interface describes a value's public shape, so the
/// Rust struct exposes its fields to match.
pub fn translate_interface(iface: &TSInterfaceDeclaration) -> Vec<Item> {
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
    let fields: Vec<TokenStream> = iface
        .body
        .body
        .iter()
        .filter_map(|sig| struct_field(sig, &parent, &mut anon))
        .collect();
    let mut items = anon;
    items.push(Item::Struct(
        parse_quote! { #[derive(Clone)] struct #name { #(#fields)* } },
    ));
    items
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
                .filter_map(|sig| struct_field(sig, &parent, &mut anon))
                .collect();
            let mut items = anon;
            let mut item: ItemStruct =
                parse_quote! { #[derive(Clone)] struct #name { #(#fields)* } };
            item.generics = make_generics(&generics);
            items.push(Item::Struct(item));
            items
        }
        TSType::TSUnionType(u) => {
            // A union of string literals or named types becomes an `enum`;
            // anything else falls back to a type alias.
            if let Some(mut item) = union_to_enum(&name, u) {
                item.generics = make_generics(&generics);
                return vec![Item::Enum(item)];
            }
            let ty = types::translate_type(&alias.type_annotation);
            vec![alias_item(&name, &generics, &ty)]
        }
        other => {
            let ty = types::translate_type(other);
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
    if gens.is_empty() {
        parse_quote!(type #name = #ty;)
    } else {
        parse_quote!(type #name<#(#gens),*> = #ty;)
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

/// One struct field from a property signature: `pub name: Type,`. An inline-
/// object type (`{ ... }`) lowers to a named `__Ds<parent><Field>` struct
/// pushed into `anon` (emitted before the owning struct). A direct self-
/// reference (`parent?: Element` on `interface Element`) wraps in `Box` so the
/// struct has finite size; an array self-reference (`elements?: Element[]`)
/// does not — `Vec` is already heap-allocated.
fn struct_field(sig: &TSSignature, parent: &str, anon: &mut Vec<Item>) -> Option<TokenStream> {
    let TSSignature::TSPropertySignature(ps) = sig else {
        return None;
    };
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
                parse_quote! { #[derive(Clone)] struct #anon_name { #(#fields)* } },
            ));
            parse_quote!(#anon_name)
        }
        TSType::TSArrayType(arr) => {
            let inner = field_type(&arr.element_type, parent, field, anon);
            parse_quote!(Vec<#inner>)
        }
        _ => types::translate_type(ty),
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
