//! Function & variable declarations, and statement translation → `syn`.
//!
//! Control flow lives in [`control_flow`], `switch` in [`switch`], and
//! destructuring patterns in [`destructure`]; this module holds the function
//! skeleton (params, body, return type) and the statement dispatcher.

mod control_flow;
mod destructure;
mod switch;

use control_flow::{
    translate_do_while, translate_for, translate_for_in, translate_for_of, translate_if,
    translate_while,
};
use destructure::{destructure_array, destructure_object};
use switch::translate_switch;

use oxc_ast::ast::{
    Argument, BindingPattern, Expression, FormalParameters, Function, FunctionBody, Statement,
    TSType, VariableDeclaration, VariableDeclarationKind,
};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{parse_quote, Block, Expr, FnArg, Ident, ItemFn, LitStr, Path, ReturnType, Stmt, Type};

use super::context::{Ctx, Locals, Narrow};
use super::registry::TypeRegistry;
use super::{bindings, declarations, expressions, types};

/// Translate a top-level statement into a `syn::Item`, if mapped.
///
/// `interface` / `type` / `function` become top-level items; other statements
/// (variable bindings, expression statements) belong inside a function body
/// and are not mapped at module scope.
pub fn translate_statement(stmt: &Statement, registry: &TypeRegistry) -> Option<syn::Item> {
    match stmt {
        Statement::FunctionDeclaration(func) => Some(syn::Item::Fn(translate_function(func, registry))),
        Statement::TSInterfaceDeclaration(iface) => {
            Some(syn::Item::Struct(declarations::translate_interface(iface)))
        }
        Statement::TSTypeAliasDeclaration(alias) => declarations::translate_type_alias(alias),
        _ => None,
    }
}

