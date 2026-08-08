//! Top-level statement classification and export-declaration translation: which
//! statements run in the implicit `fn main`, and how `export` / `export default`
//! declarations lower to Rust items. Extracted from `functions/mod.rs`.

use oxc_ast::ast::{
    BindingPattern, Declaration, ExportDefaultDeclarationKind, Expression, Statement,
    VariableDeclaration,
};
use syn::parse_quote;

use super::super::name_table::NameTable;
use super::super::registry::TypeRegistry;
use super::super::{bindings, class, declarations};
use super::engine::{engine_arrow_fn_item, translate_const_arrow_to_fn};
use super::escape;
use super::{is_dynamic_fn, translate_function};

/// Whether a top-level statement is *executable* — it runs in source order,
/// the way Node runs a script's top-level statements immediately — rather than
/// a declaration hoisted to a Rust item. Pure-TS execution semantics: a
/// `function` / `class` / `interface` / `type` / `import` / `export` declaration
/// does not run; only executable statements do. The translator collects these
/// into the implicit `fn main` it emits, so a file with only declarations
/// yields an empty `fn main {}` (like a Node script that defines functions but
/// never calls them). Shared with `check`, which treats them as legitimate
/// top-level statements rather than unmapped.
pub(in crate::translator) fn is_executable_top_level(stmt: &Statement) -> bool {
    // A `const` arrow (`const f = () => …`) lowers to a `fn` item — a
    // declaration, not an executable statement — so it is excluded: it goes
    // through `translate_statement` to `translate_const_arrow_to_fn`, never
    // into the implicit `fn main` (or, for a module, the rejected executable
    // set). Every other kind here runs in source order inside `fn main`.
    if is_const_arrow(stmt) {
        return false;
    }
    // An `export const x = …` / `export let x = …` runs in source order (it
    // binds a value that constructs at runtime → lazy static), so it is an
    // executable top-level wrapped in an `ExportNamedDeclaration`. A const-expr
    // literal (`export const N = 5` → `pub const` item) and a const arrow
    // (`export const f = () => …` → `fn` item, excluded above) are declarations,
    // not executable; `export function`/`class`/`type`/`interface` likewise.
    if let Statement::ExportNamedDeclaration(e) = stmt {
        if let Some(Declaration::VariableDeclaration(v)) = &e.declaration {
            let is_literal = v
                .declarations
                .first()
                .and_then(|d| d.init.as_ref())
                .map(|init| {
                    matches!(
                        init,
                        Expression::NumericLiteral(_)
                            | Expression::BooleanLiteral(_)
                            | Expression::StringLiteral(_)
                    )
                })
                .unwrap_or(false);
            if !is_literal {
                return true;
            }
        }
    }
    matches!(
        stmt,
        Statement::ExpressionStatement(_)
            | Statement::VariableDeclaration(_)
            | Statement::IfStatement(_)
            | Statement::WhileStatement(_)
            | Statement::DoWhileStatement(_)
            | Statement::ForStatement(_)
            | Statement::ForOfStatement(_)
            | Statement::ForInStatement(_)
            | Statement::SwitchStatement(_)
            | Statement::TryStatement(_)
            | Statement::ThrowStatement(_)
            | Statement::BlockStatement(_)
    )
}

/// Whether `stmt` is a single-declarator `const` arrow-function binding
/// (`const f = () => …`), which lowers to a `fn` item rather than an
/// executable statement.
pub(super) fn is_const_arrow(stmt: &Statement) -> bool {
    let v = match stmt {
        Statement::VariableDeclaration(v) => v,
        // `export const f = () => …` wraps the same arrow binding in an
        // `ExportNamedDeclaration`; it lowers to a `fn` item too.
        Statement::ExportNamedDeclaration(e) => match &e.declaration {
            Some(Declaration::VariableDeclaration(v)) => v,
            _ => return false,
        },
        _ => return false,
    };
    v.declarations.len() == 1
        && matches!(
            &v.declarations[0].init,
            Some(Expression::ArrowFunctionExpression(_))
        )
}

/// The snake names of every const-arrow fn (`const f = () => …` / `export
/// const f = () => …`) declared in `program_body` — the legal target of a
/// fn-alias const (`const g = f`), which lowers to a `use f as g;` item (a fn
/// value alias is a name rename, not a runtime binding — the fn is already a
/// static item, so no `OnceLock` is needed). Collected once so a forward alias
/// (an alias before the fn in source order) resolves too.
pub(in crate::translator) fn const_arrow_fn_names(
    program_body: &[Statement],
    names: &NameTable<'_>,
) -> std::collections::HashSet<String> {
    program_body
        .iter()
        .filter_map(|s| {
            let v = match s {
                Statement::VariableDeclaration(v) => v,
                Statement::ExportNamedDeclaration(e) => match &e.declaration {
                    Some(Declaration::VariableDeclaration(v)) => v,
                    _ => return None,
                },
                _ => return None,
            };
            let d = v.declarations.first()?;
            if !matches!(d.init, Some(Expression::ArrowFunctionExpression(_))) {
                return None;
            }
            Some(names.of_pattern(&d.id).to_string())
        })
        .collect()
}

