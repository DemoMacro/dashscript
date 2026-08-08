//! Type declarations (`interface` / `type`) → `syn` items.

mod enum_decl;
mod interface;
mod type_alias;
mod union_enum;

pub use enum_decl::translate_enum;
pub use interface::translate_interface;
pub use type_alias::translate_type_alias;
pub use union_enum::{inline_mixed_union_enum, scalar_union_enum, union_display_impl};

pub(in crate::translator) use enum_decl::eval_enum_members;
pub(in crate::translator) use interface::index_signature_type;
pub(in crate::translator) use union_enum::{
    anon_struct_for_literal, discriminant_property_value, discriminant_value, object_member_tag,
    set_shape_aliases,
};

use std::cell::RefCell;
use std::collections::HashSet;

use syn::{parse_quote, Ident, Item, Meta, Type};

thread_local! {
    /// Type names that cannot derive serde, accumulated across the files of one
    /// translation. A `__DsUnion` enum is often emitted in a barrel while the
    /// callback type it names is defined in another file, so the skip must
    /// propagate across files. Reset at the start of each translation
    /// ([`reset_non_serde_names`]) so one project/fixture cannot leak into the
    /// next.
    static NON_SERDE_NAMES: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
}

/// Clear the cross-file non-serde name set. Call at the start of each
/// translation (a project's `translate_sources`, or a one-shot
/// `translate_with_deps`) so a name from one cannot leak into another.
pub(crate) fn reset_non_serde_names() {
    NON_SERDE_NAMES.with(|c| c.borrow_mut().clear());
}

