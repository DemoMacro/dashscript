//! `Expression` → `syn::Expr`.

use oxc_ast::ast::{
    Argument, ArrayExpression, ArrayExpressionElement, CallExpression, Expression,
    IdentifierReference, ObjectExpression, ObjectPropertyKind, StaticMemberExpression, StringLiteral,
};
use proc_macro2::Span;
use quote::format_ident;
use syn::{parse_quote, parse_str, Expr, Type};

use super::{bindings, types};

/// Translate an expression to its `syn::Expr` form.
///
/// Unmapped expressions fall back to `todo!()` so the generated Rust compiles
/// but fails loudly at run time if reached.
pub fn translate_expr(expr: &Expression) -> Expr {
    match expr {
        Expression::StringLiteral(s) => string_expr(s),
        Expression::NumericLiteral(n) => numeric_expr(n.value),
        Expression::BooleanLiteral(b) => bool_expr(b.value),
        Expression::Identifier(id) => ident_expr(id),
        Expression::CallExpression(call) => translate_call(call),
        Expression::ArrayExpression(arr) => array_expr(arr),
        Expression::StaticMemberExpression(sm) => member_expr(sm),
        _ => parse_quote!(::core::todo!()),
    }
}

/// Translate a call argument — [`Argument`] inherits the `Expression` variants.
pub fn translate_argument(arg: &Argument) -> Expr {
    match arg {
        Argument::StringLiteral(s) => string_expr(s),
        Argument::NumericLiteral(n) => numeric_expr(n.value),
        Argument::BooleanLiteral(b) => bool_expr(b.value),
        Argument::Identifier(id) => ident_expr(id),
        Argument::CallExpression(call) => translate_call(call),
        Argument::ArrayExpression(arr) => array_expr(arr),
        Argument::StaticMemberExpression(sm) => member_expr(sm),
        _ => parse_quote!(::core::todo!()),
    }
}

/// Translate an initializer; an object literal borrows its struct name from
/// the variable's type annotation (anonymous literals are unsupported yet).
pub fn translate_init(expr: &Expression, ty_hint: Option<&Type>) -> Expr {
    if let Expression::ObjectExpression(obj) = expr {
        return object_expr(obj, ty_hint);
    }
    translate_expr(expr)
}

/// `Point { x: 1 }` — needs the target type's name from the binding annotation.
fn object_expr(obj: &ObjectExpression, ty_hint: Option<&Type>) -> Expr {
    let Some(path) = ty_hint.and_then(types::type_path) else {
        return parse_quote!(::core::todo!());
    };
    let fields: Vec<syn::FieldValue> = obj
        .properties
        .iter()
        .filter_map(|p| {
            let ObjectPropertyKind::ObjectProperty(op) = p else { return None };
            let key = bindings::property_key_name(&op.key)?;
            let value = translate_expr(&op.value);
            Some(parse_quote!(#key: #value))
        })
        .collect();
    parse_quote!(#path { #(#fields),* })
}

/// `[1, 2, 3]` → `vec![1.0, 2.0, 3.0]`. Spread / holes are unmapped yet.
fn array_expr(arr: &ArrayExpression) -> Expr {
    let elems: Vec<Expr> = arr.elements.iter().filter_map(array_element).collect();
    parse_quote!(vec![#(#elems),*])
}

fn array_element(elem: &ArrayExpressionElement) -> Option<Expr> {
    match elem {
        ArrayExpressionElement::StringLiteral(s) => Some(string_expr(s)),
        ArrayExpressionElement::NumericLiteral(n) => Some(numeric_expr(n.value)),
        ArrayExpressionElement::BooleanLiteral(b) => Some(bool_expr(b.value)),
        _ => None,
    }
}

/// `p.x` → field access. (A `console.log` callee is intercepted earlier.)
fn member_expr(sm: &StaticMemberExpression) -> Expr {
    let obj = translate_expr(&sm.object);
    let field_name: &str = &sm.property.name;
    let field = format_ident!("{}", field_name);
    parse_quote!(#obj.#field)
}

fn ident_expr(id: &IdentifierReference) -> Expr {
    let name: &str = &id.name;
    let ident = format_ident!("{}", name);
    parse_quote!(#ident)
}

/// `console.log(x)` → `println!("{}", x)`. Other calls are unmapped yet.
fn translate_call(call: &CallExpression) -> Expr {
    if is_console_log(&call.callee) {
        match call.arguments.as_slice() {
            [arg] => {
                let value = translate_argument(arg);
                parse_quote!(::std::println!("{}", #value))
            }
            _ => parse_quote!(::core::todo!()), // multi-arg console.* not mapped yet
        }
    } else {
        parse_quote!(::core::todo!())
    }
}

/// True when `callee` is `console.log` (a static member access).
fn is_console_log(callee: &Expression) -> bool {
    let Expression::StaticMemberExpression(member) = callee else {
        return false;
    };
    is_ident(&member.object, "console") && {
        let prop: &str = &member.property.name;
        prop == "log"
    }
}

fn is_ident(expr: &Expression, expected: &str) -> bool {
    let Expression::Identifier(ident) = expr else {
        return false;
    };
    let name: &str = &ident.name;
    name == expected
}

/// `.ds` string literal → Rust `String` (`"…".to_string()`).
fn string_expr(s: &StringLiteral) -> Expr {
    let lit = syn::LitStr::new(s.value.as_str(), Span::call_site());
    parse_quote!(#lit.to_string())
}

fn bool_expr(value: bool) -> Expr {
    parse_quote!(#value)
}

/// Render an `f64` as a valid Rust literal expression.
///
/// `quote!` has no `ToTokens` for floats, and `NaN` / `Infinity` are not Rust
/// literals — so we format to a string and let `syn` parse it, mapping the
/// non-finite cases to `f64` constants.
fn numeric_expr(value: f64) -> Expr {
    let s = if value.is_nan() {
        "f64::NAN".to_string()
    } else if value.is_infinite() {
        if value > 0.0 { "f64::INFINITY" } else { "f64::NEG_INFINITY" }.to_string()
    } else {
        let s = format!("{value}");
        if s.contains('.') || s.contains('e') || s.contains('E') { s } else { format!("{s}.0") }
    };
    parse_str(&s).unwrap_or_else(|_| parse_quote!(::core::f64::NAN))
}
