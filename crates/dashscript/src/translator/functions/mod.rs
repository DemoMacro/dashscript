//! Function & variable declarations, and statement translation → `syn`.
//!
//! Control flow lives in [`control_flow`], `switch` in [`switch`], and
//! destructuring patterns in [`destructure`]; this module holds the function
//! skeleton (params, body, return type) and the statement dispatcher.

mod control_flow;
mod destructure;
mod escape;
mod infer;
mod lazy_static;
mod switch;
mod try_throw;

use control_flow::{
    translate_do_while, translate_for, translate_for_in, translate_for_of, translate_if,
    translate_while,
};
use destructure::{destructure_array, destructure_object};
use infer::{index_access_type, infer_literal_type, match_result_type, object_assign_type};
use switch::translate_switch;
use try_throw::{throw_stmt, translate_try};

// Re-exported so `functions::<name>` callers (check, translator::mod) are
// unchanged after the escape/lazy_static split.
pub(in crate::translator) use escape::{
    all_promotable_const_names, promotable_const_info, promoted_const_item, promoted_const_names,
};
pub(in crate::translator) use lazy_static::{
    decl_name, escaped_lazy_static_names, escaped_mutable_static_names, lazy_static_candidate,
    lazy_static_export_info, lazy_static_items, lazy_static_sym, mutable_static_candidate,
    mutable_static_items, mutable_top_level_names,
};

use oxc_ast::ast::{
    ArrowFunctionExpression, BindingPattern, Declaration, ExportDefaultDeclarationKind, Expression,
    FormalParameters, Function, FunctionBody, Statement, TSType, TSTypeAnnotation,
    VariableDeclaration, VariableDeclarationKind,
};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use syn::{parse_quote, Block, Expr, FnArg, Ident, ItemFn, Path, ReturnType, Stmt, Type};

use super::context::{Ctx, Locals, Narrow};
use super::name_table::NameTable;
use super::registry::TypeRegistry;
use super::{bindings, declarations, expressions, types};

thread_local! {
    /// TS names of top-level functions whose body contains a low-compatibility
    /// construct (per-function engine degradation sites). Set once by
    /// `Translator::translate_with_deps_as` before any statement is translated;
    /// `translate_function` reads it to swap such a function's body for a
    /// `__ds_engine::call_fn` invocation that keeps the Rust signature.
    static DYNAMIC_FNS: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
    /// True when the project has at least one engine-degradation site in any
    /// file. A degraded function marshals its arguments as `serde_json::Value`,
    /// which needs `Serialize`/`Deserialize` on every type crossing the
    /// boundary — including types defined in a *non-degraded* file (a union at
    /// the crate root referenced by a degraded function's signature). The
    /// project emitter sets this once after probing all files, so a file that
    /// is not itself degraded still derives serde on its types.
    static FORCE_SERDE_DERIVE: Cell<bool> = const { Cell::new(false) };
}

/// Record the file's per-function engine degradation sites (TS function names),
/// replacing any previous set. Called once per `translate_with_deps_as`.
pub(in crate::translator) fn set_dynamic_fns(fns: HashSet<String>) {
    DYNAMIC_FNS.with(|c| {
        let mut set = c.borrow_mut();
        set.clear();
        set.extend(fns);
    });
}

/// Whether `ts_name` is a per-function engine degradation site this translate.
pub(in crate::translator) fn is_dynamic_fn(ts_name: &str) -> bool {
    DYNAMIC_FNS.with(|c| c.borrow().contains(ts_name))
}

/// Set whether the whole project has an engine-degradation site, so every
/// file derives `Serialize`/`Deserialize` even one not itself degraded (its
/// types may cross a degraded function's marshal boundary in another file).
/// Set once by the project emitter before translating any file.
pub(crate) fn set_force_serde_derive(b: bool) {
    FORCE_SERDE_DERIVE.with(|c| c.set(b));
}

/// Whether the project has an engine-degradation site, so this file should
/// derive `Serialize`/`Deserialize` even if it is not itself degraded.
pub(in crate::translator) fn force_serde_derive() -> bool {
    FORCE_SERDE_DERIVE.with(|c| c.get())
}

