//! Function & variable declarations, and statement translation → `syn`.

use oxc_ast::ast::{
    Argument, ArrayPattern, BindingPattern, DoWhileStatement, Expression, FormalParameters,
    ForInStatement, ForOfStatement, ForStatement, ForStatementInit, ForStatementLeft, Function,
    FunctionBody, IfStatement, ObjectPattern, Statement, SwitchCase, SwitchStatement, TSType,
    VariableDeclaration, VariableDeclarationKind, WhileStatement,
};
use oxc_syntax::operator::UnaryOperator;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{parse_quote, Arm, Block, Expr, FnArg, Ident, ItemFn, LitStr, Pat, Path, ReturnType, Stmt, Type};

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
    let block = translate_body(
        func.body.as_deref(),
        &mut locals,
        registry,
        &Narrow::default(),
        return_path.as_ref(),
    );
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
    let stmts: Vec<Stmt> = body
        .map(|b| {
            b.statements
                .iter()
                .flat_map(|s| translate_stmt(s, locals, registry, narrow, return_path))
                .collect()
        })
        .unwrap_or_default();
    parse_quote!({ #(#stmts)* })
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

fn translate_if(
    stmt: &IfStatement,
    locals: &mut Locals,
    registry: &TypeRegistry,
    narrow: &Narrow,
    return_path: Option<&Path>,
) -> Stmt {
    let cond = condition_expr(&stmt.test, locals, registry, narrow);
    let then_block = statement_block(&stmt.consequent, locals, registry, narrow, return_path);
    match &stmt.alternate {
        Some(alt) => {
            let else_block = statement_block(alt, locals, registry, narrow, return_path);
            parse_quote!(if #cond #then_block else #else_block)
        }
        None => parse_quote!(if #cond #then_block),
    }
}

fn translate_while(
    stmt: &WhileStatement,
    locals: &mut Locals,
    registry: &TypeRegistry,
    narrow: &Narrow,
    return_path: Option<&Path>,
) -> Stmt {
    let cond = condition_expr(&stmt.test, locals, registry, narrow);
    let body = statement_block(&stmt.body, locals, registry, narrow, return_path);
    parse_quote!(while #cond #body)
}

/// `do { body } while (test)` → `loop { body; if !(test) { break; } }` — Rust
/// has no do-while, so the body runs once then the test gates each repeat.
fn translate_do_while(
    stmt: &DoWhileStatement,
    locals: &mut Locals,
    registry: &TypeRegistry,
    narrow: &Narrow,
    return_path: Option<&Path>,
) -> Stmt {
    let body = statement_block(&stmt.body, locals, registry, narrow, return_path);
    let test = condition_expr(&stmt.test, locals, registry, narrow);
    parse_quote!(loop {
        #body
        if !(#test) {
            break;
        }
    })
}

/// Translate an `if`/`while` test. A bare identifier of a `Vec`/`String` type
/// maps to an emptiness check, and an `Option` maps to `is_some`; negation flips
/// to `is_empty`/`is_none`. Anything else translates as a plain boolean expr.
fn condition_expr(test: &Expression, locals: &Locals, registry: &TypeRegistry, narrow: &Narrow) -> Expr {
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
    expressions::translate_expr(test, &Ctx::new(locals, registry, narrow))
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
fn translate_for_of(
    stmt: &ForOfStatement,
    locals: &mut Locals,
    registry: &TypeRegistry,
    narrow: &Narrow,
    return_path: Option<&Path>,
) -> Vec<Stmt> {
    let Some(pat) = for_of_binding(&stmt.left) else {
        return vec![];
    };
    let iter = expressions::translate_expr(&stmt.right, &Ctx::new(&*locals, registry, narrow));
    let body = statement_block(&stmt.body, locals, registry, narrow, return_path);
    vec![parse_quote!(for &#pat in &#iter #body)]
}

/// `for (const k in m)` → `for k in m.keys().cloned()` — iterates a map's keys
/// as owned `String`s (the `.ds` `Record` is a `HashMap<String, …>`). A struct
/// source has no keys iterator, so only a `Record`/`HashMap` is supported.
fn translate_for_in(
    stmt: &ForInStatement,
    locals: &mut Locals,
    registry: &TypeRegistry,
    narrow: &Narrow,
    return_path: Option<&Path>,
) -> Vec<Stmt> {
    let Some(pat) = for_of_binding(&stmt.left) else {
        return vec![];
    };
    let iter = expressions::translate_expr(&stmt.right, &Ctx::new(&*locals, registry, narrow));
    let body = statement_block(&stmt.body, locals, registry, narrow, return_path);
    vec![parse_quote!(for #pat in #iter.keys().cloned() #body)]
}

/// `for (init; test; update) body` → `{ init; while test { body; update; } }`.
///
/// `.ds` `number` is `f64`, and `Range<f64>` isn't iterable in Rust, so a
/// C-style loop decomposes into a `while` (not `for i in 0..n`). It is wrapped
/// in a block so the loop's own bindings (e.g. `i`) don't collide across loops.
/// A `continue` inside the body skips the `update` step — a known limitation;
/// use a `while` if the update must run every iteration.
fn translate_for(
    stmt: &ForStatement,
    locals: &mut Locals,
    registry: &TypeRegistry,
    narrow: &Narrow,
    return_path: Option<&Path>,
) -> Vec<Stmt> {
    let init: Vec<Stmt> = match &stmt.init {
        Some(ForStatementInit::VariableDeclaration(decl)) => {
            translate_variable_declaration(decl, locals, registry, narrow)
        }
        _ => Vec::new(),
    };
    let test = stmt
        .test
        .as_ref()
        .map(|t| condition_expr(t, locals, registry, narrow))
        .unwrap_or_else(|| parse_quote!(true));
    let body = translate_stmt(&stmt.body, locals, registry, narrow, return_path);
    let update: Option<Stmt> = stmt.update.as_ref().map(|u| {
        let e = expressions::translate_expr(u, &Ctx::new(&*locals, registry, narrow));
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
fn statement_block(
    stmt: &Statement,
    locals: &mut Locals,
    registry: &TypeRegistry,
    narrow: &Narrow,
    return_path: Option<&Path>,
) -> Block {
    let stmts: Vec<Stmt> = translate_stmt(stmt, locals, registry, narrow, return_path);
    parse_quote!({ #(#stmts)* })
}

/// `switch (s) { case "x": …; default: … }` → `match s { … }`.
///
/// Two shapes: `switch (x.kind) { … }` on a discriminated-union local
/// destructures variants (`Shape::Circle { radius } => …`, with `s.radius` in
/// the arm body narrowed to `radius`); `switch (e) { … }` on a bare enum
/// identifier maps each string-literal case to a unit/tuple variant pattern.
fn translate_switch(
    sw: &SwitchStatement,
    locals: &mut Locals,
    registry: &TypeRegistry,
    narrow: &Narrow,
    return_path: Option<&Path>,
) -> Stmt {
    if let Some((scrut, type_name)) = discriminant_member(&sw.discriminant, locals, registry) {
        return discriminated_match(sw, &scrut, &type_name, locals, registry, return_path);
    }
    let disc = expressions::translate_expr(&sw.discriminant, &Ctx::new(&*locals, registry, narrow));
    let enum_path = discriminant_path(&sw.discriminant, locals);
    let arms: Vec<Arm> = sw
        .cases
        .iter()
        .filter_map(|c| switch_arm(c, enum_path.as_ref(), locals, registry, narrow, return_path))
        .collect();
    parse_quote!(match #disc { #(#arms)* })
}

/// When `disc` is `x.kind` and `x` is a local of a registered discriminated
/// union, return `(x's snake name, the enum's type name)`.
fn discriminant_member(
    disc: &Expression,
    locals: &Locals,
    registry: &TypeRegistry,
) -> Option<(String, String)> {
    let Expression::StaticMemberExpression(sm) = disc else {
        return None;
    };
    if sm.property.name.as_str() != "kind" {
        return None;
    }
    let Expression::Identifier(obj) = &sm.object else {
        return None;
    };
    let scrut = bindings::snake(&obj.name).to_string();
    let type_name = locals.get(&scrut)?.segments.last()?.ident.to_string();
    registry.unions.contains_key(&type_name).then_some((scrut, type_name))
}

/// `switch (s.kind) { case "circle": … }` → `match s { Shape::Circle { radius } => … }`.
/// Each arm body is translated under a [`Narrow`] that rewrites `s.field` to the
/// `field` binding.
fn discriminated_match(
    sw: &SwitchStatement,
    scrut: &str,
    type_name: &str,
    locals: &mut Locals,
    registry: &TypeRegistry,
    return_path: Option<&Path>,
) -> Stmt {
    let scrut_ident = format_ident!("{}", scrut);
    let arms: Vec<Arm> = sw
        .cases
        .iter()
        .filter_map(|c| discriminated_arm(c, scrut, type_name, locals, registry, return_path))
        .collect();
    parse_quote!(match #scrut_ident { #(#arms)* })
}

/// One arm of a discriminated-union match: `case "circle"` →
/// `Shape::Circle { radius } => <body with s.radius narrowed to radius>`.
/// A `default` arm becomes `_ => <body>` with no narrowing.
fn discriminated_arm(
    c: &SwitchCase,
    scrut: &str,
    type_name: &str,
    locals: &mut Locals,
    registry: &TypeRegistry,
    return_path: Option<&Path>,
) -> Option<Arm> {
    let (pat, narrow) = match &c.test {
        Some(Expression::StringLiteral(s)) => {
            let value = s.value.to_string();
            let shape = registry.unions.get(type_name)?.get(&value)?.clone();
            let type_ident = format_ident!("{}", type_name);
            let variant = shape.name;
            let field_idents: Vec<Ident> = shape.fields.clone();
            let narrow = Narrow::of(
                scrut.to_string(),
                field_idents.iter().map(|f| f.to_string()).collect(),
            );
            let pat: Pat = parse_quote!(#type_ident::#variant { #(#field_idents),* });
            (pat, narrow)
        }
        _ => (parse_quote!(_), Narrow::default()),
    };
    let body = case_body(&c.consequent, locals, registry, &narrow, return_path);
    Some(parse_quote!(#pat => #body,))
}

fn discriminant_path(disc: &Expression, locals: &Locals) -> Option<syn::Path> {
    let Expression::Identifier(id) = disc else { return None };
    let name: &str = &id.name;
    locals.get(&bindings::snake(name).to_string()).cloned()
}

fn switch_arm(
    c: &SwitchCase,
    enum_path: Option<&syn::Path>,
    locals: &mut Locals,
    registry: &TypeRegistry,
    narrow: &Narrow,
    return_path: Option<&Path>,
) -> Option<Arm> {
    let pat = match &c.test {
        Some(test) => switch_pattern(test, enum_path),
        None => parse_quote!(_),
    };
    let body = case_body(&c.consequent, locals, registry, narrow, return_path);
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

fn case_body(
    stmts: &[Statement],
    locals: &mut Locals,
    registry: &TypeRegistry,
    narrow: &Narrow,
    return_path: Option<&Path>,
) -> Block {
    let rust: Vec<Stmt> = stmts
        .iter()
        .filter(|s| !matches!(s, Statement::BreakStatement(_)))
        .flat_map(|s| translate_stmt(s, locals, registry, narrow, return_path))
        .collect();
    parse_quote!({ #(#rust)* })
}

/// `let x` → `let mut x` (TS `let` is mutable); `const`/`var` → `let`.
/// An object pattern (`const { x, y } = v`) destructures the struct.
fn translate_variable_declaration(
    decl: &VariableDeclaration,
    locals: &mut Locals,
    registry: &TypeRegistry,
    narrow: &Narrow,
) -> Vec<Stmt> {
    let mutable = matches!(decl.kind, VariableDeclarationKind::Let);
    decl.declarations
        .iter()
        .flat_map(|d| -> Vec<Stmt> {
            match &d.id {
                BindingPattern::ObjectPattern(obj) => {
                    vec![destructure_object(obj, d.init.as_ref(), locals, mutable, registry, narrow)]
                }
                BindingPattern::ArrayPattern(arr) => {
                    destructure_array(arr, d.init.as_ref(), locals, mutable, registry, narrow)
                }
                _ => {
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
                    let init = d.init.as_ref().map(|e| {
                        expressions::translate_init(e, ty.as_ref(), &Ctx::new(&*locals, registry, narrow))
                    });
                    vec![build_local(&name, mutable, ty.as_ref(), init.as_ref())]
                }
            }
        })
        .collect()
}

/// `const { x, y } = v` → `let Vector { x, y } = v;` (or `mut x, mut y` for
/// `let`). The struct name comes from `v`'s type in the locals table, so only a
/// plain-identifier source is supported. Fields keep their names (snake-case);
/// their types aren't registered yet — the source struct must hold scalars.
fn destructure_object(
    obj: &ObjectPattern,
    init: Option<&Expression>,
    locals: &mut Locals,
    mutable: bool,
    registry: &TypeRegistry,
    narrow: &Narrow,
) -> Stmt {
    let Some(init_expr) = init else {
        return parse_quote!(let _ = ::core::todo!(););
    };
    let value = expressions::translate_expr(init_expr, &Ctx::new(&*locals, registry, narrow));
    let Some(path) = expr_type_path(init_expr, locals) else {
        return parse_quote!(let _ = #value;);
    };
    let fields: Vec<TokenStream> = obj
        .properties
        .iter()
        .filter_map(|p| {
            let name = bindings::property_key_name(&p.key)?;
            if mutable {
                Some(quote!(mut #name))
            } else {
                Some(quote!(#name))
            }
        })
        .collect();
    parse_quote!(let #path { #(#fields),* } = #value;)
}

/// `const [a, b] = xs` → `let a = xs[0 as usize]; let b = xs[1 as usize];`
/// (positional indexing). Holes (`[, c]`) and `…rest` are unsupported; only
/// plain-identifier elements.
fn destructure_array(
    arr: &ArrayPattern,
    init: Option<&Expression>,
    locals: &mut Locals,
    mutable: bool,
    registry: &TypeRegistry,
    narrow: &Narrow,
) -> Vec<Stmt> {
    let Some(init_expr) = init else {
        return vec![parse_quote!(let _ = ::core::todo!();)];
    };
    let value = expressions::translate_expr(init_expr, &Ctx::new(&*locals, registry, narrow));
    arr.elements
        .iter()
        .enumerate()
        .filter_map(|(i, elem)| {
            let pat = elem.as_ref()?;
            let name = bindings::binding_name(pat);
            let idx = syn::Index::from(i);
            Some(build_local(&name, mutable, None, Some(&parse_quote!(#value[#idx]))))
        })
        .collect()
}

/// The `syn::Path` of an expression's type, when the expression is a plain
/// identifier local whose type is known — used to name the struct in a
/// destructure.
fn expr_type_path(expr: &Expression, locals: &Locals) -> Option<syn::Path> {
    let Expression::Identifier(id) = expr else {
        return None;
    };
    locals.get(&bindings::snake(&id.name).to_string()).cloned()
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
