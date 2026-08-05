//! `TSType` → `syn::Type` — the one-to-one mapping table.

use oxc_ast::ast::{
    TSArrayType, TSFunctionType, TSLiteralType, TSTupleElement, TSTupleType, TSType, TSTypeName,
    TSTypeOperator, TSTypeOperatorOperator, TSTypeQueryExprName, TSTypeReference, TSUnionType,
};
use quote::format_ident;
use syn::{
    parse_quote,
    visit_mut::{self, VisitMut},
    Type,
};

use super::registry::TypeRegistry;

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
        // `{ x: number; y: string }` — an anonymous object-literal type lowers
        // to a synthetic `__DsAnon_<hash>` struct (one definition per unique
        // shape, emitted by the registry pre-pass; the crate-root definition is
        // shared across modules so a `crate::`-prefixed reference resolves
        // everywhere). A literal with a sole index signature (`{ [k: string]: T
        // }`) is a map, not a struct — lowers to `HashMap<String, T>`. Anything
        // else with no Rust analogue falls back to `_`.
        TSType::TSTypeLiteral(lit) => match super::declarations::anon_struct_for_literal(lit) {
            Some((name, _)) => parse_quote!(crate::#name),
            None => match super::declarations::index_signature_type(lit) {
                Some(ty) => ty,
                None => parse_quote!(_),
            },
        },
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
        | TSType::TSTypePredicate(_)
        | TSType::TSTypeQuery(_)
        | TSType::JSDocNullableType(_)
        | TSType::JSDocNonNullableType(_)
        | TSType::JSDocUnknownType(_) => parse_quote!(_),
    }
}

/// True when a `TSType` (recursively) has no static Rust lowering — the same
/// variants [`translate_type`] falls back to `_` for (`unknown`/`any`/indexed
/// access/conditional/mapped/infer/intersection/`typeof`-query/…). A function
/// whose signature mentions one cannot be statically typed (a param or return
/// would carry `_`, which cargo check rejects in a signature), so it degrades
/// to the engine — see [`super::classify::classify_function_signature`].
///
/// Compound types recurse into their elements/arguments/members (so a
/// `Record<string, unknown>` flags the `unknown` in its argument, and a union
/// with one untypable member flags the whole union). A scalar or a
/// template-literal type (which lowers to `String`) is expressible.
///
/// Keep in sync with the `parse_quote!(_)` arms of [`translate_type`]: a
/// variant that lowers to `_` must read unmappable here, and vice versa.
pub(in crate::translator) fn type_has_unmappable(ty: &TSType) -> bool {
    use oxc_ast::ast::{TSLiteral, TSSignature};
    match ty {
        // The unmappable scalars and type-level computation — same set as
        // translate_type's `_` arms.
        TSType::TSNullKeyword(_)
        | TSType::TSSymbolKeyword(_)
        | TSType::TSBigIntKeyword(_)
        | TSType::TSUnknownKeyword(_)
        | TSType::TSAnyKeyword(_)
        | TSType::TSIntrinsicKeyword(_)
        | TSType::TSConditionalType(_)
        | TSType::TSConstructorType(_)
        | TSType::TSImportType(_)
        | TSType::TSIndexedAccessType(_)
        | TSType::TSInferType(_)
        | TSType::TSIntersectionType(_)
        | TSType::TSMappedType(_)
        | TSType::TSNamedTupleMember(_)
        | TSType::TSTypePredicate(_)
        | TSType::TSTypeQuery(_)
        | TSType::JSDocNullableType(_)
        | TSType::JSDocNonNullableType(_)
        | TSType::JSDocUnknownType(_) => true,
        // Compound types recurse into their children.
        TSType::TSArrayType(a) => type_has_unmappable(&a.element_type),
        TSType::TSUnionType(u) => u.types.iter().any(|t| {
            // `null`/`undefined` are nullable markers inside a union
            // (`string | null` → `Option<String>`), not unmappable types — only
            // a bare `null`/`undefined` type outside a union is (handled by the
            // scalar arm above). Skip them so a nullable union stays
            // expressible and is not needlessly degraded.
            !matches!(t, TSType::TSNullKeyword(_) | TSType::TSUndefinedKeyword(_))
                && type_has_unmappable(t)
        }),
        TSType::TSTypeOperatorType(op) => type_has_unmappable(&op.type_annotation),
        TSType::TSParenthesizedType(p) => type_has_unmappable(&p.type_annotation),
        // A named reference (`Record`, `Array`, a user type) carries an
        // untypable shape in its type arguments (e.g. `Record<string,
        // unknown>`) — recurse the arguments, not the bare name.
        TSType::TSTypeReference(r) => {
            // `ReturnType<typeof fn>` resolves to the named function's return
            // type in a signature position (translate_type_for_signature), so
            // treat it as expressible here — recursing its `typeof` argument
            // would flag a `TSTypeQuery` and needlessly degrade the function.
            // (A ReturnType whose target is unknown still lowers to `_` and
            // surfaces as a cargo error at the use site.)
            if let TSTypeName::IdentifierReference(id) = &r.type_name {
                if id.name.as_ref() == "ReturnType" {
                    return false;
                }
            }
            r.type_arguments
                .as_ref()
                .is_some_and(|a| a.params.iter().any(type_has_unmappable))
        }
        TSType::TSFunctionType(f) => {
            f.params.items.iter().any(|p| {
                p.type_annotation
                    .as_ref()
                    .is_some_and(|ta| type_has_unmappable(&ta.type_annotation))
            }) || type_has_unmappable(&f.return_type.type_annotation)
        }
        TSType::TSTupleType(t) => t.element_types.iter().any(|e| match e {
            TSTupleElement::TSRestType(r) => type_has_unmappable(&r.type_annotation),
            TSTupleElement::TSOptionalType(o) => type_has_unmappable(&o.type_annotation),
            other => other.as_ts_type().is_some_and(type_has_unmappable),
        }),
        TSType::TSTypeLiteral(lit) => lit.members.iter().any(|m| match m {
            TSSignature::TSIndexSignature(idx) => {
                type_has_unmappable(&idx.type_annotation.type_annotation)
            }
            TSSignature::TSPropertySignature(prop) => prop
                .type_annotation
                .as_ref()
                .is_some_and(|ta| type_has_unmappable(&ta.type_annotation)),
            _ => false,
        }),
        // `123n` — a BigInt literal type lowers to `_`.
        TSType::TSLiteralType(lit) => matches!(&lit.literal, TSLiteral::BigIntLiteral(_)),
        // Expressible scalars and the template-literal type.
        TSType::TSStringKeyword(_)
        | TSType::TSNumberKeyword(_)
        | TSType::TSBooleanKeyword(_)
        | TSType::TSVoidKeyword(_)
        | TSType::TSUndefinedKeyword(_)
        | TSType::TSNeverKeyword(_)
        | TSType::TSThisType(_)
        | TSType::TSObjectKeyword(_)
        | TSType::TSTemplateLiteralType(_) => false,
    }
}