fn translate_function(func: &Function, registry: &TypeRegistry) -> ItemFn {
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
    // The return-type path threads down to `return {…}` so the object literal
    // can borrow its struct name.
    let return_path = func.return_type.as_deref().and_then(return_path_of);
    let mut locals = Locals::new();
    for fp in &func.params.items {
        register_local(&mut locals, &fp.pattern, fp.type_annotation.as_deref());
    }
    // Default parameters unwrap their `Option` at the top of the body, so the
    // rest of the function sees the plain value.
    let defaults: Vec<Stmt> = func
        .params
        .items
        .iter()
        .filter_map(|fp| {
            let init = fp.initializer.as_deref()?;
            let name = bindings::binding_name(&fp.pattern);
            let default = expressions::translate_expr(
                init,
                &Ctx::new(&locals, registry, &Narrow::default()),
            );
            Some(parse_quote!(let #name = #name.unwrap_or(#default);))
        })
        .collect();
    if let Some(body) = func.body.as_deref() {
        let analysis = super::analysis::analyze(&body.statements);
        locals.mutated = analysis.mutated;
        locals.use_counts = analysis.use_counts;
    }
    let mut block = translate_body(
        func.body.as_deref(),
        &mut locals,
        registry,
        &Narrow::default(),
        return_path.as_ref(),
    );
    if !defaults.is_empty() {
        let mut stmts = defaults;
        stmts.extend(block.stmts);
        block.stmts = stmts;
    }
    // Generic type parameters pass through verbatim (`<T>`); Rust monomorphizes
    // and infers each call. Constraints/defaults are ignored (no `where`).
    let generics: Vec<Ident> = func
        .type_parameters
        .as_deref()
        .map_or_else(Vec::new, |tp| {
            tp.params.iter().map(|p| bindings::type_ident(&p.name.name)).collect()
        });
    if generics.is_empty() {
        parse_quote! {
            fn #name(#(#inputs),*) #output #block
        }
    } else {
        parse_quote! {
            fn #name<#(#generics),*>(#(#inputs),*) #output #block
        }
    }
}

/// The `syn::Path` of a function's return type — used to translate `return {…}`
/// object literals. `void`/`undefined` yield no path.
fn return_path_of(ta: &oxc_ast::ast::TSTypeAnnotation) -> Option<Path> {
    match &ta.type_annotation {
        TSType::TSVoidKeyword(_) | TSType::TSUndefinedKeyword(_) => None,
        ty => path_of(&types::translate_type(ty)),
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
            // A parameter with a default becomes `Option<T>` (callers pass None).
            let ty = if fp.initializer.is_some() {
                parse_quote!(Option<#ty>)
            } else {
                ty
            };
            parse_quote!(#pat : #ty)
        })
        .collect()
}

/// Record a binding's type path (if it has one) into the locals table.
fn register_local(
    locals: &mut Locals,
    pattern: &oxc_ast::ast::BindingPattern,
    type_annotation: Option<&oxc_ast::ast::TSTypeAnnotation>,
) {
    let Some(ta) = type_annotation else { return };
    let ty = types::translate_type(&ta.type_annotation);
    let Some(path) = path_of(&ty) else { return };
    let name = bindings::binding_name(pattern);
    locals.insert(name.to_string(), path);
}

fn translate_body(
    body: Option<&FunctionBody>,
    locals: &mut Locals,
    registry: &TypeRegistry,
    narrow: &Narrow,
    return_path: Option<&Path>,
) -> Block {
    let mut stmts: Vec<Stmt> = body
        .map(|b| {
            b.statements
                .iter()
                .flat_map(|s| translate_stmt(s, locals, registry, narrow, return_path))
                .collect()
        })
        .unwrap_or_default();
    // A trailing `return expr;` is the block's implicit value — emit it as a
    // bare trailing expression (no `return`, no `;`) for idiomatic Rust and to
    // keep clippy::needless_return quiet. A bare `return;` (void) stays as-is.
    drop_trailing_return(&mut stmts);
    parse_quote!({ #(#stmts)* })
}

/// Replace a trailing `return expr;` with a bare `expr` (no `return`, no `;`)
/// so the block's value is the expression — idiomatic Rust, and keeps
/// clippy::needless_return quiet. A bare `return;` (void) is left untouched.
fn drop_trailing_return(stmts: &mut [Stmt]) {
    let trailing_value = match stmts.last() {
        Some(Stmt::Expr(Expr::Return(ret), _)) => ret.expr.clone(),
        _ => None,
    };
    if let Some(value) = trailing_value {
        if let Some(slot) = stmts.last_mut() {
            *slot = Stmt::Expr(*value, None);
        }
    }
}

/// Translate a function-body statement into zero or more `syn::Stmt`s.
fn translate_stmt(
    stmt: &Statement,
    locals: &mut Locals,
    registry: &TypeRegistry,
    narrow: &Narrow,
    return_path: Option<&Path>,
) -> Vec<Stmt> {
    match stmt {
        Statement::BlockStatement(block) => block
            .body
            .iter()
            .flat_map(|s| translate_stmt(s, locals, registry, narrow, return_path))
            .collect(),
        Statement::ReturnStatement(ret) => {
            let s: Stmt = match &ret.argument {
                Some(arg) => {
                    // An object literal borrows the struct name from the return
                    // type; everything else translates as a plain expression.
                    let ret_ty = return_path.map(|p| -> Type { parse_quote!(#p) });
                    let expr =
                        expressions::translate_init(arg, ret_ty.as_ref(), &Ctx::new(&*locals, registry, narrow));
                    parse_quote!(return #expr;)
                }
                None => parse_quote!(return;),
            };
            vec![s]
        }
        Statement::ExpressionStatement(es) => {
            let expr = expressions::translate_expr(&es.expression, &Ctx::new(&*locals, registry, narrow));
            vec![parse_quote!(#expr;)]
        }
        Statement::VariableDeclaration(decl) => {
            translate_variable_declaration(decl, locals, registry, narrow)
        }
        Statement::IfStatement(if_stmt) => vec![translate_if(if_stmt, locals, registry, narrow, return_path)],
        Statement::WhileStatement(while_stmt) => {
            vec![translate_while(while_stmt, locals, registry, narrow, return_path)]
        }
        Statement::DoWhileStatement(dws) => vec![translate_do_while(dws, locals, registry, narrow, return_path)],
        Statement::ForOfStatement(for_of) => translate_for_of(for_of, locals, registry, narrow, return_path),
        Statement::ForInStatement(for_in) => translate_for_in(for_in, locals, registry, narrow, return_path),
        Statement::ForStatement(for_stmt) => translate_for(for_stmt, locals, registry, narrow, return_path),
        Statement::SwitchStatement(sw) => vec![translate_switch(sw, locals, registry, narrow, return_path)],
        Statement::BreakStatement(_) => vec![parse_quote!(break;)],
        Statement::ContinueStatement(_) => vec![parse_quote!(continue;)],
        Statement::ThrowStatement(t) => vec![throw_stmt(&t.argument, locals, registry, narrow)],
        // try-catch cannot map soundly: `throw` is an unrecoverable panic, and
        // catching it (catch_unwind) breaks function-return semantics. Surface
        // it as a compile error rather than silently dropping the block.
        Statement::TryStatement(_) => vec![parse_quote!(
            compile_error!("DashScript does not support try-catch (throw is an unrecoverable panic)");
        )],
        _ => vec![],
    }
}

/// `throw new Error("msg")` / `throw "msg"` → `panic!("msg")`; any other
/// `throw expr` → `panic!("{}", expr)` (Rust has no `throw`; `.ds` errors are
/// treated as unrecoverable panics, since there is no `try`/`catch` yet).
fn throw_stmt(
    arg: &Expression,
    locals: &Locals,
    registry: &TypeRegistry,
    narrow: &Narrow,
) -> Stmt {
    if let Some(lit) = thrown_message(arg) {
        return parse_quote!(panic!(#lit););
    }
    let ctx = Ctx::new(locals, registry, narrow);
    let e = expressions::translate_expr(arg, &ctx);
    parse_quote!(panic!("{}", #e);)
}

/// The string literal carried by `throw new Error("msg")` or `throw "msg"`.
fn thrown_message(arg: &Expression) -> Option<LitStr> {
    if let Expression::StringLiteral(s) = arg {
        return Some(LitStr::new(s.value.as_str(), proc_macro2::Span::call_site()));
    }
    let Expression::NewExpression(new) = arg else {
        return None;
    };
    if let Argument::StringLiteral(s) = new.arguments.first()? {
        return Some(LitStr::new(s.value.as_str(), proc_macro2::Span::call_site()));
    }
    None
}

/// `let x` → `let mut x` (TS `let` is mutable); `const`/`var` → `let`.
/// An object pattern (`const { x, y } = v`) destructures the struct.
fn translate_variable_declaration(
    decl: &VariableDeclaration,
    locals: &mut Locals,
    registry: &TypeRegistry,
    narrow: &Narrow,
) -> Vec<Stmt> {
    let kind_let = matches!(decl.kind, VariableDeclarationKind::Let);
    decl.declarations
        .iter()
        .flat_map(|d| -> Vec<Stmt> {
            match &d.id {
                BindingPattern::ObjectPattern(obj) => {
                    destructure_object(obj, d.init.as_ref(), locals, kind_let, registry, narrow)
                }
                BindingPattern::ArrayPattern(arr) => {
                    destructure_array(arr, d.init.as_ref(), locals, kind_let, registry, narrow)
                }
                _ => {
                    let name = bindings::binding_name(&d.id);
                    // A `let` is `mut` only when the binding is mutated in this
                    // function; `const`/`var` are never `mut`.
                    let mutable = kind_let && locals.mutated.contains(&name.to_string());
                    let ty = d
                        .type_annotation
                        .as_ref()
                        .map(|ta| types::translate_type(&ta.type_annotation));
                    if let Some(ty) = &ty {
                        if let Some(path) = path_of(ty) {
                            locals.insert(name.to_string(), path);
                        }
                    } else if let Some(path) = d.init.as_ref().and_then(infer_literal_type) {
                        // No annotation: infer a scalar type from a literal init
                        // so type-sensitive mappings (||, ??, truthiness) work.
                        locals.insert(name.to_string(), path);
                    }
                    let init = d.init.as_ref().map(|e| {
                        expressions::translate_init(e, ty.as_ref(), &Ctx::new(&*locals, registry, narrow))
                    });
                    vec![build_local(&name, mutable, ty.as_ref(), init.as_ref())]
                }
            }
        })
        .collect()
}

/// Extract the path of a `Type::Path`, if any.
fn path_of(ty: &Type) -> Option<syn::Path> {
    if let Type::Path(tp) = ty {
        Some(tp.path.clone())
    } else {
        None
    }
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

/// The scalar type inferred from a literal initializer, when a `let` has no
/// type annotation: `true` → `bool`, `1` → `f64`, `"x"` → `String`. Lets
/// type-sensitive mappings (truthiness, `??`) work on unannotated locals.
fn infer_literal_type(expr: &Expression) -> Option<Path> {
    match expr {
        Expression::BooleanLiteral(_) => Some(parse_quote!(bool)),
        Expression::NumericLiteral(_) => Some(parse_quote!(f64)),
        Expression::StringLiteral(_) => Some(parse_quote!(String)),
        _ => None,
    }
}
