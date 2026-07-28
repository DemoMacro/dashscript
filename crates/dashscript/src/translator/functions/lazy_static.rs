//! Lazy statics: a module-level `const` whose initializer is not a
//! const-expression literal (an object, a regex, a `Map`/`Set`, a call) lowers
//! to a `static OnceLock<T>` plus an accessor `fn`, since a Rust module has no
//! `fn main` to run a `let` in. Extracted from `functions/mod.rs`.

use oxc_ast::ast::{BindingPattern, Expression, Statement, VariableDeclarationKind};
use oxc_semantic::SymbolId;
use quote::format_ident;
use syn::{parse_quote, Type};

use super::super::context::{Ctx, Locals, Narrow};
use super::super::expressions;
use super::super::name_table::NameTable;
use super::super::registry::TypeRegistry;
use super::super::types;

/// Whether `stmt` is a module-level `const` that lowers to a lazy static
/// (OnceLock + accessor fn) — see [`lazy_static_items`]. A const-expression
/// literal is NOT a candidate (escape promotion handles it): the value must
/// construct at runtime (an object, a regex, a `Map`/`Set`, a call). Used by
/// `check` (to pass it) and the translate pre-pass (to register its symbol)
/// without the registry the emit path needs.
pub(in crate::translator) fn lazy_static_candidate(stmt: &Statement) -> bool {
    let Statement::VariableDeclaration(decl) = stmt else {
        return false;
    };
    if !matches!(decl.kind, VariableDeclarationKind::Const) {
        return false;
    }
    if decl.declarations.len() != 1 {
        return false;
    }
    let d = &decl.declarations[0];
    if !matches!(d.id, BindingPattern::BindingIdentifier(_)) {
        return false;
    }
    let Some(init) = d.init.as_ref() else {
        return false;
    };
    if matches!(
        init,
        Expression::NumericLiteral(_)
            | Expression::BooleanLiteral(_)
            | Expression::StringLiteral(_)
    ) {
        return false;
    }
    // An inferable type: an explicit annotation on the declarator, or a regex
    // literal (→ `regress::Regex`). Anything else without an annotation has no
    // static type to put on the OnceLock — defer.
    d.type_annotation.is_some() || matches!(init, Expression::RegExpLiteral(_))
}

/// The `SymbolId` of a lazy-static candidate's binding, for pre-pass
/// registration so a reference before the definition in source order still
/// emits the accessor call (module bindings are hoisted). `None` for an
/// unbound symbol.
pub(in crate::translator) fn lazy_static_sym(
    stmt: &Statement,
    names: &NameTable<'_>,
) -> Option<SymbolId> {
    let Statement::VariableDeclaration(decl) = stmt else {
        return None;
    };
    let d = &decl.declarations[0];
    names.symbol_of_pattern(&d.id)
}

/// Emit a lazy static (OnceLock + accessor fn) for a module-level `const` whose
/// initializer is not a const-expression literal — see
/// [`lazy_static_candidate`]. An ES module top-level `const` constructs its
/// value once at first use; a Rust module has no `fn main` to run a `let` in,
/// so the value lives behind a `static OnceLock` initialized lazily by a `fn
/// name() -> &'static T`. The accessor keeps the snake-case binding name so
/// every reference resolves to `name()` unchanged.
pub(in crate::translator) fn lazy_static_items(
    stmt: &Statement,
    names: &NameTable<'_>,
    registry: &TypeRegistry,
) -> Option<Vec<syn::Item>> {
    if !lazy_static_candidate(stmt) {
        return None;
    }
    let Statement::VariableDeclaration(decl) = stmt else {
        return None;
    };
    let d = &decl.declarations[0];
    let BindingPattern::BindingIdentifier(id) = &d.id else {
        return None;
    };
    let init = d.init.as_ref()?;
    let name = names.of_binding(id);
    let ty: Type = if let Some(ta) = d.type_annotation.as_ref() {
        types::translate_type(&ta.type_annotation)
    } else {
        // A regex literal with no annotation — `regress::Regex`.
        parse_quote!(regress::Regex)
    };
    // The initializer translates under a fresh empty-body context: a module
    // top-level binding has no locals/narrowing, only the type registry and
    // name table — enough for a literal or a constructor call.
    let locals = Locals::new();
    let narrow = Narrow::default();
    let ctx = Ctx::new(&locals, registry, &narrow, names);
    let init_expr = expressions::translate_expr(init, &ctx);
    // SCREAMING_SNAKE for the OnceLock cell (rustc convention); the accessor
    // keeps the snake name so references resolve to `name()` unchanged.
    let cell = format_ident!("{}_CELL", name.to_string().to_uppercase());
    Some(vec![
        parse_quote! {
            static #cell: ::std::sync::OnceLock<#ty> = ::std::sync::OnceLock::new();
        },
        parse_quote! {
            pub fn #name() -> &'static #ty {
                #cell.get_or_init(|| #init_expr)
            }
        },
    ])
}
