//! `class` → `#[derive(Clone)] struct Name { ... } impl Name { ... }`.
//!
//! A class becomes a `struct` plus an `impl`: instance fields → `pub` struct
//! fields; a `new` constructor fills them (from `this.f = …` assignments in the
//! constructor body, then field default initializers); instance methods become
//! `pub fn method(&self | &mut self)`. `this` → `self` (method) / `__ds_self`
//! (constructor).
use std::collections::HashSet;

use oxc_ast::ast::{
    AssignmentTarget, Class, ClassElement, Expression, Function, MethodDefinition,
    MethodDefinitionKind, PropertyDefinition, PropertyKey, Statement, TSAccessibility, TSType,
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
                } else if is_private_member(&pd.key, pd.accessibility.as_ref()) {
                    diags.push(compile_error_item(
                        "DashScript does not support private (`#`/`private`) class fields",
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
                } else if is_private_member(&md.key, md.accessibility.as_ref()) {
                    diags.push(compile_error_item(
                        "DashScript does not support private (`#`/`private`) class methods",
                    ));
                } else {
                    match md.kind {
                        MethodDefinitionKind::Constructor => ctor = Some(md),
                        MethodDefinitionKind::Method => methods.push(md),
                        MethodDefinitionKind::Get => diags.push(compile_error_item(
                            "DashScript does not support `get` accessors — use a method",
                        )),
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

    let struct_item = build_struct(&name, &fields);
    let ctor_item = build_constructor(ctor, &fields, &name, registry, names);
    let method_items: Vec<syn::ImplItem> = methods
        .iter()
        .map(|md| build_method(md, registry, names))
        .collect();
    let impl_item: Item = parse_quote! {
        impl #name {
            #ctor_item
            #(#method_items)*
        }
    };

    let mut result = vec![struct_item, impl_item];
    result.extend(diags);
    result
}

/// Whether a class member is private: a `#private` key, or a TS `private`/
/// `protected` accessibility modifier (Rust struct fields are all `pub`).
fn is_private_member(key: &PropertyKey, accessibility: Option<&TSAccessibility>) -> bool {
    matches!(key, PropertyKey::PrivateIdentifier(_))
        || matches!(
            accessibility,
            Some(TSAccessibility::Private | TSAccessibility::Protected)
        )
}

/// `#[derive(Clone)] struct Name { pub field: ty, … }`.
fn build_struct(name: &Ident, fields: &[Field]) -> Item {
    let field_lines: Vec<TokenStream> = fields
        .iter()
        .map(|f| {
            let n = &f.name;
            let t = &f.ty;
            quote!(pub #n: #t,)
        })
        .collect();
    parse_quote! {
        #[derive(Clone)]
        struct #name { #(#field_lines)* }
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
    registry: &TypeRegistry,
    names: &NameTable<'_>,
) -> syn::ImplItem {
    let mut locals = Locals::new();
    let mut params: Vec<FnArg> = Vec::new();
    let mut field_assigns: Vec<(String, &Expression)> = Vec::new();
    let mut consumed: HashSet<usize> = HashSet::new();
    let mut body_stmts: Vec<Stmt> = Vec::new();

    let self_name = format_ident!("__ds_self");
    let narrow = Narrow::in_method(self_name.clone());

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
        params = translate_params(&func.params, &locals, names);
        if let Some(body) = func.body.as_deref() {
            let analysis = super::analysis::analyze(&body.statements, names);
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

    let init: Stmt = parse_quote!(let mut #self_name = #type_name { #(#field_inits),* };);
    // A bare trailing `__ds_self` (no semicolon) is the block's value — syn's
    // Stmt parser demands a semicolon for a bare path, so build it directly.
    let trailing = Stmt::Expr(parse_quote!(#self_name), None);
    let mut all = Vec::with_capacity(body_stmts.len() + 2);
    all.push(init);
    all.extend(body_stmts);
    all.push(trailing);

    parse_quote! {
        pub fn new(#(#params),*) -> #type_name { #(#all)* }
    }
}

/// `pub fn method(&self | &mut self, args) -> ret { body }`. `&mut self` when
/// the body assigns/updates a member of `this`.
fn build_method(
    md: &MethodDefinition,
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
        let analysis = super::analysis::analyze(&body.statements, names);
        locals.mutated = analysis.mutated;
        locals.use_counts = analysis.use_counts;
        is_mut = analysis.mutates_this;
    }
    let params = translate_params(&func.params, &locals, names);

    let narrow = Narrow::in_method(format_ident!("self"));
    let return_path = func.return_type.as_deref().and_then(return_path_of);
    let block = translate_body(
        func.body.as_deref(),
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
    let ty = pd
        .type_annotation
        .as_ref()
        .map(|ta| types::translate_type(&ta.type_annotation))
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

/// A `compile_error!` item carrying `message`, so unsupported features fail
/// loudly without breaking the surrounding generated Rust.
fn compile_error_item(message: &str) -> Item {
    let msg = syn::LitStr::new(message, proc_macro2::Span::call_site());
    parse_quote!(compile_error!(#msg);)
}
