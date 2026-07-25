//! `TSType` → `syn::Type` — the one-to-one mapping table.

use oxc_ast::ast::{
    TSArrayType, TSType, TSTypeName, TSTypeOperator, TSTypeOperatorOperator, TSTypeReference,
    TSUnionType,
};
use quote::format_ident;
use syn::{parse_quote, Type};

/// Map a TypeScript type annotation to its Rust equivalent as a `syn::Type`.
///
/// Unmapped types fall back to `_` so a missing mapping surfaces as a
/// `cargo check` error rather than silent miscompilation.
pub fn translate_type(ty: &TSType) -> Type {
    match ty {
        TSType::TSStringKeyword(_) => parse_quote!(String),
        TSType::TSNumberKeyword(_) => parse_quote!(f64),
        TSType::TSBooleanKeyword(_) => parse_quote!(bool),
        TSType::TSVoidKeyword(_) | TSType::TSUndefinedKeyword(_) => parse_quote!(()),
        TSType::TSArrayType(arr) => array_type(arr),
        TSType::TSUnionType(u) => union_type(u),
        TSType::TSTypeReference(r) => reference_type(r),
        TSType::TSTypeOperatorType(op) => operator_type(op),
        _ => parse_quote!(_),
    }
}

/// `readonly T` (the `TSTypeOperator` form, e.g. `readonly string[]`) → `T`.
/// `readonly` is a TS type-level immutability constraint; Rust has no separate
/// immutable-collection type, so the inner type is the runtime shape.
/// `keyof T` / `unique symbol` have no Rust analogue and fall back to `_`.
fn operator_type(op: &TSTypeOperator) -> Type {
    match op.operator {
        TSTypeOperatorOperator::Readonly => translate_type(&op.type_annotation),
        _ => parse_quote!(_),
    }
}

fn array_type(arr: &TSArrayType) -> Type {
    let inner = translate_type(&arr.element_type);
    parse_quote!(Vec<#inner>)
}

fn reference_type(r: &TSTypeReference) -> Type {
    let TSTypeName::IdentifierReference(id) = &r.type_name else {
        return parse_quote!(_);
    };
    let name: &str = &id.name;
    // `Readonly<T>` → `T` (a TS type-level constraint; the runtime shape is `T`).
    if name == "Readonly" {
        if let Some(inner) = r.type_arguments.as_ref().and_then(|a| a.params.first()) {
            return translate_type(inner);
        }
    }
    // `Array<T>` → `Vec<T>`; other named refs pass through (e.g. `Point`).
    if name == "Array" {
        if let Some(inner) = r.type_arguments.as_ref().and_then(|a| a.params.first()) {
            let inner_ty = translate_type(inner);
            return parse_quote!(Vec<#inner_ty>);
        }
    }
    // `Record<K, V>` / `Map<K, V>` → `HashMap<K, V>` — the TS record and the ES
    // `Map` both lower to a Rust `HashMap`. (A `Map`'s insertion order is not
    // preserved — an `IndexMap` would — but DashScript targets std collections
    // today; an ordered map is a later dep.)
    if matches!(name, "Record" | "Map") {
        if let Some(args) = r.type_arguments.as_ref() {
            let ps = &args.params;
            if ps.len() == 2 {
                let k_ty = translate_type(&ps[0]);
                let v_ty = translate_type(&ps[1]);
                return parse_quote!(::std::collections::HashMap<#k_ty, #v_ty>);
            }
        }
    }
    // `Set<T>` → `HashSet<T>` (the ES `Set`).
    if name == "Set" {
        if let Some(inner) = r.type_arguments.as_ref().and_then(|a| a.params.first()) {
            let inner_ty = translate_type(inner);
            return parse_quote!(::std::collections::HashSet<#inner_ty>);
        }
    }
    let ident = format_ident!("{}", name);
    parse_quote!(#ident)
}

/// `T | null` / `T | undefined` → `Option<T>` (one non-null member); a real
/// multi-member union (`A | B`) maps to an `enum` later, so it falls back to
/// `_` here and surfaces as a `cargo check` error until then.
fn union_type(u: &TSUnionType) -> Type {
    let mut non_null: Vec<&TSType> = Vec::new();
    let mut nullable = false;
    for t in &u.types {
        match t {
            TSType::TSNullKeyword(_) | TSType::TSUndefinedKeyword(_) => nullable = true,
            other => non_null.push(other),
        }
    }
    if nullable && non_null.len() == 1 {
        let inner = translate_type(non_null[0]);
        return parse_quote!(Option<#inner>);
    }
    // An all-scalar-keyword union (the XML-attribute / JSON-value shape:
    // `string | number | boolean | undefined`) becomes a generated `__DsUnion…`
    // enum so the type is concrete rather than `_`. The enum definition itself
    // is emitted by the registry pre-pass that scans every type position
    // (`registry::inline_union_enums`) — this only names the type.
    if let Some((name, _)) = super::declarations::scalar_union_enum(u) {
        return parse_quote!(#name);
    }
    parse_quote!(_)
}

/// The path of a `Type::Path`, if any — used to name an object literal's struct.
pub fn type_path(ty: &Type) -> Option<&syn::Path> {
    if let Type::Path(tp) = ty {
        Some(&tp.path)
    } else {
        None
    }
}

/// True when a type path is `Copy`: the scalar numerics and `bool`, or
/// `Option<T>` where `T` is itself `Copy`. A `Copy` value passed by value is
/// duplicated on read, so it never needs cloning; everything else
/// (`String`/`Vec`/`HashMap`/user `struct`/`enum`) is non-`Copy` and would move.
pub fn is_copy_path(path: &syn::Path) -> bool {
    let Some(seg) = path.segments.last() else {
        return false;
    };
    match seg.ident.to_string().as_str() {
        "f64" | "i64" | "u64" | "i32" | "u32" | "usize" | "isize" | "bool" => true,
        "Option" => {
            if let syn::PathArguments::AngleBracketed(args) = &seg.arguments {
                if let Some(syn::GenericArgument::Type(ty)) = args.args.first() {
                    if let Some(inner) = type_path(ty) {
                        return is_copy_path(inner);
                    }
                }
            }
            false
        }
        _ => false,
    }
}
