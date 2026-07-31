//! `try`/`catch`/`finally` and `throw` statement translation.
//!
//! `try { … } catch (e) { … }` lowers to `catch_unwind` (DashScript owns a
//! `panic = "unwind"` Cargo profile, so unwinding is guaranteed), and `throw
//! expr` lowers to `panic!`. Extracted from the body dispatcher
//! ([`super::translate_stmt`]) so it stays focused on dispatch.

use oxc_ast::ast::{Argument, BindingPattern, Expression, Statement, TryStatement};
use proc_macro2::TokenStream;
use quote::quote;
use syn::{parse_quote, Expr, Path, Stmt};

use super::super::context::{Ctx, Locals, Narrow};
use super::super::expressions;
use super::super::name_table::NameTable;
use super::super::registry::TypeRegistry;
use super::translate_stmt;

/// `try { … } catch (e) { … } [finally { … }]` → a `catch_unwind` around the
/// try body, the catch arm binding the panic payload as a `String` (its
/// message), and the `finally` body appended after the match. DashScript emits
/// `[profile.*] panic = "unwind"` in the `Cargo.toml` it generates (see
/// `package.rs`), so unwinding is guaranteed and `catch_unwind` reliably catches
/// a `.ts` `throw` (which lowers to `panic!`) — this is sound *because*
/// DashScript owns the Cargo.toml, not despite it.
///
/// Control flow out of the try body (`return`/`break`/`continue`) cannot cross
/// the `catch_unwind` closure boundary (a `return` inside the closure would
/// return from the closure, not the function), so it is rejected up front with
/// a compile error. The catch and finally bodies are outside the closure and
/// may return normally.
pub(super) fn translate_try(
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
            // Register the catch param as a `DsError` before translating the
            // body, so `e.constructor.name`/`e.name`/`e.message` route to the
            // `DsError`'s fields (the panic payload is a `DsError`, bound
            // below via `DsError::from_panic`).
            if let Some(cp) = handler.param.as_ref() {
                if let BindingPattern::BindingIdentifier(id) = &cp.pattern {
                    let param = names.of_binding(id);
                    locals.insert(param.to_string(), parse_quote!(__ds::DsError));
                }
            }
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
                                let #param = crate::__ds::DsError::from_panic(&__panic)
                                    .unwrap_or_else(|| crate::__ds::DsError::new("Error", "panic"));
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
        match crate::__ds::catch_quiet(::std::panic::AssertUnwindSafe(|| {
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

/// `throw new RangeError("msg")` / `throw new Error("msg")` →
/// `panic_any(DsError::new("RangeError", "msg"))`; `throw "msg"` →
/// `panic_any(DsError::new("Error", "msg"))`; any other `throw expr` →
/// `panic_any(DsError::new("Error", <expr>.to_string()))`. `panic_any` carries
/// the `DsError` (Send + 'static) as the payload, so `catch (e)` downcasts it
/// back and `e.constructor.name`/`e.name`/`e.message` work without
/// string-matching panic messages.
pub(super) fn throw_stmt(
    arg: &Expression,
    locals: &Locals,
    registry: &TypeRegistry,
    narrow: &Narrow,
    names: &NameTable<'_>,
) -> Stmt {
    if let Some((name, message)) = thrown_error(arg) {
        return parse_quote!(::std::panic::panic_any(crate::__ds::DsError::new(#name, #message)););
    }
    let ctx = Ctx::new(locals, registry, narrow, names);
    let e = expressions::translate_expr(arg, &ctx);
    parse_quote!(::std::panic::panic_any(crate::__ds::DsError::new("Error", format!("{}", #e)));)
}

/// `(name, message)` for `throw new X("msg")` / `throw "msg"` — `name` is the
/// ES error class ("RangeError"/"Error"/…), `message` the rendered string
/// expression. `None` for a non-literal message or an unrecognized error
/// class, so the caller falls back to `throw expr` → `DsError { "Error", … }`.
fn thrown_error(arg: &Expression) -> Option<(&'static str, Expr)> {
    use syn::parse_quote;
    if let Expression::StringLiteral(s) = arg {
        let lit = syn::LitStr::new(s.value.as_str(), proc_macro2::Span::call_site());
        return Some(("Error", parse_quote!(#lit)));
    }
    let Expression::NewExpression(new) = arg else {
        return None;
    };
    let name = match &new.callee {
        Expression::Identifier(id) => match id.name.as_str() {
            "Error" => "Error",
            "RangeError" => "RangeError",
            "TypeError" => "TypeError",
            "SyntaxError" => "SyntaxError",
            "ReferenceError" => "ReferenceError",
            "EvalError" => "EvalError",
            "URIError" => "URIError",
            _ => return None,
        },
        _ => return None,
    };
    let message: Expr = match new.arguments.first() {
        Some(Argument::StringLiteral(s)) => {
            let lit = syn::LitStr::new(s.value.as_str(), proc_macro2::Span::call_site());
            parse_quote!(#lit)
        }
        // A non-string-literal message (a variable, a template) — let the
        // caller render it via the `throw expr` fallback.
        Some(_) => return None,
        None => parse_quote!(""),
    };
    Some((name, message))
}
