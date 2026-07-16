//! Project-wide type definitions collected in a first pass.
//!
//! Expression translation sometimes needs a type's *shape* or a callee's
//! parameter types — not just a local's type path. E.g. to tell whether
//! `{ kind: "circle", radius: 2 }` builds a struct or an enum variant, or to
//! give `f({ x, y })` its struct name from `f`'s declared parameter type.
//! `build_registry` walks top-level declarations once and records each
//! discriminated-union enum's variants and each function's parameter types.
//! The variant-extraction logic here mirrors `declarations::discriminated_variant`
//! (which emits the enum); both read the same "string-literal property is the
//! discriminant" rule.

use std::collections::HashMap;

use oxc_ast::ast::{
    Function, Statement, TSLiteral, TSSignature, TSType, TSTypeAliasDeclaration, TSTypeLiteral,
};
use syn::{Ident, Path};

use super::bindings;
use super::types;

/// A discriminated-union variant: its Rust name (from the `kind` value) and its
/// data-field names (every property except the discriminant).
#[derive(Clone)]
pub struct VariantShape {
    pub name: Ident,
    pub fields: Vec<Ident>,
}

/// Project-wide type info gathered in the first pass.
pub struct TypeRegistry {
    /// Discriminated-union enums: type name → (`kind` value → variant shape).
    pub unions: HashMap<String, HashMap<String, VariantShape>>,
    /// Function name (original `.ds` spelling) → each parameter's type path,
    /// or `None` where the parameter has no annotation.
    pub functions: HashMap<String, Vec<Option<Path>>>,
}

impl TypeRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            unions: HashMap::new(),
            functions: HashMap::new(),
        }
    }
}

impl Default for TypeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Scan top-level type aliases (discriminated unions) and function declarations
/// (parameter types), recording both into a [`TypeRegistry`].
#[must_use]
pub fn build_registry(statements: &[Statement]) -> TypeRegistry {
    let mut registry = TypeRegistry::new();
    for stmt in statements {
        match stmt {
            Statement::TSTypeAliasDeclaration(alias) => {
                if let Some(variants) = discriminated_enum(alias) {
                    registry.unions.insert(alias.id.name.to_string(), variants);
                }
            }
            Statement::FunctionDeclaration(func) => {
                registry.functions.insert(function_name(func), function_params(func));
            }
            _ => {}
        }
    }
    registry
}

/// A function's original `.ds` name (defaults to `main` for anonymous).
fn function_name(func: &Function) -> String {
    func.id
        .as_ref()
        .map_or_else(|| "main".to_string(), |id| id.name.to_string())
}

/// Each parameter's type path — `None` where the parameter is unannotated.
fn function_params(func: &Function) -> Vec<Option<Path>> {
    func.params
        .items
        .iter()
        .map(|fp| {
            fp.type_annotation
                .as_ref()
                .and_then(|ta| path_of_type(&ta.type_annotation))
        })
        .collect()
}

/// The `syn::Path` of a `.ds` type annotation, when it is a path-like type.
fn path_of_type(ty: &TSType) -> Option<Path> {
    match types::translate_type(ty) {
        syn::Type::Path(tp) => Some(tp.path),
        _ => None,
    }
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
