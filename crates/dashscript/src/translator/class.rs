//! `class` → `#[derive(Clone)] struct Name { ... } impl Name { ... }`.
//!
//! A class becomes a `struct` plus an `impl`: instance fields → `pub` struct
//! fields; a `new` constructor fills them (from `this.f = …` assignments in the
//! constructor body, then field default initializers); instance methods become
//! `pub fn method(&self | &mut self)`. `this` → `self` (method) / `__ds_self`
//! (constructor).
use std::collections::{HashMap, HashSet};

use oxc_ast::ast::{
    AssignmentTarget, Class, ClassElement, Expression, Function, MethodDefinition,
    MethodDefinitionKind, NewExpression, PropertyDefinition, PropertyKey, Statement, TSType,
};
use oxc_syntax::operator::AssignmentOperator;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{parse_quote, Expr, FnArg, Ident, Item, Path, ReturnType, Stmt, Type};

use super::bindings;
use super::context::{Ctx, Locals, Narrow};
use super::functions::{
    register_local, return_path_of, translate_body, translate_params, translate_stmt,
};
use super::name_table::NameTable;
use super::registry::TypeRegistry;
use super::{expressions, types};

/// A field: name, type, optional default initializer expression.
struct Field {
    name: Ident,
    ty: Type,
    default: Option<Expr>,
}

/// A class's instance fields as a `HashMap` keyed by snake-cased Rust field name
/// → translated type, the form [`Narrow::in_method`] consumes so a
/// `this.<field>` receiver resolves its type inside a method/constructor body.
fn self_field_map(fields: &[Field]) -> HashMap<String, Type> {
    fields
        .iter()
        .map(|f| (f.name.to_string(), f.ty.clone()))
        .collect()
}

/// Translate a `class` declaration into its `struct` plus `impl` items.
pub(in crate::translator) fn translate_class(
    class: &Class,
    registry: &TypeRegistry,
    names: &NameTable<'_>,
) -> Vec<Item> {
    let Some(id) = class.id.as_ref() else {
        return vec![compile_error_item(
            "DashScript does not support class expressions — declare a named class",
        )];
    };
    let name = bindings::type_ident(&id.name);

    let mut diags: Vec<Item> = Vec::new();
    // Class-level unsupported features.
    if class.super_class.is_some() {
        diags.push(compile_error_item(
            "DashScript does not support class inheritance (extends/super) — use composition",
        ));
    }
    if class.r#abstract {
        diags.push(compile_error_item(
            "DashScript does not support `abstract` classes",
        ));
    }
    if class.declare {
        diags.push(compile_error_item(
            "DashScript does not support `declare` classes",
        ));
    }
    if !class.decorators.is_empty() {
        diags.push(compile_error_item(
            "DashScript does not support class decorators",
        ));
    }

    let mut fields: Vec<Field> = Vec::new();
    let mut ctor: Option<&MethodDefinition> = None;
    let mut methods: Vec<&MethodDefinition> = Vec::new();
    for elem in &class.body.body {
        match elem {
            ClassElement::PropertyDefinition(pd) => {
                if pd.r#static {
                    diags.push(compile_error_item(
                        "DashScript does not support `static` class fields",
                    ));
                } else if pd.computed {
                    diags.push(compile_error_item(
                        "DashScript does not support computed property names in classes",
                    ));
                } else if is_private_member(&pd.key) {
                    diags.push(compile_error_item(
                        "DashScript does not support `#private` class fields (a TS \
                         `private`/`protected` modifier lowers as `pub`)",
                    ));
                } else if let Some(f) = instance_field(pd, registry, names) {
                    fields.push(f);
                }
            }
            ClassElement::MethodDefinition(md) => {
                if md.r#static {
                    diags.push(compile_error_item(
                        "DashScript does not support `static` class methods",
                    ));
                } else if md.computed {
                    diags.push(compile_error_item(
                        "DashScript does not support computed method names in classes",
                    ));
                } else if is_private_member(&md.key) {
                    diags.push(compile_error_item(
                        "DashScript does not support `#private` class methods (a TS \
                         `private`/`protected` modifier lowers as `pub`)",
                    ));
                } else {
                    match md.kind {
                        MethodDefinitionKind::Constructor => ctor = Some(md),
                        // A `get` accessor has no Rust property analogue, so it lowers
                        // as a zero-arg method: `get array()` → `pub fn array(&self)`.
                        // A read `obj.array` rewrites to `obj.array()` at the call site
                        // (registry-tracked, see `expressions/member.rs`); a lone file
                        // that only defines a getter compiles without that rewrite.
                        MethodDefinitionKind::Method | MethodDefinitionKind::Get => {
                            methods.push(md)
                        }
                        MethodDefinitionKind::Set => diags.push(compile_error_item(
                            "DashScript does not support `set` accessors — use a method",
                        )),
                    }
                }
            }
            ClassElement::StaticBlock(_) => {
                diags.push(compile_error_item(
                    "DashScript does not support `static` blocks",
                ));
            }
            ClassElement::AccessorProperty(_) => {
                diags.push(compile_error_item(
                    "DashScript does not support `accessor` properties",
                ));
            }
            ClassElement::TSIndexSignature(_) => {} // type-level; no runtime effect
        }
    }

    // A class type parameter (`class C<T extends …>`) lowers to a Rust generic
    // on both the struct and the impl. `T extends X` carries no Rust trait bound
    // (X is a struct, not a trait), but the class derives `Clone` and its
    // methods clone field values, so `T: Clone` is the bound the lowered Rust
    // needs — added once on the impl, not on the struct or per method.
    let type_params: Vec<Ident> = class
        .type_parameters
        .as_deref()
        .map_or_else(Vec::new, |tp| {
            tp.params
                .iter()
                .map(|p| bindings::type_ident(&p.name.name))
                .collect()
        });

    let struct_item = build_struct(&name, &fields, &type_params);
    let ctor_item = build_constructor(ctor, &fields, &name, &type_params, registry, names);
    let method_items: Vec<syn::ImplItem> = methods
        .iter()
        .map(|md| build_method(md, &fields, registry, names))
        .collect();
    let impl_item: Item = if type_params.is_empty() {
        parse_quote! {
            impl #name {
                #ctor_item
                #(#method_items)*
            }
        }
    } else {
        parse_quote! {
            impl<#(#type_params: Clone),*> #name<#(#type_params),*> {
                #ctor_item
                #(#method_items)*
            }
        }
    };

    let mut result = vec![struct_item, impl_item];
    result.extend(diags);
    result
}

