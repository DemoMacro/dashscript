use oxc_ast::ast::{Expression, NumericLiteral, TSEnumDeclaration, TSEnumMember, TSEnumMemberName};
use syn::{parse_quote, Item};

use super::super::bindings;

/// A TypeScript `enum` lowers to a Rust `mod` of typed `const` members — the
/// way an ES `enum` is a runtime object of named values. A numeric enum
/// (`enum E { A, B }` → A=0, B=1) emits `i64` consts with auto-incrementing
/// values; a string enum (`enum E { A = "a" }`) emits `&'static str` consts.
/// `Color.Red` then reads as `Color::Red` (a path constant), matching ES
/// `Color.Red === 0` — the value is a plain `i64`/`&str`, so `Color.Red + 1`
/// stays a numeric expression the way it does in TS.
///
/// Returns `None` when a member's initializer is not a literal (`A = 1 << 2`,
/// `A = Other.B`) or its name is computed — DashScript does not constant-
/// evaluate, where oxc pre-computes these in `Scoping`. The caller emits
/// nothing and `check` flags the enum unsupported.
pub fn translate_enum(decl: &TSEnumDeclaration) -> Option<Vec<Item>> {
    let name = bindings::type_ident(&decl.id.name);
    let members = eval_enum_members(&decl.body.members)?;
    let consts: Vec<Item> = members
        .iter()
        .map(|(member_name, value)| enum_const_item(member_name, value))
        .collect();
    Some(vec![Item::Mod(parse_quote! {
        #[allow(non_upper_case_globals, non_snake_case)]
        pub mod #name {
            #(#consts)*
        }
    })])
}

/// One `pub const Member: T = value;` for an enum member. The member name keeps
/// its TS spelling (commonly PascalCase, e.g. `Red`); `non_upper_case_globals`
/// is allowed on the enclosing `mod` so a non-SCREAMING const does not warn.
fn enum_const_item(name: &str, value: &EnumValue) -> Item {
    let ident = bindings::type_ident(name);
    match value {
        EnumValue::Number(n) => parse_quote!(pub const #ident: i64 = #n;),
        EnumValue::String(s) => {
            let lit = proc_macro2::Literal::string(s);
            parse_quote!(pub const #ident: &'static str = #lit;)
        }
    }
}

/// A TS enum member's evaluated value: a numeric member is an `i64`; a string
/// member is a `&'static str`. Shared by `translate_enum` (emit) and the
/// registry pre-pass (kind classification) so the two agree on what each
/// member lowers to.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::translator) enum EnumValue {
    Number(i64),
    String(String),
}

/// Evaluate every member's value, following ES enum semantics. A member with
/// no initializer takes the previous numeric value + 1 (or 0 for the first);
/// a numeric-literal initializer resets that counter; a string-literal
/// initializer yields a string value and breaks the numeric chain. Returns
/// `None` if any member's initializer is not a literal (constant evaluation is
/// out of scope) or its name is computed — the enum then stays unsupported.
pub(in crate::translator) fn eval_enum_members(
    members: &[TSEnumMember],
) -> Option<Vec<(String, EnumValue)>> {
    let mut out = Vec::new();
    let mut prev_num: Option<i64> = None;
    for member in members {
        let name = enum_member_name(&member.id)?;
        let value = match &member.initializer {
            None => {
                let n = prev_num.map_or(0, |p| p + 1);
                prev_num = Some(n);
                EnumValue::Number(n)
            }
            Some(Expression::StringLiteral(s)) => {
                prev_num = None;
                EnumValue::String(s.value.to_string())
            }
            Some(Expression::NumericLiteral(n)) => {
                let v = numeric_literal_to_i64(n)?;
                prev_num = Some(v);
                EnumValue::Number(v)
            }
            Some(_) => return None,
        };
        out.push((name, value));
    }
    Some(out)
}

/// A member's name — an identifier or a string literal. A computed name
/// (`['x']`, `` `tpl` ``) returns `None` (it is not a compile-time constant).
fn enum_member_name(name: &TSEnumMemberName) -> Option<String> {
    match name {
        TSEnumMemberName::Identifier(id) => Some(id.name.to_string()),
        TSEnumMemberName::String(s) => Some(s.value.to_string()),
        TSEnumMemberName::ComputedString(_) | TSEnumMemberName::ComputedTemplateString(_) => None,
    }
}

/// An integer enum member's value. A fractional or non-finite numeric literal
/// returns `None` — ES enums are integral in practice, and `i64` is the
/// faithful Rust repr (a `1.5` enum member is exotic enough to flag honestly).
fn numeric_literal_to_i64(n: &NumericLiteral) -> Option<i64> {
    let v = n.value;
    if v.is_finite() && v.fract() == 0.0 {
        Some(v as i64)
    } else {
        None
    }
}
