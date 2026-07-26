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
    Class, ClassElement, Declaration, Expression, Function, MethodDefinitionKind, Statement,
    TSInterfaceDeclaration, TSLiteral, TSSignature, TSType, TSTypeAliasDeclaration, TSTypeLiteral,
    VariableDeclaration,
};
use syn::{parse_quote, Ident, ItemEnum, Path, Type};

use super::analysis;
use super::bindings;
use super::declarations;
use super::name_table::NameTable;
use super::types;

/// A discriminated-union variant: its Rust name (from the `kind` value) and its
/// data-field names (every property except the discriminant).
#[derive(Clone)]
pub struct VariantShape {
    pub name: Ident,
    pub fields: Vec<Ident>,
}

/// A field of an interface (or one it `extends`): the snake-case Rust name,
/// the translated type, and whether it is optional (`?:`). Recorded in the
/// first pass so `interface B extends A` flattens `A`'s fields into `B`'s
/// struct — Rust has no struct inheritance, so the parent's fields are merged
/// verbatim (ES override semantics: a child field of the same name wins).
#[derive(Clone)]
pub struct InterfaceField {
    pub name: String,
    pub ty: Type,
    pub optional: bool,
}

/// Project-wide type info gathered in the first pass.
pub struct TypeRegistry {
    /// Discriminated-union enums: type name → (`kind` value → variant shape).
    pub unions: HashMap<String, HashMap<String, VariantShape>>,
    /// Function name (original `.ts` spelling) → each parameter's type path,
    /// or `None` where the parameter has no annotation.
    pub functions: HashMap<String, Vec<Option<Path>>>,
    /// Function name → per-parameter "has a default initializer?" flag. Callers
    /// wrap a supplied value in `Some`, and an omitted trailing one in `None`.
    pub function_defaults: HashMap<String, Vec<bool>>,
    /// Function name → per-parameter "is a reference parameter?" (`&mut`) flag.
    /// A parameter the body member-mutates (`c[i] = v`, `xs.push(…)`) but does
    /// not rebind becomes `&mut T`, so the caller's mutation is visible — ES
    /// reference semantics for arrays/objects.
    pub ref_params: HashMap<String, Vec<bool>>,
    /// Struct/interface name → its optional (`?:`) field names. A struct
    /// literal that omits one of these is filled with `None`.
    pub structs: HashMap<String, HashSet<String>>,
    /// The project's own `&mut self` class methods, by original `.ts` name. A
    /// call `obj.m()` with `m` in this set marks the receiver `let mut` — the
    /// `&mut self` analogue of the built-in `MUTATORS` (`push`, `splice` …).
    pub mut_methods: HashSet<String>,
    /// Inline all-scalar-keyword unions (`string | number | undefined`, the
    /// XML-attribute / JSON-value shape) found in any type position, keyed by
    /// the generated enum name (`__DsUnion…`). One definition per unique shape
    /// (member order is canonicalized in `declarations::scalar_union_enum`);
    /// `types::union_type` references the same name, and the translator emits
    /// these definitions before the body items.
    pub union_enums: HashMap<Ident, ItemEnum>,
    /// Interface name → the interfaces it `extends`. Used to flatten parent
    /// fields into the child struct (Rust has no struct inheritance).
    pub interface_extends: HashMap<String, Vec<String>>,
    /// Interface name → its own declared fields (excluding inherited). The
    /// translate pass merges these across the `extends` chain.
    pub interface_own_fields: HashMap<String, Vec<InterfaceField>>,
}

