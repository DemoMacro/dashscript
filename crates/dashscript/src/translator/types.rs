//! `TSType` → `syn::Type` — the one-to-one mapping table.

use oxc_ast::ast::{TSArrayType, TSType, TSTypeName, TSTypeReference, TSUnionType};
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
    // `Array<T>` → `Vec<T>`; other named refs pass through (e.g. `Point`).
    if name == "Array" {
        if let Some(inner) = r.type_arguments.as_ref().and_then(|a| a.params.first()) {
            let inner_ty = translate_type(inner);
            return parse_quote!(Vec<#inner_ty>);
        }
    }
    // `Record<K, V>` → `HashMap<K, V>` (the TS record/map type).
    if name == "Record" {
        if let Some(args) = r.type_arguments.as_ref() {
            let ps = &args.params;
            if ps.len() == 2 {
                let k_ty = translate_type(&ps[0]);
                let v_ty = translate_type(&ps[1]);
                return parse_quote!(::std::collections::HashMap<#k_ty, #v_ty>);
            }
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
