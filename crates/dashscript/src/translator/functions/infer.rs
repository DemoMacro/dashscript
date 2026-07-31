//! Type inference for unannotated `let`/`const` bindings: the type a literal
//! or call initializer lowers to when no annotation is present, so type-
//! sensitive mappings (truthiness, `??`, optional/union boxing, the array
//! builtins) work without a `: T`. Extracted from `functions/mod.rs`.

use oxc_ast::ast::{Argument, Expression, ObjectExpression, ObjectPropertyKind};
use oxc_syntax::operator::UnaryOperator;
use syn::{parse_quote, Type};

use super::super::bindings;
use super::super::context::Ctx;

/// The type inferred from a literal initializer, when a binding has no type
/// annotation: `true` → `bool`, `1`/`0.5` → `f64`, `"x"` → `String`, a
/// An object literal's type when all its property values share one scalar
/// kind (`HashMap<String, f64>` / `…, String>` / `…, bool>`); `None` for a
/// mixed, empty, or spread object. Mirrors the homogeneous-array rule so an
/// unannotated `var x = { … }` lowers to a `HashMap` and routes field access
/// through `is_hashmap_local` (matching JS object semantics).
fn homogeneous_object_type(obj: &ObjectExpression) -> Option<Type> {
    let values: Vec<&Expression> = obj
        .properties
        .iter()
        .filter_map(|p| match p {
            ObjectPropertyKind::ObjectProperty(op) => Some(&op.value),
            ObjectPropertyKind::SpreadProperty(_) => None,
        })
        .collect();
    if values.is_empty() {
        // An empty object literal defaults to `HashMap<String, f64>` so an
        // unannotated `var x = {}` has a concrete type — otherwise the empty
        // `HashMap<_, _>` leaves key/value types undetermined and a downstream
        // `Object.keys(x)` / `Object.hasOwn(x, …)` fails (E0282/E0283).
        return Some(parse_quote!(::std::collections::HashMap<String, f64>));
    }
    if values
        .iter()
        .all(|e| matches!(e, Expression::NumericLiteral(_)))
    {
        Some(parse_quote!(::std::collections::HashMap<String, f64>))
    } else if values
        .iter()
        .all(|e| matches!(e, Expression::StringLiteral(_)))
    {
        Some(parse_quote!(::std::collections::HashMap<String, String>))
    } else if values
        .iter()
        .all(|e| matches!(e, Expression::BooleanLiteral(_)))
    {
        Some(parse_quote!(::std::collections::HashMap<String, bool>))
    } else {
        None
    }
}

/// `Object.assign(target, …)` returns a value of `target`'s type, so an
/// unannotated `let r = Object.assign(t, …)` records `t`'s recorded type —
/// letting `r.foo` route through `is_hashmap_local` (HashMap field access)
/// instead of failing as a struct field. Only a plain `Object.assign` call
/// whose first argument is a typed local is recognized; anything else falls
/// through to other inference.
pub(super) fn object_assign_type(init: &Expression, ctx: &Ctx<'_>) -> Option<Type> {
    let Expression::CallExpression(c) = init else {
        return None;
    };
    let Expression::StaticMemberExpression(sm) = &c.callee else {
        return None;
    };
    let is_object_assign = matches!(&sm.object, Expression::Identifier(id) if id.name.as_str() == "Object")
        && sm.property.name.as_str() == "assign";
    if !is_object_assign {
        return None;
    }
    let first = c.arguments.first()?;
    let Argument::Identifier(tgt) = first else {
        return None;
    };
    let name = bindings::snake(&tgt.name).to_string();
    let path = ctx.local_type(&name)?;
    Some(Type::Path(syn::TypePath {
        qself: None,
        path: path.clone(),
    }))
}

