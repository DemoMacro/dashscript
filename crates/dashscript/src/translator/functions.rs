//! Function & variable declarations, and statement translation → `syn`.

use oxc_ast::ast::{
    FormalParameters, Function, FunctionBody, Statement, TSType, VariableDeclaration,
    VariableDeclarationKind,
};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{parse_quote, Block, Expr, FnArg, Ident, ItemFn, ReturnType, Stmt, Type};

use super::{bindings, declarations, expressions, types};

/// Translate a top-level statement into a `syn::Item`, if mapped.
///
/// `interface` / `type` / `function` become top-level items; other statements
/// (variable bindings, expression statements) belong inside a function body
/// and are not mapped at module scope.
pub fn translate_statement(stmt: &Statement) -> Option<syn::Item> {
    match stmt {
        Statement::FunctionDeclaration(func) => Some(syn::Item::Fn(translate_function(func))),
        Statement::TSInterfaceDeclaration(iface) => {
            Some(syn::Item::Struct(declarations::translate_interface(iface)))
        }
        Statement::TSTypeAliasDeclaration(alias) => declarations::translate_type_alias(alias),
        _ => None,
    }
}

fn translate_function(func: &Function) -> ItemFn {
    let name = func
        .id
        .as_ref()
        .map_or_else(|| format_ident!("main"), bindings::ident_of);
    let inputs = translate_params(&func.params);
    // `void` / `undefined` map to an omitted return type (Rust infers `()`).
    let output = func
        .return_type
        .as_ref()
        .and_then(|ta| match &ta.type_annotation {
            TSType::TSVoidKeyword(_) | TSType::TSUndefinedKeyword(_) => None,
            ty => Some(ReturnType::Type(Default::default(), Box::new(types::translate_type(ty)))),
        })
        .unwrap_or(ReturnType::Default);
    let block = translate_body(func.body.as_deref());
    parse_quote! {
        fn #name(#(#inputs),*) #output #block
    }
}

fn translate_params(params: &FormalParameters) -> Vec<FnArg> {
    params
        .items
        .iter()
        .map(|fp| {
            let pat = bindings::binding_name(&fp.pattern);
            let ty = fp
                .type_annotation
                .as_ref()
                .map(|ta| types::translate_type(&ta.type_annotation))
                .unwrap_or_else(|| parse_quote!(_));
            parse_quote!(#pat : #ty)
        })
        .collect()
}

fn translate_body(body: Option<&FunctionBody>) -> Block {
    let stmts: Vec<Stmt> = body
        .map(|b| b.statements.iter().flat_map(translate_stmt).collect())
        .unwrap_or_default();
    parse_quote!({ #(#stmts)* })
}

/// Translate a function-body statement into zero or more `syn::Stmt`s.
fn translate_stmt(stmt: &Statement) -> Vec<Stmt> {
    match stmt {
        Statement::ReturnStatement(ret) => {
            let s: Stmt = match &ret.argument {
                Some(arg) => {
                    let expr = expressions::translate_expr(arg);
                    parse_quote!(return #expr;)
                }
                None => parse_quote!(return;),
            };
            vec![s]
        }
        Statement::ExpressionStatement(es) => {
            let expr = expressions::translate_expr(&es.expression);
            vec![parse_quote!(#expr;)]
        }
        Statement::VariableDeclaration(decl) => translate_variable_declaration(decl),
        _ => vec![],
    }
}

/// `let x` → `let mut x` (TS `let` is mutable); `const`/`var` → `let`.
fn translate_variable_declaration(decl: &VariableDeclaration) -> Vec<Stmt> {
    let mutable = matches!(decl.kind, VariableDeclarationKind::Let);
    decl.declarations
        .iter()
        .map(|d| {
            let name = bindings::binding_name(&d.id);
            let ty = d
                .type_annotation
                .as_ref()
                .map(|ta| types::translate_type(&ta.type_annotation));
            let init = d.init.as_ref().map(|e| expressions::translate_init(e, ty.as_ref()));
            build_local(&name, mutable, ty.as_ref(), init.as_ref())
        })
        .collect()
}

/// Build `let [mut] name[: Type] [= init];` from parts.
fn build_local(name: &Ident, mutable: bool, ty: Option<&Type>, init: Option<&Expr>) -> Stmt {
    let mut tokens: TokenStream = quote!(let);
    if mutable {
        tokens.extend(quote!(mut));
    }
    tokens.extend(quote!(#name));
    if let Some(ty) = ty {
        tokens.extend(quote!(: #ty));
    }
    match init {
        Some(init) => tokens.extend(quote!(= #init)),
        // A binding without an initializer is rare; surface it loudly if reached.
        None => tokens.extend(quote!(= ::core::todo!())),
    }
    tokens.extend(quote!(;));
    syn::parse2(tokens).expect("dashscript: generated `let` should parse")
}
