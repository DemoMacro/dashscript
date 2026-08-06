//! Object and array destructuring patterns → `syn` statements.

use oxc_ast::ast::{ArrayPattern, BindingPattern, Expression, ObjectPattern};
use proc_macro2::TokenStream;
use quote::quote;
use syn::{parse_quote, Expr, Ident, Stmt};

use super::super::context::{Ctx, Locals, Narrow};
use super::super::name_table::NameTable;
use super::super::registry::TypeRegistry;
use super::super::{bindings, expressions};
use super::build_local;

/// `const { x, y } = v` → `let Vector { x, y } = v;` (or `mut x, mut y` for
/// `let`). The struct name comes from `v`'s type in the locals table, so only a
/// plain-identifier source is supported. Fields keep their names (snake-case);
/// their types aren't registered yet — the source struct must hold scalars.
pub(super) fn destructure_object(
    obj: &ObjectPattern,
    init: Option<&Expression>,
    locals: &mut Locals,
    mutable: bool,
    registry: &TypeRegistry,
    narrow: &Narrow,
    names: &NameTable<'_>,
) -> Vec<Stmt> {
    let Some(init_expr) = init else {
        return vec![parse_quote!(let _ = ::core::todo!();)];
    };
    let ctx = Ctx::new(&*locals, registry, narrow, names);
    let value = expressions::translate_expr(init_expr, &ctx);
    let Some(path) = expr_type_path(init_expr, locals) else {
        // Fallback: init is a compound expression whose type can't be statically
        // resolved (`await reader.read()`, a member/chain expression, …) —
        // `expr_type_path` only recognizes a plain identifier, so there's no
        // struct name to emit a struct pattern. The old `let _ = #value;`
        // dropped every field binding, so `done`/`value`/… were never in scope
        // (E0425). Bind a temp first (init evaluates once), then emit one field
        // access per binding so the names enter scope; Rust infers field types.
        let tmp: Ident = parse_quote!(__ds_tmp);
        let mut out: Vec<Stmt> = vec![parse_quote!(let #tmp = #value;)];
        for p in &obj.properties {
            let Some(key_name) = bindings::property_key_name(&p.key) else {
                continue;
            };
            let binding_name = match &p.value {
                BindingPattern::BindingIdentifier(id) => {
                    let var = names.of_binding(id);
                    if var != key_name {
                        var
                    } else {
                        key_name.clone()
                    }
                }
                _ => key_name.clone(),
            };
            let binding = if mutable {
                quote!(mut #binding_name)
            } else {
                quote!(#binding_name)
            };
            out.push(parse_quote!(let #binding = #tmp.#key_name;));
            if let BindingPattern::AssignmentPattern(ap) = &p.value {
                let default = expressions::translate_expr(&ap.right, &ctx);
                out.push(parse_quote!(let #binding_name = #binding_name.unwrap_or(#default);));
            }
        }
        return out;
    };
    let mut fields: Vec<TokenStream> = Vec::new();
    // `{ x = d }`: a default on a (typically optional) field — after the struct
    // pattern binds `x: Option<T>`, shadow it with `x.unwrap_or(d)`. Each
    // statement is emitted at the enclosing scope (no wrapping block) so the
    // binding stays visible to later statements.
    let mut defaults: Vec<(Ident, Expr)> = Vec::new();
    for p in &obj.properties {
        let Some(key_name) = bindings::property_key_name(&p.key) else {
            continue;
        };
        // `{ x: y }`: a renamed binding emits the Rust field-pattern `x: y`;
        // the shorthand `{ x }` stays as a bare `x`.
        let renamed = match &p.value {
            BindingPattern::BindingIdentifier(id) => {
                let var = names.of_binding(id);
                (var != key_name).then_some(var)
            }
            _ => None,
        };
        let field = match &renamed {
            Some(var) => {
                let binding = if mutable {
                    quote!(mut #var)
                } else {
                    quote!(#var)
                };
                quote!(#key_name: #binding)
            }
            None => {
                if mutable {
                    quote!(mut #key_name)
                } else {
                    quote!(#key_name)
                }
            }
        };
        fields.push(field);
        if let BindingPattern::AssignmentPattern(ap) = &p.value {
            let default = expressions::translate_expr(&ap.right, &ctx);
            let var = renamed.clone().unwrap_or_else(|| key_name.clone());
            defaults.push((var, default));
        }
    }
    // `..` lets a partial destructure (`{ tag }` on a struct with more fields)
    // compile; it's a no-op when all fields are listed.
    let mut out: Vec<Stmt> = vec![parse_quote!(let #path { #(#fields),*, .. } = #value;)];
    for (name, default) in &defaults {
        out.push(parse_quote!(let #name = #name.unwrap_or(#default);));
    }
    out
}

/// `const [a, b] = xs` → `let a = xs[0]; let b = xs[1];` (positional indexing
/// via `syn::Index`, which carries no literal suffix). Holes (`[, c]`) are
/// skipped — a `None` element is filtered out while its original index is
/// kept; `...rest` collects the tail as a new `Vec`.
pub(super) fn destructure_array(
    arr: &ArrayPattern,
    init: Option<&Expression>,
    locals: &mut Locals,
    mutable: bool,
    registry: &TypeRegistry,
    narrow: &Narrow,
    names: &NameTable<'_>,
) -> Vec<Stmt> {
    let Some(init_expr) = init else {
        return vec![parse_quote!(let _ = ::core::todo!();)];
    };
    let value =
        expressions::translate_expr(init_expr, &Ctx::new(&*locals, registry, narrow, names));
    let mut stmts: Vec<Stmt> = arr
        .elements
        .iter()
        .enumerate()
        .filter_map(|(i, elem)| {
            let pat = elem.as_ref()?;
            let name = names.of_pattern(pat);
            let idx = syn::Index::from(i);
            Some(build_local(
                &name,
                mutable,
                None,
                Some(&parse_quote!(#value[#idx])),
            ))
        })
        .collect();
    // `...rest` collects the remaining elements (after the last bound position)
    // as a new `Vec`. A default on the rest is unsupported.
    if let Some(rest) = &arr.rest {
        let name = names.of_pattern(&rest.argument);
        let start = syn::Index::from(arr.elements.len());
        stmts.push(build_local(
            &name,
            mutable,
            None,
            Some(&parse_quote!(#value[#start..].to_vec())),
        ));
    }
    stmts
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
