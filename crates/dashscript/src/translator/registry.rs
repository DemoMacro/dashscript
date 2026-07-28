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
    TSEnumDeclaration, TSInterfaceDeclaration, TSLiteral, TSSignature, TSType,
    TSTypeAliasDeclaration, TSTypeLiteral, VariableDeclaration,
};
use syn::{parse_quote, Ident, ItemEnum, ItemStruct, Path, Type};

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
    /// Function name → its declared return type path, so a
    /// `ReturnType<typeof fn>` query resolves to the function's return type.
    /// `None` where the function has no return annotation (the query then
    /// falls back to `_`, the way an unannotated return would).
    pub function_returns: HashMap<String, Option<Path>>,
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
    /// Anonymous object-literal types (`{ x: number; … }`) found in any type
    /// position (a function return/parameter, a union member, an interface
    /// field, a `type` alias body), keyed by the generated `__DsAnon_<hash>`
    /// name. One definition per unique shape (the hash is over sorted field
    /// names + translated types); `types::translate_type` references the same
    /// name, and the translator emits these at the crate root — mirroring
    /// `union_enums`.
    pub anon_structs: HashMap<Ident, ItemStruct>,
    /// Interface name → the interfaces it `extends`. Used to flatten parent
    /// fields into the child struct (Rust has no struct inheritance).
    pub interface_extends: HashMap<String, Vec<String>>,
    /// Interface name → its own declared fields (excluding inherited). The
    /// translate pass merges these across the `extends` chain.
    pub interface_own_fields: HashMap<String, Vec<InterfaceField>>,
    /// TS `enum` names (original `.ts` spelling) that lowered cleanly to a
    /// `mod` of literal consts. A `Color.Red` access on one reads as
    /// `Color::Red` (a path constant). A mixed/heterogeneous enum or one with
    /// a non-literal initializer is not registered, so the access falls
    /// through to a struct field and surfaces at `cargo check`.
    pub enums: HashSet<String>,
}

impl TypeRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            unions: HashMap::new(),
            functions: HashMap::new(),
            function_defaults: HashMap::new(),
            ref_params: HashMap::new(),
            function_returns: HashMap::new(),
            structs: HashMap::new(),
            mut_methods: HashSet::new(),
            union_enums: HashMap::new(),
            anon_structs: HashMap::new(),
            interface_extends: HashMap::new(),
            interface_own_fields: HashMap::new(),
            enums: HashSet::new(),
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
            Statement::TSEnumDeclaration(decl) => register_enum(decl, &mut registry),
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
                Some(Declaration::TSEnumDeclaration(decl)) => register_enum(decl, &mut registry),
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
    collect_inline_type_defs(&alias.type_annotation, registry);
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
    let mut optionals = collect_optionals(&iface.body.body);
    // Flatten `extends`: a child interface inherits the parent's optional
    // (`?:`) fields. Rust has no struct inheritance, so the parent's
    // optionals merge into the child's set — otherwise a child literal or
    // `child?.parentField ?? d` would not see the inherited optional. The
    // borrow ends before `extends` is moved into `interface_extends` below.
    for parent in &extends {
        if let Some(parent_opts) = registry.structs.get(parent.as_str()).cloned() {
            optionals.extend(parent_opts);
        }
    }
    if !optionals.is_empty() {
        registry.structs.insert(name.clone(), optionals);
    }
    if !extends.is_empty() {
        registry.interface_extends.insert(name, extends);
    }
    collect_inline_type_defs_in_signatures(&iface.body.body, registry);
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

