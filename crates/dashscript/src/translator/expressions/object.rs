//! Object literals: `Point { x: 1 }` → struct init, `Record` → `HashMap`,
//! `{ kind: "…" }` → discriminated-union variant.

use std::collections::HashSet;

use oxc_ast::ast::{Expression, ObjectExpression, ObjectPropertyKind, PropertyKey};
use syn::{parse_quote, Expr, GenericArgument, Ident};

use super::super::bindings;
use super::super::context::Ctx;
use super::super::types;
use super::is_hashmap;
use super::{array_elem_expr, translate_expr};

/// `Point { x: 1 }` — needs the target type's name from the binding annotation.
/// A `{ kind: "circle", … }` literal whose target is a registered
/// discriminated union instead builds a variant (`Shape::Circle { … }`).
pub(super) fn object_expr(
    obj: &ObjectExpression,
    ty_hint: Option<&syn::Type>,
    ctx: &Ctx<'_>,
) -> Expr {
    let Some(path) = ty_hint.and_then(types::type_path) else {
        // No type hint: an anonymous object literal lowers to a `HashMap` (JS
        // objects are dynamic maps). A binding infers `HashMap<String, V>` from
        // its values via `infer_literal_type`, so its field accesses route
        // through `is_hashmap_local`; a literal with no binding context still
        // produces a usable `HashMap` rather than `todo!()`.
        return hashmap_literal(obj, None, ctx);
    };
    // `Record<K, V>` (a `HashMap`) → `HashMap::from([(key, value), …])`. A
    // value type that is a scalar-union enum boxes each literal into its variant.
    if is_hashmap(path) {
        let val_ty = hashmap_value_path(path);
        return hashmap_literal(obj, val_ty.as_ref(), ctx);
    }
    if let Some(expr) = variant_construct(obj, path, ctx) {
        return expr;
    }
    // A `…v` spread records a struct-update base (`Struct { …, ..v }`); only an
    // identifier base is supported. If multiple spreads appear, the last wins.
    let optionals = optional_fields_for(path, ctx);
    // A struct-literal expression cannot carry generic arguments — `Foo<T> { .. }`
    // parses as `Foo < T > { .. }` (comparison). Use a bare path; the literal's
    // type is inferred from context, so the args are redundant in the expression.
    let bare_path = strip_generic_args(path);
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
                let mut value = array_elem_expr(&op.value, ctx);
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
        Some(b) => {
            if fields.is_empty() {
                parse_quote!(#bare_path { ..#b })
            } else {
                parse_quote!(#bare_path { #(#fields),*, ..#b })
            }
        }
        None => {
            let extras = missing_optionals(path, &fields, ctx);
            // A single repetition avoids a dangling comma when both `fields`
            // and `extras` are empty — `Element { , }` would not parse.
            let mut all = fields;
            all.extend(extras);
            parse_quote!(#bare_path { #(#all),* })
        }
    }
}

/// A copy of `path` with every segment's generic arguments stripped. A
/// struct-literal expression cannot carry generic args (`Foo<T> { .. }` parses
/// as comparison), so the literal uses this bare path while the full path still
/// feeds the HashMap/variant checks above.
fn strip_generic_args(path: &syn::Path) -> syn::Path {
    let mut bare = path.clone();
    for seg in &mut bare.segments {
        seg.arguments = syn::PathArguments::None;
    }
    bare
}

/// The optional (`?:`) field names of the struct named by `path`, if any.
fn optional_fields_for<'a>(path: &syn::Path, ctx: &Ctx<'a>) -> Option<&'a HashSet<String>> {
    let type_name = path.segments.last()?.ident.to_string();
    ctx.struct_optionals(&type_name)
}

/// `None` initializers for optional (`?:`) fields the literal omitted, so a
/// partial struct literal still names every field. Only fields registered as
/// optional on this struct type and absent from `present` are filled.
fn missing_optionals(
    path: &syn::Path,
    present: &[syn::FieldValue],
    ctx: &Ctx<'_>,
) -> Vec<syn::FieldValue> {
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
            // `name` is the stored snake-case field name; for a Rust keyword it
            // is a raw-ident string (`r#type`). Strip the raw marker and
            // re-derive via `snake` so the keyword is re-raw'd correctly.
            let stripped = name.strip_prefix("r#").unwrap_or(name.as_str());
            let id = bindings::snake(stripped);
            parse_quote!(#id: None)
        })
        .collect()
}

