//! `TSType` → `syn::Type` — the one-to-one mapping table.

use oxc_ast::ast::{
    TSArrayType, TSFunctionType, TSLiteralType, TSTupleElement, TSTupleType, TSType, TSTypeName,
    TSTypeOperator, TSTypeOperatorOperator, TSTypeReference, TSUnionType,
};
use quote::format_ident;
use syn::{parse_quote, Type};

/// Map a TypeScript type annotation to its Rust equivalent as a `syn::Type`.
///
/// Every `TSType` variant is matched explicitly (no `_` wildcard): a future
/// oxc variant lands as a `cargo check` error here rather than silently
/// miscompiling. Variants with a direct Rust shape map to it; variants whose TS
/// semantics have no Rust analogue (`any`/`never`-as-`!` aside, conditional/
/// mapped/infer/intersection/`typeof`/tuple-element type-level computation) fall
/// back to `_`, which surfaces as a `cargo check` error at the use site —
/// honest, not a silent stub.
pub fn translate_type(ty: &TSType) -> Type {
    match ty {
        // Scalar keywords → the matching Rust scalar.
        TSType::TSStringKeyword(_) => parse_quote!(String),
        TSType::TSNumberKeyword(_) => parse_quote!(f64),
        TSType::TSBooleanKeyword(_) => parse_quote!(bool),
        TSType::TSVoidKeyword(_) | TSType::TSUndefinedKeyword(_) => parse_quote!(()),
        TSType::TSNeverKeyword(_) => parse_quote!(!),
        TSType::TSThisType(_) => parse_quote!(Self),
        // Compound types with a direct Rust shape.
        TSType::TSArrayType(arr) => array_type(arr),
        TSType::TSUnionType(u) => union_type(u),
        TSType::TSTypeReference(r) => reference_type(r),
        TSType::TSTypeOperatorType(op) => operator_type(op),
        TSType::TSLiteralType(lit) => literal_type(lit),
        // `(a: number) => string` — a function-type / callback signature maps to
        // the Rust `fn(..) -> ..` pointer (the non-capturing analogue). Parameter
        // names are dropped; rest/optional/`this` params keep the call shape.
        TSType::TSFunctionType(f) => function_type(f),
        // `[number, string]` → `(f64, String)`; `[T, ...T[]]` (NonEmptyArray) →
        // `(T, Vec<T>)` — a leading run of fixed elements then a `Vec` tail.
        TSType::TSTupleType(t) => tuple_type(t),
        // `(T)` — parens are grouping only; the inner type is the shape.
        TSType::TSParenthesizedType(p) => translate_type(&p.type_annotation),
        // `` `Hello ${string}` `` — a template-literal type is a `String` at runtime.
        TSType::TSTemplateLiteralType(_) => parse_quote!(String),
        // `object` (non-primitive) → `serde_json::Value`, the runtime shape of an
        // arbitrary JSON value. The `serde_json::` prefix is the dep marker the
        // translator scans emitted text for, so a file that mentions `object`
        // pulls the dep automatically — no dep collector threaded through types.
        TSType::TSObjectKeyword(_) => parse_quote!(::serde_json::Value),
        // `null` / `symbol` / `bigint` / `unknown` / `any` have no Rust scalar;
        // `_` surfaces the gap as a cargo check error.
        TSType::TSNullKeyword(_)
        | TSType::TSSymbolKeyword(_)
        | TSType::TSBigIntKeyword(_)
        | TSType::TSUnknownKeyword(_)
        | TSType::TSAnyKeyword(_)
        | TSType::TSIntrinsicKeyword(_) => parse_quote!(_),
        // TS type-level computation with no Rust analogue (conditional/mapped/
        // infer/indexed/intersection/`typeof`/predicate/import/named-tuple). `_`
        // surfaces each as a cargo error.
        TSType::TSConditionalType(_)
        | TSType::TSConstructorType(_)
        | TSType::TSImportType(_)
        | TSType::TSIndexedAccessType(_)
        | TSType::TSInferType(_)
        | TSType::TSIntersectionType(_)
        | TSType::TSMappedType(_)
        | TSType::TSNamedTupleMember(_)
        | TSType::TSTypeLiteral(_)
        | TSType::TSTypePredicate(_)
        | TSType::TSTypeQuery(_)
        | TSType::JSDocNullableType(_)
        | TSType::JSDocNonNullableType(_)
        | TSType::JSDocUnknownType(_) => parse_quote!(_),
    }
}

