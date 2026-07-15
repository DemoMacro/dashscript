//! Function declarations → `syn::ItemFn`.

use oxc_ast::ast::{FormalParameters, Function, FunctionBody, Statement, TSType};
use quote::format_ident;
use syn::{parse_quote, Block, FnArg, ItemFn, ReturnType, Stmt};

use super::{bindings, expressions, types};

/// Translate a top-level statement into a `syn::Item`, if mapped.
///
/// Other statements (variable declarations, exports, …) are not mapped yet.
pub fn translate_statement(stmt: &Statement) -> Option<syn::Item> {
    match stmt {
        Statement::FunctionDeclaration(func) => {
            Some(syn::Item::Fn(translate_function(func)))
        }
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
    let stmts = body
        .map(|b| b.statements.iter().filter_map(translate_stmt).collect::<Vec<Stmt>>())
        .unwrap_or_default();
    parse_quote!({ #(#stmts)* })
}

fn translate_stmt(stmt: &Statement) -> Option<Stmt> {
    match stmt {
        Statement::ReturnStatement(ret) => {
            let stmt: Stmt = match &ret.argument {
                Some(arg) => {
                    let expr = expressions::translate_expr(arg);
                    parse_quote!(return #expr;)
                }
                None => parse_quote!(return;),
            };
            Some(stmt)
        }
        Statement::ExpressionStatement(es) => {
            let expr = expressions::translate_expr(&es.expression);
            Some(parse_quote!(#expr;))
        }
        _ => None,
    }
}
