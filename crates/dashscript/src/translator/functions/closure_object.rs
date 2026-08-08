//! A function returning a single object literal whose properties are typed
//! arrow functions plus captured identifiers — a "closure object factory"
//! (e.g. `bidi { return { to: (k) => …, from: (x) => …, forward, reverse } }`)
//! — lowers to a `struct` + `impl` (each arrow becomes a `&self` method; each
//! captured identifier becomes a field), so the returned object's closure
//! methods map to idiomatic Rust instead of an impossible heterogeneous
//! `HashMap` (closures + maps cannot share one `HashMap<K, V>` value type).
//!
//! Shape match is conservative: every property must be a typed arrow (→
//! method) or a bare identifier (→ captured field), and every type parameter
//! must be `<X extends string>` so `K`/`V` specialize to `String` (a generic
//! `HashMap<K, V>.get(&str)` would need an unprovable `K: Borrow<str>` bound).
//! Other shapes fall through to ordinary translation.

use std::collections::HashMap;

use oxc_ast::ast::{ArrowFunctionExpression, Expression, Function, ObjectPropertyKind, Statement};
use quote::format_ident;
use syn::{parse_quote, Block, FnArg, Ident, ImplItem, Item, ItemFn, Path, ReturnType, Type};

use super::super::bindings;
use super::super::context::{Locals, Narrow};
use super::super::name_table::NameTable;
use super::super::registry::TypeRegistry;
use super::{
    arrow_return_type, body_locals, register_local, translate_body, translate_params,
    translate_stmt,
};

/// The parsed factory shape: fields (→ struct fields) and methods (→ `&self`
/// methods), each paired with its source property key / captured identifier.
pub(in crate::translator) struct FactoryShape<'a> {
    /// `(rust_field_name, captured_ident_name)` — the identifier the arrow
    /// bodies capture becomes a `pub` struct field of the same name.
    fields: Vec<(String, String)>,
    /// `(method_name, arrow)` — each typed arrow becomes a `&self` method.
    methods: Vec<(String, &'a ArrowFunctionExpression<'a>)>,
}

