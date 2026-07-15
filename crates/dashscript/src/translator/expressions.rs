//! `Expression` → `syn::Expr`.

use oxc_ast::ast::{
    Argument, ArrayExpression, ArrayExpressionElement, ArrowFunctionExpression,
    AssignmentExpression, AssignmentTarget, BinaryExpression, CallExpression,
    ComputedMemberExpression, ConditionalExpression, Expression, FunctionBody,
    IdentifierReference, LogicalExpression, ObjectExpression, ObjectPropertyKind,
    SimpleAssignmentTarget, StaticMemberExpression, Statement, StringLiteral, TemplateLiteral,
    TSNonNullExpression, UnaryExpression, UpdateExpression,
};
use oxc_syntax::operator::{
    AssignmentOperator, BinaryOperator, LogicalOperator, UnaryOperator, UpdateOperator,
};
use proc_macro2::Span;
use quote::{format_ident, quote};
use syn::{parse_quote, parse_str, BinOp, Expr, Ident, Pat, Type, UnOp};

use super::context::Ctx;
use super::{bindings, types};

/// Translate an expression to its `syn::Expr` form.
///
/// Unmapped expressions fall back to `todo!()` so the generated Rust compiles
/// but fails loudly at run time if reached.
pub fn translate_expr(expr: &Expression, ctx: &Ctx<'_>) -> Expr {
    match expr {
        Expression::StringLiteral(s) => string_expr(s),
        Expression::NumericLiteral(n) => numeric_expr(n.value),
        Expression::BooleanLiteral(b) => bool_expr(b.value),
        Expression::NullLiteral(_) => parse_quote!(None),
        Expression::Identifier(id) => ident_or_undefined(id),
        Expression::CallExpression(call) => translate_call(call, ctx),
        Expression::ArrayExpression(arr) => array_expr(arr),
        Expression::StaticMemberExpression(sm) => member_expr(sm, ctx),
        Expression::ComputedMemberExpression(cm) => computed_member(cm, ctx),
        Expression::TemplateLiteral(t) => template_expr(t, ctx),
        Expression::BinaryExpression(bin) => binary_expr(bin, ctx),
        Expression::LogicalExpression(log) => logical_expr(log, ctx),
        Expression::ConditionalExpression(c) => conditional_expr(c, ctx),
        Expression::UnaryExpression(un) => unary_expr(un, ctx),
        Expression::AssignmentExpression(a) => assignment_expr(a, ctx),
        Expression::UpdateExpression(u) => update_expr(u),
        Expression::TSNonNullExpression(nn) => nonnull_expr(nn, ctx),
        Expression::ArrowFunctionExpression(arrow) => arrow_expr(arrow, ctx, false),
        // User-written parens are unwrapped; `prettyplease` re-adds any needed
        // for precedence (e.g. `(a + b) * c` round-trips correctly).
        Expression::ParenthesizedExpression(p) => translate_expr(&p.expression, ctx),
        _ => parse_quote!(::core::todo!()),
    }
}

/// Translate a call argument — [`Argument`] inherits the `Expression` variants.
pub fn translate_argument(arg: &Argument, ctx: &Ctx<'_>) -> Expr {
    match arg {
        Argument::StringLiteral(s) => string_expr(s),
        Argument::NumericLiteral(n) => numeric_expr(n.value),
        Argument::BooleanLiteral(b) => bool_expr(b.value),
        Argument::NullLiteral(_) => parse_quote!(None),
        Argument::Identifier(id) => ident_or_undefined(id),
        Argument::CallExpression(call) => translate_call(call, ctx),
        Argument::ArrayExpression(arr) => array_expr(arr),
        Argument::StaticMemberExpression(sm) => member_expr(sm, ctx),
        Argument::ComputedMemberExpression(cm) => computed_member(cm, ctx),
        Argument::TemplateLiteral(t) => template_expr(t, ctx),
        Argument::BinaryExpression(bin) => binary_expr(bin, ctx),
        Argument::LogicalExpression(log) => logical_expr(log, ctx),
        Argument::ConditionalExpression(c) => conditional_expr(c, ctx),
        Argument::UnaryExpression(un) => unary_expr(un, ctx),
        Argument::TSNonNullExpression(nn) => nonnull_expr(nn, ctx),
        Argument::ArrowFunctionExpression(arrow) => arrow_expr(arrow, ctx, false),
        Argument::ParenthesizedExpression(p) => translate_expr(&p.expression, ctx),
        _ => parse_quote!(::core::todo!()),
    }
}