/// Map a type for a degraded (engine) function's signature. A type the static
/// translator cannot express — anything [`type_has_unmappable`] flags — becomes
/// `serde_json::Value`, the universal marshal type, so the engine-fallback
/// signature is concrete rather than `_` (cargo check rejects `_` in a
/// signature). An expressible type maps through [`translate_type`] unchanged, so
/// a degraded function mixing expressible and untypable params keeps the
/// expressible ones concrete.
pub fn translate_type_degraded(ty: &TSType) -> Type {
    if type_has_unmappable(ty) {
        return parse_quote!(::serde_json::Value);
    }
    translate_type(ty)
}

/// Like [`translate_type_degraded`], but for a signature position — resolve a
/// `ReturnType<typeof fn>` utility type from the registry first (the way
/// [`translate_type_for_signature`] does), so a degraded function whose param
/// or return type is `ReturnType<typeof normalizeOptions>` keeps the concrete
/// resolved type rather than emitting an unresolved `ReturnType` reference.
pub fn translate_type_degraded_for_signature(ty: &TSType, registry: &TypeRegistry) -> Type {
    if let Some(resolved) = return_type_of_query(ty, registry) {
        return resolved;
    }
    translate_type_degraded(ty)
}

/// Replace every `_` (`Type::Infer`) leaf in a type with `serde_json::Value`,
/// preserving the surrounding structure — `Vec<HashMap<String, _>>` becomes
/// `Vec<HashMap<String, serde_json::Value>>`, not a flat `Value`. Used by
/// [`translate_type_for_data`] for data-position types where `_` is illegal.
struct InferToValue;
impl visit_mut::VisitMut for InferToValue {
    fn visit_type_mut(&mut self, ty: &mut Type) {
        if let Type::Infer(_) = ty {
            *ty = parse_quote!(::serde_json::Value);
        }
        visit_mut::visit_type_mut(self, ty);
    }
}

