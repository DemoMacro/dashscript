//! Type declarations (`interface` / `type`) → `syn` items.

use oxc_ast::ast::{
    Expression, NumericLiteral, TSEnumDeclaration, TSEnumMember, TSEnumMemberName,
    TSInterfaceDeclaration, TSLiteral, TSSignature, TSType, TSTypeAliasDeclaration, TSTypeLiteral,
    TSTypeName, TSTypeParameterDeclaration, TSUnionType,
};
use proc_macro2::TokenStream;
use quote::{format_ident, quote, ToTokens};
use std::collections::hash_map::DefaultHasher;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use syn::{parse_quote, Arm, Ident, Item, ItemEnum, ItemImpl, ItemStruct, Type};

use super::registry::TypeRegistry;
use super::{bindings, types};

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
        let key = Ident::new(&inherited.name, proc_macro2::Span::call_site());
        let inner = inherited.ty.clone();
        let ty = if inherited.optional {
            parse_quote!(Option<#inner>)
        } else {
            inner
        };
        fields.push(quote!(pub #key: #ty,));
    }
    let mut items = anon;
    items.push(Item::Struct(
        parse_quote! { #[derive(Clone, Debug, PartialEq)] struct #name { #(#fields)* } },
    ));
    items
}

/// Recursively collect the own fields of every interface `name` extends
/// (depth-first: a parent's parents first, then the parent itself). A `visited`
/// set breaks a cycle (`A extends B, B extends A`); the caller dedupes by name.
fn inherited_interface_fields(
    name: &str,
    registry: &TypeRegistry,
    visited: &mut HashSet<String>,
) -> Vec<super::registry::InterfaceField> {
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
            // A struct a crate-root union enum references (its alias was
            // upgraded to from an inline member) must be crate-visible, or the
            // enum cannot name it across modules (E0425).
            let vis: TokenStream = if alias_referenced_by_union(&name.to_string()) {
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
            if let Some((mut item, anons)) = union_to_enum(&name, u) {
                item.generics = make_generics(&generics);
                let mut items: Vec<Item> = anons.into_iter().map(Item::Struct).collect();
                items.push(Item::Enum(item));
                return items;
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

/// A union becomes an `enum`. Two strategies, fast path first:
///
/// 1. **All-same-kind** unions get the cleanest variants: string literals →
///    unit (`"red" | "green"` → `Red, Green`), scalar keywords → tuple
///    (`string | number` → `Str(String), Num(f64)`), type refs → tuple
///    (`Circle | Square` → `Circle(Circle)`), discriminated literals → named-
///    field (`{ kind: "circle"; radius: number }` → `Circle { radius: f64 }`).
/// 2. **Mixed** unions (`boolean | string[]`, `{ _attr } | XmlAtom | XmlAtom[]`)
///    lower member-by-member via [`member_to_variant`] — the ts2rust
///    tagged-union model: every member is one variant regardless of the others,
///    since a Rust enum admits mixed unit/tuple/struct variants. An inline-
///    object member without a discriminant becomes a tuple variant wrapping a
///    generated `__DsAnon_<hash>` struct (the "duplicate pattern": each variant
///    has an underlying struct).
///
/// Returns the enum plus any helper structs the mixed path generated. `None`
/// only when some member has no Rust analogue (a recursive self-reference, a
/// conditional/mapped type) — the caller falls back honestly.
fn union_to_enum(name: &Ident, u: &TSUnionType) -> Option<(ItemEnum, Vec<ItemStruct>)> {
    let str_variants: Vec<Ident> = u.types.iter().filter_map(string_literal_variant).collect();
    if str_variants.len() == u.types.len() {
        return Some((
            parse_quote! { #[derive(Clone, Debug, PartialEq)] enum #name { #(#str_variants),* } },
            vec![],
        ));
    }
    let scalar_variants: Vec<syn::Variant> = u.types.iter().filter_map(scalar_variant).collect();
    if scalar_variants.len() == u.types.len() {
        return Some((
            parse_quote! { #[derive(Clone, Debug, PartialEq)] enum #name { #(#scalar_variants),* } },
            vec![],
        ));
    }
    let ref_variants: Vec<syn::Variant> = u.types.iter().filter_map(type_ref_variant).collect();
    if ref_variants.len() == u.types.len() {
        return Some((
            parse_quote! { #[derive(Clone, Debug, PartialEq)] enum #name { #(#ref_variants),* } },
            vec![],
        ));
    }
    let field_variants: Vec<syn::Variant> =
        u.types.iter().filter_map(discriminated_variant).collect();
    if !field_variants.is_empty() && field_variants.len() == u.types.len() {
        return Some((
            parse_quote! { #[derive(Clone, Debug, PartialEq)] enum #name { #(#field_variants),* } },
            vec![],
        ));
    }
    // Mixed union: each member independently lowers to a variant.
    let mut anons: Vec<ItemStruct> = Vec::new();
    let mut tagged: Vec<(String, syn::Variant)> = Vec::new();
    for t in &u.types {
        match member_to_variant(t, &mut anons) {
            Some(tv) => tagged.push(tv),
            None => return None,
        }
    }
    // Sort + dedup by tag so member order (`A | B` vs `B | A`) yields one enum.
    tagged.sort_by(|a, b| a.0.cmp(&b.0));
    tagged.dedup_by(|a, b| a.0 == b.0);
    let variants = tagged.into_iter().map(|(_, v)| v).collect::<Vec<_>>();
    // Dedup helper structs by ident (two members of the same shape share one).
    let mut seen: HashSet<String> = HashSet::new();
    let anons = anons
        .into_iter()
        .filter(|s| seen.insert(s.ident.to_string()))
        .collect();
    Some((
        parse_quote! { #[derive(Clone, Debug, PartialEq)] enum #name { #(#variants),* } },
        anons,
    ))
}

/// An inline (anonymous) mixed union → its `__DsUnion<tag-stem>` enum, for
/// unions that are not all-scalar-keyword (`boolean | string[]`,
/// `string | { … }`). Each non-null member independently lowers to a variant
/// via [`member_to_variant`] (the same path [`union_to_enum`] takes for a named
/// alias); the stem is the sorted concatenation of member tags so `A | B` and
/// `B | A` dedup to one enum. Returns the enum plus any helper anon structs an
/// inline-object member needs. `None` if any non-null member has no Rust
/// analogue (recursive self-reference, conditional/mapped) — the caller falls
/// back honestly.
pub fn inline_mixed_union_enum(u: &TSUnionType) -> Option<(Ident, ItemEnum, Vec<ItemStruct>)> {
    let mut anons: Vec<ItemStruct> = Vec::new();
    let mut tagged: Vec<(String, syn::Variant)> = Vec::new();
    for t in &u.types {
        // `null`/`undefined` are handled by the `Option<…>` wrapper in
        // `types::union_type` (nullable + single non-null); a nullable multi-way
        // union is not split here — a later `Option<__DsUnion…>` batch.
        if matches!(t, TSType::TSNullKeyword(_) | TSType::TSUndefinedKeyword(_)) {
            continue;
        }
        match member_to_variant(t, &mut anons) {
            Some(tv) => tagged.push(tv),
            None => return None,
        }
    }
    if tagged.is_empty() {
        return None;
    }
    tagged.sort_by(|a, b| a.0.cmp(&b.0));
    tagged.dedup_by(|a, b| a.0 == b.0);
    let stem: String = tagged.iter().map(|(t, _)| t.as_str()).collect();
    let name = format_ident!("__DsUnion{stem}");
    let variants = tagged.into_iter().map(|(_, v)| v).collect::<Vec<_>>();
    let mut seen: HashSet<String> = HashSet::new();
    let anons = anons
        .into_iter()
        .filter(|s| seen.insert(s.ident.to_string()))
        .collect();
    Some((
        name.clone(),
        parse_quote! { #[derive(Clone, Debug, PartialEq)] enum #name { #(#variants),* } },
        anons,
    ))
}

/// One union member → its `(tag, variant)`, pushing any helper struct the
/// member needs (an inline-object member's `__DsAnon_<hash>` struct, the
/// "duplicate pattern") into `anons`. The tag names and orders the variant so
/// member order (`A | B` vs `B | A`) dedups to one enum. `None` for a member
/// with no Rust analogue (a recursive self-reference, conditional/mapped) —
/// the caller falls back honestly rather than half-building an enum.
fn member_to_variant(ty: &TSType, anons: &mut Vec<ItemStruct>) -> Option<(String, syn::Variant)> {
    match ty {
        TSType::TSStringKeyword(_) => Some(("Str".to_string(), parse_quote!(Str(String)))),
        TSType::TSNumberKeyword(_) => Some(("Num".to_string(), parse_quote!(Num(f64)))),
        TSType::TSBooleanKeyword(_) => Some(("Bool".to_string(), parse_quote!(Bool(bool)))),
        TSType::TSUndefinedKeyword(_) => Some(("Undef".to_string(), parse_quote!(Undef))),
        TSType::TSNullKeyword(_) => Some(("Null".to_string(), parse_quote!(Null))),
        TSType::TSLiteralType(_) => {
            let id = string_literal_variant(ty)?;
            Some((id.to_string(), parse_quote!(#id)))
        }
        TSType::TSTypeReference(r) => {
            let v = type_ref_variant(ty)?;
            let TSTypeName::IdentifierReference(id) = &r.type_name else {
                return None;
            };
            Some((id.name.as_ref().to_string(), v))
        }
        TSType::TSArrayType(arr) => {
            let inner_ty = types::translate_type(&arr.element_type);
            let inner_tag = array_element_tag(&arr.element_type)?;
            let tag = format!("ArrayOf{inner_tag}");
            let id = bindings::pascal(&tag);
            Some((tag, parse_quote!(#id(Vec<#inner_ty>))))
        }
        TSType::TSTypeLiteral(_) => object_member_variant(ty, anons),
        TSType::TSParenthesizedType(p) => member_to_variant(&p.type_annotation, anons),
        _ => None,
    }
}

/// The element tag of an array union member, for the `ArrayOf<Tag>` variant
/// name (`string[]` → `ArrayOfStr`, `Foo[]` → `ArrayOfFoo`).
fn array_element_tag(ty: &TSType) -> Option<String> {
    match ty {
        TSType::TSStringKeyword(_) => Some("Str".to_string()),
        TSType::TSNumberKeyword(_) => Some("Num".to_string()),
        TSType::TSBooleanKeyword(_) => Some("Bool".to_string()),
        TSType::TSTypeReference(r) => match &r.type_name {
            TSTypeName::IdentifierReference(id) => {
                Some(bindings::pascal(id.name.as_ref()).to_string())
            }
            _ => None,
        },
        TSType::TSTypeLiteral(_) => Some("Obj".to_string()),
        _ => None,
    }
}

// Inline-object shape signatures that resolve to a named `type` alias in the
// same file (`{ indent?, declaration? }` → `XmlInputOptions`). Set by the
// registry pre-pass before any union is named. Thread-local because union
// naming (`inline_mixed_union_enum`) runs both in the registry pre-pass and in
// `types::union_type` — a pure function with no registry handle — so the table
// is published here and read by both naming sites. It is file-scoped, owned
// (`String` keys/values — no AST lifetime), and reset per file by
// `set_shape_aliases`.
thread_local! {
    static SHAPE_ALIASES: std::cell::RefCell<std::collections::HashMap<String, String>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// Publish the file's inline-object → alias map before any union is named. The
/// registry pre-pass calls this once after scanning the file's `type` aliases.
pub(in crate::translator) fn set_shape_aliases(
    aliases: &std::collections::HashMap<String, String>,
) {
    SHAPE_ALIASES.with(|m| {
        let mut map = m.borrow_mut();
        map.clear();
        map.extend(aliases.iter().map(|(k, v)| (k.clone(), v.clone())));
    });
}

/// The named alias for an inline-object shape signature, when a same-shape
/// `type` alias is in scope — `None` when none matches (the member stays
/// anonymous). Structural typing: the inline member upgrades to the alias so
/// same-shape unions (inline vs alias) unify to one enum.
fn shape_alias_for(sig: &str) -> Option<String> {
    SHAPE_ALIASES.with(|m| m.borrow().get(sig).cloned())
}

/// Whether a named `type` alias is referenced by some union — an inline-object
/// member of the same shape upgraded to it. The alias's struct must then be
/// crate-visible, since the crate-root union enum references it: a module-
/// private struct would be invisible to that enum (E0425).
fn alias_referenced_by_union(name: &str) -> bool {
    SHAPE_ALIASES.with(|m| m.borrow().values().any(|v| v == name))
}

/// An inline-object union member → its `(tag, variant)`. A literal carrying a
/// string-literal discriminant becomes a named-field variant (`{ kind: "circle"
/// }` → `Circle { .. }`); a plain literal becomes a tuple variant wrapping its
/// generated `__DsAnon_<hash>` struct (`Attr(__DsAnon_..)`) — the duplicate
/// pattern: each variant has an underlying struct, so boxing/unboxing a member
/// value is a struct↔variant hop.
fn object_member_variant(
    ty: &TSType,
    anons: &mut Vec<ItemStruct>,
) -> Option<(String, syn::Variant)> {
    if let Some(v) = discriminated_variant(ty) {
        let tag = v.ident.to_string();
        return Some((tag, v));
    }
    let TSType::TSTypeLiteral(lit) = ty else {
        return None;
    };
    // Structural typing: if this inline object's shape matches a named
    // object-literal `type` alias in scope, upgrade the member to that alias
    // (`{ indent?, declaration? }` → `XmlInputOptions`) so an inline member and
    // its named alias unify to one enum variant — otherwise the same-shape
    // union spelled inline vs via an alias emits two incompatible enums.
    if let Some(sig) = object_member_tag(lit) {
        if let Some(alias) = shape_alias_for(&sig) {
            let variant = bindings::type_ident(&alias);
            return Some((alias, parse_quote!(#variant(#variant))));
        }
    }
    let (struct_name, item) = anon_struct_for_literal(lit)?;
    let tag = object_member_tag(lit)?;
    let var_id = bindings::pascal(&tag);
    anons.push(item);
    Some((tag, parse_quote!(#var_id(#struct_name))))
}

/// The variant tag of a plain inline-object member: the sorted field names,
/// PascalCase-joined (`{ _attr: .. }` → `Attr`; `{ _attr; _cdata }` →
/// `AttrCdata`) — readable and shape-distinct. `pub` so the registry pre-pass
/// can compute the same signature for an object-literal `type` alias body and
/// map it to the alias name (structural typing → alias upgrade).
pub(in crate::translator) fn object_member_tag(lit: &TSTypeLiteral) -> Option<String> {
    let mut names: Vec<String> = lit
        .members
        .iter()
        .filter_map(|sig| {
            let TSSignature::TSPropertySignature(ps) = sig else {
                return None;
            };
            bindings::property_key_name(&ps.key).map(|n| n.to_string())
        })
        .collect();
    if names.is_empty() {
        return None;
    }
    names.sort();
    names.dedup();
    Some(
        names
            .iter()
            .map(|n| bindings::pascal(n).to_string())
            .collect(),
    )
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
/// impl still compiles. The fallback pattern mirrors the variant's shape: a
/// tuple variant (`Record(Record)`) needs `Self::#id(..)` — a unit pattern
/// would not match it (E0533).
fn display_arm(v: &syn::Variant) -> Arm {
    let id = &v.ident;
    match v.ident.to_string().as_str() {
        "Str" => parse_quote!(Self::Str(__s) => __f.write_str(__s.as_str()),),
        "Num" => parse_quote!(Self::Num(__n) => ::std::write!(__f, "{}", __n),),
        "Bool" => parse_quote!(Self::Bool(__b) => ::std::write!(__f, "{}", __b),),
        "Undef" => parse_quote!(Self::Undef => __f.write_str("undefined"),),
        "Null" => parse_quote!(Self::Null => __f.write_str("null"),),
        _ => match &v.fields {
            syn::Fields::Unnamed(_) => parse_quote!(Self::#id(..) => ::core::unreachable!(),),
            syn::Fields::Named(_) => parse_quote!(Self::#id { .. } => ::core::unreachable!(),),
            syn::Fields::Unit => parse_quote!(Self::#id => ::core::unreachable!(),),
        },
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

/// The discriminant value of one object-literal property signature, when its
/// *type* is a string literal — `kind: "circle"` → `"circle"`. `None` for a
/// property without a nameable key, without a type annotation, or whose type is
/// not a string literal. The single source of truth for "what counts as a
/// discriminant", shared by [`discriminated_variant`] (emit) and
/// `registry::variant_of` (the shape table), so the two cannot drift on the
/// rule (the registry once mirrored it).
pub(in crate::translator) fn discriminant_property_value(sig: &TSSignature<'_>) -> Option<String> {
    let TSSignature::TSPropertySignature(ps) = sig else {
        return None;
    };
    bindings::property_key_name(&ps.key)?; // a discriminant must carry a nameable key
    let ta = ps.type_annotation.as_ref()?;
    let TSType::TSLiteralType(lt) = &ta.type_annotation else {
        return None;
    };
    let TSLiteral::StringLiteral(s) = &lt.literal else {
        return None;
    };
    Some(s.value.to_string())
}

/// The discriminant value of a discriminated-union object-literal member — the
/// value of its string-literal-typed property (`kind: "circle"` → `"circle"`),
/// or `None` when it has none. When more than one property is string-literal-
/// typed, the last wins (matching the original emit path's loop). See
/// [`discriminant_property_value`] for the per-property rule.
pub(in crate::translator) fn discriminant_value(lit: &TSTypeLiteral<'_>) -> Option<String> {
    let mut value: Option<String> = None;
    for sig in &lit.members {
        if let Some(v) = discriminant_property_value(sig) {
            value = Some(v);
        }
    }
    value
}

/// `{ kind: "circle"; radius: number }` → `Circle { radius: f64 }` — a
/// named-field variant of a discriminated union. The discriminant is the
/// property whose *type* is a string literal (see [`discriminant_value`]); its
/// value names the variant and is not emitted as a field. The remaining
/// properties become the variant's named fields. Returns `None` when the
/// literal has no string-literal discriminant.
fn discriminated_variant(ty: &TSType) -> Option<syn::Variant> {
    let TSType::TSTypeLiteral(lit) = ty else {
        return None;
    };
    let variant = bindings::pascal(&discriminant_value(lit)?);
    let fields: Vec<TokenStream> = lit
        .members
        .iter()
        .filter_map(|sig| {
            // The discriminant property names the variant; it is not a field.
            if discriminant_property_value(sig).is_some() {
                return None;
            }
            let TSSignature::TSPropertySignature(ps) = sig else {
                return None;
            };
            let key = bindings::property_key_name(&ps.key)?;
            let ta = ps.type_annotation.as_ref()?;
            let field_ty = types::translate_type(&ta.type_annotation);
            Some(quote!(#key: #field_ty))
        })
        .collect();
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
                parse_quote! { #[derive(Clone, Debug, PartialEq)] struct #anon_name { #(#fields)* } },
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

/// An anonymous object-literal type (`{ x: number; y: string }`) → a synthetic
/// `__DsAnon_<hash>` struct, the generalization of [`field_type`]'s inline-
/// object handling to any type position (a function return/parameter, a union
/// member, a `type` alias body) where there is no parent+field naming context.
/// Pure and structure-keyed: the name is a stable hash of the sorted field
/// names and their translated types, so two literals with the same shape share
/// one struct (dedup) and different shapes never collide. `None` when the
/// literal has an index signature or any non-property member — that is a map
/// (`HashMap`), not a struct, and falls back to `_` at the use site. The single
/// source of truth: [`types::translate_type`] calls this to name the type, and
/// the registry pre-pass calls it to emit the definition — mirroring
/// [`scalar_union_enum`].
pub fn anon_struct_for_literal(lit: &TSTypeLiteral) -> Option<(Ident, ItemStruct)> {
    // An index signature or any non-property member → a map shape, not a struct.
    if lit
        .members
        .iter()
        .any(|m| !matches!(m, TSSignature::TSPropertySignature(_)))
    {
        return None;
    }
    let mut fields: Vec<(Ident, Type)> = Vec::new();
    for sig in &lit.members {
        let TSSignature::TSPropertySignature(ps) = sig else {
            continue;
        };
        let key = bindings::property_key_name(&ps.key)?;
        let ta = ps.type_annotation.as_ref()?;
        // `translate_type` recurses: a nested inline-object field lowers through
        // this same function, so `{ outer: { inner: number } }` emits two structs
        // (the outer references the inner by its hash name).
        let ty = types::translate_type(&ta.type_annotation);
        let ty = if ps.optional {
            parse_quote!(Option<#ty>)
        } else {
            ty
        };
        fields.push((key, ty));
    }
    let mut hasher = DefaultHasher::new();
    let mut names: Vec<String> = fields.iter().map(|(k, _)| k.to_string()).collect();
    names.sort();
    names.hash(&mut hasher);
    for (_, t) in &fields {
        t.to_token_stream().to_string().hash(&mut hasher);
    }
    let hex = format!("{:016x}", hasher.finish());
    let name = format_ident!("__DsAnon_{}", &hex[..12]);
    let field_tokens: Vec<TokenStream> = fields.iter().map(|(k, t)| quote!(pub #k: #t,)).collect();
    let item: ItemStruct =
        parse_quote! { #[derive(Clone, Debug, PartialEq)] pub struct #name { #(#field_tokens)* } };
    Some((name, item))
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

/// A TypeScript `enum` lowers to a Rust `mod` of typed `const` members — the
/// way an ES `enum` is a runtime object of named values. A numeric enum
/// (`enum E { A, B }` → A=0, B=1) emits `i64` consts with auto-incrementing
/// values; a string enum (`enum E { A = "a" }`) emits `&'static str` consts.
/// `Color.Red` then reads as `Color::Red` (a path constant), matching ES
/// `Color.Red === 0` — the value is a plain `i64`/`&str`, so `Color.Red + 1`
/// stays a numeric expression the way it does in TS.
///
/// Returns `None` when a member's initializer is not a literal (`A = 1 << 2`,
/// `A = Other.B`) or its name is computed — DashScript does not constant-
/// evaluate, where oxc pre-computes these in `Scoping`. The caller emits
/// nothing and `check` flags the enum unsupported.
pub fn translate_enum(decl: &TSEnumDeclaration) -> Option<Vec<Item>> {
    let name = bindings::type_ident(&decl.id.name);
    let members = eval_enum_members(&decl.body.members)?;
    let consts: Vec<Item> = members
        .iter()
        .map(|(member_name, value)| enum_const_item(member_name, value))
        .collect();
    Some(vec![Item::Mod(parse_quote! {
        #[allow(non_upper_case_globals, non_snake_case)]
        pub mod #name {
            #(#consts)*
        }
    })])
}

/// One `pub const Member: T = value;` for an enum member. The member name keeps
/// its TS spelling (commonly PascalCase, e.g. `Red`); `non_upper_case_globals`
/// is allowed on the enclosing `mod` so a non-SCREAMING const does not warn.
fn enum_const_item(name: &str, value: &EnumValue) -> Item {
    let ident = bindings::type_ident(name);
    match value {
        EnumValue::Number(n) => parse_quote!(pub const #ident: i64 = #n;),
        EnumValue::String(s) => {
            let lit = proc_macro2::Literal::string(s);
            parse_quote!(pub const #ident: &'static str = #lit;)
        }
    }
}

/// A TS enum member's evaluated value: a numeric member is an `i64`; a string
/// member is a `&'static str`. Shared by `translate_enum` (emit) and the
/// registry pre-pass (kind classification) so the two agree on what each
/// member lowers to.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::translator) enum EnumValue {
    Number(i64),
    String(String),
}

/// Evaluate every member's value, following ES enum semantics. A member with
/// no initializer takes the previous numeric value + 1 (or 0 for the first);
/// a numeric-literal initializer resets that counter; a string-literal
/// initializer yields a string value and breaks the numeric chain. Returns
/// `None` if any member's initializer is not a literal (constant evaluation is
/// out of scope) or its name is computed — the enum then stays unsupported.
pub(in crate::translator) fn eval_enum_members(
    members: &[TSEnumMember],
) -> Option<Vec<(String, EnumValue)>> {
    let mut out = Vec::new();
    let mut prev_num: Option<i64> = None;
    for member in members {
        let name = enum_member_name(&member.id)?;
        let value = match &member.initializer {
            None => {
                let n = prev_num.map_or(0, |p| p + 1);
                prev_num = Some(n);
                EnumValue::Number(n)
            }
            Some(Expression::StringLiteral(s)) => {
                prev_num = None;
                EnumValue::String(s.value.to_string())
            }
            Some(Expression::NumericLiteral(n)) => {
                let v = numeric_literal_to_i64(n)?;
                prev_num = Some(v);
                EnumValue::Number(v)
            }
            Some(_) => return None,
        };
        out.push((name, value));
    }
    Some(out)
}

/// A member's name — an identifier or a string literal. A computed name
/// (`['x']`, `` `tpl` ``) returns `None` (it is not a compile-time constant).
fn enum_member_name(name: &TSEnumMemberName) -> Option<String> {
    match name {
        TSEnumMemberName::Identifier(id) => Some(id.name.to_string()),
        TSEnumMemberName::String(s) => Some(s.value.to_string()),
        TSEnumMemberName::ComputedString(_) | TSEnumMemberName::ComputedTemplateString(_) => None,
    }
}

/// An integer enum member's value. A fractional or non-finite numeric literal
/// returns `None` — ES enums are integral in practice, and `i64` is the
/// faithful Rust repr (a `1.5` enum member is exotic enough to flag honestly).
fn numeric_literal_to_i64(n: &NumericLiteral) -> Option<i64> {
    let v = n.value;
    if v.is_finite() && v.fract() == 0.0 {
        Some(v as i64)
    } else {
        None
    }
}
