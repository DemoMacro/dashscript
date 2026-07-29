//! Lazy statics: a module-level `const` or non-mutated `let` whose initializer
//! is not a const-expression literal (an object, a regex, a `Map`/`Set`, a
//! call) lowers to a `static OnceLock<T>` plus an accessor `fn`, since a Rust
//! module has no `fn main` to run a `let` in. Extracted from `functions/mod.rs`.

use std::collections::HashSet;

use oxc_ast::ast::{
    BindingPattern, CallExpression, Expression, Statement, TSTypeParameterInstantiation,
    VariableDeclarationKind,
};
use oxc_semantic::SymbolId;
use quote::format_ident;
use syn::{
    parse_quote,
    visit_mut::{self, VisitMut},
    Type,
};

use super::super::analysis;
use super::super::context::{Ctx, Locals, Narrow};
use super::super::expressions;
use super::super::name_table::NameTable;
use super::super::registry::TypeRegistry;
use super::super::types;

/// Whether `stmt` is a module-level `const` or non-mutated `let` that lowers to
/// a lazy static (OnceLock + accessor fn) — see [`lazy_static_items`]. A
/// const-expression literal is NOT a candidate (escape promotion handles it):
/// the value must construct at runtime (an object, a regex, a `Map`/`Set`, a
/// call). A `let` that is mutated anywhere (rebound or member-mutated — see
/// [`mutable_top_level_names`]) is NOT a candidate either: an immutable
/// `OnceLock` cannot hold it; it needs a `thread_local!` `RefCell` (B3-2). Used
/// by `check` (to pass it) and the translate pre-pass (to register its symbol)
/// without the registry the emit path needs.
pub(in crate::translator) fn lazy_static_candidate(
    stmt: &Statement,
    mutable_names: &HashSet<String>,
    names: &NameTable<'_>,
) -> bool {
    let Statement::VariableDeclaration(decl) = stmt else {
        return false;
    };
    if !matches!(
        decl.kind,
        VariableDeclarationKind::Const | VariableDeclarationKind::Let
    ) {
        return false;
    }
    if decl.declarations.len() != 1 {
        return false;
    }
    let d = &decl.declarations[0];
    let BindingPattern::BindingIdentifier(id) = &d.id else {
        return false;
    };
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
    // An inferable type: an explicit annotation, a regex literal (→
    // `regress::Regex`), or a runtime factory call (`const p = createFactory<T>(...)`)
    // whose return type is inferred from the callee's cross-file signature. Any
    // other init without an annotation has no static type to put on the
    // OnceLock — defer.
    if d.type_annotation.is_none()
        && !matches!(init, Expression::RegExpLiteral(_))
        && !is_factory_call(init)
    {
        return false;
    }
    // A `let` mutated anywhere cannot live behind an immutable OnceLock — it
    // needs a `thread_local!` `RefCell` (B3-2). A `const` is never mutated.
    if matches!(decl.kind, VariableDeclarationKind::Let) {
        let name = names.of_binding(id).to_string();
        if mutable_names.contains(&name) {
            return false;
        }
    }
    true
}

/// The rust names of top-level bindings that are mutated (rebound or
/// member-mutated) from inside any top-level `function` — a `let` in this set
/// cannot lower to an immutable `OnceLock` and needs a `thread_local!`
/// `RefCell` (B3-2). Keyed by the same per-symbol rust name `analysis::analyze`
/// records, mirroring `escape::promoted_const_names`. A `let` mutated only at
/// the top level (not from a function) stays a plain local — it does not
/// escape, so it is not in this set.
pub(in crate::translator) fn mutable_top_level_names(
    program_body: &[Statement],
    names: &NameTable<'_>,
    registry: &TypeRegistry,
) -> HashSet<String> {
    let top_names: HashSet<String> = program_body
        .iter()
        .filter_map(|s| match s {
            Statement::VariableDeclaration(v) => Some(v),
            _ => None,
        })
        .flat_map(|v| v.declarations.iter())
        .filter_map(|d| match &d.id {
            BindingPattern::BindingIdentifier(id) => Some(names.of_binding(id).to_string()),
            _ => None,
        })
        .collect();
    if top_names.is_empty() {
        return HashSet::new();
    }
    let mut mutable = HashSet::new();
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
        for k in analysis.mutated.iter().chain(&analysis.member_mutated) {
            if top_names.contains(k) {
                mutable.insert(k.clone());
            }
        }
    }
    mutable
}

/// The rust name of a top-level `VariableDeclaration`'s binding (the first
/// declarator), or `None` for any other statement / a non-identifier pattern.
pub(in crate::translator) fn decl_name(stmt: &Statement, names: &NameTable<'_>) -> Option<String> {
    let Statement::VariableDeclaration(decl) = stmt else {
        return None;
    };
    let d = decl.declarations.first()?;
    let BindingPattern::BindingIdentifier(id) = &d.id else {
        return None;
    };
    Some(names.of_binding(id).to_string())
}

