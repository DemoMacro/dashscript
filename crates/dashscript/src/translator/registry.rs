//! Project-wide type definitions collected in a first pass.
//!
//! Expression translation sometimes needs a type's *shape*, not just a local's
//! type path — e.g. to tell whether `{ kind: "circle", radius: 2 }` builds a
//! struct or an enum variant. `build_registry` walks top-level declarations
//! once and records each discriminated-union enum's variants so later passes
//! can query them. The variant-extraction logic here mirrors
//! `declarations::discriminated_variant` (which emits the enum); both read the
//! same "string-literal property is the discriminant" rule.

use std::collections::HashMap;

use oxc_ast::ast::{Statement, TSLiteral, TSSignature, TSType, TSTypeAliasDeclaration, TSTypeLiteral};
use syn::Ident;

use super::bindings;

/// A discriminated-union variant: its Rust name (from the `kind` value) and its
/// data-field names (every property except the discriminant).
#[derive(Clone)]
pub struct VariantShape {
    pub name: Ident,
    pub fields: Vec<Ident>,
}

/// `type name` → (`kind` value → variant shape). Only discriminated-union
/// enums are recorded; structs and other enums are absent, so callers treat a
/// miss as "not a discriminated union".
pub type TypeRegistry = HashMap<String, HashMap<String, VariantShape>>;

/// Scan top-level type aliases and record every discriminated-union enum.
#[must_use]
pub fn build_registry(statements: &[Statement]) -> TypeRegistry {
    let mut registry = TypeRegistry::new();
    for stmt in statements {
        let Statement::TSTypeAliasDeclaration(alias) = stmt else {
            continue;
        };
        if let Some(variants) = discriminated_enum(alias) {
            registry.insert(alias.id.name.to_string(), variants);
        }
    }
    registry
}

/// The variant table for a discriminated-union alias (`{ kind: "x"; …} | …`),
/// or `None` when the alias is not a union of object literals each carrying a
/// string-literal discriminant.
fn discriminated_enum(alias: &TSTypeAliasDeclaration) -> Option<HashMap<String, VariantShape>> {
    let TSType::TSUnionType(u) = &alias.type_annotation else {
        return None;
    };
    let mut variants = HashMap::new();
    for t in &u.types {
        let TSType::TSTypeLiteral(lit) = t else {
            return None;
        };
        let (kind_value, name, fields) = variant_of(lit)?;
        variants.insert(kind_value, VariantShape { name, fields });
    }
    Some(variants)
}

/// `(kind value, variant name, data fields)` from one object-literal union
/// member. The string-literal-typed property is the discriminant; the rest are
/// data fields. Returns `None` if there is no string-literal discriminant.
fn variant_of(lit: &TSTypeLiteral) -> Option<(String, Ident, Vec<Ident>)> {
    let mut kind_value: Option<String> = None;
    let mut fields: Vec<Ident> = Vec::new();
    for sig in &lit.members {
        let TSSignature::TSPropertySignature(ps) = sig else {
            continue;
        };
        let Some(key) = bindings::property_key_name(&ps.key) else {
            continue;
        };
        let Some(ta) = ps.type_annotation.as_ref() else {
            continue;
        };
        if let TSType::TSLiteralType(lt) = &ta.type_annotation {
            if let TSLiteral::StringLiteral(s) = &lt.literal {
                kind_value = Some(s.value.to_string());
                continue;
            }
        }
        fields.push(key);
    }
    let value = kind_value?;
    let name = bindings::pascal(&value);
    Some((value, name, fields))
}
