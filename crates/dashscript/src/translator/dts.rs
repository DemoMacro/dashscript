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