/// Append `serde::Serialize`/`serde::Deserialize` to every emitted struct/enum's
/// `#[derive(...)]`. Per-function engine degradation marshals argument/return
/// values through `serde_json::Value`, so every user type crossing the
/// `call_fn` boundary needs both. Idempotent (skips a derive list already
/// carrying a serde derive) and ignores items with no `derive(...)` attribute.
pub(crate) fn add_serde_derives(items: &mut [Item]) {
    // Strip any prior serde derives first so this recomputes idempotently — a
    // global fixpoint pass re-runs this on already-emitted items.
    for item in items.iter_mut() {
        strip_serde_derive(item);
    }
    // Names that cannot derive serde, to a fixpoint: a struct/enum owning a
    // *required* function-pointer field (a bare `fn(...)` — no `Default`, so
    // `#[serde(skip)]` cannot substitute it), plus every struct/enum that names
    // such a type in a field/variant, transitively. Containment propagates the
    // unsatisfiable bound (a `__DsUnion` enum over a callback type cannot
    // derive serde either). Such items keep `Clone`/`Debug`/`PartialEq` (`fn`
    // is `Copy`) but skip serde. A `?`-optional callback lowers to
    // `Option<fn(...)>` (`Default` = `None`); those fields still
    // `#[serde(skip)]` and the item derives serde normally.
    NON_SERDE_NAMES.with(|cell| {
        for item in items.iter() {
            if let Some(name) = item_name_if_bare_fn(item) {
                cell.borrow_mut().insert(name);
            }
        }
    });
    let mut non_serde: HashSet<String> = NON_SERDE_NAMES.with(|c| c.borrow().clone());
    loop {
        let added: Vec<String> = items
            .iter()
            .filter_map(|item| {
                let name = item_ident(item)?;
                if non_serde.contains(name.to_string().as_str()) {
                    return None;
                }
                item_refs_non_serde(item, &non_serde).then(|| name.to_string())
            })
            .collect();
        if added.is_empty() {
            break;
        }
        non_serde.extend(added.clone());
        NON_SERDE_NAMES.with(|c| c.borrow_mut().extend(added));
    }
    for item in items.iter_mut() {
        let skip = item_ident(item).is_some_and(|n| non_serde.contains(n.to_string().as_str()));
        if skip {
            continue;
        }
        match item {
            Item::Struct(s) => {
                for f in s.fields.iter_mut() {
                    if type_contains_fn(&f.ty)
                        && !f.attrs.iter().any(|a| a.path().is_ident("serde"))
                    {
                        f.attrs.push(parse_quote!(#[serde(skip)]));
                    }
                }
            }
            Item::Enum(e) => {
                for v in e.variants.iter_mut() {
                    for f in v.fields.iter_mut() {
                        if type_contains_fn(&f.ty)
                            && !f.attrs.iter().any(|a| a.path().is_ident("serde"))
                        {
                            f.attrs.push(parse_quote!(#[serde(skip)]));
                        }
                    }
                }
            }
            _ => continue,
        }
        let attrs = match item {
            Item::Struct(s) => &mut s.attrs,
            Item::Enum(e) => &mut e.attrs,
            _ => continue,
        };
        for attr in attrs.iter_mut() {
            // The existing `#[derive(Clone, Debug, PartialEq)]` token stream.
            let prev = match &attr.meta {
                Meta::List(ml) if attr.path().is_ident("derive") => ml.tokens.clone(),
                _ => continue,
            };
            if prev.to_string().contains("Serialize") {
                continue;
            }
            *attr = parse_quote!(#[derive(#prev, serde::Serialize, serde::Deserialize)]);
        }
    }
}

/// The name of a struct/enum that directly owns a bare (required)
/// function-pointer field, else `None`. Seeds the `non_serde` set in
/// [`add_serde_derives`]: a bare `fn` has no `Default`, so the whole item
/// skips serde.
fn item_name_if_bare_fn(item: &Item) -> Option<String> {
    let (name, has_bare) = match item {
        Item::Struct(s) => (&s.ident, s.fields.iter().any(|f| type_is_bare_fn(&f.ty))),
        Item::Enum(e) => (
            &e.ident,
            e.variants
                .iter()
                .any(|v| v.fields.iter().any(|f| type_is_bare_fn(&f.ty))),
        ),
        _ => return None,
    };
    has_bare.then(|| name.to_string())
}

/// The identifier of a struct/enum item, else `None`.
fn item_ident(item: &Item) -> Option<&Ident> {
    match item {
        Item::Struct(s) => Some(&s.ident),
        Item::Enum(e) => Some(&e.ident),
        Item::Type(t) => Some(&t.ident),
        _ => None,
    }
}

/// True where any field/variant type names a type in `non_serde` (transitive
/// containment) — propagates the skip across the fixpoint.
fn item_refs_non_serde(item: &Item, non_serde: &HashSet<String>) -> bool {
    match item {
        Item::Struct(s) => s.fields.iter().any(|f| type_refs_name(&f.ty, non_serde)),
        Item::Enum(e) => e
            .variants
            .iter()
            .any(|v| v.fields.iter().any(|f| type_refs_name(&f.ty, non_serde))),
        // A type alias naming a non-serde type (`type X = HashMap<…, Union>`)
        // propagates the skip: a struct/enum whose field is the alias must
        // skip serde too. The alias itself carries no derive, so the apply
        // pass leaves it untouched — only the fixpoint set grows.
        Item::Type(t) => type_refs_name(&t.ty, non_serde),
        _ => false,
    }
}

/// True where a type mentions any name in `names` — recursing through the
/// containers a TS field lowers into (`Vec<…>`, `Option<…>`, `HashMap<…>`,
/// tuples, slices, arrays, references).
fn type_refs_name(ty: &Type, names: &HashSet<String>) -> bool {
    match ty {
        Type::Path(p) => p.path.segments.iter().any(|seg| {
            names.contains(seg.ident.to_string().as_str())
                || matches!(&seg.arguments,
                    syn::PathArguments::AngleBracketed(ab)
                        if ab.args.iter().any(|arg| matches!(arg,
                            syn::GenericArgument::Type(t)
                                if type_refs_name(t, names))))
        }),
        Type::Paren(pa) => type_refs_name(&pa.elem, names),
        Type::Reference(r) => type_refs_name(&r.elem, names),
        Type::Slice(s) => type_refs_name(&s.elem, names),
        Type::Array(a) => type_refs_name(&a.elem, names),
        Type::Tuple(t) => t.elems.iter().any(|t| type_refs_name(t, names)),
        _ => false,
    }
}

/// True where a type is a bare function pointer (`fn(...) -> ...`) at its
/// surface — without recursing through `Option`/`Vec`/`&` containers (unlike
/// [`type_contains_fn`], which does). `Option<fn(...)>` is therefore not a
/// bare `fn`: its `Default` (`None`) lets the field skip serde.
fn type_is_bare_fn(ty: &Type) -> bool {
    match ty {
        Type::BareFn(_) => true,
        Type::Paren(p) => type_is_bare_fn(&p.elem),
        _ => false,
    }
}

/// True where a type mention is not serde-serializable — a function pointer
/// (`fn(...) -> ...`, a TS callback) or a `DsPromise<T>` (a `Pin<Box<dyn
/// Future>>` — a runtime value, not data). Recurses through the wrappers a TS
/// field lowers into — `Option<…>`, `Vec<…>`, `&…`, tuples, slices, arrays.
/// Used to `#[serde(skip)]` the field so a struct/enum carrying it still
/// derives `Serialize`/`Deserialize`.
fn type_contains_fn(ty: &Type) -> bool {
    match ty {
        Type::BareFn(_) => true,
        Type::Paren(p) => type_contains_fn(&p.elem),
        Type::Reference(r) => type_contains_fn(&r.elem),
        Type::Slice(s) => type_contains_fn(&s.elem),
        Type::Array(a) => type_contains_fn(&a.elem),
        Type::Tuple(t) => t.elems.iter().any(type_contains_fn),
        Type::Path(p) => p.path.segments.iter().any(|seg| {
            // `DsPromise<T>` (`Pin<Box<dyn Future>>`) is a runtime value, not
            // serde data — a struct field holding a promise skips serde the way
            // a callback field does.
            if seg.ident == "DsPromise" {
                return true;
            }
            match &seg.arguments {
                syn::PathArguments::AngleBracketed(ab) => ab.args.iter().any(|arg| match arg {
                    syn::GenericArgument::Type(t) => type_contains_fn(t),
                    _ => false,
                }),
                _ => false,
            }
        }),
        _ => false,
    }
}

/// Remove `serde::Serialize`/`serde::Deserialize` from a struct/enum's
/// `#[derive(...)]` list, leaving `Clone`/`Debug`/`PartialEq` intact. Used at
/// the top of [`add_serde_derives`] so the verdict recomputes idempotently on
/// already-emitted items — a global fixpoint pass re-parses emitted files and
/// re-runs the fixpoint, so a prior per-file pass's (possibly wrong) serde
/// derives must clear first.
fn strip_serde_derive(item: &mut Item) {
    let attrs = match item {
        Item::Struct(s) => &mut s.attrs,
        Item::Enum(e) => &mut e.attrs,
        _ => return,
    };
    for attr in attrs.iter_mut() {
        if !attr.path().is_ident("derive") {
            continue;
        }
        let Meta::List(ml) = &mut attr.meta else {
            continue;
        };
        let kept: String = ml
            .tokens
            .to_string()
            .split(',')
            .map(str::trim)
            .filter(|p| !p.is_empty() && !p.contains("Serialize") && !p.contains("Deserialize"))
            .collect::<Vec<_>>()
            .join(", ");
        if let Ok(ts) = kept.parse() {
            ml.tokens = ts;
        }
    }
}
