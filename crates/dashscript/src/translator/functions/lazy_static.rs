//! Lazy statics: a module-level `const` or non-mutated `let` whose initializer
//! is not a const-expression literal (an object, a regex, a `Map`/`Set`, a
//! call) lowers to a `static OnceLock<T>` plus an accessor `fn`, since a Rust
//! module has no `fn main` to run a `let` in. Extracted from `functions/mod.rs`.

use std::collections::HashSet;

use oxc_ast::ast::{
    ArrayExpression, ArrayExpressionElement, BinaryExpression, BinaryOperator, BindingPattern,
    CallExpression, Declaration, Expression, NewExpression, ObjectExpression, ObjectPropertyKind,
    Statement, TSTypeParameterInstantiation, VariableDeclarationKind,
};
use oxc_semantic::SymbolId;
use quote::format_ident;
use syn::{
    parse_quote,
    visit_mut::{self, VisitMut},
    Ident, Type,
};

use super::super::analysis;
use super::super::context::{Ctx, Locals, Narrow};
use super::super::expressions;
use super::super::name_table::NameTable;
use super::super::registry::TypeRegistry;
use super::super::types;

/// The body of a top-level `function` — a bare `function f() {}` or an `export
/// function f() {}` (an `ExportNamedDeclaration` wrapping the declaration).
/// `None` for any other statement. Shared by the mutation/escape passes so an
/// `export function` rebinding a module-global is recognized the same as a bare
/// one (B3-2).
fn function_body<'a>(stmt: &'a Statement<'a>) -> Option<&'a oxc_ast::ast::FunctionBody<'a>> {
    match stmt {
        Statement::FunctionDeclaration(f) => f.body.as_deref(),
        Statement::ExportNamedDeclaration(e) => match &e.declaration {
            Some(Declaration::FunctionDeclaration(f)) => f.body.as_deref(),
            _ => None,
        },
        _ => None,
    }
}

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
    // `regress::Regex`), a runtime factory call (`const p = createFactory<T>(...)`)
    // whose return type is inferred from the callee's cross-file signature, a
    // collection constructor (`const s = new Set([…])`) whose element type is
    // inferred from the literal, or an options/config object literal
    // (`const opts = { flag: true, … }`) whose uniform value type is inferred from
    // its properties. Any other init without an annotation has no static type to
    // put on the OnceLock — defer.
    if d.type_annotation.is_none()
        && !matches!(init, Expression::RegExpLiteral(_))
        && !is_inferable_call(init)
        && !is_inferable_new(init)
        && !is_inferable_object(init)
        && !is_inferable_binary(init)
        && !is_typed_assertion(init)
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
        let Some(body) = function_body(stmt) else {
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
        let Some(body) = function_body(stmt) else {
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
    } else if let Expression::TSAsExpression(as_expr) = init {
        // `const X = expr as T` — the `as T` assertion is the type to put on
        // the OnceLock. The init translates as the inner `expr` (the assertion
        // is stripped, see `translate_expr`), so only the type feeds the cell.
        types::translate_type(&as_expr.type_annotation)
    } else if let Expression::TSTypeAssertion(t) = init {
        types::translate_type(&t.type_annotation)
    } else if let Expression::CallExpression(call) = init {
        // A factory or method call with no annotation — infer the return type:
        // an identifier factory call resolves via its cross-file signature
        // (`createFactory<TFile>` ← `createFactory<Opts>`); a method call via its
        // builtin return type (`arr.join("")` → `String`).
        infer_call_return_type(call, registry).unwrap_or_else(|| parse_quote!(_))
    } else if let Expression::NewExpression(new) = init {
        // A collection constructor with no annotation — `new Set([literal])`
        // infers its element type from the first array element so the OnceLock
        // holds `HashSet<T>` (`["jpg", …]` → `HashSet<String>`).
        new_collection_return_type(new).unwrap_or_else(|| parse_quote!(_))
    } else if let Expression::ObjectExpression(obj) = init {
        // An options/config object literal with no annotation — `const opts =
        // { flag: true, … }` infers a uniform value type `V` from its properties
        // so the OnceLock holds `HashMap<String, V>` (`{ a: true, b: true }` →
        // `HashMap<String, bool>`). An anonymous object literal lowers to a
        // `HashMap` (JS objects are dynamic maps); a uniform scalar value type
        // is read off the properties so the cell has a concrete type.
        let val =
            object_literal_value_type(obj).unwrap_or_else(|| parse_quote!(::serde_json::Value));
        parse_quote!(::std::collections::HashMap<String, #val>)
    } else if let Expression::BinaryExpression(bin) = init {
        // A string-concatenation `+` chain — `'<a>' + NS + '</a>'` — lowers to
        // `String`: the init translates to `format!(...)` (Rust's `+` does not
        // apply to `String`), so the OnceLock holds a `String`. A numeric `+`
        // chain is not a candidate (`is_inferable_binary` requires a string leaf).
        let _ = bin;
        parse_quote!(String)
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

/// The scalar value type of a mutable module-global's initializer — the `T` in
/// `thread_local! { RefCell<T> }` (B3-2). A value type only (a number/string/
/// boolean literal), so the get accessor clones the whole value in and out. A
/// collection (`Map`/`Set`/`WeakMap`) or non-literal init is not a value type →
/// `None` (it needs the borrow/borrow_mut path, B3-2b, not get/set accessors).
fn value_init_type(init: &Expression) -> Option<Type> {
    match init {
        Expression::NumericLiteral(_) => Some(parse_quote!(f64)),
        Expression::StringLiteral(_) => Some(parse_quote!(String)),
        Expression::BooleanLiteral(_) => Some(parse_quote!(bool)),
        _ => None,
    }
}

/// Whether `stmt` is a mutable module-global `let` that lowers to a thread-local
/// `RefCell` + get/set accessors (B3-2) — see [`mutable_static_items`]. A `let`
/// rebound or member-mutated from a top-level `function` (per
/// [`mutable_top_level_names`]) whose initializer is a scalar value literal — a
/// value type clones in and out of the `RefCell`, so get/set suffice. A `const`
/// is never mutated; a collection init is not a value type (B3-2b).
pub(in crate::translator) fn mutable_static_candidate(
    stmt: &Statement,
    mutable_names: &HashSet<String>,
    names: &NameTable<'_>,
) -> bool {
    let Statement::VariableDeclaration(decl) = stmt else {
        return false;
    };
    if !matches!(decl.kind, VariableDeclarationKind::Let) {
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
    // A scalar value type only — a collection (B3-2b) defers.
    if value_init_type(init).is_none() {
        return false;
    }
    // Must be mutated from a function — else it stays a plain `fn main` local.
    let name = names.of_binding(id).to_string();
    mutable_names.contains(&name)
}

/// Emit a thread-local `RefCell` + get/set accessors for a mutable module-global
/// value `let` (B3-2) — see [`mutable_static_candidate`]. A top-level `let`
/// mutated from a function cannot live in `fn main` (a Rust fn item cannot close
/// over a `main` local) and cannot live behind an immutable `OnceLock` (B3-1), so
/// it hoists behind a per-thread `RefCell` — matching TS's single-threaded
/// module-global semantics, lock-free. The get accessor clones the value out
/// (`name() -> T`); the set accessor writes it back (`set_name(v)`). Returns the
/// emitted items + the set-accessor ident, so a reassignment `x = v` lowers to
/// `set_x(v)` (the get accessor returns a clone, not an lvalue).
pub(in crate::translator) fn mutable_static_items(
    stmt: &Statement,
    names: &NameTable<'_>,
    registry: &TypeRegistry,
    mutable_names: &HashSet<String>,
) -> Option<(Vec<syn::Item>, Ident)> {
    if !mutable_static_candidate(stmt, mutable_names, names) {
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
    let ty = value_init_type(init)?;
    let setter = format_ident!("set_{}", name);
    // The initializer translates under a fresh empty-body context, the way a
    // lazy static's does — a module top-level binding has no locals/narrowing.
    let locals = Locals::new();
    let narrow = Narrow::default();
    let ctx = Ctx::new(&locals, registry, &narrow, names);
    let init_expr = expressions::translate_expr(init, &ctx);
    let cell = format_ident!("{}_CELL", name.to_string().to_uppercase());
    Some((
        vec![
            parse_quote! {
                thread_local! {
                    static #cell: ::std::cell::RefCell<#ty> = const { ::std::cell::RefCell::new(#init_expr) };
                }
            },
            parse_quote! {
                pub fn #name() -> #ty {
                    #cell.with(|c| c.borrow().clone())
                }
            },
            parse_quote! {
                pub fn #setter(v: #ty) {
                    #cell.with(|c| *c.borrow_mut() = v)
                }
            },
        ],
        setter,
    ))
}

/// The rust names of mutable module-global value `let` candidates (per
/// [`mutable_static_candidate`]) that are referenced — read or written — from
/// any top-level `function`. An entry file runs its executables in `fn main`, so
/// an *unreferenced* binding stays a plain local; a *referenced* one hoists to a
/// `RefCell` (B3-2). A module file hoists every candidate (no `fn main`), so
/// this set is the entry-file filter only. Mirrors [`escaped_lazy_static_names`].
pub(in crate::translator) fn escaped_mutable_static_names(
    program_body: &[Statement],
    names: &NameTable<'_>,
    registry: &TypeRegistry,
    mutable_names: &HashSet<String>,
) -> HashSet<String> {
    let candidates: HashSet<String> = program_body
        .iter()
        .filter(|s| mutable_static_candidate(s, mutable_names, names))
        .filter_map(|s| decl_name(s, names))
        .collect();
    if candidates.is_empty() {
        return HashSet::new();
    }
    let mut escaped = HashSet::new();
    for stmt in program_body {
        let Some(body) = function_body(stmt) else {
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

/// Whether `init` is a call whose return type the translator can infer without
/// an annotation — so a module-global singleton lowers to a `OnceLock` even
/// without one. Two shapes: an identifier factory call (`createFactory<T>(...)`,
/// return type from the callee's cross-file signature) or a method call whose
/// builtin return type is known (`entries.map(...).join("")` → `String`).
fn is_inferable_call(init: &Expression) -> bool {
    let Expression::CallExpression(call) = init else {
        return false;
    };
    match &call.callee {
        Expression::Identifier(_) => true,
        Expression::StaticMemberExpression(sm) => {
            builtin_method_return_type(&sm.property.name).is_some()
        }
        _ => false,
    }
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
    // Prefix the toplevel type with its source crate when the factory is
    // defined in another package — the return type (e.g. `Packer`) is not
    // imported by the consumer, so the OnceLock type needs
    // `crate::<pkg>::Packer<…>` to resolve. Applied before substitution: the
    // substitutor only replaces single-segment type-param idents (`TFile`),
    // so a prefixed multi-segment path (`crate::pkg::Packer`) is untouched
    // while the inner type arg is still instantiated.
    let ret = match sig.source_crate.as_ref() {
        Some(c) => prefix_toplevel_crate(&ret, c),
        None => ret,
    };
    let bindings = bind_type_params(&sig.type_params, call.type_arguments.as_deref());
    Some(substitute_type(ret, &bindings))
}

/// Prefix a return type's toplevel path with `crate::<crate_name>` so a
/// cross-package factory's return type — collected without a prefix in its home
/// package — resolves from the consumer's module: `Packer<TFile>` →
/// `crate::office_open_core::Packer<TFile>`. Non-path types (fn pointers,
/// tuples) are returned cloned unchanged; factory returns are nominal path
/// types.
fn prefix_toplevel_crate(ty: &Type, crate_name: &str) -> Type {
    let Type::Path(tp) = ty else {
        return ty.clone();
    };
    let crate_ident = format_ident!("{}", crate_name);
    let original = &tp.path;
    parse_quote!(crate::#crate_ident::#original)
}

/// The return type of a module-global initializer call, inferred without an
/// annotation: an identifier factory call resolves via its cross-file signature
/// ([`factory_call_return_type`]); a method call resolves via its builtin return
/// type ([`builtin_method_return_type`]). `None` otherwise.
fn infer_call_return_type(call: &CallExpression, registry: &TypeRegistry) -> Option<Type> {
    match &call.callee {
        Expression::Identifier(_) => factory_call_return_type(call, registry),
        Expression::StaticMemberExpression(sm) => builtin_method_return_type(&sm.property.name),
        _ => None,
    }
}

/// The always-`String` ES built-in methods (on `Array` / `String` / `Number`),
/// so a module-level constant ending in one (`arr.map(...).join("")`) has a
/// known static type without an annotation. Matched by method name only — the
/// receiver type is irrelevant since each of these yields `String`.
fn builtin_method_return_type(method: &str) -> Option<Type> {
    match method {
        "join" | "toString" | "toLocaleString" | "slice" | "substring" | "substr"
        | "toUpperCase" | "toLowerCase" | "trim" | "trimStart" | "trimEnd" | "repeat"
        | "padStart" | "padEnd" | "replace" | "replaceAll" | "concat" | "toFixed"
        | "toExponential" | "toPrecision" => Some(parse_quote!(String)),
        _ => None,
    }
}

/// Whether `init` is a `new Set([literal array])` whose element type the
/// translator can infer without an annotation — so a module-global collection
/// singleton lowers to a `OnceLock<HashSet<T>>` even without one. A `Map` or a
/// user class with no static element type to read yields `false` (defer).
fn is_inferable_new(init: &Expression) -> bool {
    let Expression::NewExpression(new) = init else {
        return false;
    };
    new_collection_return_type(new).is_some()
}

/// Whether `init` is an object literal whose value type the translator can infer
/// without an annotation — so a module-global options/config singleton lowers to
/// a `OnceLock<HashMap<String, V>>` even without one. Every property value must be
/// the same scalar kind (all `boolean`, all `number`, or all `string`) so `V` is
/// uniform; a mixed-kind or non-scalar object defers (no single `V`).
fn is_inferable_object(init: &Expression) -> bool {
    let Expression::ObjectExpression(obj) = init else {
        return false;
    };
    object_literal_value_type(obj).is_some()
}

/// The uniform value type `V` of an object literal whose properties are all the
/// same scalar kind — the `V` in `HashMap<String, V>` for a module-global
/// options object (`{ flag: true, on: true }` → `bool`). `None` for an empty
/// object, a non-identifier-keyed/non-ObjectProperty member, a non-scalar value,
/// or mixed kinds (so a uniform `V` cannot be chosen).
fn object_literal_value_type(obj: &ObjectExpression) -> Option<Type> {
    let kinds: Vec<&str> = obj
        .properties
        .iter()
        .filter_map(|p| {
            let ObjectPropertyKind::ObjectProperty(op) = p else {
                return None;
            };
            Some(match &op.value {
                Expression::StringLiteral(_) => "str",
                Expression::NumericLiteral(_) => "num",
                Expression::BooleanLiteral(_) => "bool",
                _ => return None,
            })
        })
        .collect();
    if kinds.is_empty() || kinds.iter().any(|k| *k != kinds[0]) {
        return None;
    }
    Some(match kinds[0] {
        "str" => parse_quote!(String),
        "num" => parse_quote!(f64),
        _ => parse_quote!(bool),
    })
}

/// Whether `init` is a TS type assertion (`expr as T` or `<T>expr`) — so a
/// module-global binding whose only type clue is the assertion lowers to a
/// OnceLock<T> with `T` from the assertion. The init translates as the inner
/// expression (the assertion is stripped, see `translate_expr`); the assertion's
/// type is the cell type, so a `const X = Object.fromEntries(…) as Record<…>`
/// singleton gets a concrete `HashMap<…>` type from the assertion.
fn is_typed_assertion(init: &Expression) -> bool {
    matches!(
        init,
        Expression::TSAsExpression(_) | Expression::TSTypeAssertion(_)
    )
}

/// Whether `init` is a `+` chain that is string concatenation — so a
/// module-global string-built constant (`const XML = '<a>' + NS + '</a>'`)
/// lowers to a `OnceLock<String>` even without an annotation. A `+` chain is
/// string concatenation when any leaf operand is a string literal (TS `+`
/// semantics: one string operand makes the whole chain a string). A purely
/// numeric `+` chain is arithmetic (`f64`) and is not a candidate.
fn is_inferable_binary(init: &Expression) -> bool {
    let Expression::BinaryExpression(bin) = init else {
        return false;
    };
    if !matches!(bin.operator, BinaryOperator::Addition) {
        return false;
    }
    binary_is_string_concat(bin)
}

/// Whether a `+` BinaryExpression is string concatenation: any leaf operand is
/// a string literal, recursing through nested `+` and parens. A syntactic check
/// (no `Ctx`/`local_type`), so it is usable from the candidate filter — the
/// emit path's `concat_is_string` (binary.rs) re-checks with type context.
fn binary_is_string_concat(bin: &BinaryExpression) -> bool {
    operand_is_str_literal(&bin.left) || operand_is_str_literal(&bin.right)
}

/// Whether `expr` is (or contains, via nested `+`/parens) a string literal —
/// the marker that a `+` chain is string concatenation.
fn operand_is_str_literal(expr: &Expression) -> bool {
    match expr {
        Expression::StringLiteral(_) => true,
        Expression::BinaryExpression(bin) if matches!(bin.operator, BinaryOperator::Addition) => {
            binary_is_string_concat(bin)
        }
        Expression::ParenthesizedExpression(p) => operand_is_str_literal(&p.expression),
        _ => false,
    }
}

/// The collection type of a `new Set([literal array])` initializer, inferred
/// without an annotation: `HashSet<T>` where `T` is the first scalar element's
/// type (`["jpg", …]` → `HashSet<String>`). A non-Set `new`, a non-array arg,
/// or a non-scalar first element yields `None`.
fn new_collection_return_type(new: &NewExpression) -> Option<Type> {
    let Expression::Identifier(id) = &new.callee else {
        return None;
    };
    match id.name.as_str() {
        "Set" => {
            // `new Set([literal])` → HashSet<T> (T from the first array element);
            // `new Set<T>()` → HashSet<T> (T from the single type argument).
            if let Some(elem) = set_array_elem_type(new) {
                return Some(parse_quote!(::std::collections::HashSet<#elem>));
            }
            let mut args = new_type_args(new)?.into_iter();
            let elem = args.next()?;
            Some(parse_quote!(::std::collections::HashSet<#elem>))
        }
        "Map" => {
            // `new Map<K, V>()` → HashMap<K, V> (K, V from the two type args).
            let mut args = new_type_args(new)?.into_iter();
            let key = args.next()?;
            let val = args.next()?;
            Some(parse_quote!(::std::collections::HashMap<#key, #val>))
        }
        _ => None,
    }
}

/// The element type of a `new Set([literal array])` argument, inferred from the
/// first array element, or `None` for a non-array-arg shape.
fn set_array_elem_type(new: &NewExpression) -> Option<Type> {
    let arr = new.arguments.first()?.as_expression()?;
    let Expression::ArrayExpression(arr) = arr else {
        return None;
    };
    array_literal_elem_type(arr)
}

/// The translated type arguments of a `new` expression's type-parameter list
/// (`new Map<K, V>()`), or `None` if it has no type arguments.
fn new_type_args(new: &NewExpression) -> Option<Vec<Type>> {
    let ta = new.type_arguments.as_deref()?;
    Some(ta.params.iter().map(types::translate_type).collect())
}

/// The scalar element type of a literal array — taken from its first element
/// (`"jpg"` → `String`, `1` → `f64`, `true` → `bool`), mirroring the element
/// translation `array_elem_expr` applies (`"jpg".to_string()` for a string
/// literal) so the inferred `HashSet<T>` matches what `HashSet::from([...])`
/// builds. `None` for an empty array or a non-scalar (spread / variable /
/// object) first element.
fn array_literal_elem_type(arr: &ArrayExpression) -> Option<Type> {
    match arr.elements.first()? {
        ArrayExpressionElement::StringLiteral(_) => Some(parse_quote!(String)),
        ArrayExpressionElement::NumericLiteral(_) => Some(parse_quote!(f64)),
        ArrayExpressionElement::BooleanLiteral(_) => Some(parse_quote!(bool)),
        _ => None,
    }
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
