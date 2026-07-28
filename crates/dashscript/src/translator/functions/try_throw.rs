//! `try`/`catch`/`finally` and `throw` statement translation.
//!
//! `try { … } catch (e) { … }` lowers to `catch_unwind` (DashScript owns a
//! `panic = "unwind"` Cargo profile, so unwinding is guaranteed), and `throw
//! expr` lowers to `panic!`. Extracted from the body dispatcher
//! ([`super::translate_stmt`]) so it stays focused on dispatch.

use oxc_ast::ast::{Argument, BindingPattern, Expression, Statement, TryStatement};
use proc_macro2::TokenStream;
use quote::quote;
use syn::{parse_quote, LitStr, Path, Stmt};

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
/// `throw expr` → `panic!("{}", expr)` (Rust has no `throw`; `.ts` errors are
/// treated as unrecoverable panics, since there is no `try`/`catch` yet).
pub(super) fn throw_stmt(
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
