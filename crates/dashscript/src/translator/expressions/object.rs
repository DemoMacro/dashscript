//! Object literals: `Point { x: 1 }` → struct init, `Record` → `HashMap`,
//! `{ kind: "…" }` → discriminated-union variant.

use std::collections::HashSet;

use oxc_ast::ast::{Expression, ObjectExpression, ObjectPropertyKind};
use proc_macro2::Span;
use syn::{parse_quote, Expr, Ident};

use super::super::bindings;
use super::super::context::Ctx;
use super::super::types;
use super::is_hashmap;
use super::translate_expr;

/// `Point { x: 1 }` — needs the target type's name from the binding annotation.
/// A `{ kind: "circle", … }` literal whose target is a registered
/// discriminated union instead builds a variant (`Shape::Circle { … }`).
pub(super) fn object_expr(obj: &ObjectExpression, ty_hint: Option<&syn::Type>, ctx: &Ctx<'_>) -> Expr {
    let Some(path) = ty_hint.and_then(types::type_path) else {
        return parse_quote!(::core::todo!());
    };
    // `Record<K, V>` (a `HashMap`) → `HashMap::from([(key, value), …])`.
    if is_hashmap(path) {
        return hashmap_literal(obj, ctx);
    }
    if let Some(expr) = variant_construct(obj, path, ctx) {
        return expr;
    }
    // A `…v` spread records a struct-update base (`Struct { …, ..v }`); only an
    // identifier base is supported. If multiple spreads appear, the last wins.
    let optionals = optional_fields_for(path, ctx);
    let mut base: Option<Expr> = None;
    let fields: Vec<syn::FieldValue> = obj
        .properties
        .iter()
        .filter_map(|p| match p {
            ObjectPropertyKind::ObjectProperty(op) => {
                let key = bindings::property_key_name(&op.key)?;
                let key_str = key.to_string();
                let is_optional = optionals.is_some_and(|s| s.contains(&key_str));
                // Field-init shorthand: a non-optional `x: x` becomes `x`
                // (value is the same-named bare identifier) for idiomatic Rust.
                if !is_optional {
                    if let Expression::Identifier(id) = &op.value {
                        if bindings::snake(&id.name) == key {
                            return Some(parse_quote!(#key));
                        }
                    }
                }
                let mut value = translate_expr(&op.value, ctx);
                if is_optional {
                    value = parse_quote!(Some(#value));
                }
                Some(parse_quote!(#key: #value))
            }
            ObjectPropertyKind::SpreadProperty(sp) => {
                base = Some(translate_expr(&sp.argument, ctx));
                None
            }
        })
        .collect();
    match base {
        Some(b) => parse_quote!(#path { #(#fields),*, ..#b }),
        None => {
            let extras = missing_optionals(path, &fields, ctx);
            parse_quote!(#path { #(#fields),*, #(#extras),* })
        }
    }
}

/// The optional (`?:`) field names of the struct named by `path`, if any.
fn optional_fields_for<'a>(path: &syn::Path, ctx: &Ctx<'a>) -> Option<&'a HashSet<String>> {
    let type_name = path.segments.last()?.ident.to_string();
    ctx.struct_optionals(&type_name)
}

/// `None` initializers for optional (`?:`) fields the literal omitted, so a
/// partial struct literal still names every field. Only fields registered as
/// optional on this struct type and absent from `present` are filled.
fn missing_optionals(path: &syn::Path, present: &[syn::FieldValue], ctx: &Ctx<'_>) -> Vec<syn::FieldValue> {
    let Some(type_name) = path.segments.last().map(|s| s.ident.to_string()) else {
        return Vec::new();
    };
    let Some(optionals) = ctx.struct_optionals(&type_name) else {
        return Vec::new();
    };
    let present: HashSet<String> = present
        .iter()
        .filter_map(|f| match &f.member {
            syn::Member::Named(id) => Some(id.to_string()),
            syn::Member::Unnamed(_) => None,
        })
        .collect();
    optionals
        .iter()
        .filter(|name| !present.contains(*name))
        .map(|name| {
            let id = Ident::new(name.as_str(), Span::call_site());
            parse_quote!(#id: None)
        })
        .collect()
}

/// `{ a: 1, b: 2 }` as a `HashMap` → `HashMap::from([("a".to_string(), 1.0), …])`.
/// Keys are the `.ds` property names, owned so the map outlives the literal.
fn hashmap_literal(obj: &ObjectExpression, ctx: &Ctx<'_>) -> Expr {
    let entries: Vec<Expr> = obj
        .properties
        .iter()
        .filter_map(|p| {
            let ObjectPropertyKind::ObjectProperty(op) = p else { return None };
            let value = translate_expr(&op.value, ctx);
            let key = if op.computed {
                // `[k]: v` — a dynamic key (an expression, typically a String).
                translate_expr(op.key.as_expression()?, ctx)
            } else {
                let key_str = bindings::property_key_name(&op.key)?.to_string();
                parse_quote!(#key_str.to_string())
            };
            Some(parse_quote!((#key, #value)))
        })
        .collect();
    parse_quote!(::std::collections::HashMap::from([#(#entries),*]))
}

/// `{ kind: "circle", radius: 2 }` → `Shape::Circle { radius: 2.0 }` when `path`
/// is a registered discriminated-union enum and the literal carries a matching
/// `kind` string. Returns `None` for a plain struct literal (no `kind`, or a
/// `kind` whose value isn't a registered variant of this enum).
fn variant_construct(obj: &ObjectExpression, path: &syn::Path, ctx: &Ctx<'_>) -> Option<Expr> {
    let type_name = path.segments.last()?.ident.to_string();
    let kind_value = kind_string(obj)?;
    let shape = ctx.variant(&type_name, &kind_value)?;
    let variant = &shape.name;
    let fields: Vec<syn::FieldValue> = obj
        .properties
        .iter()
        .filter_map(|p| {
            let ObjectPropertyKind::ObjectProperty(op) = p else { return None };
            let key = bindings::property_key_name(&op.key)?;
            // The discriminant is consumed by the variant name, not a field.
            if key == "kind" {
                return None;
            }
            let value = translate_expr(&op.value, ctx);
            Some(parse_quote!(#key: #value))
        })
        .collect();
    Some(parse_quote!(#path::#variant { #(#fields),* }))
}

/// The value of a `kind: "…"` string-literal property, if the object has one.
fn kind_string(obj: &ObjectExpression) -> Option<String> {
    for p in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(op) = p else {
            continue;
        };
        if bindings::property_key_name(&op.key).is_some_and(|k| k == "kind") {
            if let Expression::StringLiteral(s) = &op.value {
                return Some(s.value.to_string());
            }
        }
    }
    None
}