/// Whether a class member is a `#private` identifier — the only private form
/// with no Rust analogue (no runtime name mangling). A TS `private`/`protected`
/// modifier is access control only, and Rust struct fields / impl methods are
/// all `pub`, so it lowers as a normal member.
fn is_private_member(key: &PropertyKey) -> bool {
    matches!(key, PropertyKey::PrivateIdentifier(_))
}

/// `#[derive(Clone)] struct Name<P, …> { pub field: ty, … }` — generic over
/// the class's type parameters (if any); no bound on the struct itself (the
/// bound lives on the impl).
fn build_struct(name: &Ident, fields: &[Field], type_params: &[Ident]) -> Item {
    let field_lines: Vec<TokenStream> = fields
        .iter()
        .map(|f| {
            let n = &f.name;
            let t = &f.ty;
            quote!(pub #n: #t,)
        })
        .collect();
    if type_params.is_empty() {
        parse_quote! {
            #[derive(Clone)]
            struct #name { #(#field_lines)* }
        }
    } else {
        parse_quote! {
            #[derive(Clone)]
            struct #name<#(#type_params),*> { #(#field_lines)* }
        }
    }
}

/// `fn new(...) -> Name { let mut __ds_self = Name { … }; <body>; __ds_self }`.
///
/// Field values come from `this.field = …` assignments in the constructor
/// (those statements are dropped from the body so they run once, at init),
/// then field default initializers, else `todo!()`. A field initializer must
/// not be `todo!()` left to run — Rust evaluates it before any override — so
/// `this.field = expr` is folded into the struct literal instead.
fn build_constructor(
    ctor: Option<&MethodDefinition>,
    fields: &[Field],
    type_name: &Ident,
    type_params: &[Ident],
    registry: &TypeRegistry,
    names: &NameTable<'_>,
) -> syn::ImplItem {
    let mut locals = Locals::new();
    let mut params: Vec<FnArg> = Vec::new();
    let mut field_assigns: Vec<(String, &Expression)> = Vec::new();
    let mut consumed: HashSet<usize> = HashSet::new();
    let mut body_stmts: Vec<Stmt> = Vec::new();

    let self_name = format_ident!("__ds_self");
    let self_fields = self_field_map(fields);
    let narrow = Narrow::in_method(self_name.clone(), self_fields);

    if let Some(md) = ctor {
        let func = &md.value;
        for fp in &func.params.items {
            register_local(
                &mut locals,
                &fp.pattern,
                fp.type_annotation.as_deref(),
                names,
            );
        }
        params = translate_params(&func.params, &locals, registry, names);
        if let Some(body) = func.body.as_deref() {
            let analysis = super::analysis::analyze(
                &body.statements,
                names,
                &registry.mut_methods,
                &registry.ref_params,
            );
            locals.mutated = analysis.mutated;
            locals.use_counts = analysis.use_counts;
            // Fold `this.field = expr` into the struct literal; drop those stmts.
            for (i, stmt) in body.statements.iter().enumerate() {
                if let Some((field, expr)) = ctor_field_assign(stmt) {
                    field_assigns.push((field, expr));
                    consumed.insert(i);
                }
            }
            let return_path: Option<Path> = Some(parse_quote!(#type_name));
            for (i, stmt) in body.statements.iter().enumerate() {
                if !consumed.contains(&i) {
                    body_stmts.extend(translate_stmt(
                        stmt,
                        &mut locals,
                        registry,
                        &narrow,
                        return_path.as_ref(),
                        names,
                    ));
                }
            }
        }
    }

    // Field initializers: a ctor `this.f = e` wins, then the field default,
    // else `todo!()`.
    let ctx = Ctx::new(&locals, registry, &narrow, names);
    let field_inits: Vec<TokenStream> = fields
        .iter()
        .map(|f| {
            let n = &f.name;
            match field_assigns
                .iter()
                .find(|(name, _)| name == &n.to_string())
            {
                Some((_, expr)) => {
                    let e = expressions::translate_expr(expr, &ctx);
                    quote!(#n: #e)
                }
                None => match &f.default {
                    Some(d) => quote!(#n: #d),
                    None => quote!(#n: ::core::todo!()),
                },
            }
        })
        .collect();

    // `fn new` returns `Name<P>` (a generic path type), but a struct literal of
    // a generic type needs turbofish — `Name::<P> { … }` — so the two spellings
    // diverge when the class is generic.
    let (ret_ty, lit_ty): (TokenStream, TokenStream) = if type_params.is_empty() {
        (quote!(#type_name), quote!(#type_name))
    } else {
        (
            quote!(#type_name<#(#type_params),*>),
            quote!(#type_name::<#(#type_params),*>),
        )
    };
    let init: Stmt = parse_quote!(let mut #self_name = #lit_ty { #(#field_inits),* };);
    // A bare trailing `__ds_self` (no semicolon) is the block's value — syn's
    // Stmt parser demands a semicolon for a bare path, so build it directly.
    let trailing = Stmt::Expr(parse_quote!(#self_name), None);
    let mut all = Vec::with_capacity(body_stmts.len() + 2);
    all.push(init);
    all.extend(body_stmts);
    all.push(trailing);

    parse_quote! {
        pub fn new(#(#params),*) -> #ret_ty { #(#all)* }
    }
}

/// `pub fn method(&self | &mut self, args) -> ret { body }`. `&mut self` when
/// the body assigns/updates a member of `this`.
fn build_method(
    md: &MethodDefinition,
    fields: &[Field],
    registry: &TypeRegistry,
    names: &NameTable<'_>,
) -> syn::ImplItem {
    let func = &md.value;
    let name = bindings::property_key_name(&md.key).unwrap_or_else(|| format_ident!("__method"));

    let mut locals = Locals::new();
    for fp in &func.params.items {
        register_local(
            &mut locals,
            &fp.pattern,
            fp.type_annotation.as_deref(),
            names,
        );
    }
    let mut is_mut = false;
    if let Some(body) = func.body.as_deref() {
        let analysis = super::analysis::analyze(
            &body.statements,
            names,
            &registry.mut_methods,
            &registry.ref_params,
        );
        locals.mutated = analysis.mutated;
        locals.use_counts = analysis.use_counts;
        is_mut = analysis.mutates_this;
    }
    let params = translate_params(&func.params, &locals, registry, names);

    let self_fields = self_field_map(fields);
    let narrow = Narrow::in_method(format_ident!("self"), self_fields);
    let return_path = func.return_type.as_deref().and_then(return_path_of);
    let body_stmts: &[Statement] = func.body.as_deref().map_or(&[], |b| &b.statements[..]);
    let block = translate_body(
        body_stmts,
        &mut locals,
        registry,
        &narrow,
        return_path.as_ref(),
        names,
    );

    let output = method_return_type(func);
    let generics: Vec<Ident> = func.type_parameters.as_deref().map_or_else(Vec::new, |tp| {
        tp.params
            .iter()
            .map(|p| bindings::type_ident(&p.name.name))
            .collect()
    });

    let self_arg: FnArg = if is_mut {
        parse_quote!(&mut self)
    } else {
        parse_quote!(&self)
    };
    let all_params: Vec<FnArg> = std::iter::once(self_arg).chain(params).collect();

    if generics.is_empty() {
        parse_quote! { pub fn #name(#(#all_params),*) #output #block }
    } else {
        parse_quote! { pub fn #name<#(#generics),*>(#(#all_params),*) #output #block }
    }
}

/// A method's return type: `void`/`undefined` → inferred `()`, else the type.
fn method_return_type(func: &Function) -> ReturnType {
    func.return_type
        .as_ref()
        .and_then(|ta| match &ta.type_annotation {
            TSType::TSVoidKeyword(_) | TSType::TSUndefinedKeyword(_) => None,
            ty => Some(ReturnType::Type(
                Default::default(),
                Box::new(types::translate_type(ty)),
            )),
        })
        .unwrap_or(ReturnType::Default)
}

/// `this.field = expr` → `(field_name, &expr)`, when the statement is exactly a
/// plain `=` assignment of a static `this.<key>` member. Anything else returns
/// `None` (left in the body to translate normally).
fn ctor_field_assign<'a>(stmt: &'a Statement<'a>) -> Option<(String, &'a Expression<'a>)> {
    let Statement::ExpressionStatement(es) = stmt else {
        return None;
    };
    let Expression::AssignmentExpression(asg) = &es.expression else {
        return None;
    };
    if asg.operator != AssignmentOperator::Assign {
        return None;
    }
    let AssignmentTarget::StaticMemberExpression(sm) = &asg.left else {
        return None;
    };
    if !matches!(&sm.object, Expression::ThisExpression(_)) {
        return None;
    }
    let field = bindings::snake(sm.property.name.as_str()).to_string();
    Some((field, &asg.right))
}

/// An instance field `x: T` / `x?: T` / `x = v` → a [`Field`]. Static,
/// computed, or private fields are unsupported (None).
fn instance_field(
    pd: &PropertyDefinition,
    registry: &TypeRegistry,
    names: &NameTable<'_>,
) -> Option<Field> {
    if pd.r#static || pd.computed {
        return None;
    }
    let name = bindings::property_key_name(&pd.key)?;
    // A field with no annotation falls back to its initializer's type — an
    // initializer-only field (`map = new Map<string, T>()`) is common in TS, and
    // a Rust struct field cannot be `_`, so the collection/literal cases infer a
    // concrete type; an unknown initializer stays `_` (a later `cargo check`
    // error, never a silent mis-type).
    let ty = pd
        .type_annotation
        .as_ref()
        .map(|ta| types::translate_type(&ta.type_annotation))
        .or_else(|| pd.value.as_ref().map(infer_field_type))
        .unwrap_or_else(|| parse_quote!(_));
    let ty = if pd.optional {
        parse_quote!(Option<#ty>)
    } else {
        ty
    };
    // A field initializer `x = 5` runs at class scope (no `this`), translated
    // against an empty locals table.
    let default = pd.value.as_ref().map(|e| {
        let locals = Locals::new();
        let narrow = Narrow::default();
        let ctx = Ctx::new(&locals, registry, &narrow, names);
        expressions::translate_expr(e, &ctx)
    });
    Some(Field { name, ty, default })
}

/// Infer a field's type from its initializer when the field has no annotation
/// (an initializer-only class field). The common collection and literal cases
/// lower to their concrete Rust type; an unknown initializer falls back to `_`.
fn infer_field_type(default: &Expression) -> Type {
    match default {
        Expression::NewExpression(n) => infer_ctor_type(n),
        Expression::NumericLiteral(_) => parse_quote!(f64),
        Expression::StringLiteral(_) => parse_quote!(String),
        Expression::BooleanLiteral(_) => parse_quote!(bool),
        _ => parse_quote!(_),
    }
}

/// `new Map<K, V>()` / `new WeakMap<K, V>()` → `HashMap<K, V>`, and
/// `new Set<E>()` / `new WeakSet<E>()` → `HashSet<E>` — the field type of an
/// initializer-only collection field, read off the constructor's type arguments.
/// `WeakMap`/`WeakSet` use the same strong-collection backing (no GC-precise
/// weak refs; a `WeakMap` keyed by `Uint8Array` is a `HashMap<Vec<u8>, V>`).
fn infer_ctor_type(n: &NewExpression) -> Type {
    let Expression::Identifier(id) = &n.callee else {
        return parse_quote!(_);
    };
    let targs = n.type_arguments.as_deref();
    match id.name.as_str() {
        "Map" | "WeakMap" => match targs.map(|a| &a.params).filter(|p| p.len() == 2) {
            Some(p) => {
                let k = types::translate_type(&p[0]);
                let v = types::translate_type(&p[1]);
                parse_quote!(::std::collections::HashMap<#k, #v>)
            }
            None => parse_quote!(_),
        },
        "Set" | "WeakSet" => match targs.and_then(|a| a.params.first()) {
            Some(e) => {
                let e = types::translate_type(e);
                parse_quote!(::std::collections::HashSet<#e>)
            }
            None => parse_quote!(_),
        },
        _ => parse_quote!(_),
    }
}

/// A `compile_error!` item carrying `message`, so unsupported features fail
/// loudly without breaking the surrounding generated Rust.
fn compile_error_item(message: &str) -> Item {
    let msg = syn::LitStr::new(message, proc_macro2::Span::call_site());
    parse_quote!(compile_error!(#msg);)
}