/// Translate an initializer; an object literal borrows its struct name from
/// the variable's type annotation (anonymous literals are unsupported yet).
pub fn translate_init(expr: &Expression, ty_hint: Option<&Type>, ctx: &Ctx<'_>) -> Expr {
    if let Expression::ObjectExpression(obj) = expr {
        return object_expr(obj, ty_hint, ctx);
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
        let value = translate_expr(expr, ctx);
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
    translate_expr(expr, ctx)
}

/// True when `ty` is `Option<…>` — decides whether to wrap an initializer.
fn is_option(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Path(tp) if tp.path.segments.last().is_some_and(|s| s.ident == "Option")
    )
}

/// `Point { x: 1 }` — needs the target type's name from the binding annotation.
/// A `{ kind: "circle", … }` literal whose target is a registered
/// discriminated union instead builds a variant (`Shape::Circle { … }`).
fn object_expr(obj: &ObjectExpression, ty_hint: Option<&Type>, ctx: &Ctx<'_>) -> Expr {
    let Some(path) = ty_hint.and_then(types::type_path) else {
        return parse_quote!(::core::todo!());
    };
    if let Some(expr) = variant_construct(obj, path, ctx) {
        return expr;
    }
    let fields: Vec<syn::FieldValue> = obj
        .properties
        .iter()
        .filter_map(|p| {
            let ObjectPropertyKind::ObjectProperty(op) = p else { return None };
            let key = bindings::property_key_name(&op.key)?;
            let value = translate_expr(&op.value, ctx);
            Some(parse_quote!(#key: #value))
        })
        .collect();
    parse_quote!(#path { #(#fields),* })
}

/// `{ kind: "circle", radius: 2 }` → `Shape::Circle { radius: 2.0 }` when `path`
/// is a registered discriminated-union enum and the literal carries a matching
/// `kind` string. Returns `None` for a plain struct literal (no `kind`, or a
/// `kind` whose value isn't a registered variant of this enum).
fn variant_construct(obj: &ObjectExpression, path: &syn::Path, ctx: &Ctx<'_>) -> Option<Expr> {
    let type_name = path.segments.last()?.ident.to_string();
    let kind_value = kind_string(obj)?;
    let shape = ctx.variant(&type_name, &kind_value)?;
    let variant = &shape.name;
    let fields: Vec<syn::FieldValue> = obj
        .properties
        .iter()
        .filter_map(|p| {
            let ObjectPropertyKind::ObjectProperty(op) = p else { return None };
            let key = bindings::property_key_name(&op.key)?;
            // The discriminant is consumed by the variant name, not a field.
            if key == "kind" {
                return None;
            }
            let value = translate_expr(&op.value, ctx);
            Some(parse_quote!(#key: #value))
        })
        .collect();
    Some(parse_quote!(#path::#variant { #(#fields),* }))
}

/// The value of a `kind: "…"` string-literal property, if the object has one.
fn kind_string(obj: &ObjectExpression) -> Option<String> {
    for p in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(op) = p else {
            continue;
        };
        if bindings::property_key_name(&op.key).is_some_and(|k| k == "kind") {
            if let Expression::StringLiteral(s) = &op.value {
                return Some(s.value.to_string());
            }
        }
    }
    None
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
fn member_expr(sm: &StaticMemberExpression, ctx: &Ctx<'_>) -> Expr {
    let field_name: &str = &sm.property.name;
    // `Math.PI` / `Math.E` → the corresponding Rust constant.
    if is_ident(&sm.object, "Math") {
        if let Some(p) = math_constant(field_name) {
            return p;
        }
    }
    // Inside a discriminated-union match arm, `s.field` reads as the `field`
    // binding the pattern destructured (TS narrowing).
    if let Expression::Identifier(id) = &sm.object {
        let scrut = bindings::snake(&id.name);
        let field = bindings::snake(field_name);
        if ctx.narrow_binds(&scrut.to_string(), &field.to_string()) {
            return parse_quote!(#field);
        }
    }
    let obj = translate_expr(&sm.object, ctx);
    // `.length` on a Vec/String maps to Rust's `.len()` (a method, not a field).
    if field_name == "length" {
        return parse_quote!(#obj.len());
    }
    let field = bindings::snake(field_name);
    parse_quote!(#obj.#field)
}

/// `Math.<m>(args)` → the idiomatic Rust float operation. Single-arg methods
/// (`floor`, `ceil`, `abs`, …) become a method on the argument; `max`/`min`
/// become `a.max(b)`; `pow` becomes `a.powf(b)`. Returns `None` when unmapped.
fn math_method(name: &str, args: &[Argument], ctx: &Ctx<'_>) -> Option<Expr> {
    match name {
        "floor" | "ceil" | "round" | "abs" | "sqrt" | "trunc" | "sign" | "exp" | "ln" => {
            let recv = math_receiver(args.first()?, ctx);
            Some(method_call(recv, name, Vec::new()))
        }
        "max" | "min" => {
            let a = math_receiver(args.first()?, ctx);
            let b = translate_argument(args.get(1)?, ctx);
            Some(method_call(a, name, vec![b]))
        }
        "pow" => {
            let a = math_receiver(args.first()?, ctx);
            let b = translate_argument(args.get(1)?, ctx);
            Some(method_call(a, "powf", vec![b]))
        }
        _ => None,
    }
}

/// Build `recv.method(args)` as an `ExprMethodCall` so `prettyplease`
/// parenthesizes the receiver by precedence — `(a + b).sqrt()`, not
/// `a + b.sqrt()` (which would bind `.sqrt()` to `b` only).
fn method_call(recv: Expr, method: &str, args: Vec<Expr>) -> Expr {
    Expr::MethodCall(syn::ExprMethodCall {
        attrs: Vec::new(),
        receiver: Box::new(recv),
        dot_token: Default::default(),
        method: format_ident!("{}", method),
        turbofish: None,
        args: args.into_iter().collect(),
        paren_token: Default::default(),
    })
}

/// A `Math.` receiver: a numeric literal gets an `_f64` suffix so a bare
/// literal like `3` isn't an ambiguous `{float}` (`3.0.max(7.0)` won't infer);
/// any other receiver translates normally (its context already pins `f64`).
fn math_receiver(arg: &Argument, ctx: &Ctx<'_>) -> Expr {
    if let Argument::NumericLiteral(n) = arg {
        let s = format!("{}_f64", n.value);
        return parse_str(&s).unwrap_or_else(|_| parse_quote!(::core::f64::NAN));
    }
    translate_argument(arg, ctx)
}

/// `Math.PI` → `std::f64::consts::PI`, `Math.E` → `…::E`.
fn math_constant(name: &str) -> Option<Expr> {
    let path = match name {
        "PI" => quote!(::std::f64::consts::PI),
        "E" => quote!(::std::f64::consts::E),
        _ => return None,
    };
    syn::parse2(path).ok()
}

/// Base of `**`: a numeric literal gets an `_f64` suffix so `2 ** 3` isn't an
/// ambiguous `{float}` receiver; any other operand translates normally.
fn pow_receiver(expr: &Expression, ctx: &Ctx<'_>) -> Expr {
    if let Expression::NumericLiteral(n) = expr {
        let s = format!("{}_f64", n.value);
        return parse_str(&s).unwrap_or_else(|_| parse_quote!(::core::f64::NAN));
    }
    translate_expr(expr, ctx)
}

/// `arr[i]` → `arr[i as usize]`. A `.ds` index is `f64`; Rust indexes by
/// `usize`, so the index is cast. Bounds are unchecked (panics out of range) —
/// a safe `.get`-based mapping can come later.
fn computed_member(cm: &ComputedMemberExpression, ctx: &Ctx<'_>) -> Expr {
    let obj = translate_expr(&cm.object, ctx);
    let idx = translate_expr(&cm.expression, ctx);
    let idx = Expr::Cast(syn::ExprCast {
        attrs: Vec::new(),
        expr: Box::new(idx),
        as_token: syn::Token![as](Span::call_site()),
        ty: Box::new(parse_quote!(usize)),
    });
    parse_quote!(#obj[#idx])
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
fn nonnull_expr(nn: &TSNonNullExpression, ctx: &Ctx<'_>) -> Expr {
    let inner = translate_expr(&nn.expression, ctx);
    parse_quote!(#inner.unwrap())
}

/// `(x) => expr` → `|x| expr` (expression body only; a block body is unmapped).
/// Parameter type annotations are dropped — Rust infers them at the call site.
/// Translate an arrow to a Rust closure. `borrow_params` wraps each parameter
/// in a `&` pattern (`|&n|`) so the closure body reads owned values — used for
/// `.filter` callbacks, whose closure receives `&Item` even after `.copied()`.
fn arrow_expr(arrow: &ArrowFunctionExpression, ctx: &Ctx<'_>, borrow_params: bool) -> Expr {
    let params: Vec<Pat> = arrow
        .params
        .items
        .iter()
        .map(|fp| {
            let name = bindings::binding_name(&fp.pattern);
            if borrow_params {
                parse_quote!(&#name)
            } else {
                parse_quote!(#name)
            }
        })
        .collect();
    let body = if arrow.expression {
        single_expression_body(&arrow.body)
            .map(|e| translate_expr(e, ctx))
            .unwrap_or_else(|| parse_quote!(::core::todo!()))
    } else {
        parse_quote!(::core::todo!())
    };
    parse_quote!(|#(#params),*| #body)
}

/// The single expression of an expression-body arrow (`() => expr`), when the
/// body is exactly one expression statement.
fn single_expression_body<'a, 'b>(body: &'b FunctionBody<'a>) -> Option<&'b Expression<'a>> {
    let [Statement::ExpressionStatement(es)] = body.statements.as_slice() else {
        return None;
    };
    Some(&es.expression)
}

/// `` `Hello, ${name}!` `` → `format!("Hello, {}!", name)`.
///
/// `{`/`}` in the literal are escaped (`{{`/`}}`) so they survive `format!`.
fn template_expr(t: &TemplateLiteral, ctx: &Ctx<'_>) -> Expr {
    let mut fmt = String::new();
    for (i, q) in t.quasis.iter().enumerate() {
        let raw: &str = q.value.raw.as_str();
        fmt.push_str(&raw.replace('{', "{{").replace('}', "}}"));
        if i < t.expressions.len() {
            fmt.push_str("{}");
        }
    }
    let exprs: Vec<Expr> = t.expressions.iter().map(|e| translate_expr(e, ctx)).collect();
    let fmt_lit = syn::LitStr::new(&fmt, Span::call_site());
    parse_quote!(::std::format!(#fmt_lit, #(#exprs),*))
}

/// Binary ops. TS `==`/`===` collapse to Rust `==` (Rust has no coercive `==`);
/// likewise `!=`/`!==`. `**`, bitwise, shifts, `in`, `instanceof` are unmapped.
///
/// A `+` chain that contains a string literal is TS string concatenation and is
/// mapped to `format!` — Rust's `+` does not apply to `String`.
///
/// We build `syn::Expr::Binary` directly (not `quote!` tokens) so `prettyplease`
/// adds parentheses by precedence instead of emitting a redundant pair around
/// every sub-expression.
fn binary_expr(bin: &BinaryExpression, ctx: &Ctx<'_>) -> Expr {
    // `x === null` / `x !== null` → `x.is_none()` / `x.is_some()` when `x` is an
    // Option-typed local; any other comparison returns `None` and falls through.
    if let Some(expr) = null_equality(bin, ctx) {
        return expr;
    }
    if matches!(bin.operator, BinaryOperator::Addition) && concat_is_string(bin) {
        return string_concat(bin, ctx);
    }
    // `a ** b` → `a.powf(b)`; a numeric-literal base gets an `_f64` suffix so
    // `2 ** 3` isn't an ambiguous `{float}` receiver.
    if matches!(bin.operator, BinaryOperator::Exponential) {
        let base = pow_receiver(&bin.left, ctx);
        let exp = translate_expr(&bin.right, ctx);
        return parse_quote!(#base.powf(#exp));
    }
    let left = translate_expr(&bin.left, ctx);
    let right = translate_expr(&bin.right, ctx);
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

/// `x === null` / `null === x` → `x.is_none()`; `x !== null` → `x.is_some()`,
/// but only when the non-null side is an `Option`-typed local. Other
/// comparisons return `None` and fall through to a plain Rust `==`/`!=` (which
/// `cargo check` rejects for type mismatches — a loud, not silent, failure).
fn null_equality(bin: &BinaryExpression, ctx: &Ctx<'_>) -> Option<Expr> {
    let negate = match bin.operator {
        BinaryOperator::Equality | BinaryOperator::StrictEquality => false,
        BinaryOperator::Inequality | BinaryOperator::StrictInequality => true,
        _ => return None,
    };
    let (left_null, right_null) = (is_nullish(&bin.left), is_nullish(&bin.right));
    let name = if right_null {
        option_local_name(&bin.left, ctx)
    } else if left_null {
        option_local_name(&bin.right, ctx)
    } else {
        None
    }?;
    let ident = bindings::snake(name);
    Some(if negate {
        parse_quote!(#ident.is_some())
    } else {
        parse_quote!(#ident.is_none())
    })
}

/// The source name of `expr` when it is a plain identifier bound to an
/// `Option<…>` local; `None` otherwise.
fn option_local_name<'a>(expr: &'a Expression, ctx: &Ctx<'_>) -> Option<&'a str> {
    let Expression::Identifier(id) = expr else {
        return None;
    };
    let name: &str = &id.name;
    if name == "undefined" {
        return None;
    }
    ctx.is_option(&bindings::snake(name).to_string()).then_some(name)
}

/// `null` or the `undefined` global.
fn is_nullish(expr: &Expression) -> bool {
    matches!(expr, Expression::NullLiteral(_))
        || matches!(expr, Expression::Identifier(id) if id.name.as_str() == "undefined")
}

/// True when a `+` chain is string concatenation: any leaf operand is a string
/// literal. TS makes the entire chain a string concat as soon as one operand is
/// a string, so this syntactic check is sound — and the only unhandled case
/// (`stringVar + stringVar`, no literal) fails loudly under `cargo check`.
fn concat_is_string(bin: &BinaryExpression) -> bool {
    operand_is_string(&bin.left) || operand_is_string(&bin.right)
}

fn operand_is_string(expr: &Expression) -> bool {
    match expr {
        Expression::StringLiteral(_) => true,
        Expression::BinaryExpression(inner) if matches!(inner.operator, BinaryOperator::Addition) => {
            concat_is_string(inner)
        }
        _ => false,
    }
}

/// Flatten a `+` chain to its leaf operands (left to right) and emit
/// `format!("{}", …)` with one `{}` placeholder per operand.
fn string_concat(bin: &BinaryExpression, ctx: &Ctx<'_>) -> Expr {
    let mut parts: Vec<Expr> = Vec::new();
    flatten_add(&bin.left, &mut parts, ctx);
    flatten_add(&bin.right, &mut parts, ctx);
    let fmt = syn::LitStr::new(&"{}".repeat(parts.len()), Span::call_site());
    parse_quote!(::std::format!(#fmt, #(#parts),*))
}

/// Flatten a `+` chain, translating each leaf to `syn::Expr`. A non-`+`
/// sub-expression (e.g. `a * b` inside a concat) is a leaf, translated as a whole.
fn flatten_add(expr: &Expression, parts: &mut Vec<Expr>, ctx: &Ctx<'_>) {
    if let Expression::BinaryExpression(bin) = expr {
        if matches!(bin.operator, BinaryOperator::Addition) {
            flatten_add(&bin.left, parts, ctx);
            flatten_add(&bin.right, parts, ctx);
            return;
        }
    }
    parts.push(translate_expr(expr, ctx));
}

/// `&&`/`||` are a separate `LogicalExpression` in oxc (not `BinaryExpression`).
/// `??` (nullish coalescing) maps to `Option::unwrap_or_else` (see `coalesce_expr`).
fn logical_expr(log: &LogicalExpression, ctx: &Ctx<'_>) -> Expr {
    if matches!(log.operator, LogicalOperator::Coalesce) {
        return coalesce_expr(&log.left, &log.right, ctx);
    }
    let left = translate_expr(&log.left, ctx);
    let right = translate_expr(&log.right, ctx);
    let op = match log.operator {
        LogicalOperator::And => BinOp::And(Default::default()),
        LogicalOperator::Or => BinOp::Or(Default::default()),
        LogicalOperator::Coalesce => unreachable!("?? is handled above"),
    };
    Expr::Binary(syn::ExprBinary {
        attrs: Vec::new(),
        left: Box::new(left),
        op,
        right: Box::new(right),
    })
}

/// `a ?? b`: when `a` is an `Option`-typed local, `a.unwrap_or_else(|| b)`; when
/// `a` is non-nullable the result is just `a` (a `??` on a never-null value is a
/// no-op).
fn coalesce_expr(left: &Expression, right: &Expression, ctx: &Ctx<'_>) -> Expr {
    let right_val = translate_expr(right, ctx);
    if let Some(name) = option_local_name(left, ctx) {
        let ident = bindings::snake(name);
        return parse_quote!(#ident.unwrap_or_else(|| #right_val));
    }
    translate_expr(left, ctx)
}

/// Unary `-`/`!`. (`+` is a no-op; `~`, `typeof`, `void`, `delete` are unmapped.)
fn unary_expr(un: &UnaryExpression, ctx: &Ctx<'_>) -> Expr {
    let arg = translate_expr(&un.argument, ctx);
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
fn conditional_expr(c: &ConditionalExpression, ctx: &Ctx<'_>) -> Expr {
    let test = translate_expr(&c.test, ctx);
    let then = translate_expr(&c.consequent, ctx);
    let els = translate_expr(&c.alternate, ctx);
    parse_quote!(if #test { #then } else { #els })
}

/// `x = …`, `x += …`, … — only simple identifier targets (no `obj.x = …`).
fn assignment_expr(a: &AssignmentExpression, ctx: &Ctx<'_>) -> Expr {
    let Some(target) = assignment_target(&a.left) else {
        return parse_quote!(::core::todo!());
    };
    let right = translate_expr(&a.right, ctx);
    let tokens = match a.operator {
        AssignmentOperator::Assign => quote!(#target = #right),
        // `s += "lit"` is string append (String has no AddAssign) → `push_str`.
        AssignmentOperator::Addition => match &a.right {
            Expression::StringLiteral(s) => {
                let lit = syn::LitStr::new(s.value.as_str(), Span::call_site());
                quote!(#target.push_str(#lit))
            }
            _ => quote!(#target += #right),
        },
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
fn translate_call(call: &CallExpression, ctx: &Ctx<'_>) -> Expr {
    if is_console_log(&call.callee) {
        let vals: Vec<Expr> = call.arguments.iter().map(|a| translate_argument(a, ctx)).collect();
        let placeholders: String = vals.iter().map(|_| "{}").collect::<Vec<_>>().join(" ");
        let fmt = syn::LitStr::new(&placeholders, Span::call_site());
        return parse_quote!(::std::println!(#fmt, #(#vals),*));
    }
    // `Math.floor(x)` → `x.floor()`; `Math.max(a, b)` → `a.max(b)`.
    if let Expression::StaticMemberExpression(sm) = &call.callee {
        if is_ident(&sm.object, "Math") {
            if let Some(expr) = math_method(&sm.property.name, call.arguments.as_slice(), ctx) {
                return expr;
            }
        }
    }
    // A method call (`s.toUpperCase()`) maps the method name, not the receiver.
    if let Expression::StaticMemberExpression(sm) = &call.callee {
        if let Some(expr) = string_method(sm, call.arguments.as_slice(), ctx) {
            return expr;
        }
        if let Some(expr) = array_method(sm, call.arguments.as_slice(), ctx) {
            return expr;
        }
        if let Some(method) = map_method(&sm.property.name) {
            let obj = translate_expr(&sm.object, ctx);
            let args: Vec<Expr> = call.arguments.iter().map(|a| translate_argument(a, ctx)).collect();
            return parse_quote!(#obj.#method(#(#args),*));
        }
    }
    let callee = translate_expr(&call.callee, ctx);
    let args: Vec<Expr> = call.arguments.iter().map(|a| translate_argument(a, ctx)).collect();
    parse_quote!(#callee(#(#args),*))
}

/// Array methods on a `Vec` of Copy elements: `.map`/`.filter` take a callback
/// (`xs.iter().copied().<m>(f).collect::<Vec<_>>()`, with `.filter`'s param
/// destructured since its closure receives `&Item`); `.slice(a, b)` →
/// `xs[a as usize..b as usize].to_vec()` (`.slice(a)` / `.slice()` clamp the
/// range / clone). Returns `None` otherwise.
fn array_method(sm: &StaticMemberExpression, args: &[Argument], ctx: &Ctx<'_>) -> Option<Expr> {
    let Expression::Identifier(id) = &sm.object else {
        return None;
    };
    let recv = bindings::snake(&id.name);
    let path = ctx.local_type(&recv.to_string())?;
    let is_vec = path.segments.last().is_some_and(|s| s.ident == "Vec");
    if !is_vec {
        return None;
    }
    Some(match sm.property.name.as_str() {
        "map" => {
            let cb = translate_argument(args.first()?, ctx);
            parse_quote!(#recv.iter().copied().map(#cb).collect::<Vec<_>>())
        }
        "filter" => {
            let Argument::ArrowFunctionExpression(arrow) = args.first()? else {
                return None;
            };
            let cb = arrow_expr(arrow, ctx, true);
            parse_quote!(#recv.iter().copied().filter(#cb).collect::<Vec<_>>())
        }
        // `.ds` indices are `f64`; cast each bound to `usize`.
        "slice" => {
            let start = args.first().map(|a| usize_arg(a, ctx));
            let end = args.get(1).map(|a| usize_arg(a, ctx));
            match (start, end) {
                (Some(s), Some(e)) => parse_quote!(#recv[#s..#e].to_vec()),
                (Some(s), None) => parse_quote!(#recv[#s..].to_vec()),
                (None, _) => parse_quote!(#recv.clone()),
            }
        }
        _ => return None,
    })
}

/// A handful of TS method names map to a different Rust method name; the
/// receiver and arguments are passed through unchanged. Unmapped methods fall
/// through to a plain call on the receiver expression.
fn map_method(name: &str) -> Option<Ident> {
    let mapped = match name {
        "toUpperCase" => "to_uppercase",
        "toLowerCase" => "to_lowercase",
        "trim" => "trim",
        "push" => "push",
        "pop" => "pop",
        _ => return None,
    };
    Some(format_ident!("{}", mapped))
}

/// String methods whose arguments need adapting to Rust's `&str`-oriented API:
/// `includes`/`startsWith`/`endsWith` → `contains`/`starts_with`/`ends_with`;
/// `replace` → `replacen(.., 1)` (TS replaces the first match only); `repeat`
/// → `repeat(n as usize)`. Returns `None` for unmapped names.
fn string_method(sm: &StaticMemberExpression, args: &[Argument], ctx: &Ctx<'_>) -> Option<Expr> {
    let obj = translate_expr(&sm.object, ctx);
    let name: &str = &sm.property.name;
    Some(match name {
        "includes" => {
            let a = str_method_arg(args.first()?, ctx);
            parse_quote!(#obj.contains(#a))
        }
        "startsWith" => {
            let a = str_method_arg(args.first()?, ctx);
            parse_quote!(#obj.starts_with(#a))
        }
        "endsWith" => {
            let a = str_method_arg(args.first()?, ctx);
            parse_quote!(#obj.ends_with(#a))
        }
        "replace" => {
            let a = str_method_arg(args.first()?, ctx);
            let b = str_method_arg(args.get(1)?, ctx);
            parse_quote!(#obj.replacen(#a, #b, 1))
        }
        "repeat" => {
            let n = usize_arg(args.first()?, ctx);
            parse_quote!(#obj.repeat(#n))
        }
        "split" => {
            // `split` yields `&str`; map to owned so the result is `Vec<String>`.
            let delim = str_method_arg(args.first()?, ctx);
            parse_quote!(#obj.split(#delim).map(|part| part.to_string()).collect::<Vec<String>>())
        }
        _ => return None,
    })
}

/// A string-method argument as a `&str`: a string literal stays a bare literal
/// (a perfect `Pattern`); any other expression (a `String` var or call) gets
/// `.as_str()` so it satisfies Rust's `&str`-typed string APIs.
fn str_method_arg(arg: &Argument, ctx: &Ctx<'_>) -> Expr {
    if let Argument::StringLiteral(s) = arg {
        let lit = syn::LitStr::new(s.value.as_str(), Span::call_site());
        return parse_quote!(#lit);
    }
    let e = translate_argument(arg, ctx);
    parse_quote!(#e.as_str())
}

/// A `.ds` `number` argument cast to `usize` (e.g. for `repeat`).
fn usize_arg(arg: &Argument, ctx: &Ctx<'_>) -> Expr {
    let e = translate_argument(arg, ctx);
    parse_quote!(#e as usize)
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