/// A TS literal type (`5` / `"x"` / `true`) → its scalar Rust type, the way the
/// bare literal would infer. `TSLiteral` shares the literal-node variants with
/// `Expression`; a signed-literal unary (`-1`) defaults to `f64`, matching the
/// bare-literal inference in `infer_literal_type`.
fn literal_type(lit: &TSLiteralType) -> Type {
    use oxc_ast::ast::TSLiteral;
    match &lit.literal {
        TSLiteral::BooleanLiteral(_) => parse_quote!(bool),
        TSLiteral::NumericLiteral(_) => parse_quote!(f64),
        TSLiteral::StringLiteral(_) => parse_quote!(String),
        TSLiteral::TemplateLiteral(_) => parse_quote!(String),
        TSLiteral::BigIntLiteral(_) => parse_quote!(_),
        TSLiteral::UnaryExpression(u) => match &u.argument {
            oxc_ast::ast::Expression::NumericLiteral(_) => parse_quote!(f64),
            oxc_ast::ast::Expression::BooleanLiteral(_) => parse_quote!(bool),
            oxc_ast::ast::Expression::StringLiteral(_) => parse_quote!(String),
            _ => parse_quote!(_),
        },
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

/// `(a: number, b: string) => boolean` → `fn(f64, String) -> bool` — a Rust
/// function pointer. A TS function type is a callback signature; `fn(..) -> ..`
/// is the closest non-capturing analogue a field/param can store. Parameter
/// names are dropped, and `void` maps to `()`. Rest/optional/`this` params keep
/// the call shape (their type only — a captured-scope closure would need
/// `Box<dyn Fn>`, out of scope for a pure type lowering).
fn function_type(f: &TSFunctionType) -> Type {
    let params: Vec<Type> = f
        .params
        .items
        .iter()
        .filter_map(|p| {
            p.type_annotation
                .as_ref()
                .map(|ta| translate_type(&ta.type_annotation))
        })
        .collect();
    let ret = translate_type(&f.return_type.type_annotation);
    parse_quote!(fn(#(#params),*) -> #ret)
}

/// `[number, string]` → `(f64, String)`; `[T, ...T[]]` (NonEmptyArray) →
/// `(T, Vec<T>)`. Each fixed element maps through [`translate_type`]; a rest
/// element (`...T[]`) becomes a `Vec<T>` tail; an optional element (`[T?]`)
/// becomes `Option<T>`. A sole rest (`[...T[]]`) is just `Vec<T>` (no tuple
/// wrapper); an empty tuple is `()`.
fn tuple_type(t: &TSTupleType) -> Type {
    let mut elems: Vec<Type> = Vec::new();
    for e in &t.element_types {
        match e {
            TSTupleElement::TSRestType(r) => {
                // `...T[]` / `...ReadonlyArray<T>` — the rest carries an array
                // shape; the tail is `Vec<element>`, not `Vec<Vec<element>>`.
                let inner = rest_element_type(&r.type_annotation);
                elems.push(parse_quote!(Vec<#inner>));
            }
            TSTupleElement::TSOptionalType(o) => {
                let inner = translate_type(&o.type_annotation);
                elems.push(parse_quote!(Option<#inner>));
            }
            other => {
                // An inherited `TSType` variant — reuse `translate_type`.
                let ty = other
                    .as_ts_type()
                    .expect("non-rest/optional tuple element is an inherited TSType");
                elems.push(translate_type(ty));
            }
        }
    }
    if elems.is_empty() {
        return parse_quote!(());
    }
    if elems.len() == 1 {
        return elems.into_iter().next().unwrap();
    }
    parse_quote!((#(#elems),*))
}

/// The element type a tuple rest (`...X`) spreads. TS spells the array shape
/// four ways — `T[]`, `readonly T[]`, `Array<T>`, `ReadonlyArray<T>` — and the
/// rest tail is `Vec<element>`, so each unwraps to its element rather than
/// re-wrapping (which would give `Vec<Vec<element>>`).
fn rest_element_type(ty: &TSType) -> Type {
    match ty {
        TSType::TSArrayType(arr) => translate_type(&arr.element_type),
        TSType::TSTypeOperatorType(op) => rest_element_type(&op.type_annotation),
        TSType::TSTypeReference(r) => {
            if let TSTypeName::IdentifierReference(id) = &r.type_name {
                if matches!(id.name.as_ref(), "ReadonlyArray" | "Array") {
                    if let Some(inner) = r.type_arguments.as_ref().and_then(|a| a.params.first()) {
                        return translate_type(inner);
                    }
                }
            }
            translate_type(ty)
        }
        other => translate_type(other),
    }
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
    // `Array<T>` / `ReadonlyArray<T>` → `Vec<T>`; other named refs pass through.
    if matches!(name, "Array" | "ReadonlyArray") {
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
        // `crate::`-prefixed so the enum resolves at the crate root whether the
        // file is a lone entry (the enum lives in its own crate root) or a
        // project module (the entry emits the enum; modules reference it). A
        // bare name would name a distinct type per module (E0308).
        return parse_quote!(crate::#name);
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