impl TypeRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            unions: HashMap::new(),
            functions: HashMap::new(),
            function_defaults: HashMap::new(),
            ref_params: HashMap::new(),
            structs: HashMap::new(),
            mut_methods: HashSet::new(),
            union_enums: HashMap::new(),
            interface_extends: HashMap::new(),
            interface_own_fields: HashMap::new(),
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
            Statement::TSTypeAliasDeclaration(alias) => register_type_alias(alias, &mut registry),
            Statement::TSInterfaceDeclaration(iface) => register_interface(iface, &mut registry),
            Statement::FunctionDeclaration(func) => register_function(func, names, &mut registry),
            Statement::ClassDeclaration(class) => register_class(class, names, &mut registry),
            // A top-level `const r: Record<K, …|…> = …` annotation carries an
            // inline scalar union — register it so the initializer's object
            // literal can box its values into the enum variants.
            Statement::VariableDeclaration(decl) => {
                register_variable_declaration(decl, &mut registry)
            }
            // `export function f` / `export type T` is wrapped in
            // `ExportNamedDeclaration { declaration }` — recurse the inner
            // declaration so an exported function's inline-union parameter still
            // gets its enum emitted (and its params/ref-flags registered).
            Statement::ExportNamedDeclaration(exp) => match &exp.declaration {
                Some(Declaration::FunctionDeclaration(func)) => {
                    register_function(func, names, &mut registry)
                }
                Some(Declaration::TSTypeAliasDeclaration(alias)) => {
                    register_type_alias(alias, &mut registry)
                }
                Some(Declaration::ClassDeclaration(class)) => {
                    register_class(class, names, &mut registry)
                }
                Some(Declaration::TSInterfaceDeclaration(iface)) => {
                    register_interface(iface, &mut registry)
                }
                Some(Declaration::VariableDeclaration(decl)) => {
                    register_variable_declaration(decl, &mut registry)
                }
                _ => {}
            },
            _ => {}
        }
    }
    registry
}

fn register_type_alias(alias: &TSTypeAliasDeclaration, registry: &mut TypeRegistry) {
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
    collect_unions_in_type(&alias.type_annotation, &mut registry.union_enums);
}

fn register_interface(iface: &TSInterfaceDeclaration, registry: &mut TypeRegistry) {
    let name = iface.id.name.to_string();
    let fields: Vec<InterfaceField> = iface
        .body
        .body
        .iter()
        .filter_map(|sig| {
            let TSSignature::TSPropertySignature(ps) = sig else {
                return None;
            };
            let key = bindings::property_key_name(&ps.key)?;
            let ty = ps
                .type_annotation
                .as_ref()
                .map(|ta| types::translate_type(&ta.type_annotation))
                .unwrap_or_else(|| parse_quote!(_));
            Some(InterfaceField {
                name: key.to_string(),
                ty,
                optional: ps.optional,
            })
        })
        .collect();
    registry.interface_own_fields.insert(name.clone(), fields);
    let extends: Vec<String> = iface
        .extends
        .iter()
        .filter_map(|h| heritage_name(&h.expression))
        .collect();
    if !extends.is_empty() {
        registry.interface_extends.insert(name.clone(), extends);
    }
    let optionals = collect_optionals(&iface.body.body);
    if !optionals.is_empty() {
        registry.structs.insert(name, optionals);
    }
    collect_unions_in_signatures(&iface.body.body, &mut registry.union_enums);
}

/// The name an `extends` clause references (the parent interface), if it is a
/// plain identifier — `interface B extends A` → `Some("A")`. A non-identifier
/// heritage (a computed/membership expression) is not a static parent.
fn heritage_name(expr: &Expression) -> Option<String> {
    match expr {
        Expression::Identifier(id) => Some(id.name.to_string()),
        _ => None,
    }
}

fn register_function(func: &Function, names: &NameTable, registry: &mut TypeRegistry) {
    let name = function_name(func);
    registry
        .functions
        .insert(name.clone(), function_params(func));
    registry
        .function_defaults
        .insert(name.clone(), function_default_flags(func));
    registry
        .ref_params
        .insert(name, ref_param_flags(func, names));
    // An inline scalar union in a parameter type (e.g. `Record<string,
    // string|number>`) needs its `__DsUnion…` enum emitted; recurse into every
    // annotation so a union nested in a `Record` value type is found.
    for fp in &func.params.items {
        if let Some(ta) = fp.type_annotation.as_ref() {
            collect_unions_in_type(&ta.type_annotation, &mut registry.union_enums);
        }
    }
    if let Some(ta) = func.return_type.as_deref() {
        collect_unions_in_type(&ta.type_annotation, &mut registry.union_enums);
    }
}

/// A top-level `const`/`let`/`var` with a type annotation. An inline scalar
/// union in the annotation (`const r: Record<K, string|number> = …`) needs its
/// `__DsUnion…` enum registered (and emitted) so the initializer's object
/// literal can box each value into the matching variant.
fn register_variable_declaration(decl: &VariableDeclaration, registry: &mut TypeRegistry) {
    for d in &decl.declarations {
        if let Some(ta) = d.type_annotation.as_ref() {
            collect_unions_in_type(&ta.type_annotation, &mut registry.union_enums);
        }
    }
}

