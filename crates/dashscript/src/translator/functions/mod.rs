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
    Argument, BindingPattern, Declaration, Expression, FormalParameters, Function, FunctionBody,
    Statement, TSType, TryStatement, VariableDeclaration, VariableDeclarationKind,
};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{parse_quote, Block, Expr, FnArg, Ident, ItemFn, LitStr, Path, ReturnType, Stmt, Type};

use super::context::{Ctx, Locals, Narrow};
use super::name_table::NameTable;
use super::registry::TypeRegistry;
use super::{bindings, declarations, expressions, types};

/// Translate a top-level statement into a `syn::Item`, if mapped.
///
/// `interface` / `type` / `function` become top-level items; other statements
/// (variable bindings, expression statements) belong inside a function body
/// and are not mapped at module scope.
pub fn translate_statement(
    stmt: &Statement,
    registry: &TypeRegistry,
    names: &NameTable<'_>,
) -> Vec<syn::Item> {
    match stmt {
        Statement::FunctionDeclaration(func) => {
            vec![syn::Item::Fn(translate_function(func, registry, names))]
        }
        Statement::ClassDeclaration(class) => super::class::translate_class(class, registry, names),
        Statement::TSInterfaceDeclaration(iface) => {
            vec![syn::Item::Struct(declarations::translate_interface(iface))]
        }
        Statement::TSTypeAliasDeclaration(alias) => declarations::translate_type_alias(alias)
            .into_iter()
            .collect(),
        // `export function/interface/type/class` lowers the declaration(s) and
        // marks each `pub` so another `.ds` module can `import` it. Re-export
        // lists (`export { x } from "…"`) have no declaration and yield `[]`.
        Statement::ExportNamedDeclaration(exp) => {
            let Some(decl) = exp.declaration.as_ref() else {
                return Vec::new();
            };
            let mut items = translate_exported_declaration(decl, registry, names);
            for item in &mut items {
                make_pub(item);
            }
            items
        }
        // `import { foo, bar } from "./other"` → `use other::{foo, bar};`. A
        // bare specifier (`"serde"`) lowers the same way (`use serde::{…}`).
        // A default/namespace import has no named specifier and yields `[]`.
        Statement::ImportDeclaration(imp) => {
            let Some(mod_ident) = super::imports::module_ident(&imp.source.value) else {
                return Vec::new();
            };
            let Some(specifiers) = imp.specifiers.as_ref() else {
                return Vec::new();
            };
            let names: Vec<Ident> = specifiers
                .iter()
                .filter_map(super::imports::named_local)
                .collect();
            if names.is_empty() {
                return Vec::new();
            }
            vec![syn::Item::Use(parse_quote!(use #mod_ident::{#(#names),*};))]
        }
        _ => Vec::new(),
    }
}

/// Translate the inner declaration of an `export` (`export function` /
/// `export class` / `export interface` / `export type`). Re-exports and
/// unsupported kinds (enum) yield `[]`. A class yields its `struct` plus `impl`.
fn translate_exported_declaration(
    decl: &Declaration,
    registry: &TypeRegistry,
    names: &NameTable<'_>,
) -> Vec<syn::Item> {
    match decl {
        Declaration::FunctionDeclaration(func) => {
            vec![syn::Item::Fn(translate_function(func, registry, names))]
        }
        Declaration::ClassDeclaration(class) => {
            super::class::translate_class(class, registry, names)
        }
        Declaration::TSInterfaceDeclaration(iface) => {
            vec![syn::Item::Struct(declarations::translate_interface(iface))]
        }
        Declaration::TSTypeAliasDeclaration(alias) => declarations::translate_type_alias(alias)
            .into_iter()
            .collect(),
        _ => Vec::new(),
    }
}

/// Mark a top-level item `pub` — used for `export`ed declarations.
fn make_pub(item: &mut syn::Item) {
    match item {
        syn::Item::Fn(f) => f.vis = parse_quote!(pub),
        syn::Item::Struct(s) => s.vis = parse_quote!(pub),
        syn::Item::Enum(e) => e.vis = parse_quote!(pub),
        syn::Item::Type(t) => t.vis = parse_quote!(pub),
        // An `impl` block has no visibility of its own; its methods are `pub`
        // individually, and the struct is marked `pub` by the arm above.
        syn::Item::Impl(_) => {}
        _ => {}
    }
}

fn translate_function(func: &Function, registry: &TypeRegistry, names: &NameTable<'_>) -> ItemFn {
    let name = func
        .id
        .as_ref()
        .map_or_else(|| format_ident!("main"), bindings::ident_of);
    let mut locals = Locals::new();
    for fp in &func.params.items {
        register_local(
            &mut locals,
            &fp.pattern,
            fp.type_annotation.as_deref(),
            names,
        );
    }
    // Mutations analysis runs before parameter emission so a reassigned parameter
    // — including via `??=`/`||=`/`&&=` — is declared `mut`. TS params reassign;
    // Rust params are immutable by default.
    if let Some(body) = func.body.as_deref() {
        let analysis = super::analysis::analyze(&body.statements, names);
        locals.mutated = analysis.mutated;
        locals.use_counts = analysis.use_counts;
    }
    let inputs = translate_params(&func.params, &locals, names);
    // `void` / `undefined` map to an omitted return type (Rust infers `()`).
    let output = func
        .return_type
        .as_ref()
        .and_then(|ta| match &ta.type_annotation {
            TSType::TSVoidKeyword(_) | TSType::TSUndefinedKeyword(_) => None,
            ty => Some(ReturnType::Type(
                Default::default(),
                Box::new(types::translate_type(ty)),
            )),
        })
        .unwrap_or(ReturnType::Default);
    // The return-type path threads down to `return {…}` so the object literal
    // can borrow its struct name.
    let return_path = func.return_type.as_deref().and_then(return_path_of);
    // Default parameters unwrap their `Option` at the top of the body, so the
    // rest of the function sees the plain value.
    let defaults: Vec<Stmt> = func
        .params
        .items
        .iter()
        .filter_map(|fp| {
            let init = fp.initializer.as_deref()?;
            let name = names.of_pattern(&fp.pattern);
            let default = expressions::translate_expr(
                init,
                &Ctx::new(&locals, registry, &Narrow::default(), names),
            );
            Some(parse_quote!(let #name = #name.unwrap_or(#default);))
        })
        .collect();
    let mut block = translate_body(
        func.body.as_deref(),
        &mut locals,
        registry,
        &Narrow::default(),
        return_path.as_ref(),
        names,
    );
    if !defaults.is_empty() {
        let mut stmts = defaults;
        stmts.extend(block.stmts);
        block.stmts = stmts;
    }
    // Generic type parameters pass through verbatim (`<T>`); Rust monomorphizes
    // and infers each call. Constraints/defaults are ignored (no `where`).
    let generics: Vec<Ident> = func.type_parameters.as_deref().map_or_else(Vec::new, |tp| {
        tp.params
            .iter()
            .map(|p| bindings::type_ident(&p.name.name))
            .collect()
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
pub(in crate::translator) fn return_path_of(ta: &oxc_ast::ast::TSTypeAnnotation) -> Option<Path> {
    match &ta.type_annotation {
        TSType::TSVoidKeyword(_) | TSType::TSUndefinedKeyword(_) => None,
        ty => path_of(&types::translate_type(ty)),
    }
}

pub(in crate::translator) fn translate_params(
    params: &FormalParameters,
    locals: &Locals,
    names: &NameTable<'_>,
) -> Vec<FnArg> {
    params
        .items
        .iter()
        .map(|fp| {
            let pat = names.of_pattern(&fp.pattern);
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
            // A reassigned parameter (incl. `??=`/`||=`/`&&=`) needs `mut`.
            if locals.mutated.contains(&pat.to_string()) {
                parse_quote!(mut #pat : #ty)
            } else {
                parse_quote!(#pat : #ty)
            }
        })
        .collect()
}

/// Record a binding's type path (if it has one) into the locals table.
pub(in crate::translator) fn register_local(
    locals: &mut Locals,
    pattern: &oxc_ast::ast::BindingPattern,
    type_annotation: Option<&oxc_ast::ast::TSTypeAnnotation>,
    names: &NameTable<'_>,
) {
    let Some(ta) = type_annotation else { return };
    let ty = types::translate_type(&ta.type_annotation);
    let Some(path) = path_of(&ty) else { return };
    let name = names.of_pattern(pattern);
    locals.insert(name.to_string(), path);
}

pub(in crate::translator) fn translate_body(
    body: Option<&FunctionBody>,
    locals: &mut Locals,
    registry: &TypeRegistry,
    narrow: &Narrow,
    return_path: Option<&Path>,
    names: &NameTable<'_>,
) -> Block {
    let mut stmts: Vec<Stmt> = body
        .map(|b| {
            b.statements
                .iter()
                .flat_map(|s| translate_stmt(s, locals, registry, narrow, return_path, names))
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
pub(in crate::translator) fn translate_stmt(
    stmt: &Statement,
    locals: &mut Locals,
    registry: &TypeRegistry,
    narrow: &Narrow,
    return_path: Option<&Path>,
    names: &NameTable<'_>,
) -> Vec<Stmt> {
    match stmt {
        Statement::BlockStatement(block) => block
            .body
            .iter()
            .flat_map(|s| translate_stmt(s, locals, registry, narrow, return_path, names))
            .collect(),
        Statement::ReturnStatement(ret) => {
            let s: Stmt = match &ret.argument {
                Some(arg) => {
                    // An object literal borrows the struct name from the return
                    // type; everything else translates as a plain expression.
                    let ret_ty = return_path.map(|p| -> Type { parse_quote!(#p) });
                    let expr = expressions::translate_init(
                        arg,
                        ret_ty.as_ref(),
                        &Ctx::new(&*locals, registry, narrow, names),
                    );
                    parse_quote!(return #expr;)
                }
                None => parse_quote!(return;),
            };
            vec![s]
        }
        Statement::ExpressionStatement(es) => {
            let expr = expressions::translate_expr(
                &es.expression,
                &Ctx::new(&*locals, registry, narrow, names),
            );
            vec![parse_quote!(#expr;)]
        }
        Statement::VariableDeclaration(decl) => {
            translate_variable_declaration(decl, locals, registry, narrow, names)
        }
        Statement::IfStatement(if_stmt) => {
            vec![translate_if(
                if_stmt,
                locals,
                registry,
                narrow,
                return_path,
                names,
            )]
        }
        Statement::WhileStatement(while_stmt) => {
            vec![translate_while(
                while_stmt,
                locals,
                registry,
                narrow,
                return_path,
                names,
            )]
        }
        Statement::DoWhileStatement(dws) => vec![translate_do_while(
            dws,
            locals,
            registry,
            narrow,
            return_path,
            names,
        )],
        Statement::ForOfStatement(for_of) => {
            translate_for_of(for_of, locals, registry, narrow, return_path, names)
        }
        Statement::ForInStatement(for_in) => {
            translate_for_in(for_in, locals, registry, narrow, return_path, names)
        }
        Statement::ForStatement(for_stmt) => {
            translate_for(for_stmt, locals, registry, narrow, return_path, names)
        }
        Statement::SwitchStatement(sw) => {
            vec![translate_switch(
                sw,
                locals,
                registry,
                narrow,
                return_path,
                names,
            )]
        }
        Statement::BreakStatement(_) => vec![parse_quote!(break;)],
        Statement::ContinueStatement(_) => vec![parse_quote!(continue;)],
        Statement::ThrowStatement(t) => {
            vec![throw_stmt(&t.argument, locals, registry, narrow, names)]
        }
        // `try { … } catch (e) { … }` → `catch_unwind` (see `translate_try`).
        Statement::TryStatement(t) => {
            translate_try(t, locals, registry, narrow, return_path, names)
        }
        _ => vec![],
    }
}

/// `try { … } catch (e) { … } [finally { … }]` → a `catch_unwind` around the
/// try body, the catch arm binding the panic payload as a `String` (its
/// message), and the `finally` body appended after the match. DashScript emits
/// `[profile.*] panic = "unwind"` in the `Cargo.toml` it generates (see
/// `manifest`), so unwinding is guaranteed and `catch_unwind` reliably catches
/// a `.ds` `throw` (which lowers to `panic!`) — this is sound *because*
/// DashScript owns the manifest, not despite it.
///
/// Control flow out of the try body (`return`/`break`/`continue`) cannot cross
/// the `catch_unwind` closure boundary (a `return` inside the closure would
/// return from the closure, not the function), so it is rejected up front with
/// a compile error. The catch and finally bodies are outside the closure and
/// may return normally.
fn translate_try(
    t: &TryStatement,
    locals: &mut Locals,
    registry: &TypeRegistry,
    narrow: &Narrow,
    return_path: Option<&Path>,
    names: &NameTable<'_>,
) -> Vec<Stmt> {
    // Reject `return`/`break`/`continue` directly in the try block (one level
    // — a return nested deeper is rare and surfaces as a Rust type error).
    if control_flow_in(&t.block.body) {
        let msg = "DashScript try blocks cannot contain return/break/continue \
                   (control flow cannot cross the catch boundary)";
        return vec![parse_quote!(compile_error!(#msg);)];
    }
    let body: Vec<Stmt> = t
        .block
        .body
        .iter()
        .flat_map(|s| translate_stmt(s, locals, registry, narrow, return_path, names))
        .collect();

    let catch_arm: TokenStream = match &t.handler {
        Some(handler) => {
            let catch_body: Vec<Stmt> = handler
                .body
                .body
                .iter()
                .flat_map(|s| translate_stmt(s, locals, registry, narrow, return_path, names))
                .collect();
            match handler.param.as_ref() {
                // `catch (e) { … }` → bind the panic payload's message as `e`.
                Some(cp) => match &cp.pattern {
                    BindingPattern::BindingIdentifier(id) => {
                        let param = names.of_binding(id);
                        quote! {
                            Err(__panic) => {
                                let #param = __panic
                                    .downcast_ref::<&'static str>().copied().map(|s| s.to_string())
                                    .or_else(|| __panic.downcast_ref::<String>().map(|s| s.clone()))
                                    .unwrap_or_else(|| "panic".to_string());
                                #(#catch_body)*
                            }
                        }
                    }
                    // An unsupported binding shape — discard the payload.
                    _ => quote!(Err(_) => { #(#catch_body)* }),
                },
                // `catch { … }` (no binding) — discard the payload.
                None => quote!(Err(_) => { #(#catch_body)* }),
            }
        }
        // No catch clause (a try/finally) — swallow the panic, finally still runs.
        None => quote!(Err(_) => {}),
    };

    let mut result = vec![parse_quote! {
        match ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {
            #(#body)*
        })) {
            Ok(_) => {},
            #catch_arm
        }
    }];
    if let Some(fin) = &t.finalizer {
        let finally: Vec<Stmt> = fin
            .body
            .iter()
            .flat_map(|s| translate_stmt(s, locals, registry, narrow, return_path, names))
            .collect();
        result.extend(finally);
    }
    result
}

/// True when a statement list contains a `return`/`break`/`continue` directly
/// (one level) — used to keep control flow out of a `try` block.
fn control_flow_in(stmts: &[Statement]) -> bool {
    stmts.iter().any(|s| {
        matches!(
            s,
            Statement::ReturnStatement(_)
                | Statement::BreakStatement(_)
                | Statement::ContinueStatement(_)
        )
    })
}

/// `throw new Error("msg")` / `throw "msg"` → `panic!("msg")`; any other
/// `throw expr` → `panic!("{}", expr)` (Rust has no `throw`; `.ds` errors are
/// treated as unrecoverable panics, since there is no `try`/`catch` yet).
fn throw_stmt(
    arg: &Expression,
    locals: &Locals,
    registry: &TypeRegistry,
    narrow: &Narrow,
    names: &NameTable<'_>,
) -> Stmt {
    if let Some(lit) = thrown_message(arg) {
        return parse_quote!(panic!(#lit););
    }
    let ctx = Ctx::new(locals, registry, narrow, names);
    let e = expressions::translate_expr(arg, &ctx);
    parse_quote!(panic!("{}", #e);)
}

/// The string literal carried by `throw new Error("msg")` or `throw "msg"`.
fn thrown_message(arg: &Expression) -> Option<LitStr> {
    if let Expression::StringLiteral(s) = arg {
        return Some(LitStr::new(
            s.value.as_str(),
            proc_macro2::Span::call_site(),
        ));
    }
    let Expression::NewExpression(new) = arg else {
        return None;
    };
    if let Argument::StringLiteral(s) = new.arguments.first()? {
        return Some(LitStr::new(
            s.value.as_str(),
            proc_macro2::Span::call_site(),
        ));
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
    names: &NameTable<'_>,
) -> Vec<Stmt> {
    let kind_let = matches!(decl.kind, VariableDeclarationKind::Let);
    decl.declarations
        .iter()
        .flat_map(|d| -> Vec<Stmt> {
            match &d.id {
                BindingPattern::ObjectPattern(obj) => destructure_object(
                    obj,
                    d.init.as_ref(),
                    locals,
                    kind_let,
                    registry,
                    narrow,
                    names,
                ),
                BindingPattern::ArrayPattern(arr) => destructure_array(
                    arr,
                    d.init.as_ref(),
                    locals,
                    kind_let,
                    registry,
                    narrow,
                    names,
                ),
                _ => {
                    let name = names.of_pattern(&d.id);
                    // `mut` when a reassignable binding (`let`/`var`) is actually
                    // mutated in this function. JS `var` is reassignable, so
                    // `var i = …; i++` needs `let mut i` (E0384 otherwise);
                    // `const` is never reassignable.
                    let mutable = matches!(
                        decl.kind,
                        VariableDeclarationKind::Let | VariableDeclarationKind::Var
                    ) && locals.mutated.contains(&name.to_string());
                    let ty = d
                        .type_annotation
                        .as_ref()
                        .map(|ta| types::translate_type(&ta.type_annotation))
                        .or_else(|| d.init.as_ref().and_then(infer_literal_type));
                    if let Some(path) = ty.as_ref().and_then(path_of) {
                        locals.insert(name.to_string(), path);
                    }
                    let init = d.init.as_ref().map(|e| {
                        expressions::translate_init(
                            e,
                            ty.as_ref(),
                            &Ctx::new(&*locals, registry, narrow, names),
                        )
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

/// The type inferred from a literal initializer, when a binding has no type
/// annotation: `true` → `bool`, `1`/`0.5` → `f64`, `"x"` → `String`, a
/// homogeneous array → `Vec<f64>` / `Vec<String>`. Anchors the binding's type
/// (a bare float literal is otherwise an ambiguous `{float}` — E0689 on
/// `.acosh()` etc.) and lets type-sensitive mappings (truthiness, `??`, the
/// array builtins) work on unannotated locals.
fn infer_literal_type(expr: &Expression) -> Option<Type> {
    use oxc_syntax::operator::UnaryOperator;
    match expr {
        Expression::BooleanLiteral(_) => Some(parse_quote!(bool)),
        Expression::NumericLiteral(_) => Some(parse_quote!(f64)),
        Expression::StringLiteral(_) => Some(parse_quote!(String)),
        // A homogeneous array literal infers its element type so the builtin
        // array methods (`.map`/`.filter`/`.includes`/…) map correctly without
        // an annotation. A mixed, empty, or spread array is left uninferred
        // (Rust infers at the use site, or the user adds a `number[]` type).
        Expression::ArrayExpression(arr) => {
            let elems: Vec<&Expression> = arr
                .elements
                .iter()
                .filter_map(|e| e.as_expression())
                .collect();
            if elems.is_empty() {
                return None;
            }
            if elems
                .iter()
                .all(|e| matches!(e, Expression::NumericLiteral(_)))
            {
                Some(parse_quote!(Vec<f64>))
            } else if elems
                .iter()
                .all(|e| matches!(e, Expression::StringLiteral(_)))
            {
                Some(parse_quote!(Vec<String>))
            } else {
                None
            }
        }
        // oxc parses a signed literal (`-1000`, `+0`) as
        // `UnaryExpression(-/+, …)` rather than a `NumericLiteral`, so a binding
        // `var i = -1000` / `var x = +0` would otherwise lose its f64 anchor
        // (→ E0689 on `i < …` or `x.cos()`). `unary_expr` strips `+` and keeps
        // `-`, so the inner literal's scalar type is the binding's type.
        Expression::UnaryExpression(un)
            if matches!(
                un.operator,
                UnaryOperator::UnaryNegation | UnaryOperator::UnaryPlus
            ) =>
        {
            match &un.argument {
                Expression::NumericLiteral(_) => Some(parse_quote!(f64)),
                Expression::BooleanLiteral(_) => Some(parse_quote!(bool)),
                Expression::StringLiteral(_) => Some(parse_quote!(String)),
                _ => None,
            }
        }
        _ => None,
    }
}