/// `s.match(/pat/)` or `r.exec(s)` (non-global) returns an ES match result or
/// `null`, so an unannotated `let m = …` records `Option<DsMatch>` — letting
/// `m[0]`/`m.index`/`m.input`/`m.length` route through the `DsMatch` accessors
/// instead of failing on `Option`'s missing `Index`/`len`. Recognized: a
/// `.match` call with a regex-literal argument, or an `.exec` call on a regex
/// literal or a local inferred to be one (`let r = /pat/; r.exec(s)`).
pub(super) fn match_result_type(init: &Expression, ctx: &Ctx<'_>) -> Option<Type> {
    let Expression::CallExpression(c) = init else {
        return None;
    };
    let Expression::StaticMemberExpression(sm) = &c.callee else {
        return None;
    };
    let is_match = sm.property.name.as_str() == "match"
        && matches!(c.arguments.first(), Some(Argument::RegExpLiteral(_)));
    let is_exec = sm.property.name.as_str() == "exec" && is_regex_receiver(&sm.object, ctx);
    (is_match || is_exec).then(|| parse_quote!(Option<crate::__ds::DsMatch>))
}

/// `record[key]` (a `HashMap` index access) records the map's value type, so an
/// unannotated `const v = record[key]` carries the union-enum type — letting
/// `v !== undefined` route through `union_null_equality` (`!matches!(v, Undef)`)
/// instead of falling through to a wrong `!= None` (the enum has no `None`, so
/// that path is an E0308). Only a `HashMap<K, V>` whose object is a typed local
/// is recognized; the key expression is irrelevant (a `HashMap` lookup yields
/// `V` for any present key).
pub(super) fn index_access_type(init: &Expression, ctx: &Ctx<'_>) -> Option<Type> {
    let Expression::ComputedMemberExpression(cm) = init else {
        return None;
    };
    let Expression::Identifier(id) = &cm.object else {
        return None;
    };
    let path = ctx.local_type(&bindings::snake(&id.name).to_string())?;
    let seg = path.segments.last()?;
    if seg.ident != "HashMap" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(args) = &seg.arguments else {
        return None;
    };
    args.args
        .iter()
        .filter_map(|g| match g {
            syn::GenericArgument::Type(t) => Some(t.clone()),
            _ => None,
        })
        .nth(1)
}

/// Whether `obj` is a regex value `exec` can be called on — a `/pat/` literal
/// or a local inferred to be `regress::Regex` (so `let r = /pat/; r.exec(s)`
/// records `Option<DsMatch>` on `m`, matching the literal case).
fn is_regex_receiver(obj: &Expression, ctx: &Ctx<'_>) -> bool {
    if matches!(obj, Expression::RegExpLiteral(_)) {
        return true;
    }
    let Expression::Identifier(id) = obj else {
        return false;
    };
    let name = bindings::snake(&id.name).to_string();
    ctx.local_type(&name)
        .is_some_and(|ty| ty.segments.last().is_some_and(|s| s.ident == "Regex"))
}