fn register_class(class: &Class, names: &NameTable, registry: &mut TypeRegistry) {
    collect_mut_methods(class, names, &mut registry.mut_methods);
}

/// Recursively collect every all-scalar-keyword union nested in a type
/// annotation, registering a `__DsUnion…` enum definition for each unique
/// shape. Descends into array elements, `Record`/`Array` type arguments,
/// `readonly` operands, union members, and object-literal property types, so
/// an inline union anywhere in a parameter/field/return type is found. The
/// enum name and variants come from [`declarations::scalar_union_enum`], the
/// single source of truth that `types::union_type` also reads.
fn collect_unions_in_type(ty: &TSType, out: &mut HashMap<Ident, ItemEnum>) {
    match ty {
        TSType::TSUnionType(u) => {
            if let Some((name, variants)) = declarations::scalar_union_enum(u) {
                out.entry(name.clone()).or_insert_with(|| {
                    parse_quote! {
                        #[derive(Clone, Debug, PartialEq)]
                        enum #name { #(#variants),* }
                    }
                });
            }
            for t in &u.types {
                collect_unions_in_type(t, out);
            }
        }
        TSType::TSArrayType(arr) => collect_unions_in_type(&arr.element_type, out),
        TSType::TSTypeReference(r) => {
            if let Some(args) = r.type_arguments.as_ref() {
                for p in &args.params {
                    collect_unions_in_type(p, out);
                }
            }
        }
        TSType::TSTypeOperatorType(op) => collect_unions_in_type(&op.type_annotation, out),
        TSType::TSTypeLiteral(lit) => collect_unions_in_signatures(&lit.members, out),
        _ => {}
    }
}

/// Collect inline unions from each property type of an interface or
/// object-literal type. A pure index-signature interface (`[key: string]: A |
/// B`) lowers to a `HashMap` alias whose value type may itself be an inline
/// union — that union's `__DsUnion…` enum must be collected too, or the alias
/// references a `crate::__DsUnion…` the crate root never defines (E0425).
fn collect_unions_in_signatures(members: &[TSSignature], out: &mut HashMap<Ident, ItemEnum>) {
    for sig in members {
        match sig {
            TSSignature::TSPropertySignature(ps) => {
                if let Some(ta) = ps.type_annotation.as_ref() {
                    collect_unions_in_type(&ta.type_annotation, out);
                }
            }
            TSSignature::TSIndexSignature(idx) => {
                collect_unions_in_type(&idx.type_annotation.type_annotation, out);
            }
            _ => {}
        }
    }
}

/// Collect every `&mut self` instance method name across a class. A method is
/// `&mut self` when its body assigns/updates a member of `this` — the same
/// `mutates_this` test `build_method` applies at emit time, run here in the
/// first pass so call sites can mark their receiver `let mut`.
fn collect_mut_methods(class: &Class, names: &NameTable, out: &mut HashSet<String>) {
    let empty: HashSet<String> = HashSet::new();
    let no_ref_params: HashMap<String, Vec<bool>> = HashMap::new();
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
        let analysis = analysis::analyze(&body.statements, names, &empty, &no_ref_params);
        if analysis.mutates_this {
            if let Some(name) = bindings::property_key_name(&md.key) {
                out.insert(name.to_string());
            }
        }
    }
}

/// A function's original `.ts` name (defaults to `main` for anonymous).
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

/// Per-parameter "is a reference parameter?" (`&mut`) flag: `true` where the
/// body member-mutates the parameter (`c[i] = v`, `xs.push(…)`) but does not
/// rebind it — the ES reference-parameter case. A rebound parameter is owned
/// (`mut c`), since rebinding does not propagate to the caller.
fn ref_param_flags(func: &Function, names: &NameTable) -> Vec<bool> {
    let Some(body) = func.body.as_deref() else {
        return vec![false; func.params.items.len()];
    };
    let empty: HashSet<String> = HashSet::new();
    let no_ref_params: HashMap<String, Vec<bool>> = HashMap::new();
    let analysis = analysis::analyze(&body.statements, names, &empty, &no_ref_params);
    func.params
        .items
        .iter()
        .map(|fp| {
            let name = names.of_pattern(&fp.pattern).to_string();
            analysis.member_mutated.contains(&name) && !analysis.mutated.contains(&name)
        })
        .collect()
}

/// The `syn::Path` of a `.ts` type annotation, when it is a path-like type.
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