/// A TS `enum` whose members all lower to literals registers under its name,
/// so a `Color.Red` access reads as `Color::Red` (a path constant). An enum
/// with a non-literal initializer or a computed member name is skipped — the
/// access then falls through to a struct field and surfaces at `cargo check`.
fn register_enum(decl: &TSEnumDeclaration, registry: &mut TypeRegistry) {
    if declarations::eval_enum_members(&decl.body.members).is_some() {
        registry.enums.insert(decl.id.name.to_string());
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
        .insert(name.clone(), ref_param_flags(func, names));
    registry
        .function_returns
        .insert(name, function_return_type(func));
    // An inline scalar union in a parameter type (e.g. `Record<string,
    // string|number>`) needs its `__DsUnion…` enum emitted; recurse into every
    // annotation so a union nested in a `Record` value type is found.
    for fp in &func.params.items {
        if let Some(ta) = fp.type_annotation.as_ref() {
            collect_inline_type_defs(&ta.type_annotation, registry);
        }
    }
    if let Some(ta) = func.return_type.as_deref() {
        collect_inline_type_defs(&ta.type_annotation, registry);
    }
}

/// A top-level `const`/`let`/`var` with a type annotation. An inline scalar
/// union in the annotation (`const r: Record<K, string|number> = …`) needs its
/// `__DsUnion…` enum registered (and emitted) so the initializer's object
/// literal can box each value into the matching variant.
fn register_variable_declaration(decl: &VariableDeclaration, registry: &mut TypeRegistry) {
    for d in &decl.declarations {
        if let Some(ta) = d.type_annotation.as_ref() {
            collect_inline_type_defs(&ta.type_annotation, registry);
        }
    }
}

fn register_class(class: &Class, names: &NameTable, registry: &mut TypeRegistry) {
    collect_mut_methods(class, names, &mut registry.mut_methods);
}

/// Recursively collect every inline type definition nested in a type
/// annotation — both the all-scalar-keyword unions (`__DsUnion…`) and the
/// anonymous object-literal types (`__DsAnon_…`) — registering one definition
/// per unique shape. Descends into array/tuple elements, `Record`/`Array`
/// type arguments, `readonly`/paren operands, union members, and object-
/// literal property types, so an inline type anywhere in a parameter/field/
/// return type is found. The names come from [`declarations::scalar_union_enum`]
/// and [`declarations::anon_struct_for_literal`], the single sources of truth
/// that `types::translate_type` also reads.
fn collect_inline_type_defs(ty: &TSType, registry: &mut TypeRegistry) {
    match ty {
        TSType::TSUnionType(u) => {
            if let Some((name, variants)) = declarations::scalar_union_enum(u) {
                registry.union_enums.entry(name.clone()).or_insert_with(|| {
                    parse_quote! {
                        #[derive(Clone, Debug, PartialEq)]
                        enum #name { #(#variants),* }
                    }
                });
            } else if let Some((name, item, anons)) = declarations::inline_mixed_union_enum(u) {
                // A mixed union (`boolean | string[]`) — emit the enum plus any
                // helper anon structs an inline-object member needs.
                registry.union_enums.entry(name).or_insert(item);
                for a in anons {
                    registry.anon_structs.entry(a.ident.clone()).or_insert(a);
                }
            }
            for t in &u.types {
                collect_inline_type_defs(t, registry);
            }
        }
        TSType::TSTypeLiteral(lit) => {
            if let Some((name, item)) = declarations::anon_struct_for_literal(lit) {
                registry.anon_structs.entry(name).or_insert(item);
            }
            // Recurse into each property's type so a nested inline object is
            // found even when this literal itself has an index signature (and
            // so does not become a struct).
            for sig in &lit.members {
                if let TSSignature::TSPropertySignature(ps) = sig {
                    if let Some(ta) = ps.type_annotation.as_ref() {
                        collect_inline_type_defs(&ta.type_annotation, registry);
                    }
                }
            }
        }
        TSType::TSArrayType(arr) => collect_inline_type_defs(&arr.element_type, registry),
        TSType::TSTupleType(t) => {
            for e in &t.element_types {
                if let Some(inner) = e.as_ts_type() {
                    collect_inline_type_defs(inner, registry);
                }
            }
        }
        TSType::TSTypeReference(r) => {
            if let Some(args) = r.type_arguments.as_ref() {
                for p in &args.params {
                    collect_inline_type_defs(p, registry);
                }
            }
        }
        TSType::TSTypeOperatorType(op) => collect_inline_type_defs(&op.type_annotation, registry),
        TSType::TSParenthesizedType(p) => collect_inline_type_defs(&p.type_annotation, registry),
        _ => {}
    }
}

/// Collect inline type definitions from each property/index type of an
/// interface or object-literal type. A pure index-signature interface
/// (`[key: string]: A | B`) lowers to a `HashMap` alias whose value type may
/// itself carry an inline union/anon-struct — that definition must be
/// collected too, or the alias references a `crate::__Ds…` the crate root
/// never defines (E0425).
fn collect_inline_type_defs_in_signatures(members: &[TSSignature], registry: &mut TypeRegistry) {
    for sig in members {
        match sig {
            TSSignature::TSPropertySignature(ps) => {
                if let Some(ta) = ps.type_annotation.as_ref() {
                    collect_inline_type_defs(&ta.type_annotation, registry);
                }
            }
            TSSignature::TSIndexSignature(idx) => {
                collect_inline_type_defs(&idx.type_annotation.type_annotation, registry);
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

/// The function's declared return type path — `None` where it is unannotated.
/// Recorded so a `ReturnType<typeof fn>` query resolves to this path.
fn function_return_type(func: &Function) -> Option<Path> {
    func.return_type
        .as_deref()
        .and_then(|ta| path_of_type(&ta.type_annotation))
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