/// Map a `TSType` for a **data position** — a struct field, an enum variant
/// field, or a `type` alias body — where a `_` placeholder is illegal (cargo
/// rejects `_` in an item signature with E0121). [`translate_type`] first, then
/// every `_` leaf (from an unmappable `unknown`/`any`/conditional/… type)
/// becomes the universal marshal type. Local-variable positions (`let x: _`)
/// keep `_` so inference still works — this is the data-position overlay, not
/// the default.
pub fn translate_type_for_data(ty: &TSType) -> Type {
    let mut t = translate_type(ty);
    InferToValue.visit_type_mut(&mut t);
    t
}

/// Whether `ident` appears as a path segment anywhere in `ty`. Used to drop a
/// `type` alias generic param the translated body never references — a generic
/// alias whose body lowered to `serde_json::Value` (an unmappable
/// conditional/utility type, e.g. `type NonNullable<T> = T extends … ? … : T`)
/// would otherwise carry an unused param (E0392).
pub fn type_uses_ident(ty: &Type, ident: &str) -> bool {
    struct Uses<'a>(&'a str, bool);
    impl VisitMut for Uses<'_> {
        fn visit_path_mut(&mut self, p: &mut syn::Path) {
            if p.segments.iter().any(|s| s.ident == self.0) {
                self.1 = true;
            }
            visit_mut::visit_path_mut(self, p);
        }
    }
    let mut clone = ty.clone();
    let mut u = Uses(ident, false);
    u.visit_type_mut(&mut clone);
    u.1
}

/// Translate a type that appears in a function signature (a parameter or
/// return type), resolving the `ReturnType<typeof fn>` utility type to the
/// named function's declared return type. Other types map through
/// [`translate_type`] unchanged — a thin overlay so signature positions get
/// `ReturnType` resolution without threading the registry through every
/// `translate_type` call site. A `ReturnType` whose argument is not
/// `typeof <identifier>`, or whose function is unknown or unannotated, falls
/// back to `_` (the way an unannotated return would).
pub fn translate_type_for_signature(ty: &TSType, registry: &TypeRegistry) -> Type {
    if let Some(resolved) = return_type_of_query(ty, registry) {
        return resolved;
    }
    translate_type(ty)
}