/// A fn-alias const (`const g = f` / `export const g = f` where `f` is a
/// same-file const-arrow fn) lowers to a `use f as g;` item — a fn value alias
/// renames the fn rather than binding a runtime value (the fn is a static item,
/// so no `OnceLock`). `pub use` when the alias is exported. `None` for any other
/// shape (a non-identifier init, or an identifier that is not a const-arrow fn
/// — a lazy-static alias goes through the lazy-static path instead).
pub(in crate::translator) fn fn_alias_use_item(
    stmt: &Statement,
    const_arrow_names: &std::collections::HashSet<String>,
    names: &NameTable<'_>,
) -> Option<syn::Item> {
    let (v, is_export) = match stmt {
        Statement::VariableDeclaration(v) => (v, false),
        Statement::ExportNamedDeclaration(e) => match &e.declaration {
            Some(Declaration::VariableDeclaration(v)) => (v, true),
            _ => return None,
        },
        _ => return None,
    };
    let d = v.declarations.first()?;
    let BindingPattern::BindingIdentifier(_id) = &d.id else {
        return None;
    };
    let Expression::Identifier(callee) = d.init.as_ref()? else {
        return None;
    };
    let alias = names.of_pattern(&d.id);
    let target = bindings::snake(&callee.name);
    if alias == target || !const_arrow_names.contains(&target.to_string()) {
        return None;
    }
    Some(if is_export {
        parse_quote! { pub use #target as #alias; }
    } else {
        parse_quote! { use #target as #alias; }
    })
}

/// Translate the inner declaration of an `export` (`export function` /
/// `export class` / `export interface` / `export type`). Re-exports and
/// unsupported kinds (enum) yield `[]`. A class yields its `struct` plus `impl`.
pub(super) fn translate_exported_declaration(
    decl: &Declaration,
    registry: &TypeRegistry,
    names: &NameTable<'_>,
) -> Vec<syn::Item> {
    match decl {
        Declaration::FunctionDeclaration(func) => {
            vec![syn::Item::Fn(translate_function(func, registry, names))]
        }
        Declaration::ClassDeclaration(class) => class::translate_class(class, registry, names),
        Declaration::TSInterfaceDeclaration(iface) => {
            declarations::translate_interface(iface, registry)
        }
        Declaration::TSTypeAliasDeclaration(alias) => declarations::translate_type_alias(alias),
        // `export const name = <T>(params): ret => body` — a const-bound arrow
        // is a named function (the binding names it), so it lowers to a `fn`
        // item. Only arrow initializers map; any other const value stays a
        // top-level executable statement (the implicit-`main` path).
        Declaration::VariableDeclaration(var) => {
            let mut items = const_arrow_fn_items(var, registry, names);
            // A non-arrow `export const X = <literal>` (Number/Bool/String) is a
            // const-expr literal → a `pub const` item, not a dropped executable
            // statement (an arrow initializer is already a `fn` item above).
            if items.is_empty() {
                if let Some(item) = escape::const_item_from_var(var, names, true) {
                    items.push(item);
                }
            }
            items
        }
        Declaration::TSEnumDeclaration(decl) => {
            declarations::translate_enum(decl).unwrap_or_default()
        }
        _ => Vec::new(),
    }
}

/// `const` / `export const` arrow declarations (`const f = (x) => …`) lower to
/// `fn` items — the binding names each function. Shared by the export path
/// ([`translate_exported_declaration`]) and the plain top-level path
/// ([`translate_statement`]). Non-arrow initializers yield nothing (a const
/// value is an executable statement, not an item).
///
/// Per-function engine degrade (B6d #312 extension): a const-arrow fn whose
/// signature carries an unmappable type (`<T>(data, type): OutputByType[T]`)
/// lowers to the same engine stub as a degraded `function` declaration. The
/// TS source name (the const binding's `BindingIdentifier`) keys the dynamic-fn
/// set, the same key `program_engine_sites` inserted under.
pub(super) fn const_arrow_fn_items(
    var: &VariableDeclaration,
    registry: &TypeRegistry,
    names: &NameTable<'_>,
) -> Vec<syn::Item> {
    var.declarations
        .iter()
        .filter_map(|d| match d.init.as_ref()? {
            Expression::ArrowFunctionExpression(arrow) => {
                let name = names.of_pattern(&d.id);
                // The const binding's TS name is the degrade key (the same
                // string `program_engine_sites` inserted via the const binding's
                // `BindingIdentifier`). A non-`BindingIdentifier` pattern (a
                // destructured const arrow) is not in the dynamic-fn set, so it
                // never degrades — matching `top_level_function`'s lifter.
                let ts_name = match &d.id {
                    BindingPattern::BindingIdentifier(id) => Some(id.name.as_str()),
                    _ => None,
                };
                let item = if ts_name.is_some_and(is_dynamic_fn) {
                    engine_arrow_fn_item(&name, ts_name.unwrap(), arrow, registry, names)
                } else {
                    translate_const_arrow_to_fn(name, arrow, registry, names)
                };
                Some(syn::Item::Fn(item))
            }
            _ => None,
        })
        .collect()
}

/// Translate the declaration of an `export default` (`export default
/// function` / `export default class` / `export default interface`). A default
/// expression (`export default 42`) names no item and yields `[]` — Rust has no
/// anonymous default value. The item is marked `pub` by the caller.
pub(super) fn translate_default_declaration(
    decl: &ExportDefaultDeclarationKind,
    registry: &TypeRegistry,
    names: &NameTable<'_>,
) -> Vec<syn::Item> {
    match decl {
        ExportDefaultDeclarationKind::FunctionDeclaration(func) => {
            vec![syn::Item::Fn(translate_function(func, registry, names))]
        }
        ExportDefaultDeclarationKind::ClassDeclaration(class) => {
            class::translate_class(class, registry, names)
        }
        ExportDefaultDeclarationKind::TSInterfaceDeclaration(iface) => {
            declarations::translate_interface(iface, registry)
        }
        // `export default <expression>` (a value, not a declaration) names no
        // item — Rust has no anonymous default value, so it stays unsupported.
        _ => Vec::new(),
    }
}

/// Mark a top-level item `pub` — used for `export`ed declarations.
pub(super) fn make_pub(item: &mut syn::Item) {
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
