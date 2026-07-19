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

use std::collections::{HashMap, HashSet};

use oxc_ast::ast::{
    Class, ClassElement, Function, MethodDefinitionKind, Statement, TSLiteral, TSSignature, TSType,
    TSTypeAliasDeclaration, TSTypeLiteral,
};
use syn::{Ident, Path};

use super::analysis;
use super::bindings;
use super::name_table::NameTable;
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
    /// Function name → per-parameter "has a default initializer?" flag. Callers
    /// wrap a supplied value in `Some`, and an omitted trailing one in `None`.
    pub function_defaults: HashMap<String, Vec<bool>>,
    /// Struct/interface name → its optional (`?:`) field names. A struct
    /// literal that omits one of these is filled with `None`.
    pub structs: HashMap<String, HashSet<String>>,
    /// The project's own `&mut self` class methods, by original `.ds` name. A
    /// call `obj.m()` with `m` in this set marks the receiver `let mut` — the
    /// `&mut self` analogue of the built-in `MUTATORS` (`push`, `splice` …).
    pub mut_methods: HashSet<String>,
}

impl TypeRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            unions: HashMap::new(),
            functions: HashMap::new(),
            function_defaults: HashMap::new(),
            structs: HashMap::new(),
            mut_methods: HashSet::new(),
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
pub fn build_registry(statements: &[Statement], names: &NameTable) -> TypeRegistry {
    let mut registry = TypeRegistry::new();
    for stmt in statements {
        match stmt {
            Statement::TSTypeAliasDeclaration(alias) => {
                if let Some(variants) = discriminated_enum(alias) {
                    registry.unions.insert(alias.id.name.to_string(), variants);
                }
                if let Some(optionals) = struct_optional_fields_of_alias(alias) {
                    if !optionals.is_empty() {
                        registry
                            .structs
                            .insert(alias.id.name.to_string(), optionals);
                    }
                }
            }
            Statement::TSInterfaceDeclaration(iface) => {
                let optionals = collect_optionals(&iface.body.body);
                if !optionals.is_empty() {
                    registry
                        .structs
                        .insert(iface.id.name.to_string(), optionals);
                }
            }
            Statement::FunctionDeclaration(func) => {
                let name = function_name(func);
                registry
                    .functions
                    .insert(name.clone(), function_params(func));
                registry
                    .function_defaults
                    .insert(name, function_default_flags(func));
            }
            Statement::ClassDeclaration(class) => {
                collect_mut_methods(class, names, &mut registry.mut_methods);
            }
            _ => {}
        }
    }
    registry
}

/// Collect every `&mut self` instance method name across a class. A method is
/// `&mut self` when its body assigns/updates a member of `this` — the same
/// `mutates_this` test `build_method` applies at emit time, run here in the
/// first pass so call sites can mark their receiver `let mut`.
fn collect_mut_methods(class: &Class, names: &NameTable, out: &mut HashSet<String>) {
    let empty: HashSet<String> = HashSet::new();
    for elem in &class.body.body {
        let ClassElement::MethodDefinition(md) = elem else {
            continue;
        };
        if md.kind != MethodDefinitionKind::Method {
            continue;
        }
        let Some(body) = md.value.body.as_deref() else {
            continue;
        };
        let analysis = analysis::analyze(&body.statements, names, &empty);
        if analysis.mutates_this {
            if let Some(name) = bindings::property_key_name(&md.key) {
                out.insert(name.to_string());
            }
        }
    }
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

/// Per-parameter "has a default initializer (`= …`)" flag.
fn function_default_flags(func: &Function) -> Vec<bool> {
    func.params
        .items
        .iter()
        .map(|fp| fp.initializer.is_some())
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

/// Names of the optional (`?:`) properties among a list of signatures. These
/// become `Option<T>` struct fields; a literal that omits one is filled `None`.
fn collect_optionals(members: &[TSSignature]) -> HashSet<String> {
    members
        .iter()
        .filter_map(|sig| {
            let TSSignature::TSPropertySignature(ps) = sig else {
                return None;
            };
            if !ps.optional {
                return None;
            }
            bindings::property_key_name(&ps.key).map(|k| k.to_string())
        })
        .collect()
}

/// Optional fields of a `type T = { … }` alias (not a union). `None` when the
/// alias is not a plain object-literal type.
fn struct_optional_fields_of_alias(alias: &TSTypeAliasDeclaration) -> Option<HashSet<String>> {
    let TSType::TSTypeLiteral(lit) = &alias.type_annotation else {
        return None;
    };
    Some(collect_optionals(&lit.members))
}
