//! Escape promotion: top-level `const` bindings whose initializer is a Rust
//! const-expression literal (`number`/`boolean`/`string`) hoist to crate-level
//! `const` items, so a top-level `function` can reference them (a Rust fn item
//! cannot close over an `fn main` local). Extracted from `functions/mod.rs`.

use std::collections::HashSet;

use oxc_ast::ast::{
    BindingPattern, Expression, Statement, VariableDeclaration, VariableDeclarationKind,
};
use oxc_semantic::SymbolId;
use syn::{parse_quote, Expr, Type};

use super::super::analysis;
use super::super::expressions;
use super::super::name_table::NameTable;
use super::super::registry::TypeRegistry;

/// The scalar kind a promoted `const` literal lowers to — see
/// [`promotable_const_info`].
#[derive(PartialEq, Eq, Clone, Copy)]
pub(in crate::translator) enum ConstKind {
    Number,
    Bool,
    Str,
}

impl ConstKind {
    pub(in crate::translator) fn is_number(self) -> bool {
        matches!(self, Self::Number)
    }
}

/// The (symbol, rust-name, kind) of a top-level `const` or non-mutated `let`
/// binding whose initializer is a Rust const-expression literal (a `number`,
/// `boolean`, or `string`) — a candidate for escape promotion to a crate-level
/// `const` item (A3). A string literal lowers to `&'static str`, which is a
/// Rust const, so `const X = "    "` (a module-level format/indent constant)
/// promotes just like a number. A non-mutated `let` with a literal initializer
/// promotes the same way (B3-1a): it is immutable in practice, so a `const`
/// item is the zero-overhead lowering. `None` for `var`, a mutated `let` (it
/// needs a `thread_local!` `RefCell`, B3-2), a multi-declarator declaration,
/// destructuring, a missing initializer, or any non-literal initializer (a
/// runtime value needs `static` + interior mutability — `lazy_static`).
/// Reused by `check` so the two agree on what is promotable.
pub(in crate::translator) fn promotable_const_info(
    decl: &VariableDeclaration,
    names: &NameTable<'_>,
    mutable_names: &HashSet<String>,
) -> Option<(SymbolId, String, ConstKind)> {
    if !matches!(
        decl.kind,
        VariableDeclarationKind::Const | VariableDeclarationKind::Let
    ) {
        return None;
    }
    if decl.declarations.len() != 1 {
        return None;
    }
    let d = &decl.declarations[0];
    let BindingPattern::BindingIdentifier(id) = &d.id else {
        return None;
    };
    let kind = match d.init.as_ref()? {
        Expression::NumericLiteral(_) => ConstKind::Number,
        Expression::BooleanLiteral(_) => ConstKind::Bool,
        Expression::StringLiteral(_) => ConstKind::Str,
        _ => return None,
    };
    let sym = names.symbol_of_pattern(&d.id)?;
    // A `let` must be non-mutated to lower to a `const` item — a mutated `let`
    // needs a `thread_local!` `RefCell` (B3-2). A `const` is never mutated.
    if matches!(decl.kind, VariableDeclarationKind::Let) {
        let name = names.of_binding(id).to_string();
        if mutable_names.contains(&name) {
            return None;
        }
    }
    Some((sym, names.of_binding(id).to_string(), kind))
}

/// The rust names of top-level `const` or non-mutated `let` bindings that are
/// (a) const-expression literals and (b) referenced from at least one top-level
/// `function` — the escape set promoted to crate-level `const` items (A3,
/// extended to non-mutated `let` in B3-1a). Keyed by the same per-body rust
/// name `analysis::analyze` records in `use_counts`, mirroring
/// `check::check_escape`.
pub(in crate::translator) fn promoted_const_names(
    program_body: &[Statement],
    names: &NameTable<'_>,
    registry: &TypeRegistry,
    mutable_names: &HashSet<String>,
) -> HashSet<String> {
    let candidates: HashSet<String> = program_body
        .iter()
        .filter_map(|s| match s {
            Statement::VariableDeclaration(v) => Some(v),
            _ => None,
        })
        .filter_map(|v| promotable_const_info(v, names, mutable_names).map(|(_, n, _)| n))
        .collect();
    if candidates.is_empty() {
        return HashSet::new();
    }
    let mut escaped = HashSet::new();
    for stmt in program_body {
        let Statement::FunctionDeclaration(f) = stmt else {
            continue;
        };
        let Some(body) = f.body.as_deref() else {
            continue;
        };
        let analysis = analysis::analyze(
            &body.statements,
            names,
            &registry.mut_methods,
            &registry.ref_params,
        );
        for k in analysis.use_counts.keys() {
            if candidates.contains(k) {
                escaped.insert(k.clone());
            }
        }
    }
    escaped
}

/// The rust names of *every* top-level const-expr `const` or non-mutated `let`
/// binding in the file, regardless of whether a function references it. A
/// module file has no `fn main` to run a top-level binding in, so each
/// const-expr `const`/`let` must promote to a crate item — there is no
/// "escape" set to compute, unlike the entry path ([`promoted_const_names`]).
pub(in crate::translator) fn all_promotable_const_names(
    program_body: &[Statement],
    names: &NameTable<'_>,
    mutable_names: &HashSet<String>,
) -> HashSet<String> {
    program_body
        .iter()
        .filter_map(|s| match s {
            Statement::VariableDeclaration(v) => Some(v),
            _ => None,
        })
        .filter_map(|v| promotable_const_info(v, names, mutable_names).map(|(_, n, _)| n))
        .collect()
}

/// Build the crate-level `const` item for a promoted top-level `const` or
/// non-mutated `let` declaration, if `stmt` is one whose rust name is in
/// `promoted`. The item keeps the binding's snake-case rust name (lowercase) so
/// every reference — in `fn main` and in any function — resolves to it
/// unchanged; the `#[allow(non_upper_case_globals)]` attribute silences the
/// rustc lint for a lowercase `const` (the lint is the only reason a `const`
/// is conventionally SCREAMING_SNAKE; the name itself is arbitrary, and
/// matching the reference resolution avoids touching every call site).
pub(in crate::translator) fn promoted_const_item(
    stmt: &Statement,
    promoted: &HashSet<String>,
    names: &NameTable<'_>,
) -> Option<syn::Item> {
    let Statement::VariableDeclaration(decl) = stmt else {
        return None;
    };
    if !matches!(
        decl.kind,
        VariableDeclarationKind::Const | VariableDeclarationKind::Let
    ) {
        return None;
    }
    if decl.declarations.len() != 1 {
        return None;
    }
    let d = &decl.declarations[0];
    let BindingPattern::BindingIdentifier(id) = &d.id else {
        return None;
    };
    let name = names.of_binding(id);
    if !promoted.contains(&name.to_string()) {
        return None;
    }
    let init = d.init.as_ref()?;
    let (ty, init_expr): (Type, Expr) = match init {
        Expression::NumericLiteral(n) => (
            parse_quote!(f64),
            expressions::literals::numeric_expr(n.value),
        ),
        Expression::BooleanLiteral(b) => (
            parse_quote!(bool),
            expressions::literals::bool_expr(b.value),
        ),
        // A string literal is `&'static str` — a Rust const, so a module-level
        // `const INDENT = "    "` promotes to `const INDENT: &str = "    ";`.
        Expression::StringLiteral(s) => {
            let lit = proc_macro2::Literal::string(&s.value);
            (parse_quote!(&'static str), parse_quote!(#lit))
        }
        _ => return None,
    };
    Some(parse_quote! {
        #[allow(non_upper_case_globals)]
        const #name: #ty = #init_expr;
    })
}