/// homogeneous array → `Vec<f64>` / `Vec<String>`. Anchors the binding's type
/// (a bare float literal is otherwise an ambiguous `{float}` — E0689 on
/// `.acosh()` etc.) and lets type-sensitive mappings (truthiness, `??`, the
/// array builtins) work on unannotated locals.
pub(super) fn infer_literal_type(expr: &Expression) -> Option<Type> {
    match expr {
        Expression::BooleanLiteral(_) => Some(parse_quote!(bool)),
        Expression::NumericLiteral(_) => Some(parse_quote!(f64)),
        Expression::StringLiteral(_) => Some(parse_quote!(String)),
        // `/pat/` lowers to a `regress::Regex` (see
        // `expressions::regex_literal_expr`), so `let r = /pat/` infers the
        // type `.test` dispatches on.
        Expression::RegExpLiteral(_) => Some(parse_quote!(regress::Regex)),
        // A homogeneous array literal infers its element type so the builtin
        // array methods (`.map`/`.filter`/`.includes`/…) map correctly without
        // an annotation. A mixed, empty, or spread array is left uninferred
        // (Rust infers at the use site, or the user adds a `number[]` type).
        Expression::ArrayExpression(arr) => {
            let elems: Vec<&Expression> = arr
                .elements
                .iter()
                .filter_map(|e| e.as_expression())
                .collect();
            if elems.is_empty() {
                return None;
            }
            if elems
                .iter()
                .all(|e| matches!(e, Expression::NumericLiteral(_)))
            {
                Some(parse_quote!(Vec<f64>))
            } else if elems
                .iter()
                .all(|e| matches!(e, Expression::StringLiteral(_)))
            {
                Some(parse_quote!(Vec<String>))
            } else {
                None
            }
        }
        // An anonymous object literal infers `HashMap<String, V>` when its
        // values share one scalar kind (see `homogeneous_object_type`).
        Expression::ObjectExpression(obj) => homogeneous_object_type(obj),
        // oxc parses a signed literal (`-1000`, `+0`) as
        // `UnaryExpression(-/+, …)` rather than a `NumericLiteral`, so a binding
        // `var i = -1000` / `var x = +0` would otherwise lose its f64 anchor
        // (→ E0689 on `i < …` or `x.cos()`). `unary_expr` strips `+` and keeps
        // `-`, so the inner literal's scalar type is the binding's type.
        Expression::UnaryExpression(un)
            if matches!(
                un.operator,
                UnaryOperator::UnaryNegation | UnaryOperator::UnaryPlus
            ) =>
        {
            match &un.argument {
                Expression::NumericLiteral(_) => Some(parse_quote!(f64)),
                Expression::BooleanLiteral(_) => Some(parse_quote!(bool)),
                Expression::StringLiteral(_) => Some(parse_quote!(String)),
                _ => None,
            }
        }
        // `RegExp("pat")` / `new RegExp("pat")` — the constructor returns a
        // compiled regex, so `let r = RegExp("pat")` infers `regress::Regex`
        // (the type `.test`/`.exec` dispatch on), matching the `/pat/` literal.
        Expression::CallExpression(c) if matches!(&c.callee, Expression::Identifier(id) if id.name.as_str() == "RegExp") => {
            Some(parse_quote!(regress::Regex))
        }
        Expression::NewExpression(n) if matches!(&n.callee, Expression::Identifier(id) if id.name.as_str() == "RegExp") => {
            Some(parse_quote!(regress::Regex))
        }
        // `Temporal.<Type>.from(s)` → `temporal_rs::<Type>` (the type
        // `.toString`/`.year`/`.hour`/… dispatch on), for the five date/time
        // types sharing the `from_utf8` constructor + accessor shape.
        Expression::CallExpression(_) => temporal_from_type(expr),
        _ => None,
    }
}

/// True when `expr` is `Temporal.<ty>.<method>(…)` — a nested static-member
/// call on the `Temporal` namespace (`Temporal.PlainDate.from`, …).
fn is_temporal_static_call(expr: &Expression, ty: &str, method: &str) -> bool {
    let Expression::CallExpression(c) = expr else {
        return false;
    };
    let Expression::StaticMemberExpression(sm) = &c.callee else {
        return false;
    };
    sm.property.name.as_str() == method
        && matches!(
            &sm.object,
            Expression::StaticMemberExpression(tm)
            if tm.property.name.as_str() == ty
                && matches!(&tm.object, Expression::Identifier(id) if id.name.as_str() == "Temporal")
        )
}

/// `Temporal.<Type>.from(s)` infers `temporal_rs::<Type>` (the type the
/// accessors + `Display` dispatch on), for the five types with an infallible
/// `from_utf8` constructor. Reads the shared `TEMPORAL_DATE_TIME_TYPES` list so
/// it stays in sync with `temporal.rs::temporal_static` +
/// `member.rs::is_temporal_local` — one list, three readers.
fn temporal_from_type(expr: &Expression) -> Option<Type> {
    let ty = super::super::builtins::TEMPORAL_DATE_TIME_TYPES
        .iter()
        .copied()
        .find(|t| is_temporal_static_call(expr, t, "from"))?;
    let ident = syn::Ident::new(ty, proc_macro2::Span::call_site());
    Some(parse_quote!(temporal_rs::#ident))
}
