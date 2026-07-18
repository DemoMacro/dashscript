//! `switch` translation → `syn::match`: discriminated-union destructuring and
//! bare-enum string-literal cases.

use oxc_ast::ast::{Expression, Statement, SwitchCase, SwitchStatement};
use quote::format_ident;
use syn::{parse_quote, Arm, Block, Ident, Pat, Path, Stmt};

use super::super::context::{Ctx, Locals, Narrow};
use super::super::name_table::NameTable;
use super::super::registry::TypeRegistry;
use super::super::{bindings, expressions};
use super::{drop_trailing_return, translate_stmt};

/// `switch (s) { case "x": …; default: … }` → `match s { … }`.
///
/// Two shapes: `switch (x.kind) { … }` on a discriminated-union local
/// destructures variants (`Shape::Circle { radius } => …`, with `s.radius` in
/// the arm body narrowed to `radius`); `switch (e) { … }` on a bare enum
/// identifier maps each string-literal case to a unit/tuple variant pattern.
pub(super) fn translate_switch(
    sw: &SwitchStatement,
    locals: &mut Locals,
    registry: &TypeRegistry,
    narrow: &Narrow,
    return_path: Option<&Path>,
    names: &NameTable<'_>,
) -> Stmt {
    if let Some((scrut, type_name)) = discriminant_member(&sw.discriminant, locals, registry) {
        return discriminated_match(sw, &scrut, &type_name, locals, registry, return_path, names);
    }
    let enum_path = discriminant_path(&sw.discriminant, locals);
    let is_string = enum_path.as_ref().is_some_and(|p| p.is_ident("String"));
    let disc = expressions::translate_expr(
        &sw.discriminant,
        &Ctx::new(&*locals, registry, narrow, names),
    );
    // A switch on a plain `string` matches against `&str` literal cases, so the
    // discriminant is borrowed as a slice.
    let disc = if is_string {
        parse_quote!(#disc.as_str())
    } else {
        disc
    };
    let mut arms: Vec<Arm> = sw
        .cases
        .iter()
        .filter_map(|c| {
            switch_arm(
                c,
                enum_path.as_ref(),
                locals,
                registry,
                narrow,
                return_path,
                names,
            )
        })
        .collect();
    // A `&str` match is never exhaustive (any other string is possible), so a
    // switch with no `default` — the common JS form — needs a catch-all to
    // compile. JS simply skips unmatched cases; the arm is a no-op.
    if is_string && !arms.iter().any(|a| matches!(&a.pat, syn::Pat::Wild(_))) {
        arms.push(parse_quote!(_ => {}));
    }
    parse_quote!(match #disc { #(#arms)* })
}

/// When `disc` is `x.kind` and `x` is a local of a registered discriminated
/// union, return `(x's snake name, the enum's type name)`.
fn discriminant_member(
    disc: &Expression,
    locals: &Locals,
    registry: &TypeRegistry,
) -> Option<(String, String)> {
    let Expression::StaticMemberExpression(sm) = disc else {
        return None;
    };
    if sm.property.name.as_str() != "kind" {
        return None;
    }
    let Expression::Identifier(obj) = &sm.object else {
        return None;
    };
    let scrut = bindings::snake(&obj.name).to_string();
    let type_name = locals.get(&scrut)?.segments.last()?.ident.to_string();
    registry
        .unions
        .contains_key(&type_name)
        .then_some((scrut, type_name))
}

/// `switch (s.kind) { case "circle": … }` → `match s { Shape::Circle { radius } => … }`.
/// Each arm body is translated under a [`Narrow`] that rewrites `s.field` to the
/// `field` binding.
fn discriminated_match(
    sw: &SwitchStatement,
    scrut: &str,
    type_name: &str,
    locals: &mut Locals,
    registry: &TypeRegistry,
    return_path: Option<&Path>,
    names: &NameTable<'_>,
) -> Stmt {
    let scrut_ident = format_ident!("{}", scrut);
    let arms: Vec<Arm> = sw
        .cases
        .iter()
        .filter_map(|c| {
            discriminated_arm(c, scrut, type_name, locals, registry, return_path, names)
        })
        .collect();
    parse_quote!(match #scrut_ident { #(#arms)* })
}

/// One arm of a discriminated-union match: `case "circle"` →
/// `Shape::Circle { radius } => <body with s.radius narrowed to radius>`.
/// A `default` arm becomes `_ => <body>` with no narrowing.
fn discriminated_arm(
    c: &SwitchCase,
    scrut: &str,
    type_name: &str,
    locals: &mut Locals,
    registry: &TypeRegistry,
    return_path: Option<&Path>,
    names: &NameTable<'_>,
) -> Option<Arm> {
    let (pat, narrow) = match &c.test {
        Some(Expression::StringLiteral(s)) => {
            let value = s.value.to_string();
            let shape = registry.unions.get(type_name)?.get(&value)?.clone();
            let type_ident = format_ident!("{}", type_name);
            let variant = shape.name;
            let field_idents: Vec<Ident> = shape.fields.clone();
            let narrow = Narrow::of(
                scrut.to_string(),
                field_idents.iter().map(|f| f.to_string()).collect(),
            );
            let pat: Pat = parse_quote!(#type_ident::#variant { #(#field_idents),* });
            (pat, narrow)
        }
        _ => (parse_quote!(_), Narrow::default()),
    };
    let body = case_body(&c.consequent, locals, registry, &narrow, return_path, names);
    Some(parse_quote!(#pat => #body,))
}

fn discriminant_path(disc: &Expression, locals: &Locals) -> Option<syn::Path> {
    let Expression::Identifier(id) = disc else {
        return None;
    };
    let name: &str = &id.name;
    locals.get(&bindings::snake(name).to_string()).cloned()
}

fn switch_arm(
    c: &SwitchCase,
    enum_path: Option<&syn::Path>,
    locals: &mut Locals,
    registry: &TypeRegistry,
    narrow: &Narrow,
    return_path: Option<&Path>,
    names: &NameTable<'_>,
) -> Option<Arm> {
    let pat = match &c.test {
        Some(test) => switch_pattern(test, enum_path),
        None => parse_quote!(_),
    };
    let body = case_body(&c.consequent, locals, registry, narrow, return_path, names);
    Some(parse_quote!(#pat => #body,))
}

/// A string-literal case becomes a pattern: on a plain `string` discriminant
/// it is a `&str` literal pattern (`"idle" => …`); on an enum it is the
/// matching variant (`Status::Idle`). Anything else (non-string, no enum path)
/// falls back to `_` — number switches on `f64` aren't valid Rust patterns, so
/// prefer `if` there.
fn switch_pattern(test: &Expression, enum_path: Option<&syn::Path>) -> Pat {
    let Expression::StringLiteral(s) = test else {
        return parse_quote!(_);
    };
    let value: &str = &s.value;
    // A switch on a plain `string`: each case is a `&str` literal matched
    // against `disc.as_str()` (set up in `translate_switch`).
    if enum_path.is_some_and(|p| p.is_ident("String")) {
        let lit = syn::LitStr::new(value, proc_macro2::Span::call_site());
        return parse_quote!(#lit);
    }
    let Some(path) = enum_path else {
        return parse_quote!(_);
    };
    let variant = bindings::pascal(value);
    parse_quote!(#path::#variant)
}

fn case_body(
    stmts: &[Statement],
    locals: &mut Locals,
    registry: &TypeRegistry,
    narrow: &Narrow,
    return_path: Option<&Path>,
    names: &NameTable<'_>,
) -> Block {
    let mut rust: Vec<Stmt> = stmts
        .iter()
        .filter(|s| !matches!(s, Statement::BreakStatement(_)))
        .flat_map(|s| translate_stmt(s, locals, registry, narrow, return_path, names))
        .collect();
    drop_trailing_return(&mut rust);
    parse_quote!({ #(#rust)* })
}
