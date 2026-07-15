//! `Expression` → `syn::Expr`.

use oxc_ast::ast::{
    Argument, ArrayExpression, ArrayExpressionElement, AssignmentExpression, AssignmentTarget,
    BinaryExpression, CallExpression, ConditionalExpression, Expression, IdentifierReference, LogicalExpression,
    ObjectExpression, ObjectPropertyKind, SimpleAssignmentTarget, StaticMemberExpression,
    StringLiteral, TemplateLiteral, TSNonNullExpression, UnaryExpression, UpdateExpression,
};
use oxc_syntax::operator::{
    AssignmentOperator, BinaryOperator, LogicalOperator, UnaryOperator, UpdateOperator,
};
use proc_macro2::Span;
use quote::quote;
use syn::{parse_quote, parse_str, BinOp, Expr, Type, UnOp};

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
        Expression::NullLiteral(_) => parse_quote!(None),
        Expression::Identifier(id) => ident_or_undefined(id),
        Expression::CallExpression(call) => translate_call(call),
        Expression::ArrayExpression(arr) => array_expr(arr),
        Expression::StaticMemberExpression(sm) => member_expr(sm),
        Expression::TemplateLiteral(t) => template_expr(t),
        Expression::BinaryExpression(bin) => binary_expr(bin),
        Expression::LogicalExpression(log) => logical_expr(log),
        Expression::ConditionalExpression(c) => conditional_expr(c),
        Expression::UnaryExpression(un) => unary_expr(un),
        Expression::AssignmentExpression(a) => assignment_expr(a),
        Expression::UpdateExpression(u) => update_expr(u),
        Expression::TSNonNullExpression(nn) => nonnull_expr(nn),
        _ => parse_quote!(::core::todo!()),
    }
}

/// Translate a call argument — [`Argument`] inherits the `Expression` variants.
pub fn translate_argument(arg: &Argument) -> Expr {
    match arg {
        Argument::StringLiteral(s) => string_expr(s),
        Argument::NumericLiteral(n) => numeric_expr(n.value),
        Argument::BooleanLiteral(b) => bool_expr(b.value),
        Argument::NullLiteral(_) => parse_quote!(None),
        Argument::Identifier(id) => ident_or_undefined(id),
        Argument::CallExpression(call) => translate_call(call),
        Argument::ArrayExpression(arr) => array_expr(arr),
        Argument::StaticMemberExpression(sm) => member_expr(sm),
        Argument::TemplateLiteral(t) => template_expr(t),
        Argument::BinaryExpression(bin) => binary_expr(bin),
        Argument::LogicalExpression(log) => logical_expr(log),
        Argument::ConditionalExpression(c) => conditional_expr(c),
        Argument::UnaryExpression(un) => unary_expr(un),
        Argument::TSNonNullExpression(nn) => nonnull_expr(nn),
        _ => parse_quote!(::core::todo!()),
    }
}

