//! `class` → `#[derive(Clone)] struct Name { ... } impl Name { ... }`.
//!
//! A class becomes a `struct` plus an `impl`. Instance fields map to `pub`
//! struct fields; a `new` constructor fills them. Constructors with parameters
//! and instance methods (`this` → `self`) land in the next phase.
use oxc_ast::ast::{Class, ClassElement, PropertyDefinition};
use proc_macro2::TokenStream;
use quote::quote;
use syn::{parse_quote, Expr, Ident, Item, Type};

use super::bindings;
use super::context::{Ctx, Locals, Narrow};
use super::registry::TypeRegistry;
use super::{expressions, types};

/// Translate a `class` declaration into one or more items: the `struct` plus
/// its `impl` (and any `compile_error!` items for unsupported features).
pub(in crate::translator) fn translate_class(class: &Class, registry: &TypeRegistry) -> Vec<Item> {
    let Some(id) = class.id.as_ref() else {
        return vec![compile_error_item(
            "DashScript does not support class expressions — declare a named class",
        )];
    };
    let name = bindings::type_ident(&id.name);

    // Collect instance fields (static/computed/private are unsupported yet).
    let mut fields: Vec<(Ident, Type, Option<Expr>)> = Vec::new();
    for elem in &class.body.body {
        if let ClassElement::PropertyDefinition(pd) = elem {
            if let Some(field) = instance_field(pd, registry) {
                fields.push(field);
            }
        }
        // MethodDefinition (constructor / methods) is handled in the next phase.
    }

    let struct_fields: Vec<TokenStream> = fields
        .iter()
        .map(|(k, ty, _)| quote!(pub #k: #ty,))
        .collect();
    let struct_item: Item = parse_quote! {
        #[derive(Clone)]
        struct #name { #(#struct_fields)* }
    };

    // `fn new()` fills each field from its default initializer, or `todo!()`
    // when the field has neither a default nor a constructor assignment.
    let field_inits: Vec<TokenStream> = fields
        .iter()
        .map(|(k, _, default)| match default {
            Some(d) => quote!(#k: #d),
            None => quote!(#k: ::core::todo!()),
        })
        .collect();
    let impl_item: Item = parse_quote! {
        impl #name {
            pub fn new() -> #name { #name { #(#field_inits),* } }
        }
    };

    vec![struct_item, impl_item]
}

/// An instance field `x: T` / `x?: T` / `x = v` → (name, type, optional default).
/// Static, computed, or private fields are unsupported (None).
fn instance_field(
    pd: &PropertyDefinition,
    registry: &TypeRegistry,
) -> Option<(Ident, Type, Option<Expr>)> {
    if pd.r#static || pd.computed {
        return None;
    }
    let name = bindings::property_key_name(&pd.key)?;
    let ty = pd
        .type_annotation
        .as_ref()
        .map(|ta| types::translate_type(&ta.type_annotation))
        .unwrap_or_else(|| parse_quote!(_));
    let ty = if pd.optional {
        parse_quote!(Option<#ty>)
    } else {
        ty
    };
    // A field initializer `x = 5` runs at class scope (no `this`), translated
    // against an empty locals table.
    let default = pd.value.as_ref().map(|e| {
        let locals = Locals::new();
        let narrow = Narrow::default();
        let ctx = Ctx::new(&locals, registry, &narrow);
        expressions::translate_expr(e, &ctx)
    });
    Some((name, ty, default))
}

/// A `compile_error!` item carrying `message`, so unsupported features fail
/// loudly without breaking the surrounding generated Rust.
fn compile_error_item(message: &str) -> Item {
    let msg = syn::LitStr::new(message, proc_macro2::Span::call_site());
    parse_quote!(compile_error!(#msg);)
}