/// The synthesized struct path a closure-object factory returns (e.g. `bidi`
/// → `__DsBidi`), registered as the function's return type so a caller's
/// `OnceLock<_>` infers the concrete struct. `None` when `func` is not a
/// recognized factory shape.
pub(in crate::translator) fn factory_return_type(func: &Function) -> Option<syn::Type> {
    recognize(func).map(|_| {
        let name = struct_name_for(func);
        parse_quote!(#name)
    })
}

/// Recognize the closure-object-factory shape, or `None` to fall through.
pub(in crate::translator) fn recognize<'a>(func: &'a Function<'a>) -> Option<FactoryShape<'a>> {
    let body = func.body.as_deref()?;
    // Body = (declarations…) + a single trailing `return {…}`. The preceding
    // statements stay (they declare the captured locals); only the trailing
    // return is the factory payload.
    let last = body.statements.last()?;
    let return_obj = match last {
        Statement::ReturnStatement(ret) => {
            let arg = ret.argument.as_ref()?;
            let Expression::ObjectExpression(o) = arg else {
                return None;
            };
            o
        }
        _ => return None,
    };
    let mut fields: Vec<(String, String)> = Vec::new();
    let mut methods: Vec<(String, &ArrowFunctionExpression)> = Vec::new();
    for prop in &return_obj.properties {
        let op = match prop {
            ObjectPropertyKind::ObjectProperty(op) => op,
            _ => return None,
        };
        let key = bindings::property_key_name(&op.key)?.to_string();
        match &op.value {
            Expression::ArrowFunctionExpression(arrow) => {
                // Require typed params so the method signature is concrete
                // (an untyped `(k) => …` leaves `k: _`, E0121).
                if arrow
                    .params
                    .items
                    .iter()
                    .any(|p| p.type_annotation.is_none())
                {
                    return None;
                }
                methods.push((key, arrow));
            }
            Expression::Identifier(id) => {
                fields.push((key, bindings::snake(&id.name).to_string()));
            }
            _ => return None,
        }
    }
    if methods.is_empty() || fields.is_empty() {
        return None;
    }
    // Exactly one parameter — the record the factory wraps. The struct field
    // type is derived from it (specialized below), so a multi-param factory
    // would need per-param specialization, which this conservative shape does
    // not attempt.
    if func.params.items.len() != 1 {
        return None;
    }
    // Every type parameter must be `<X extends string>` so it specializes to
    // `String`. Any other constraint (or no constraint) → do not recognize.
    if let Some(tp) = func.type_parameters.as_deref() {
        for p in &tp.params {
            let Some(constraint) = &p.constraint else {
                return None;
            };
            if !matches!(constraint, oxc_ast::ast::TSType::TSStringKeyword(_)) {
                return None;
            }
        }
    }
    Some(FactoryShape { fields, methods })
}

/// Translate a recognized factory into `[struct, impl, fn]` items.
pub(in crate::translator) fn translate<'a>(
    func: &'a Function<'a>,
    shape: &FactoryShape<'a>,
    registry: &TypeRegistry,
    names: &NameTable<'_>,
) -> Vec<Item> {
    let struct_name = struct_name_for(func);

    // Every captured field shares the factory's single record/map type,
    // specialized to `HashMap<String, String>` (the `<X extends string>`
    // constraint guarantees `String`). `bidi`'s `forward` is the param and
    // `reverse = invert_map(forward)` is the same map type inverted, so both
    // resolve to `HashMap<String, String>`.
    let field_ty = factory_field_type(func).unwrap_or_else(|| parse_quote!(::serde_json::Value));

    let struct_fields: Vec<syn::Field> = shape
        .fields
        .iter()
        .map(|(fname, _)| {
            let fident = format_ident!("{}", fname);
            parse_quote!(pub #fident: #field_ty)
        })
        .collect();
    let struct_item: Item = parse_quote! {
        #[derive(Clone)]
        pub struct #struct_name { #(#struct_fields),* }
    };

    let field_paths: HashMap<String, Path> = shape
        .fields
        .iter()
        .filter_map(|(fname, _)| path_of_type(&field_ty).map(|p| (fname.clone(), p)))
        .collect();
    let impl_methods: Vec<ImplItem> = shape
        .methods
        .iter()
        .map(|(mname, arrow)| {
            translate_method(mname, arrow, &shape.fields, &field_paths, registry, names)
        })
        .collect();
    let impl_item: Item = parse_quote! { impl #struct_name { #(#impl_methods)* } };

    let fn_item = translate_factory_fn(func, &struct_name, &field_ty, shape, registry, names);

    vec![struct_item, impl_item, Item::Fn(fn_item)]
}

/// `bidi` → `__DsBidi` (the synthesized struct bears the factory's name so a
/// caller's `OnceLock<_>` infers a concrete type).
fn struct_name_for(func: &Function) -> Ident {
    let base = func
        .id
        .as_ref()
        .map(|id| id.name.as_str())
        .unwrap_or("factory");
    // Capitalize the first ASCII letter; leave the rest so `bidi` → `Bidi`.
    let mut chars = base.chars();
    let cap = chars
        .next()
        .map(|c| c.to_ascii_uppercase())
        .map(|c| c.to_string())
        .unwrap_or_default();
    let rest: String = chars.collect();
    format_ident!("__Ds{}{}", cap, rest)
}

/// The factory's record/map param specialized to `HashMap<String, String>`,
/// or `None` if the first param isn't a `Record<K, V>` (then the factory shape
/// is left unmapped — only the bidi-style record factory lowers here).
fn factory_field_type(func: &Function) -> Option<Type> {
    let first = func.params.items.first()?;
    let ta = first.type_annotation.as_deref()?;
    // `Record<K, V>` — the only factory shape recognized here. A `Record` with
    // `<X extends string>` params specializes to `HashMap<String, String>`.
    if !matches!(
        &ta.type_annotation,
        oxc_ast::ast::TSType::TSTypeReference(ref tr)
            if matches!(&tr.type_name, oxc_ast::ast::TSTypeName::IdentifierReference(id) if id.name == "Record"),
    ) {
        return None;
    }
    Some(parse_quote!(::std::collections::HashMap<String, String>))
}

/// Translate one arrow property into a `pub fn <name>(&self, params) -> ret`
/// whose body is the arrow body preceded by a prelude rebinding each captured
/// field to a local (`let forward = self.forward.clone();`). The body then
/// translates unchanged: the captured identifier is now a typed local
/// `HashMap`, so `forward[key]` routes through the existing `is_hashmap_local`
/// path (`.get(key).cloned()`), not the array-index fallback.
fn translate_method(
    name: &str,
    arrow: &ArrowFunctionExpression,
    fields: &[(String, String)],
    field_paths: &HashMap<String, Path>,
    registry: &TypeRegistry,
    names: &NameTable<'_>,
) -> ImplItem {
    let method_ident = format_ident!("{}", bindings::snake(name));

    let mut locals = Locals::new();
    for fp in &arrow.params.items {
        register_local(
            &mut locals,
            &fp.pattern,
            fp.type_annotation.as_deref(),
            names,
        );
    }
    let mut params: Vec<FnArg> = vec![parse_quote!(&self)];
    params.extend(translate_params(&arrow.params, &locals, registry, names));
    let output: ReturnType = arrow_return_type(arrow.return_type.as_deref());

    // `&self` context: the method body is translated against the captured
    // fields as if they were `self` fields (so `self.forward` is available).
    // The prelude below rebinds each to a local so the arrow body reads a
    // typed `HashMap` (matches the original closure's captured-by-value view).
    // Cloning a small map per call is the YAGNI path; a borrow rewrite can
    // displace it if a hot caller proves the cost.
    let narrow = Narrow::default();

    // Prelude: rebind each captured field to a local. Register the local's
    // type so `forward[key]` is recognized as a `HashMap` index (`.get`)
    // rather than an array index.
    let mut prelude: Vec<syn::Stmt> = Vec::new();
    for (fname, _) in fields {
        let fident = format_ident!("{}", fname);
        prelude.push(parse_quote!(let #fident = self.#fident.clone();));
        if let Some(path) = field_paths.get(fname) {
            locals.insert(fname.clone(), path.clone());
        }
    }

    let body_block = translate_arrow_body(arrow, &mut locals, registry, &narrow, names);
    let mut stmts = prelude;
    stmts.extend(body_block.stmts);
    let block = Block {
        brace_token: Default::default(),
        stmts,
    };

    parse_quote!(pub fn #method_ident(#(#params),*) #output #block)
}

/// An arrow's body → a `Block`: an expression body (`=> expr`) becomes `{ expr }`
/// (trailing expression), a block body translates statement-by-statement.
fn translate_arrow_body(
    arrow: &ArrowFunctionExpression,
    locals: &mut Locals,
    registry: &TypeRegistry,
    narrow: &Narrow,
    names: &NameTable<'_>,
) -> Block {
    let body = &*arrow.body;
    let mut stmts: Vec<syn::Stmt> = Vec::new();
    for s in &body.statements {
        stmts.extend(translate_stmt(s, locals, registry, narrow, None, names));
    }
    // An expression-body arrow stores its value as a single trailing
    // `ExpressionStatement`; `translate_stmt` emits it as a discarded `expr;`,
    // but a method must return it. Convert a trailing `expr;` into the block's
    // trailing expression when the method has a non-`()` return.
    promote_trailing_expr(&mut stmts);
    Block {
        brace_token: Default::default(),
        stmts,
    }
}

/// Turn a trailing `expr;` into a trailing expression (no semicolon) so the
/// block returns it. Only the last statement is touched.
fn promote_trailing_expr(stmts: &mut [syn::Stmt]) {
    let Some(last) = stmts.last_mut() else {
        return;
    };
    if let syn::Stmt::Expr(expr, semi) = last {
        if semi.is_some() {
            *last = syn::Stmt::Expr(std::mem::replace(expr, parse_quote!(())), None);
        }
    }
}

/// Translate the factory function itself: the body's non-return statements
/// translate normally, and the trailing `return { … }` becomes
/// `return <Struct> { field, … }` (only the captured-identifier properties —
/// the arrow properties are now methods, not fields).
fn translate_factory_fn(
    func: &Function,
    struct_name: &Ident,
    field_ty: &Type,
    shape: &FactoryShape<'_>,
    registry: &TypeRegistry,
    names: &NameTable<'_>,
) -> ItemFn {
    let fn_name = func
        .id
        .as_ref()
        .map_or_else(|| format_ident!("__ds_main"), |id| names.of_binding(id));
    let mut locals = body_locals(&func.params, func.body.as_deref(), registry, names);
    // Specialize the generic record param: `Record<K, V>` (K/V extend string)
    // → `HashMap<String, String>` (the struct field type). The body sees a
    // concrete type, so `invert_map(forward)` and the struct init both type-check
    // against the specialized map rather than the unbound `K`/`V`.
    let inputs: Vec<FnArg> = func
        .params
        .items
        .iter()
        .map(|fp| {
            let pat = names.of_pattern(&fp.pattern);
            parse_quote!(#pat : #field_ty)
        })
        .collect();
    if let Some(fp) = func.params.items.first() {
        let name = names.of_pattern(&fp.pattern).to_string();
        if let Some(path) = path_of_type(field_ty) {
            locals.insert(name, path);
        }
    }
    let output: ReturnType = parse_quote!(-> #struct_name);

    let body_stmts: &[Statement] = func.body.as_deref().map_or(&[], |b| &b.statements[..]);
    // All statements except the trailing factory `return {…}` translate
    // normally; the trailing return is rebuilt as a struct construction.
    let (pre, tail) = body_stmts.split_at(body_stmts.len().saturating_sub(1));
    let mut block = translate_body(pre, &mut locals, registry, &Narrow::default(), None, names);
    if let Some(Statement::ReturnStatement(_)) = tail.first() {
        let init = struct_construction(struct_name, shape);
        block.stmts.push(parse_quote!(return #init;));
    }

    parse_quote! {
        fn #fn_name(#(#inputs),*) #output #block
    }
}

/// `<Struct> { field, field, … }` — shorthand init from each captured
/// identifier (the field name equals the captured local's name).
fn struct_construction(struct_name: &Ident, shape: &FactoryShape<'_>) -> syn::Expr {
    let fields: Vec<Ident> = shape
        .fields
        .iter()
        .map(|(fname, _)| format_ident!("{}", fname))
        .collect();
    parse_quote!(#struct_name { #(#fields),* })
}

/// `Type` → its `Path`, when the type is a path (used to register a captured
/// field's type so its index routes through the `HashMap` path).
fn path_of_type(ty: &Type) -> Option<Path> {
    match ty {
        Type::Path(tp) => Some(tp.path.clone()),
        _ => None,
    }
}
