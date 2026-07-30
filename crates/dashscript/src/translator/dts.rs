//! `.d.ts` declaration files → Rust module source. A `.d.ts` carries only
//! types — no runtime implementation — so a type-only import (`import type
//! { Foo }`) maps its `interface`/`type` to Rust `struct`/`type` items the
//! importer uses directly. A value import from a `.d.ts` with no sibling
//! `.js` (a pure `@types/*` package) has no implementation to lower: the
//! symbol is not emitted, and `cargo check` reports "cannot find function"
//! honestly.
//!
//! `declare class` / `declare module` / `declare namespace` are a later batch.

use oxc_allocator::Allocator;
use oxc_ast::ast::{Declaration, Statement, TSInterfaceDeclaration, TSTypeAliasDeclaration};
use oxc_parser::Parser;
use oxc_span::SourceType;
use syn::{parse_quote, Item};

use super::declarations;

/// Translate a `.d.ts` source to a Rust module body: each `interface`/`type`
/// (bare or `export`-ed) becomes a `pub` struct/alias — an imported type must
/// be visible across the module boundary. `declare function`/`declare const`
/// emit nothing — a value import surfaces as a `cargo check` "cannot find
/// function" error honestly, since a pure `.d.ts` carries no implementation.
pub fn translate_dts(source: &str) -> String {
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, SourceType::ts()).parse();
    let mut items: Vec<Item> = Vec::new();
    for stmt in &ret.program.body {
        match stmt {
            // A bare `interface`/`type` (no `export`) — uncommon in a `.d.ts`,
            // but handled for completeness.
            Statement::TSInterfaceDeclaration(i) => push_interface(&mut items, i),
            Statement::TSTypeAliasDeclaration(a) => push_type_alias(&mut items, a),
            // `export interface` / `export type` — the common form. oxc wraps
            // the type declaration in an ExportNamedDeclaration.
            Statement::ExportNamedDeclaration(exp) => {
                if let Some(decl) = exp.declaration.as_ref() {
                    match decl {
                        Declaration::TSInterfaceDeclaration(i) => {
                            push_interface(&mut items, i);
                        }
                        Declaration::TSTypeAliasDeclaration(a) => {
                            push_type_alias(&mut items, a);
                        }
                        // declare function / declare const (pure .d.ts): no item
                        // — a value import fails at cargo check (no .js impl).
                        _ => {}
                    }
                }
            }
            // declare class / module / namespace: later batch.
            _ => {}
        }
    }
    let file = syn::File {
        shebang: None,
        attrs: Vec::new(),
        items,
    };
    prettyplease::unparse(&file)
}

fn push_interface(items: &mut Vec<Item>, iface: &TSInterfaceDeclaration) {
    for mut item in
        declarations::translate_interface(iface, &super::registry::TypeRegistry::default())
    {
        make_pub(&mut item);
        items.push(item);
    }
}

fn push_type_alias(items: &mut Vec<Item>, alias: &TSTypeAliasDeclaration) {
    for mut item in declarations::translate_type_alias(alias) {
        make_pub(&mut item);
        items.push(item);
    }
}

/// Mark a type item `pub` — an imported `interface`/`type` must be visible
/// across the `mod` boundary. A reduced copy of `functions::make_pub` for the
/// type-only items a `.d.ts` produces (no `Fn`/`Impl`).
fn make_pub(item: &mut Item) {
    match item {
        Item::Struct(s) => s.vis = parse_quote!(pub),
        Item::Enum(e) => e.vis = parse_quote!(pub),
        Item::Type(t) => t.vis = parse_quote!(pub),
        _ => {}
    }
}

/// A `declare function` signature extracted from a `.d.ts`: the name, each
/// parameter's Rust type (an unmappable TS type becomes `serde_json::Value` via
/// [`types::translate_type_degraded`]; an optional `?:` parameter is
/// `Option<T>`), and the return type (`None` when the annotation is absent).
/// A degraded `.js` module's stub fns specialize their signatures from this, so
/// a static call site stays type-correct when every type is marshal-safe.
pub struct DtsFnSig {
    /// The declared function name (the stub fn uses it verbatim).
    pub name: String,
    /// Each parameter's Rust type, in declaration order.
    pub params: Vec<syn::Type>,
    /// The return type, or `None` when no `: T` annotation is present.
    pub ret: Option<syn::Type>,
}

/// The `declare function` signatures in a `.d.ts` source (bare or `export`-ed).
/// A parse error yields none. A degraded `.js` module's stub emitter uses this
/// to specialize a stub fn's signature from its `.d.ts` when possible; the
/// `Option<T>` shape of an optional parameter signals "not marshal-safe" to the
/// emitter, so it falls back to a `Value` stub rather than guessing.
#[must_use]
pub fn dts_fn_signatures(source: &str) -> Vec<DtsFnSig> {
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, SourceType::ts()).parse();
    if !ret.diagnostics.is_empty() {
        return Vec::new();
    }
    let program = allocator.alloc(ret.program);
    let mut out = Vec::new();
    for stmt in &program.body {
        // A `declare function f()` is `FunctionDeclaration` — bare at the top
        // level (the `Statement` variant inherits from `Declaration`) or wrapped
        // in `ExportNamedDeclaration` for `export declare function f()`.
        let f = match stmt {
            Statement::ExportNamedDeclaration(exp) => match &exp.declaration {
                Some(Declaration::FunctionDeclaration(b)) => &**b,
                _ => continue,
            },
            Statement::FunctionDeclaration(b) => &**b,
            _ => continue,
        };
        let Some(id) = &f.id else { continue };
        let params = f
            .params
            .items
            .iter()
            .map(|fp| {
                // An unmappable annotation (a complex union, `unknown`) degrades
                // to `serde_json::Value`; an unannotated param (rare in a
                // `.d.ts`) defaults to `Value` too, since `_` is not a legal
                // signature type.
                let inner = fp
                    .type_annotation
                    .as_deref()
                    .map(|ta| super::types::translate_type_degraded(&ta.type_annotation))
                    .unwrap_or_else(|| parse_quote!(::serde_json::Value));
                // An optional (`?:`) or default-initialized parameter is
                // `Option<T>` — the same shape `translate_params` emits.
                if fp.optional || fp.initializer.is_some() {
                    parse_quote!(Option<#inner>)
                } else {
                    inner
                }
            })
            .collect();
        let ret_ty = f
            .return_type
            .as_deref()
            .map(|ta| super::types::translate_type_degraded(&ta.type_annotation));
        out.push(DtsFnSig {
            name: id.name.to_string(),
            params,
            ret: ret_ty,
        });
    }
    out
}