/// Translate a top-level statement into a `syn::Item`, if mapped.
///
/// `interface` / `type` / `function` become top-level items; other statements
/// (variable bindings, expression statements) belong inside a function body
/// and are not mapped at module scope.
pub fn translate_statement(
    stmt: &Statement,
    registry: &TypeRegistry,
    names: &NameTable<'_>,
) -> Vec<syn::Item> {
    match stmt {
        Statement::FunctionDeclaration(func) => {
            vec![syn::Item::Fn(translate_function(func, registry, names))]
        }
        Statement::ClassDeclaration(class) => super::class::translate_class(class, registry, names),
        Statement::TSInterfaceDeclaration(iface) => {
            declarations::translate_interface(iface, registry)
        }
        Statement::TSTypeAliasDeclaration(alias) => declarations::translate_type_alias(alias),
        // `export function/interface/type/class` lowers the declaration(s) and
        // marks each `pub` so another `.ts` module can `import` it. A re-export
        // list (`export { foo } from "./m"` / `export { foo as bar } from …`)
        // has no declaration: it lowers to `pub use m::foo;` / `pub use m::foo
        // as bar;` — the source path aliased to the exported name. With no
        // `from` (`export { foo }`) it re-exports a local binding (`pub use
        // foo;`), the way `pub use` widens a module's public surface.
        Statement::ExportNamedDeclaration(exp) => {
            if let Some(decl) = exp.declaration.as_ref() {
                let mut items = translate_exported_declaration(decl, registry, names);
                for item in &mut items {
                    make_pub(item);
                }
                return items;
            }
            if exp.specifiers.is_empty() {
                return Vec::new();
            }
            let trees: Vec<syn::UseTree> = exp
                .specifiers
                .iter()
                .map(super::imports::export_use_tree)
                .collect();
            let source_path = exp.source.as_ref().and_then(|s| {
                let mod_ident = super::imports::module_ident(&s.value)?;
                Some(super::imports::mod_use_path(&s.value, &mod_ident))
            });
            match source_path {
                Some(path) => vec![syn::Item::Use(
                    parse_quote!(pub use #path::{#(#trees),*};),
                )],
                None => trees
                    .into_iter()
                    .map(|t| syn::Item::Use(parse_quote!(pub use #t;)))
                    .collect(),
            }
        }
        // `export * from "./m"` → `pub use m::*;`; `export * as ns from "./m"`
        // → `pub use m as ns;` (a namespace re-export — importers read `ns::foo`).
        // A bare-specifier source the translator cannot resolve yields nothing
        // (and `check` flags it).
        Statement::ExportAllDeclaration(decl) => {
            let Some(mod_ident) = super::imports::module_ident(&decl.source.value) else {
                return Vec::new();
            };
            let path = super::imports::mod_use_path(&decl.source.value, &mod_ident);
            match &decl.exported {
                None => vec![syn::Item::Use(parse_quote!(pub use #path::*;))],
                Some(name) => {
                    let ns = super::imports::export_alias_ident(name);
                    vec![syn::Item::Use(parse_quote!(pub use #path as #ns;))]
                }
            }
        }
        // `import { foo, bar } from "./other"` → `use other::{foo, bar};`;
        // `import { x } from "cargo:serde"` → `use serde::{x}`. A bare
        // specifier (`"lodash"`) has no resolver → `module_ident` returns
        // `None` → emits nothing, and `check` flags it unsupported. A rename
        // (`import { foo as fooA }`) lowers to `use other::foo as fooA;`; a
        // namespace import (`import * as ns`) lowers to its own
        // `use other as ns;` (a module-path alias, not a group leaf).
        Statement::ImportDeclaration(imp) => {
            let Some(mod_ident) = super::imports::module_ident(&imp.source.value) else {
                return Vec::new();
            };
            let path = super::imports::mod_use_path(&imp.source.value, &mod_ident);
            let Some(specifiers) = imp.specifiers.as_ref() else {
                return Vec::new();
            };
            let mut out: Vec<syn::Item> = Vec::new();
            // Named / default imports → `use other::{foo, bar as baz};`
            // (prettyplease drops the braces for a single item). A local module
            // (`./other`) lowers to `crate::other` so the `use` resolves from a
            // sibling module, not only the crate root.
            let trees: Vec<syn::UseTree> = specifiers
                .iter()
                .filter_map(super::imports::named_use_tree)
                .collect();
            if !trees.is_empty() {
                out.push(syn::Item::Use(parse_quote!(use #path::{#(#trees),*};)));
            }
            if let Some(ns_ident) = super::imports::namespace_local(specifiers) {
                out.push(syn::Item::Use(parse_quote!(use #path as #ns_ident;)));
            }
            out
        }
        // `export default function foo()` / `export default class Foo` lowers
        // the declaration as `pub` — a default export is a public item like any
        // named export (which file is "the entry" is a build-pipeline concern).
        // A default expression (`export default 42`) names no item and stays
        // unsupported (the `_` arm): Rust has no anonymous default value.
        Statement::ExportDefaultDeclaration(exp) => {
            let mut items = translate_default_declaration(&exp.declaration, registry, names);
            for item in &mut items {
                make_pub(item);
            }
            items
        }
        // `enum Color { Red, Green }` → `pub mod Color { pub const Red: i64 =
        // 0; pub const Green: i64 = 1; }` — a runtime object of named values,
        // the way an ES enum works. A non-literal initializer yields nothing
        // here and `check` flags the enum unsupported.
        Statement::TSEnumDeclaration(decl) => declarations::translate_enum(decl).unwrap_or_default(),
        // A non-export `const` arrow (`const f = () => …`) lowers to a `fn`
        // item, mirroring the `export const f = …` path. `is_executable_top_level`
        // routes it here (a const arrow is a declaration, not executable), so
        // only an arrow initializer maps; any other const value never reaches
        // this arm (it is executable → implicit `fn main`).
        Statement::VariableDeclaration(var) => const_arrow_fn_items(var, registry, names),
        // Executable statements run in the implicit `fn main`, not as items
        // (see `is_executable_top_level`); an empty item list is correct.
        Statement::ExpressionStatement(_)
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
        | Statement::ReturnStatement(_)
        | Statement::BreakStatement(_)
        | Statement::ContinueStatement(_)
        // No-op statements.
        | Statement::EmptyStatement(_)
        | Statement::DebuggerStatement(_)
        // Unsupported statement kinds (`labeled:` / `with`, TS `enum`/`namespace`/
        // `global`/`import =`/`export =`) — `check` flags these; explicit arms
        // keep dispatch exhaustive (no `_` wildcard).
        | Statement::LabeledStatement(_)
        | Statement::WithStatement(_)
        | Statement::TSModuleDeclaration(_)
        | Statement::TSGlobalDeclaration(_)
        | Statement::TSImportEqualsDeclaration(_)
        | Statement::TSExportAssignment(_)
        | Statement::TSNamespaceExportDeclaration(_) => Vec::new(),
    }
}

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
fn is_const_arrow(stmt: &Statement) -> bool {
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

/// Translate the inner declaration of an `export` (`export function` /
/// `export class` / `export interface` / `export type`). Re-exports and
/// unsupported kinds (enum) yield `[]`. A class yields its `struct` plus `impl`.
fn translate_exported_declaration(
    decl: &Declaration,
    registry: &TypeRegistry,
    names: &NameTable<'_>,
) -> Vec<syn::Item> {
    match decl {
        Declaration::FunctionDeclaration(func) => {
            vec![syn::Item::Fn(translate_function(func, registry, names))]
        }
        Declaration::ClassDeclaration(class) => {
            super::class::translate_class(class, registry, names)
        }
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
fn const_arrow_fn_items(
    var: &VariableDeclaration,
    registry: &TypeRegistry,
    names: &NameTable<'_>,
) -> Vec<syn::Item> {
    var.declarations
        .iter()
        .filter_map(|d| match d.init.as_ref()? {
            Expression::ArrowFunctionExpression(arrow) => {
                let name = names.of_pattern(&d.id);
                Some(syn::Item::Fn(translate_const_arrow_to_fn(
                    name, arrow, registry, names,
                )))
            }
            _ => None,
        })
        .collect()
}

/// Translate the declaration of an `export default` (`export default
/// function` / `export default class` / `export default interface`). A default
/// expression (`export default 42`) names no item and yields `[]` — Rust has no
/// anonymous default value. The item is marked `pub` by the caller.
fn translate_default_declaration(
    decl: &ExportDefaultDeclarationKind,
    registry: &TypeRegistry,
    names: &NameTable<'_>,
) -> Vec<syn::Item> {
    match decl {
        ExportDefaultDeclarationKind::FunctionDeclaration(func) => {
            vec![syn::Item::Fn(translate_function(func, registry, names))]
        }
        ExportDefaultDeclarationKind::ClassDeclaration(class) => {
            super::class::translate_class(class, registry, names)
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
fn make_pub(item: &mut syn::Item) {
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

/// The Rust return type of a `function`: `void`/`undefined` → omitted (`()`);
/// an annotated type → that type; an untyped body that returns a value → `f64`
/// (mirroring the untyped-param-defaults-to-f64 rule so a plain numeric
/// `add(a, b) { return a + b }` compiles). Shared by the static body and the
/// per-function engine body, so both keep the same Rust signature.
fn fn_output(func: &Function, registry: &TypeRegistry) -> ReturnType {
    func.return_type
        .as_ref()
        .and_then(|ta| match &ta.type_annotation {
            TSType::TSVoidKeyword(_) | TSType::TSUndefinedKeyword(_) => None,
            ty => Some(ReturnType::Type(
                Default::default(),
                Box::new(types::translate_type_for_signature(ty, registry)),
            )),
        })
        .or_else(|| {
            // No return annotation — a `.js` function or an untyped test262
            // callback. Default to `-> f64` when the body returns a value,
            // mirroring the untyped-param-defaults-to-f64 rule so a plain
            // numeric `add(a, b) { return a + b }` compiles. A void body (no
            // `return expr`) stays `()`; a non-f64 return (e.g. a `String`)
            // fails `cargo check` honestly — add an annotation or a `.d.ts`.
            // Only a top-level `return expr;` is detected; a return nested in
            // control flow is rarer and surfaces as a Rust type error.
            let body = func.body.as_deref()?;
            body.statements
                .iter()
                .any(|s| matches!(s, Statement::ReturnStatement(ret) if ret.argument.is_some()))
                .then(|| ReturnType::Type(Default::default(), parse_quote!(f64)))
        })
        .unwrap_or(ReturnType::Default)
}

fn translate_function(func: &Function, registry: &TypeRegistry, names: &NameTable<'_>) -> ItemFn {
    let name = func
        .id
        .as_ref()
        .map_or_else(|| format_ident!("__ds_main"), |id| names.of_binding(id));
    // Per-function engine degradation: a function whose body contains a
    // construct the static translator cannot lower keeps its Rust signature but
    // runs under QuickJS via `__ds_engine::call_fn`. The dynamic-fn set is set
    // once per translate, so a function matched here skips body translation.
    if let Some(id) = &func.id {
        if is_dynamic_fn(id.name.as_str()) {
            return engine_fn_item(&name, id.name.as_str(), func, registry, names);
        }
    }
    // A named `fn` and a block-body arrow set up their body `Locals` the same
    // way — register params, run mutation analysis, infer number flavors — so
    // both go through [`body_locals`] (the single source of truth for "how a
    // function body's locals are built").
    let mut locals = body_locals(&func.params, func.body.as_deref(), registry, names);
    let inputs = translate_params(&func.params, &locals, registry, names);
    // `void` / `undefined` map to an omitted return type (Rust infers `()`).
    let output = fn_output(func, registry);
    // The return-type path threads down to `return {…}` so the object literal
    // can borrow its struct name.
    let return_path = func.return_type.as_deref().and_then(return_path_of);
    // Default parameters unwrap their `Option` at the top of the body, so the
    // rest of the function sees the plain value.
    let defaults: Vec<Stmt> = func
        .params
        .items
        .iter()
        .filter_map(|fp| {
            let init = fp.initializer.as_deref()?;
            let name = names.of_pattern(&fp.pattern);
            let default = expressions::translate_expr(
                init,
                &Ctx::new(&locals, registry, &Narrow::default(), names),
            );
            Some(parse_quote!(let #name = #name.unwrap_or(#default);))
        })
        .collect();
    let body_stmts: &[Statement] = func.body.as_deref().map_or(&[], |b| &b.statements[..]);
    let mut block = translate_body(
        body_stmts,
        &mut locals,
        registry,
        &Narrow::default(),
        return_path.as_ref(),
        names,
    );
    if !defaults.is_empty() {
        let mut stmts = defaults;
        stmts.extend(block.stmts);
        block.stmts = stmts;
    }
    // Generic type parameters pass through verbatim (`<T>`); Rust monomorphizes
    // and infers each call. Constraints/defaults are ignored (no `where`).
    let generics: Vec<Ident> = func.type_parameters.as_deref().map_or_else(Vec::new, |tp| {
        tp.params
            .iter()
            .map(|p| bindings::type_ident(&p.name.name))
            .collect()
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

/// A per-function engine degradation site: keep the Rust signature (params,
/// return type, generics) but replace the body with a `__ds_engine::call_fn`
/// invocation. Each argument is marshaled to `serde_json::Value` (every emitted
/// struct/enum derives `Serialize`/`Deserialize` in this mode), and a non-unit
/// return is marshaled back. `__DS_MODULE_JS` is the whole module's
/// annotation-stripped JS, eval'd per call so the function's helper
/// dependencies are in scope (a dynamic function usually leans on other module
/// functions; the engine defines all of them before the call).
fn engine_fn_item(
    name: &Ident,
    ts_name: &str,
    func: &Function,
    registry: &TypeRegistry,
    names: &NameTable<'_>,
) -> ItemFn {
    // Degraded signature: a param/return type the static translator cannot
    // express (unknown/indexed access/…) becomes `serde_json::Value` — the
    // marshal type — so the signature is concrete rather than `_`. An
    // expressible type maps normally, so a degraded function mixing the two
    // keeps the expressible params concrete.
    let inputs: Vec<FnArg> = func
        .params
        .items
        .iter()
        .map(|fp| {
            let pname = names.of_pattern(&fp.pattern);
            let mut ty = fp.type_annotation.as_deref().map_or_else(
                || parse_quote!(::serde_json::Value),
                |ta| types::translate_type_degraded_for_signature(&ta.type_annotation, registry),
            );
            // An optional (`?:`) or default-initialized param is `Option<T>`,
            // mirroring translate_params — a static caller passes an
            // `Option<Js2XmlOptions>` for an `options?` param, so the degraded
            // signature must accept `Option<_>` or the call site mismatches.
            if fp.optional || fp.initializer.is_some() {
                ty = parse_quote!(Option<#ty>);
            }
            parse_quote!(#pname: #ty)
        })
        .collect();
    let output: ReturnType = func
        .return_type
        .as_deref()
        .map_or(ReturnType::Default, |rt| {
            let ty = types::translate_type_degraded_for_signature(&rt.type_annotation, registry);
            parse_quote!(-> #ty)
        });
    let generics: Vec<Ident> = func.type_parameters.as_deref().map_or_else(Vec::new, |tp| {
        tp.params
            .iter()
            .map(|p| bindings::type_ident(&p.name.name))
            .collect()
    });
    // Marshal each argument to `serde_json::Value` (Serialize is derived on
    // every emitted struct/enum in per-function mode).
    let args: Vec<Expr> = func
        .params
        .items
        .iter()
        .map(|fp| {
            let pname = names.of_pattern(&fp.pattern);
            parse_quote!(serde_json::to_value(&#pname).unwrap_or(serde_json::Value::Null))
        })
        .collect();
    let ts_lit = syn::LitStr::new(ts_name, proc_macro2::Span::call_site());
    // A unit/void return discards the engine's `Value`; a typed return
    // deserializes it back to the signature's Rust type.
    let block: Block = match &output {
        ReturnType::Default => parse_quote!({
            let __ds_args: Vec<serde_json::Value> = vec![#(#args),*];
            let _ = crate::__ds_engine::call_fn(#ts_lit, __DS_MODULE_JS, &__ds_args);
        }),
        ReturnType::Type(_, ret_ty) => parse_quote!({
            let __ds_args: Vec<serde_json::Value> = vec![#(#args),*];
            let __ds_ret = crate::__ds_engine::call_fn(#ts_lit, __DS_MODULE_JS, &__ds_args);
            serde_json::from_value::<#ret_ty>(__ds_ret)
                .expect("engine return value did not deserialize to the declared return type")
        }),
    };
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

/// `export const name = <T>(params): ret => body` → a `fn name<T>(params) -> ret
/// { body }`. The const binding names the function; the arrow supplies the
/// generic type parameters, params, and return type. A type predicate
/// (`arg is X`) returns `bool` — the runtime shape of a TS type guard. An
/// expression body (`=> expr`) becomes the block's trailing expression; a block
/// body (`=> { … }`) maps through [`translate_body`]. Mirrors
/// [`translate_function`] so a const arrow compiles identically to a `function`
/// declaration.
fn translate_const_arrow_to_fn(
    name: Ident,
    arrow: &ArrowFunctionExpression,
    registry: &TypeRegistry,
    names: &NameTable<'_>,
) -> ItemFn {
    let mut locals = body_locals(&arrow.params, Some(arrow.body.as_ref()), registry, names);
    let inputs = translate_params(&arrow.params, &locals, registry, names);
    let output = arrow_return_type(arrow.return_type.as_deref());
    let return_path = arrow.return_type.as_deref().and_then(return_path_of);
    let block = if arrow.expression {
        arrow_expression_block(
            &arrow.body,
            &locals,
            registry,
            &output,
            return_path.as_ref(),
            names,
        )
    } else {
        translate_body(
            &arrow.body.statements[..],
            &mut locals,
            registry,
            &Narrow::default(),
            return_path.as_ref(),
            names,
        )
    };
    let generics: Vec<Ident> = arrow
        .type_parameters
        .as_deref()
        .map_or_else(Vec::new, |tp| {
            tp.params
                .iter()
                .map(|p| bindings::type_ident(&p.name.name))
                .collect()
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

/// An arrow's return type: `void`/`undefined` → omitted (Rust infers `()`); a
/// type predicate (`arg is X`) → `bool` (a TS type guard narrows at runtime);
/// anything else maps through [`types::translate_type`].
fn arrow_return_type(rt: Option<&TSTypeAnnotation>) -> ReturnType {
    rt.and_then(|ta| match &ta.type_annotation {
        TSType::TSVoidKeyword(_) | TSType::TSUndefinedKeyword(_) => None,
        TSType::TSTypePredicate(_) => Some(ReturnType::Type(
            Default::default(),
            Box::new(parse_quote!(bool)),
        )),
        ty => Some(ReturnType::Type(
            Default::default(),
            Box::new(types::translate_type(ty)),
        )),
    })
    .unwrap_or(ReturnType::Default)
}

/// An arrow expression body `=> expr` (oxc stores it as a single
/// `ExpressionStatement`) → `{ expr }` (the block's trailing expression) for a
/// valued return, or `{ expr; }` (discarded) for `void`. The return-type path
/// threads in so an object-literal body borrows its struct name.
fn arrow_expression_block(
    body: &FunctionBody,
    locals: &Locals,
    registry: &TypeRegistry,
    output: &ReturnType,
    return_path: Option<&Path>,
    names: &NameTable<'_>,
) -> Block {
    let expr = body.statements.iter().find_map(|s| match s {
        Statement::ExpressionStatement(es) => Some(&es.expression),
        _ => None,
    });
    let Some(e) = expr else {
        return parse_quote!({});
    };
    let ret_ty = return_path.map(|p| -> Type { parse_quote!(#p) });
    let e = expressions::translate_init(
        e,
        ret_ty.as_ref(),
        &Ctx::new(locals, registry, &Narrow::default(), names),
    );
    if matches!(output, ReturnType::Default) {
        parse_quote!({ #e; })
    } else {
        parse_quote!({ #e })
    }
}

/// The `syn::Path` of a function's return type — used to translate `return {…}`
/// object literals. `void`/`undefined` yield no path.
pub(in crate::translator) fn return_path_of(ta: &oxc_ast::ast::TSTypeAnnotation) -> Option<Path> {
    match &ta.type_annotation {
        TSType::TSVoidKeyword(_) | TSType::TSUndefinedKeyword(_) => None,
        ty => path_of(&types::translate_type(ty)),
    }
}

pub(in crate::translator) fn translate_params(
    params: &FormalParameters,
    locals: &Locals,
    registry: &TypeRegistry,
    names: &NameTable<'_>,
) -> Vec<FnArg> {
    params
        .items
        .iter()
        .map(|fp| {
            let pat = names.of_pattern(&fp.pattern);
            // An unannotated parameter (common in test262 callbacks like
            // `callbackfn(val, idx, obj)`) defaults to `f64`: Rust fn params
            // need a concrete type (`_` is E0121), and a DashScript `number` is
            // `f64`. A parameter the body actually uses as a string/array/bool
            // then fails cargo check (a partial) — honest, and rarer than the
            // self-contained number callback it now lets compile.
            let ty = fp
                .type_annotation
                .as_ref()
                .map(|ta| types::translate_type_for_signature(&ta.type_annotation, registry))
                .unwrap_or_else(|| parse_quote!(f64));
            // An optional (`?:`) or default-initialized parameter is `Option<T>`
            // — callers pass `None` for a missing/`undefined` argument, and the
            // body sees the parameter as `Option<T>` (narrowed on truthiness).
            let ty = if fp.optional || fp.initializer.is_some() {
                parse_quote!(Option<#ty>)
            } else {
                ty
            };
            let pat_str = pat.to_string();
            // A member-mutated, non-rebound parameter is a reference parameter
            // (`&mut T`): ES arrays/objects pass by reference, so `c[i] = v`
            // inside the function is visible to the caller. A rebound parameter
            // (`c = …`) stays owned `mut c` — a rebind does not propagate.
            if locals.ref_params.contains(&pat_str) {
                parse_quote!(#pat : &mut #ty)
            } else if locals.mutated.contains(&pat_str) {
                parse_quote!(mut #pat : #ty)
            } else {
                parse_quote!(#pat : #ty)
            }
        })
        .collect()
}

/// Record a binding's type path (if it has one) into the locals table.
pub(in crate::translator) fn register_local(
    locals: &mut Locals,
    pattern: &oxc_ast::ast::BindingPattern,
    type_annotation: Option<&oxc_ast::ast::TSTypeAnnotation>,
    names: &NameTable<'_>,
) {
    let Some(ta) = type_annotation else { return };
    let ty = types::translate_type(&ta.type_annotation);
    let Some(path) = path_of(&ty) else { return };
    let name = names.of_pattern(pattern);
    locals.insert(name.to_string(), path);
}

/// Build the per-body [`Locals`] for a function or a block-body arrow: register
/// each parameter's declared type, then — when a body is present — run mutation
/// analysis (so a reassigned `let` or param becomes `mut`) and number-flavor
/// inference (so a pure-integer counter becomes `i64`). The single source of
/// truth shared by [`translate_function`] and `arrow_expr`; a block-body arrow
/// `(x) => { … }` is a function body in everything but syntax, so its locals are
/// built identically. `body` is `None` only for an ambient (body-less) function
/// declaration, which keeps its params but gathers no analysis.
pub(in crate::translator) fn body_locals(
    params: &oxc_ast::ast::FormalParameters,
    body: Option<&oxc_ast::ast::FunctionBody>,
    registry: &TypeRegistry,
    names: &NameTable<'_>,
) -> Locals {
    let mut locals = Locals::new();
    for fp in &params.items {
        register_local(
            &mut locals,
            &fp.pattern,
            fp.type_annotation.as_deref(),
            names,
        );
        // An optional (`?:`) or default-initialized parameter is `Option<T>` at
        // the call site, so record it that way — `translate_params` emits the
        // same `Option<T>` in the signature, and the body's narrowing/truthiness
        // (`if (opt)`, `opt ? … : …`) queries `is_option` through this record.
        if fp.optional || fp.initializer.is_some() {
            let name = names.of_pattern(&fp.pattern).to_string();
            match locals.get(&name).cloned() {
                Some(inner) => locals.insert(name, parse_quote!(Option<#inner>)),
                None => locals.insert(name, parse_quote!(Option<f64>)),
            }
        }
    }
    let Some(body) = body else {
        return locals;
    };
    // Mutations analysis runs before parameter emission so a reassigned
    // parameter — including via `??=`/`||=`/`&&=` — is declared `mut`. TS params
    // reassign; Rust params are immutable by default.
    let analysis = super::analysis::analyze(
        &body.statements,
        names,
        &registry.mut_methods,
        &registry.ref_params,
    );
    // Reference parameters: member-mutated but not rebound. Computed before
    // moving `analysis.{mutated, member_mutated}` into locals below.
    locals.ref_params = params
        .items
        .iter()
        .filter_map(|fp| {
            let name = names.of_pattern(&fp.pattern).to_string();
            (analysis.member_mutated.contains(&name) && !analysis.reassigned.contains(&name))
                .then_some(name)
        })
        .collect();
    locals.mutated = analysis.mutated;
    locals.member_mutated = analysis.member_mutated;
    locals.use_counts = analysis.use_counts;
    // Number-flavor inference (i64 vs f64): which `number` locals hold only
    // pure integers. Conservative — a `: number` annotation or any fractional /
    // division / `Math.*` value forces `F64` in `flavor::infer`.
    locals.number_flavors = super::flavor::infer(&body.statements, names);
    // Record each top-level `let`/`const` binding's type — an explicit
    // annotation, else the return type of an initializing function call — so a
    // later `obj.optional_field` access knows the field's struct type.
    register_let_types(body, &mut locals, registry);
    locals
}

/// Scan a body's `let`/`const` declarations (recursing into nested blocks and
/// control-flow bodies, the way `flavor::walk_stmt` does) and record each
/// binding's type path into `locals`: an explicit annotation wins
/// (`let x: T = …` → `T`), else the return type of an initializing bare-call
/// (`let x = f()` → f's declared return type). Destructuring and member-call
/// inits (`obj.method()`) are out of scope — a later batch. This closes the gap
/// that an unannotated `let parent = peek(stack)` inside a `while` body left
/// `parent`'s type unknown, so an optional-field store (`parent.elements = v`)
/// could not be recognized as `Option<Vec<…>>`.
fn register_let_types(
    body: &oxc_ast::ast::FunctionBody,
    locals: &mut Locals,
    registry: &TypeRegistry,
) {
    for stmt in &body.statements {
        register_let_walk(stmt, locals, registry);
    }
}

/// Recursive statement walk for [`register_let_types`] — mirrors
/// `flavor::walk_stmt`'s descent into every block-bearing statement so a `let`
/// deep inside a `while`/`if`/`for` is reached.
fn register_let_walk(stmt: &oxc_ast::ast::Statement, locals: &mut Locals, registry: &TypeRegistry) {
    use oxc_ast::ast::{ForStatementInit, Statement};
    match stmt {
        Statement::BlockStatement(b) => {
            for s in &b.body {
                register_let_walk(s, locals, registry);
            }
        }
        Statement::VariableDeclaration(v) => {
            for d in &v.declarations {
                register_declarator(d, locals, registry);
            }
        }
        Statement::IfStatement(if_stmt) => {
            register_let_walk(&if_stmt.consequent, locals, registry);
            if let Some(alt) = &if_stmt.alternate {
                register_let_walk(alt, locals, registry);
            }
        }
        Statement::WhileStatement(w) => register_let_walk(&w.body, locals, registry),
        Statement::DoWhileStatement(dw) => register_let_walk(&dw.body, locals, registry),
        Statement::ForStatement(f) => {
            if let Some(ForStatementInit::VariableDeclaration(v)) = &f.init {
                for d in &v.declarations {
                    register_declarator(d, locals, registry);
                }
            }
            register_let_walk(&f.body, locals, registry);
        }
        Statement::ForOfStatement(fo) => register_let_walk(&fo.body, locals, registry),
        Statement::ForInStatement(fi) => register_let_walk(&fi.body, locals, registry),
        Statement::SwitchStatement(sw) => {
            for c in &sw.cases {
                for s in &c.consequent {
                    register_let_walk(s, locals, registry);
                }
            }
        }
        _ => {}
    }
}

/// One `let`/`const` declarator's contribution to `locals` (see
/// [`register_let_types`]). A non-binding-identifier pattern (destructuring) is
/// skipped. An unannotated `let x = fn()` records the callee's return type, and
/// a `let x = arr[i]` records the `Vec`'s element type — so a later
/// `x.optional_field` access (and a union-widening call argument) resolves the
/// field's struct. Any other initializer is left to its existing lowering.
fn register_declarator(
    decl: &oxc_ast::ast::VariableDeclarator,
    locals: &mut Locals,
    registry: &TypeRegistry,
) {
    use oxc_ast::ast::{BindingPattern, Expression};
    let BindingPattern::BindingIdentifier(id) = &decl.id else {
        return;
    };
    let name = bindings::snake(id.name.as_str()).to_string();
    let path = match &decl.init {
        Some(Expression::CallExpression(call)) => callee_return_path(call, registry),
        // `new Uint8Array(…)` → `Vec<u8>` (typed_array_path), so a later
        // `x[0] = v` stores with a `u8` cast. Any other `new` callee falls
        // through to the element-path probe (which yields `None` for it).
        Some(Expression::NewExpression(n)) => typed_array_path(n),
        Some(other) => vec_index_elem_path(other, locals),
        None => return,
    };
    if let Some(path) = path {
        locals.insert(name, path);
    }
}

/// `new Uint8Array(…)` / `ArrayBuffer` / `Uint8ClampedArray` → `Vec<u8>`, so an
/// unannotated `let x = new Uint8Array(3)` records `Vec<u8>` and a later
/// `x[0] = v` stores the value with a `u8` cast. Mirrors the type mapping;
/// `None` for any other `new` callee.
fn typed_array_path(new_expr: &oxc_ast::ast::NewExpression) -> Option<Path> {
    use oxc_ast::ast::Expression;
    let Expression::Identifier(id) = &new_expr.callee else {
        return None;
    };
    if matches!(
        id.name.as_str(),
        "Uint8Array" | "ArrayBuffer" | "Uint8ClampedArray"
    ) {
        Some(parse_quote!(Vec<u8>))
    } else {
        None
    }
}

/// `arr[i]` where `arr` is a tracked `Vec<T>` (or `Option<Vec<T>>`) local → `T`,
/// so an unannotated `let element = elements[i]` records `Element` and a later
/// `element.text` access resolves its struct. Works on the source AST — the
/// emitted `elements[i as usize].clone()` is plain `elements[i]` here, since the
/// cast and `.clone()` are added only at emit time. Returns `None` for any other
/// initializer shape.
fn vec_index_elem_path(init: &oxc_ast::ast::Expression, locals: &Locals) -> Option<Path> {
    use oxc_ast::ast::Expression;
    let Expression::ComputedMemberExpression(cm) = init else {
        return None;
    };
    let Expression::Identifier(id) = &cm.object else {
        return None;
    };
    let arr_path = locals.get(&bindings::snake(id.name.as_str()).to_string())?;
    let outer = arr_path.segments.last()?;
    // The `Vec<…>` segment: directly when `arr` is `Vec<T>`, or the inner type
    // argument when `arr` is `Option<Vec<T>>` (an optional `T[] | undefined`).
    let vec_seg = if outer.ident == "Vec" {
        outer
    } else if outer.ident == "Option" {
        first_type_arg_seg(outer).filter(|s| s.ident == "Vec")?
    } else {
        return None;
    };
    // The element type `T` is the `Vec`'s first type-argument segment
    // (`Element`, `__DsUnion…`); reconstruct a single-segment path from it.
    let elem_ident = first_type_arg_seg(vec_seg)?.ident.clone();
    Some(parse_quote!(#elem_ident))
}

/// The first `syn::PathSegment` inside a path segment's first generic type
/// argument — `Vec<Element>` → the `Element` segment, `Option<Vec<T>>`'s
/// `Option` → the `Vec<T>` segment. `None` when the segment has no generic type
/// argument or it is not a plain path.
fn first_type_arg_seg(seg: &syn::PathSegment) -> Option<&syn::PathSegment> {
    let syn::PathArguments::AngleBracketed(args) = &seg.arguments else {
        return None;
    };
    let first_ty = args.args.iter().find_map(|g| match g {
        syn::GenericArgument::Type(t) => Some(t),
        _ => None,
    })?;
    match first_ty {
        syn::Type::Path(tp) => tp.path.segments.last(),
        _ => None,
    }
}

/// The declared return-type path of a `fn_name(…)` call's callee, when the
/// callee is a bare identifier naming a function with an annotated return type.
fn callee_return_path(
    call: &oxc_ast::ast::CallExpression,
    registry: &TypeRegistry,
) -> Option<Path> {
    use oxc_ast::ast::Expression;
    let Expression::Identifier(id) = &call.callee else {
        return None;
    };
    registry
        .function_returns
        .get(id.name.as_str())
        .cloned()
        .flatten()
}

pub(in crate::translator) fn translate_body(
    stmts: &[Statement],
    locals: &mut Locals,
    registry: &TypeRegistry,
    narrow: &Narrow,
    return_path: Option<&Path>,
    names: &NameTable<'_>,
) -> Block {
    let mut out: Vec<Stmt> = stmts
        .iter()
        .flat_map(|s| translate_stmt(s, locals, registry, narrow, return_path, names))
        .collect();
    // A trailing `return expr;` is the block's implicit value — emit it as a
    // bare trailing expression (no `return`, no `;`) for idiomatic Rust and to
    // keep clippy::needless_return quiet. A bare `return;` (void) stays as-is.
    drop_trailing_return(&mut out);
    parse_quote!({ #(#out)* })
}

/// Replace a trailing `return expr;` with a bare `expr` (no `return`, no `;`)
/// so the block's value is the expression — idiomatic Rust, and keeps
/// clippy::needless_return quiet. A bare `return;` (void) is left untouched.
pub(in crate::translator) fn drop_trailing_return(stmts: &mut [Stmt]) {
    let trailing_value = match stmts.last() {
        Some(Stmt::Expr(Expr::Return(ret), _)) => ret.expr.clone(),
        _ => None,
    };
    if let Some(value) = trailing_value {
        if let Some(slot) = stmts.last_mut() {
            *slot = Stmt::Expr(*value, None);
        }
    }
}

/// Translate a function-body statement into zero or more `syn::Stmt`s.
pub(in crate::translator) fn translate_stmt(
    stmt: &Statement,
    locals: &mut Locals,
    registry: &TypeRegistry,
    narrow: &Narrow,
    return_path: Option<&Path>,
    names: &NameTable<'_>,
) -> Vec<Stmt> {
    match stmt {
        Statement::BlockStatement(block) => block
            .body
            .iter()
            .flat_map(|s| translate_stmt(s, locals, registry, narrow, return_path, names))
            .collect(),
        Statement::ReturnStatement(ret) => {
            let s: Stmt = match &ret.argument {
                Some(arg) => {
                    // An object literal borrows the struct name from the return
                    // type; everything else translates as a plain expression.
                    let ret_ty = return_path.map(|p| -> Type { parse_quote!(#p) });
                    let ctx = Ctx::new(&*locals, registry, narrow, names);
                    let expr = expressions::translate_init(arg, ret_ty.as_ref(), &ctx);
                    // A `T` returned where the signature is `Option<T>` (`.ts`
                    // `T | undefined`) needs an explicit `Some` in Rust.
                    let expr = expressions::implicit_some(arg, expr, ret_ty.as_ref(), &ctx);
                    parse_quote!(return #expr;)
                }
                None => parse_quote!(return;),
            };
            vec![s]
        }
        Statement::ExpressionStatement(es) => {
            let expr = expressions::translate_expr(
                &es.expression,
                &Ctx::new(&*locals, registry, narrow, names),
            );
            vec![parse_quote!(#expr;)]
        }
        Statement::VariableDeclaration(decl) => {
            translate_variable_declaration(decl, locals, registry, narrow, names)
        }
        Statement::IfStatement(if_stmt) => {
            vec![translate_if(
                if_stmt,
                locals,
                registry,
                narrow,
                return_path,
                names,
            )]
        }
        Statement::WhileStatement(while_stmt) => {
            vec![translate_while(
                while_stmt,
                locals,
                registry,
                narrow,
                return_path,
                names,
            )]
        }
        Statement::DoWhileStatement(dws) => vec![translate_do_while(
            dws,
            locals,
            registry,
            narrow,
            return_path,
            names,
        )],
        Statement::ForOfStatement(for_of) => {
            translate_for_of(for_of, locals, registry, narrow, return_path, names)
        }
        Statement::ForInStatement(for_in) => {
            translate_for_in(for_in, locals, registry, narrow, return_path, names)
        }
        Statement::ForStatement(for_stmt) => {
            translate_for(for_stmt, locals, registry, narrow, return_path, names)
        }
        Statement::SwitchStatement(sw) => {
            vec![translate_switch(
                sw,
                locals,
                registry,
                narrow,
                return_path,
                names,
            )]
        }
        Statement::BreakStatement(_) => vec![parse_quote!(break;)],
        Statement::ContinueStatement(_) => vec![parse_quote!(continue;)],
        Statement::ThrowStatement(t) => {
            vec![throw_stmt(&t.argument, locals, registry, narrow, names)]
        }
        // `try { … } catch (e) { … }` → `catch_unwind` (see `translate_try`).
        Statement::TryStatement(t) => {
            translate_try(t, locals, registry, narrow, return_path, names)
        }
        // No-op statements — legal to drop (no runtime effect).
        Statement::EmptyStatement(_) | Statement::DebuggerStatement(_) => vec![],
        // Unsupported control flow (`labeled:` / `with`) — `check` flags these;
        // the explicit arm keeps dispatch exhaustive (no `_` wildcard).
        Statement::LabeledStatement(_) | Statement::WithStatement(_) => vec![],
        // Top-level-only constructs — declarations and module declarations do
        // not appear in a function body; a nested one stays unmapped (a nested
        // function declaration is rarer and `check` flags it).
        Statement::FunctionDeclaration(_)
        | Statement::ClassDeclaration(_)
        | Statement::TSTypeAliasDeclaration(_)
        | Statement::TSInterfaceDeclaration(_)
        | Statement::TSEnumDeclaration(_)
        | Statement::TSModuleDeclaration(_)
        | Statement::TSGlobalDeclaration(_)
        | Statement::TSImportEqualsDeclaration(_)
        | Statement::ImportDeclaration(_)
        | Statement::ExportAllDeclaration(_)
        | Statement::ExportDefaultDeclaration(_)
        | Statement::ExportNamedDeclaration(_)
        | Statement::TSExportAssignment(_)
        | Statement::TSNamespaceExportDeclaration(_) => vec![],
    }
}

/// `let x` → `let mut x` (TS `let` is mutable); `const`/`var` → `let`.
/// An object pattern (`const { x, y } = v`) destructures the struct.
fn translate_variable_declaration(
    decl: &VariableDeclaration,
    locals: &mut Locals,
    registry: &TypeRegistry,
    narrow: &Narrow,
    names: &NameTable<'_>,
) -> Vec<Stmt> {
    let kind_let = matches!(decl.kind, VariableDeclarationKind::Let);
    decl.declarations
        .iter()
        .flat_map(|d| -> Vec<Stmt> {
            match &d.id {
                BindingPattern::ObjectPattern(obj) => destructure_object(
                    obj,
                    d.init.as_ref(),
                    locals,
                    kind_let,
                    registry,
                    narrow,
                    names,
                ),
                BindingPattern::ArrayPattern(arr) => destructure_array(
                    arr,
                    d.init.as_ref(),
                    locals,
                    kind_let,
                    registry,
                    narrow,
                    names,
                ),
                _ => {
                    let name = names.of_pattern(&d.id);
                    // `mut` when the binding is actually mutated. `let`/`var`
                    // are reassignable (`x = …`) or member-mutated (`xs.push`);
                    // `const` forbids rebind but still allows member mutation
                    // (`const xs = []; xs.push(1)` needs `let mut xs` for the
                    // `&mut` borrow), so a member-mutated `const` is `let mut`.
                    let name_str = name.to_string();
                    let member_mut = locals.member_mutated.contains(&name_str);
                    let mutable = match decl.kind {
                        // `const` forbids a rebind, but a member mutation
                        // (`xs.push`) or a borrow through a reference-parameter
                        // call site (`f(&mut xs)`) still needs `let mut` for the
                        // `&mut` borrow.
                        VariableDeclarationKind::Const => {
                            member_mut || locals.mutated.contains(&name_str)
                        }
                        VariableDeclarationKind::Let | VariableDeclarationKind::Var => {
                            member_mut || locals.mutated.contains(&name_str)
                        }
                        _ => false,
                    };
                    let mut ty = d
                        .type_annotation
                        .as_ref()
                        .map(|ta| types::translate_type(&ta.type_annotation))
                        .or_else(|| d.init.as_ref().and_then(infer_literal_type))
                        .or_else(|| {
                            // An initializer that needs the local's recorded
                            // type context. `Object.assign(target, …)` returns a
                            // value of `target`'s type, so an unannotated
                            // `let r = Object.assign(t, …)` records `t`'s type —
                            // letting `r.foo` route through `is_hashmap_local`
                            // (HashMap field access). A `record[key]` index
                            // access records the map's value type, so an
                            // unannotated `const v = record[key]` carries the
                            // union-enum type and `v !== undefined` routes
                            // through `union_null_equality`. Otherwise a numeric
                            // expression (`-0`, arithmetic, a `Math` call, a
                            // known `f64` local) records as `f64` so number→
                            // string emit routes through `__ds::number_to_string`.
                            let init = d.init.as_ref()?;
                            let ctx = Ctx::new(&*locals, registry, narrow, names);
                            object_assign_type(init, &ctx)
                                .or_else(|| match_result_type(init, &ctx))
                                .or_else(|| index_access_type(init, &ctx))
                                .or_else(|| {
                                    expressions::is_number_expr(init, &ctx)
                                        .then(|| parse_quote!(f64))
                                })
                        });
                    // Number-flavor promotion: an unannotated integer-only local
                    // emits as `i64` (loop counters / accumulators) so the loop
                    // skips the `f64` cast chain. `: number` already forced `F64`
                    // in `flavor::infer` (R1), so an `I64` here is genuinely
                    // integer-only. Only a current `f64` type is promoted —
                    // `String`/`bool`/`Vec` are left untouched.
                    if names
                        .symbol_of_pattern(&d.id)
                        .and_then(|sym| locals.number_flavors.get(&sym).copied())
                        == Some(super::flavor::NumberFlavor::I64)
                        && ty
                            .as_ref()
                            .and_then(path_of)
                            .is_some_and(|p| p.segments.last().is_some_and(|s| s.ident == "f64"))
                    {
                        ty = Some(parse_quote!(i64));
                    }
                    if let Some(path) = ty.as_ref().and_then(path_of) {
                        locals.insert(name.to_string(), path);
                    }
                    let init = d.init.as_ref().map(|e| {
                        expressions::translate_init(
                            e,
                            ty.as_ref(),
                            &Ctx::new(&*locals, registry, narrow, names),
                        )
                    });
                    vec![build_local(&name, mutable, ty.as_ref(), init.as_ref())]
                }
            }
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