/// `ReturnType<typeof normalizeOptions>` → the return-type path of
/// `normalizeOptions` from the registry, as a Rust type. `None` for any other
/// shape (the caller falls back to [`translate_type`]).
fn return_type_of_query(ty: &TSType, registry: &TypeRegistry) -> Option<Type> {
    let TSType::TSTypeReference(r) = ty else {
        return None;
    };
    let TSTypeName::IdentifierReference(id) = &r.type_name else {
        return None;
    };
    if id.name.as_ref() != "ReturnType" {
        return None;
    }
    let arg = r.type_arguments.as_ref()?.params.first()?;
    let TSType::TSTypeQuery(q) = arg else {
        return None;
    };
    let TSTypeQueryExprName::IdentifierReference(fn_id) = &q.expr_name else {
        return None;
    };
    let ret_path = registry.function_returns.get(fn_id.name.as_ref())?;
    Some(
        ret_path
            .clone()
            .map_or_else(|| parse_quote!(_), |p| parse_quote!(#p)),
    )
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

/// If `ty` is `Promise<T>`, return the inner `T`; else `ty` unchanged. Used at
/// an `async fn`'s return-type position: an ES `async function f(): Promise<T>`
/// maps to a Rust `async fn f() -> T` (the async fn wraps the return in
/// `Future<Output = T>` itself), so the `Promise<>` wrapper is unwrapped there
/// only — a `let x: Promise<T>` binding keeps the wrapper.
pub fn unwrap_promise<'a>(ty: &'a TSType<'a>) -> &'a TSType<'a> {
    if let TSType::TSTypeReference(r) = ty {
        if let TSTypeName::IdentifierReference(id) = &r.type_name {
            if id.name.as_str() == "Promise" {
                if let Some(args) = r.type_arguments.as_ref() {
                    if let Some(inner) = args.params.first() {
                        return inner;
                    }
                }
            }
        }
    }
    ty
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
    // An ES TypedArray name → `Vec<elem>` (`Int8Array`→`Vec<i8>`, …,
    // `Float64Array`→`Vec<f64>`); `ArrayBuffer` → `Vec<u8>` (a raw byte buffer,
    // no element type). A `sha1(): Uint8Array` return or a `bytesToHex(buf:
    // Uint8Array)` param thus marshals as a Rust vec of the element type,
    // matching the constructor (`new Int32Array(n)` → `Vec<i32>`).
    if name == "ArrayBuffer" {
        return parse_quote!(Vec<u8>);
    }
    if let Some(elem) = super::expressions::typed_array_elem_type(name) {
        let ty = format_ident!("{}", elem);
        return parse_quote!(Vec<#ty>);
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
                // A `number` key (`f64`) wraps in `DsF64Key` (SameValueZero
                // Eq+Hash — f64 lacks both); the value type is unaffected.
                if is_f64_type(&k_ty) {
                    return parse_quote!(::std::collections::HashMap<crate::__ds::DsF64Key, #v_ty>);
                }
                return parse_quote!(::std::collections::HashMap<#k_ty, #v_ty>);
            }
        }
    }
    // `Set<T>` → `HashSet<T>` (the ES `Set`). A `number` element (`f64`) wraps
    // in `DsF64Key` — f64 lacks Eq/Hash, so `Set<number>` is a
    // `HashSet<DsF64Key>` keyed by SameValueZero.
    if name == "Set" {
        if let Some(inner) = r.type_arguments.as_ref().and_then(|a| a.params.first()) {
            let inner_ty = translate_type(inner);
            if is_f64_type(&inner_ty) {
                return parse_quote!(::std::collections::HashSet<crate::__ds::DsF64Key>);
            }
            return parse_quote!(::std::collections::HashSet<#inner_ty>);
        }
    }
    // `Promise<T>` → `crate::__ds::DsPromise<T>` — the value layer's
    // `ds_promise_resolve`/`new Promise(…)`/`.then` already emit `DsPromise<T>`
    // (the `Future<Output = T>` alias in `DS_PROMISE_HELPER`), so a `let p:
    // Promise<T>` annotation (or a non-async fn returning `Promise<T>`) must
    // agree, or it surfaces as E0425 cannot find type `Promise`. An `async fn …
    // : Promise<T>` strips the wrapper at the return-type position
    // ([`unwrap_promise`]) since the async fn wraps the return in
    // `Future<Output = T>` itself; this branch is the plain annotation path.
    if name == "Promise" {
        if let Some(inner) = r.type_arguments.as_ref().and_then(|a| a.params.first()) {
            let inner_ty = translate_type(inner);
            return parse_quote!(crate::__ds::DsPromise<#inner_ty>);
        }
    }
    // A WinterTC Web API global constructor used as a type annotation
    // (`const p: URLPattern = …`, `const u: URL = …`, `… : URLSearchParams`)
    // → its `__ds::Ds*` wrapper — the same type the `new` lowering builds, so
    // the annotation and the constructor agree. Otherwise the bare `URLPattern`
    // / `URL` name emits as an unresolved Rust type (E0433) and the fixture
    // falls to `partial`.
    if let Some(ty) = super::builtins::url_ctor_type(name) {
        return ty;
    }
    if let Some(ty) = super::builtins::urlpattern_ctor_type(name) {
        return ty;
    }
    // A named reference with type arguments (`Packer<TFile>`, `Promise<T>`)
    // keeps its arguments — a generic return type is what cross-file singleton
    // inference instantiates from. Readonly/Array/Record/Map/Set above already
    // unwrapped their argument; any other generic ref passes through with its
    // type arguments intact (previously dropped, which lost the generic shape).
    // `type_has_unmappable` rejects a ref whose args contain an unmappable type
    // (it degrades the function to the engine), so every arg reaching here is
    // mappable and safe to recurse on.
    let ident = format_ident!("{}", name);
    if let Some(args) = r.type_arguments.as_ref() {
        let arg_types: Vec<Type> = args.params.iter().map(translate_type).collect();
        parse_quote!(#ident<#(#arg_types),*>)
    } else {
        parse_quote!(#ident)
    }
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
    // A mixed union (`boolean | string[]`, `string | { … }`) — each member
    // independently lowers to a variant. Same `crate::` prefix rationale.
    if let Some((name, _, _)) = super::declarations::inline_mixed_union_enum(u) {
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

/// True when `ty` is `f64` (the ES `number` lowering) — a `Set<T>`/`Map<K, _>`
/// element/key of `f64` wraps in `DsF64Key` (f64 lacks Eq/Hash, so it cannot
/// back a `HashSet`/`HashMap` directly).
pub(in crate::translator) fn is_f64_type(ty: &Type) -> bool {
    type_path(ty).is_some_and(|p| p.is_ident("f64"))
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