/// Translate an initializer; an object literal borrows its struct name from
/// the variable's type annotation (anonymous literals are unsupported yet).
pub fn translate_init(expr: &Expression, ty_hint: Option<&Type>) -> Expr {
    if let Expression::ObjectExpression(obj) = expr {
        return object_expr(obj, ty_hint);
    }
    // null / undefined map to `None` directly — never wrapped in `Some`.
    let nullish = matches!(expr, Expression::NullLiteral(_))
        || matches!(expr, Expression::Identifier(id) if id.name.as_str() == "undefined");
    if nullish {
        return parse_quote!(None);
    }
    // A non-null *value literal* into an `Option<T>` binding wraps in `Some`.
    // Identifiers/calls may already yield an `Option`, so only literals wrap.
    let is_value_literal = matches!(
        expr,
        Expression::NumericLiteral(_) | Expression::StringLiteral(_) | Expression::BooleanLiteral(_)
    );
    if is_value_literal && ty_hint.is_some_and(is_option) {
        let value = translate_expr(expr);
        return parse_quote!(Some(#value));
    }
    // A string literal into a named (non-`String`) type is an enum variant:
    // `let s: Status = "done"` → `Status::Done`.
    if let Expression::StringLiteral(s) = expr {
        if let Some(path) = ty_hint.and_then(types::type_path) {
            if path.is_ident("String") {
                return string_expr(s);
            }
            let value: &str = &s.value;
            let variant = bindings::pascal(value);
            return parse_quote!(#path::#variant);
        }
    }
    translate_expr(expr)
}

/// True when `ty` is `Option<…>` — decides whether to wrap an initializer.
fn is_option(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Path(tp) if tp.path.segments.last().is_some_and(|s| s.ident == "Option")
    )
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
    let field = bindings::snake(field_name);
    parse_quote!(#obj.#field)
}

fn ident_expr(id: &IdentifierReference) -> Expr {
    let name: &str = &id.name;
    let ident = bindings::snake(name);
    parse_quote!(#ident)
}

/// `undefined` (a global identifier in TS) maps to `None`; any other
/// identifier is a plain reference.
fn ident_or_undefined(id: &IdentifierReference) -> Expr {
    let name: &str = &id.name;
    if name == "undefined" {
        return parse_quote!(None);
    }
    ident_expr(id)
}

/// `x!` (TS non-null assertion) → `x.unwrap()`. The author asserts non-null, so
/// a panic on `None` is their explicit choice, not an implicit assumption.
fn nonnull_expr(nn: &TSNonNullExpression) -> Expr {
    let inner = translate_expr(&nn.expression);
    parse_quote!(#inner.unwrap())
}

/// `` `Hello, ${name}!` `` → `format!("Hello, {}!", name)`.
///
/// `{`/`}` in the literal are escaped (`{{`/`}}`) so they survive `format!`.
fn template_expr(t: &TemplateLiteral) -> Expr {
    let mut fmt = String::new();
    for (i, q) in t.quasis.iter().enumerate() {
        let raw: &str = q.value.raw.as_str();
        fmt.push_str(&raw.replace('{', "{{").replace('}', "}}"));
        if i < t.expressions.len() {
            fmt.push_str("{}");
        }
    }
    let exprs: Vec<Expr> = t.expressions.iter().map(translate_expr).collect();
    let fmt_lit = syn::LitStr::new(&fmt, Span::call_site());
    parse_quote!(::std::format!(#fmt_lit, #(#exprs),*))
}

/// Binary ops. TS `==`/`===` collapse to Rust `==` (Rust has no coercive `==`);
/// likewise `!=`/`!==`. `**`, bitwise, shifts, `in`, `instanceof` are unmapped.
///
/// We build `syn::Expr::Binary` directly (not `quote!` tokens) so `prettyplease`
/// adds parentheses by precedence instead of emitting a redundant pair around
/// every sub-expression.
fn binary_expr(bin: &BinaryExpression) -> Expr {
    let left = translate_expr(&bin.left);
    let right = translate_expr(&bin.right);
    let op = match bin.operator {
        BinaryOperator::Addition => BinOp::Add(Default::default()),
        BinaryOperator::Subtraction => BinOp::Sub(Default::default()),
        BinaryOperator::Multiplication => BinOp::Mul(Default::default()),
        BinaryOperator::Division => BinOp::Div(Default::default()),
        BinaryOperator::Remainder => BinOp::Rem(Default::default()),
        BinaryOperator::Equality | BinaryOperator::StrictEquality => BinOp::Eq(Default::default()),
        BinaryOperator::Inequality | BinaryOperator::StrictInequality => {
            BinOp::Ne(Default::default())
        }
        BinaryOperator::LessThan => BinOp::Lt(Default::default()),
        BinaryOperator::LessEqualThan => BinOp::Le(Default::default()),
        BinaryOperator::GreaterThan => BinOp::Gt(Default::default()),
        BinaryOperator::GreaterEqualThan => BinOp::Ge(Default::default()),
        _ => return parse_quote!(::core::todo!()),
    };
    Expr::Binary(syn::ExprBinary {
        attrs: Vec::new(),
        left: Box::new(left),
        op,
        right: Box::new(right),
    })
}

/// `&&`/`||` are a separate `LogicalExpression` in oxc (not `BinaryExpression`).
/// `??` (nullish coalescing) maps to `Option` semantics in wave 3.
fn logical_expr(log: &LogicalExpression) -> Expr {
    let left = translate_expr(&log.left);
    let right = translate_expr(&log.right);
    let op = match log.operator {
        LogicalOperator::And => BinOp::And(Default::default()),
        LogicalOperator::Or => BinOp::Or(Default::default()),
        LogicalOperator::Coalesce => return parse_quote!(::core::todo!()),
    };
    Expr::Binary(syn::ExprBinary {
        attrs: Vec::new(),
        left: Box::new(left),
        op,
        right: Box::new(right),
    })
}

/// Unary `-`/`!`. (`+` is a no-op; `~`, `typeof`, `void`, `delete` are unmapped.)
fn unary_expr(un: &UnaryExpression) -> Expr {
    let arg = translate_expr(&un.argument);
    match un.operator {
        UnaryOperator::UnaryPlus => arg,
        UnaryOperator::UnaryNegation => Expr::Unary(syn::ExprUnary {
            attrs: Vec::new(),
            op: UnOp::Neg(Default::default()),
            expr: Box::new(arg),
        }),
        UnaryOperator::LogicalNot => Expr::Unary(syn::ExprUnary {
            attrs: Vec::new(),
            op: UnOp::Not(Default::default()),
            expr: Box::new(arg),
        }),
        _ => parse_quote!(::core::todo!()),
    }
}

/// `cond ? a : b` → `if cond { a } else { b }` — Rust's `if` is an expression.
fn conditional_expr(c: &ConditionalExpression) -> Expr {
    let test = translate_expr(&c.test);
    let then = translate_expr(&c.consequent);
    let els = translate_expr(&c.alternate);
    parse_quote!(if #test { #then } else { #els })
}

/// `x = …`, `x += …`, … — only simple identifier targets (no `obj.x = …`).
fn assignment_expr(a: &AssignmentExpression) -> Expr {
    let Some(target) = assignment_target(&a.left) else {
        return parse_quote!(::core::todo!());
    };
    let right = translate_expr(&a.right);
    let tokens = match a.operator {
        AssignmentOperator::Assign => quote!(#target = #right),
        AssignmentOperator::Addition => quote!(#target += #right),
        AssignmentOperator::Subtraction => quote!(#target -= #right),
        AssignmentOperator::Multiplication => quote!(#target *= #right),
        AssignmentOperator::Division => quote!(#target /= #right),
        AssignmentOperator::Remainder => quote!(#target %= #right),
        _ => quote!(::core::todo!()),
    };
    syn::parse2(tokens).unwrap_or_else(|_| parse_quote!(::core::todo!()))
}

/// `i++` / `i--` → `i += 1.0` / `i -= 1.0`. Statement-context only: TS returns
/// the old value, which we don't preserve — fine for `i++;` but not `return i++`.
/// The step is `1.0` because `.ds` `number` is `f64`; an integer step would be a
/// type error against an `f64` target.
fn update_expr(u: &UpdateExpression) -> Expr {
    let Some(target) = simple_target(&u.argument) else {
        return parse_quote!(::core::todo!());
    };
    let tokens = match u.operator {
        UpdateOperator::Increment => quote!(#target += 1.0),
        UpdateOperator::Decrement => quote!(#target -= 1.0),
    };
    syn::parse2(tokens).unwrap_or_else(|_| parse_quote!(::core::todo!()))
}

fn assignment_target(target: &AssignmentTarget) -> Option<Expr> {
    match target {
        AssignmentTarget::AssignmentTargetIdentifier(id) => Some(ident_expr(id)),
        _ => None,
    }
}

fn simple_target(target: &SimpleAssignmentTarget) -> Option<Expr> {
    match target {
        SimpleAssignmentTarget::AssignmentTargetIdentifier(id) => Some(ident_expr(id)),
        _ => None,
    }
}

/// `console.log(x)` → `println!("{}", x)`; any other call maps the callee and
/// its arguments to a plain Rust call expression.
fn translate_call(call: &CallExpression) -> Expr {
    if is_console_log(&call.callee) {
        return match call.arguments.as_slice() {
            [arg] => {
                let value = translate_argument(arg);
                parse_quote!(::std::println!("{}", #value))
            }
            // multi-arg `console.*` is not mapped yet
            _ => parse_quote!(::core::todo!()),
        };
    }
    let callee = translate_expr(&call.callee);
    let args: Vec<Expr> = call.arguments.iter().map(translate_argument).collect();
    parse_quote!(#callee(#(#args),*))
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
