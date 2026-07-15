//! Function & variable declarations, and statement translation → `syn`.

use std::collections::HashMap;

use oxc_ast::ast::{
    Expression, FormalParameters, ForOfStatement, ForStatement, ForStatementInit,
    ForStatementLeft, Function, FunctionBody, IfStatement, Statement, SwitchCase, SwitchStatement,
    TSType, VariableDeclaration, VariableDeclarationKind, WhileStatement,
};
use oxc_syntax::operator::UnaryOperator;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{parse_quote, Arm, Block, Expr, FnArg, Ident, ItemFn, Pat, ReturnType, Stmt, Type};

use super::{bindings, declarations, expressions, types};

/// Variable name → its type's path, tracked so `switch` can turn a string-literal
/// case into an enum variant pattern. Only path-typed locals are recorded.
type Locals = HashMap<String, syn::Path>;

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
    let mut locals = Locals::new();
    for fp in &func.params.items {
        register_local(&mut locals, &fp.pattern, fp.type_annotation.as_deref());
    }
    let block = translate_body(func.body.as_deref(), &mut locals);
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

fn translate_body(body: Option<&FunctionBody>, locals: &mut Locals) -> Block {
    let stmts: Vec<Stmt> = body
        .map(|b| b.statements.iter().flat_map(|s| translate_stmt(s, locals)).collect())
        .unwrap_or_default();
    parse_quote!({ #(#stmts)* })
}

/// Translate a function-body statement into zero or more `syn::Stmt`s.
fn translate_stmt(stmt: &Statement, locals: &mut Locals) -> Vec<Stmt> {
    match stmt {
        Statement::BlockStatement(block) => {
            block.body.iter().flat_map(|s| translate_stmt(s, locals)).collect()
        }
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
        Statement::VariableDeclaration(decl) => translate_variable_declaration(decl, locals),
        Statement::IfStatement(if_stmt) => vec![translate_if(if_stmt, locals)],
        Statement::WhileStatement(while_stmt) => vec![translate_while(while_stmt, locals)],
        Statement::ForOfStatement(for_of) => translate_for_of(for_of, locals),
        Statement::ForStatement(for_stmt) => translate_for(for_stmt, locals),
        Statement::SwitchStatement(sw) => vec![translate_switch(sw, locals)],
        Statement::BreakStatement(_) => vec![parse_quote!(break;)],
        Statement::ContinueStatement(_) => vec![parse_quote!(continue;)],
        _ => vec![],
    }
}

fn translate_if(stmt: &IfStatement, locals: &mut Locals) -> Stmt {
    let cond = condition_expr(&stmt.test, locals);
    let then_block = statement_block(&stmt.consequent, locals);
    match &stmt.alternate {
        Some(alt) => {
            let else_block = statement_block(alt, locals);
            parse_quote!(if #cond #then_block else #else_block)
        }
        None => parse_quote!(if #cond #then_block),
    }
}

fn translate_while(stmt: &WhileStatement, locals: &mut Locals) -> Stmt {
    let cond = condition_expr(&stmt.test, locals);
    let body = statement_block(&stmt.body, locals);
    parse_quote!(while #cond #body)
}

/// Translate an `if`/`while` test. A bare identifier of a `Vec`/`String` type
/// maps to an emptiness check, and an `Option` maps to `is_some`; negation flips
/// to `is_empty`/`is_none`. Anything else translates as a plain boolean expr.
fn condition_expr(test: &Expression, locals: &Locals) -> Expr {
    if let Some(expr) = truthiness(test, false, locals) {
        return expr;
    }
    if let Expression::UnaryExpression(un) = test {
        if matches!(un.operator, UnaryOperator::LogicalNot) {
            if let Some(expr) = truthiness(&un.argument, true, locals) {
                return expr;
            }
        }
    }
    expressions::translate_expr(test)
}

/// If `expr` is a bare identifier of a collection (`Vec`/`String`) or `Option`
/// type, return its Rust boolean form. `negated` selects the falsy side
/// (`is_empty`/`is_none`) vs the truthy side (`!is_empty`/`is_some`).
fn truthiness(expr: &Expression, negated: bool, locals: &Locals) -> Option<Expr> {
    let Expression::Identifier(id) = expr else {
        return None;
    };
    let ident = bindings::snake(&id.name);
    let last = locals
        .get(&ident.to_string())?
        .segments
        .last()?
        .ident
        .to_string();
    match last.as_str() {
        "Vec" | "String" => Some(if negated {
            parse_quote!(#ident.is_empty())
        } else {
            parse_quote!(!#ident.is_empty())
        }),
        "Option" => Some(if negated {
            parse_quote!(#ident.is_none())
        } else {
            parse_quote!(#ident.is_some())
        }),
        _ => None,
    }
}

/// `for (const v of xs)` → `for &v in &xs { … }`.
///
/// The `&v` pattern destructures the borrow so `v` is an owned `f64` (Copy),
/// avoiding a `&f64`/`f64` mismatch on comparisons inside the body. This works
/// for Copy elements (DashScript `number`/`boolean`); iterating owned values
/// out of a `Vec<String>` is unsupported yet.
fn translate_for_of(stmt: &ForOfStatement, locals: &mut Locals) -> Vec<Stmt> {
    let Some(pat) = for_of_binding(&stmt.left) else {
        return vec![];
    };
    let iter = expressions::translate_expr(&stmt.right);
    let body = statement_block(&stmt.body, locals);
    vec![parse_quote!(for &#pat in &#iter #body)]
}

/// `for (init; test; update) body` → `{ init; while test { body; update; } }`.
///
/// `.ds` `number` is `f64`, and `Range<f64>` isn't iterable in Rust, so a
/// C-style loop decomposes into a `while` (not `for i in 0..n`). It is wrapped
/// in a block so the loop's own bindings (e.g. `i`) don't collide across loops.
/// A `continue` inside the body skips the `update` step — a known limitation;
/// use a `while` if the update must run every iteration.
fn translate_for(stmt: &ForStatement, locals: &mut Locals) -> Vec<Stmt> {
    let init: Vec<Stmt> = match &stmt.init {
        Some(ForStatementInit::VariableDeclaration(decl)) => {
            translate_variable_declaration(decl, locals)
        }
        _ => Vec::new(),
    };
    let test = stmt
        .test
        .as_ref()
        .map(|t| condition_expr(t, locals))
        .unwrap_or_else(|| parse_quote!(true));
    let body = translate_stmt(&stmt.body, locals);
    let update: Option<Stmt> = stmt.update.as_ref().map(|u| {
        let e = expressions::translate_expr(u);
        parse_quote!(#e;)
    });
    vec![parse_quote!({
        #(#init)*
        while #test {
            #(#body)*
            #update
        }
    })]
}

/// Binding name from `for (const v of …)`; other left forms are unmapped.
fn for_of_binding(left: &ForStatementLeft) -> Option<Ident> {
    let ForStatementLeft::VariableDeclaration(decl) = left else {
        return None;
    };
    let d = decl.declarations.first()?;
    Some(bindings::binding_name(&d.id))
}

/// Turn any statement into a `{ … }` block (used by if/while/for bodies).
fn statement_block(stmt: &Statement, locals: &mut Locals) -> Block {
    let stmts: Vec<Stmt> = translate_stmt(stmt, locals);
    parse_quote!({ #(#stmts)* })
}

/// `switch (s) { case "x": …; default: … }` → `match s { … }`.
///
/// When `s` is a variable of enum type, each string-literal case becomes a
/// variant pattern (`Status::Pending`); a `default` case becomes `_`. `break`
/// is dropped — Rust `match` arms don't fall through.
fn translate_switch(sw: &SwitchStatement, locals: &mut Locals) -> Stmt {
    let disc = expressions::translate_expr(&sw.discriminant);
    let enum_path = discriminant_path(&sw.discriminant, locals);
    let arms: Vec<Arm> = sw
        .cases
        .iter()
        .filter_map(|c| switch_arm(c, enum_path.as_ref(), locals))
        .collect();
    parse_quote!(match #disc { #(#arms)* })
}

fn discriminant_path(disc: &Expression, locals: &Locals) -> Option<syn::Path> {
    let Expression::Identifier(id) = disc else { return None };
    let name: &str = &id.name;
    locals.get(&bindings::snake(name).to_string()).cloned()
}

fn switch_arm(c: &SwitchCase, enum_path: Option<&syn::Path>, locals: &mut Locals) -> Option<Arm> {
    let pat = match &c.test {
        Some(test) => switch_pattern(test, enum_path),
        None => parse_quote!(_),
    };
    let body = case_body(&c.consequent, locals);
    Some(parse_quote!(#pat => #body,))
}

/// A string-literal case on an enum becomes a variant pattern; anything else
/// (non-enum, numeric, …) falls back to `_` — number switches on `f64` aren't
/// valid Rust patterns, so prefer `if` there.
fn switch_pattern(test: &Expression, enum_path: Option<&syn::Path>) -> Pat {
    let Expression::StringLiteral(s) = test else {
        return parse_quote!(_);
    };
    let Some(path) = enum_path else {
        return parse_quote!(_);
    };
    let value: &str = &s.value;
    let variant = bindings::pascal(value);
    parse_quote!(#path::#variant)
}

fn case_body(stmts: &[Statement], locals: &mut Locals) -> Block {
    let rust: Vec<Stmt> = stmts
        .iter()
        .filter(|s| !matches!(s, Statement::BreakStatement(_)))
        .flat_map(|s| translate_stmt(s, locals))
        .collect();
    parse_quote!({ #(#rust)* })
}

/// `let x` → `let mut x` (TS `let` is mutable); `const`/`var` → `let`.
fn translate_variable_declaration(decl: &VariableDeclaration, locals: &mut Locals) -> Vec<Stmt> {
    let mutable = matches!(decl.kind, VariableDeclarationKind::Let);
    decl.declarations
        .iter()
        .map(|d| {
            let name = bindings::binding_name(&d.id);
            let ty = d
                .type_annotation
                .as_ref()
                .map(|ta| types::translate_type(&ta.type_annotation));
            if let Some(ty) = &ty {
                if let Some(path) = path_of(ty) {
                    locals.insert(name.to_string(), path);
                }
            }
            let init = d.init.as_ref().map(|e| expressions::translate_init(e, ty.as_ref()));
            build_local(&name, mutable, ty.as_ref(), init.as_ref())
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