/// The rust names of top-level lazy-static candidates (a non-const-expr `const`
/// or a non-mutated `let`, per [`lazy_static_candidate`]) that are referenced —
/// read or written — from any top-level `function`. An entry file runs its
/// executables in `fn main`, so an *unreferenced* binding stays a plain local
/// (source-order, zero-cost); a *referenced* one cannot — a Rust fn item cannot
/// close over an `fn main` local — so it hoists to a `static OnceLock` +
/// accessor (B3-1b). A module file hoists every candidate regardless (no `fn
/// main`), so this set is the entry-file filter only. Keyed by the per-symbol
/// rust name `analysis::analyze` records, mirroring [`mutable_top_level_names`].
pub(in crate::translator) fn escaped_lazy_static_names(
    program_body: &[Statement],
    names: &NameTable<'_>,
    registry: &TypeRegistry,
    mutable_names: &HashSet<String>,
) -> HashSet<String> {
    let candidates: HashSet<String> = program_body
        .iter()
        .filter(|s| lazy_static_candidate(s, mutable_names, names))
        .filter_map(|s| decl_name(s, names))
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
        for k in analysis
            .use_counts
            .keys()
            .chain(analysis.mutated.iter())
            .chain(analysis.member_mutated.iter())
        {
            if candidates.contains(k) {
                escaped.insert(k.clone());
            }
        }
    }
    escaped
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

/// Emit a lazy static (OnceLock + accessor fn) for a module-level `const` or
/// non-mutated `let` whose initializer is not a const-expression literal — see
/// [`lazy_static_candidate`]. An ES module top-level `const`/`let` constructs
/// its value once at first use; a Rust module has no `fn main` to run a `let`
/// in, so the value lives behind a `static OnceLock` initialized lazily by a
/// `fn name() -> &'static T`. The accessor keeps the snake-case binding name so
/// every reference resolves to `name()` unchanged.
pub(in crate::translator) fn lazy_static_items(
    stmt: &Statement,
    names: &NameTable<'_>,
    registry: &TypeRegistry,
    mutable_names: &HashSet<String>,
) -> Option<Vec<syn::Item>> {
    if !lazy_static_candidate(stmt, mutable_names, names) {
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
    } else if let Expression::CallExpression(call) = init {
        // A factory call with no annotation — infer the return type from the
        // callee's cross-file signature, instantiating its generic params with
        // the call's type arguments (`createFactory<TFile>` ← `createFactory<Opts>`).
        factory_call_return_type(call, registry).unwrap_or_else(|| parse_quote!(_))
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

/// Whether `init` is a runtime factory call (`ident<args>(...)`) — its return
/// type is inferred from the callee's cross-file signature, so a module-global
/// singleton (`const p = createFactory<T>(...)`) lowers to a `OnceLock` even
/// without an explicit annotation.
fn is_factory_call(init: &Expression) -> bool {
    matches!(
        init,
        Expression::CallExpression(call) if matches!(&call.callee, Expression::Identifier(_))
    )
}

/// The instantiated return type of a factory call, looked up from the callee's
/// cross-file signature (`registry.function_signatures`) with its generic type
/// parameters substituted by the call's type arguments. `None` if the callee is
/// not a known signature or its return type is absent/void.
fn factory_call_return_type(call: &CallExpression, registry: &TypeRegistry) -> Option<Type> {
    let Expression::Identifier(id) = &call.callee else {
        return None;
    };
    let sig = registry.function_signatures.get(id.name.as_ref())?;
    let ret = sig.return_type.clone()?;
    let bindings = bind_type_params(&sig.type_params, call.type_arguments.as_deref());
    Some(substitute_type(ret, &bindings))
}

/// Bind a signature's generic type parameters to a call's type arguments
/// (`TFile` ← `WorkbookOptions`), keyed by parameter name.
fn bind_type_params(
    params: &[String],
    args: Option<&TSTypeParameterInstantiation>,
) -> std::collections::HashMap<String, Type> {
    let mut bindings = std::collections::HashMap::new();
    if let Some(args) = args {
        for (param, arg) in params.iter().zip(args.params.iter()) {
            bindings.insert(param.clone(), types::translate_type(arg));
        }
    }
    bindings
}

/// Substitute a type's generic parameters per `bindings`: a single-segment path
/// type whose ident is a bound param is replaced by the bound type
/// (`Packer<TFile>` with `TFile → WorkbookOptions` → `Packer<WorkbookOptions>`).
fn substitute_type(mut ty: Type, bindings: &std::collections::HashMap<String, Type>) -> Type {
    visit_mut::visit_type_mut(&mut Subst { bindings }, &mut ty);
    ty
}

struct Subst<'a> {
    bindings: &'a std::collections::HashMap<String, Type>,
}

impl<'a> VisitMut for Subst<'a> {
    fn visit_type_mut(&mut self, ty: &mut Type) {
        if let Type::Path(tp) = ty {
            if tp.path.segments.len() == 1 {
                if let Some(repl) = self.bindings.get(&tp.path.segments[0].ident.to_string()) {
                    *ty = repl.clone();
                    return;
                }
            }
        }
        visit_mut::visit_type_mut(self, ty);
    }
}