/// `{ a: 1, b: 2 }` as a `HashMap` → `HashMap::from([("a".to_string(), 1_f64), …])`.
/// Keys are the `.ts` property names, owned so the map outlives the literal.
/// When `val_ty` is a registered scalar-union enum, each literal value is boxed
/// into its variant so the map matches a `HashMap<K, Enum>` parameter type.
fn hashmap_literal(obj: &ObjectExpression, val_ty: Option<&syn::Path>, ctx: &Ctx<'_>) -> Expr {
    // A `Record<K, Union>` whose value type is a registered scalar-union enum
    // boxes each literal value into its variant (`{id: 1}` → `Enum::Num(1.0)`);
    // any other value type leaves the value unboxed (the common `HashMap<K, V>`
    // case).
    let union_ident = val_ty
        .and_then(|p| p.segments.last())
        .map(|s| s.ident.clone())
        .filter(|id| ctx.registry().union_enums.contains_key(id));
    let entries: Vec<Expr> = obj
        .properties
        .iter()
        .filter_map(|p| {
            let ObjectPropertyKind::ObjectProperty(op) = p else {
                return None;
            };
            let value = match &union_ident {
                Some(e) => box_union_value(&op.value, e, ctx),
                None => array_elem_expr(&op.value, ctx),
            };
            let key = if op.computed {
                // `[k]: v` — a dynamic key (an expression, typically a String).
                translate_expr(op.key.as_expression()?, ctx)
            } else {
                // A string-literal key (`"&amp;": "&"`) keeps its literal value
                // verbatim — `property_key_name` is for identifier keys only.
                match &op.key {
                    PropertyKey::StringLiteral(s) => {
                        let v = s.value.to_string();
                        parse_quote!(#v.to_string())
                    }
                    _ => {
                        let key_str = bindings::property_key_name(&op.key)?.to_string();
                        parse_quote!(#key_str.to_string())
                    }
                }
            };
            Some(parse_quote!((#key, #value)))
        })
        .collect();
    parse_quote!(::std::collections::HashMap::from([#(#entries),*]))
}

/// The value-type path of a `HashMap<K, V>` (the 2nd type parameter), so a
/// `Record<K, Union>` literal can box its values into the union enum. `None`
/// for a non-`HashMap` or a value type that isn't a plain path.
fn hashmap_value_path(path: &syn::Path) -> Option<syn::Path> {
    let seg = path.segments.last()?;
    if seg.ident != "HashMap" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(args) = &seg.arguments else {
        return None;
    };
    let mut it = args.args.iter().filter_map(|g| match g {
        GenericArgument::Type(t) => types::type_path(t).cloned(),
        _ => None,
    });
    let _key = it.next();
    it.next()
}

/// Box a literal value into the matching variant of a scalar-union enum:
/// `"foo"` → `Enum::Str("foo".to_string())`, `1` → `Enum::Num(1.0)`, `true` →
/// `Enum::Bool(true)`, `undefined` → `Enum::Undef`, `null` → `Enum::Null`. A
/// non-literal value (a variable, a call) is left as-is — boxing it needs a
/// runtime discriminant and is out of scope; cargo check surfaces the mismatch.
fn box_union_value(value: &Expression, enum_ident: &Ident, ctx: &Ctx<'_>) -> Expr {
    match value {
        Expression::StringLiteral(s) => {
            let v = s.value.to_string();
            parse_quote!(crate::#enum_ident::Str(#v.to_string()))
        }
        Expression::NumericLiteral(n) => {
            let v = super::literals::numeric_expr(n.value);
            parse_quote!(crate::#enum_ident::Num(#v))
        }
        Expression::BooleanLiteral(b) => {
            let v = b.value;
            parse_quote!(crate::#enum_ident::Bool(#v))
        }
        Expression::Identifier(id) if id.name.as_str() == "undefined" => {
            parse_quote!(crate::#enum_ident::Undef)
        }
        Expression::NullLiteral(_) => parse_quote!(crate::#enum_ident::Null),
        _ => super::translate_expr(value, ctx),
    }
}

/// `{ kind: "circle", radius: 2 }` → `Shape::Circle { radius: 2_f64 }` when `path`
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
            let ObjectPropertyKind::ObjectProperty(op) = p else {
                return None;
            };
            let key = bindings::property_key_name(&op.key)?;
            // The discriminant is consumed by the variant name, not a field.
            if key == "kind" {
                return None;
            }
            let value = array_elem_expr(&op.value, ctx);
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
