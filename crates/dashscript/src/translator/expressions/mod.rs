//! `Expression` → `syn::Expr`.
//!
//! The per-node-kind logic lives in one file per AST family (`literals`,
//! `object`, `array`, `member`, `binary`, `logical`, `unary`, `assignment`,
//! `call`); this module is the dispatch table (`translate_expr` /
//! `translate_argument`) plus the helpers shared across families
//! (`ident_expr`, `option_local_name`, `is_hashmap`, `arrow_expr`, …). New
//! expression kinds land as a new family file (or an arm in an existing one),
//! not as growth here.

mod array;
mod assignment;
mod binary;
mod call;
mod fmt_merge;
mod literals;
mod logical;
mod member;
mod new;
mod object;
mod unary;

// Re-exports only for callers outside this module's dispatch: `builtins` reads
// `bool_expr`/`string_expr` via `super::super::expressions::…`, and `functions`
// reads `array_slice_expr`. Sibling families use fully-qualified paths
// (`super::logical::assign_truthy`) instead, so they need no re-export.
pub(in crate::translator) use array::array_slice_expr;
pub(in crate::translator) use assignment::assignment_expr;
pub(in crate::translator) use literals::{bool_expr, string_expr};

use oxc_ast::ast::{
    Argument, ArrowFunctionExpression, Expression, FunctionBody, IdentifierReference, Statement,
    TemplateLiteral,
};
use proc_macro2::Span;
use syn::{parse_quote, Expr, Pat, Type};

use super::context::Ctx;
use super::{bindings, types};

/// Translate an expression to its `syn::Expr` form.
///
/// Unmapped expressions fall back to `todo!()` so the generated Rust compiles
/// but fails loudly at run time if reached.
pub fn translate_expr(expr: &Expression, ctx: &Ctx<'_>) -> Expr {
    match expr {
        Expression::StringLiteral(s) => literals::string_expr(s),
        Expression::NumericLiteral(n) => literals::numeric_expr(n.value),
        Expression::BooleanLiteral(b) => literals::bool_expr(b.value),
        Expression::NullLiteral(_) => parse_quote!(None),
        Expression::Identifier(id) => ident_or_undefined(id),
        Expression::CallExpression(call) => call::translate_call(call, ctx),
        Expression::ArrayExpression(arr) => array::array_expr(arr, ctx),
        Expression::StaticMemberExpression(sm) => member::member_expr(sm, ctx),
        Expression::ComputedMemberExpression(cm) => member::computed_member(cm, ctx),
        Expression::TemplateLiteral(t) => template_expr(t, ctx),
        Expression::BinaryExpression(bin) => binary::binary_expr(bin, ctx),
        Expression::LogicalExpression(log) => logical::logical_expr(log, ctx),
        Expression::ConditionalExpression(c) => unary::conditional_expr(c, ctx),
        Expression::UnaryExpression(un) => unary::unary_expr(un, ctx),
        Expression::AssignmentExpression(a) => assignment::assignment_expr(a, ctx),
        Expression::UpdateExpression(u) => assignment::update_expr(u),
        Expression::TSNonNullExpression(nn) => unary::nonnull_expr(nn, ctx),
        // A TS type assertion (`x as T` / `<T>x`) has no runtime effect — the
        // inner expression is passed through unchanged.
        Expression::TSAsExpression(a) => translate_expr(&a.expression, ctx),
        Expression::TSTypeAssertion(t) => translate_expr(&t.expression, ctx),
        Expression::ArrowFunctionExpression(arrow) => arrow_expr(arrow, ctx, false),
        // User-written parens are unwrapped; `prettyplease` re-adds any needed
        // for precedence (e.g. `(a + b) * c` round-trips correctly).
        Expression::ParenthesizedExpression(p) => translate_expr(&p.expression, ctx),
        Expression::ChainExpression(c) => member::chain_expr(&c.expression, ctx),
        // `this` inside a class method → the receiver (`self`/`__ds_self`);
        // outside a method → a `compile_error!`.
        Expression::ThisExpression(_) => super::context::this_expr(ctx),
        Expression::NewExpression(n) => new::new_expr(n, ctx),
        _ => parse_quote!(::core::todo!()),
    }
}

/// Translate a call argument — [`Argument`] inherits the `Expression` variants.
pub fn translate_argument(arg: &Argument, ctx: &Ctx<'_>) -> Expr {
    match arg {
        Argument::StringLiteral(s) => literals::string_expr(s),
        Argument::NumericLiteral(n) => literals::numeric_expr(n.value),
        Argument::BooleanLiteral(b) => literals::bool_expr(b.value),
        Argument::NullLiteral(_) => parse_quote!(None),
        Argument::Identifier(id) => ident_or_undefined(id),
        Argument::CallExpression(call) => call::translate_call(call, ctx),
        Argument::ArrayExpression(arr) => array::array_expr(arr, ctx),
        Argument::StaticMemberExpression(sm) => member::member_expr(sm, ctx),
        Argument::ComputedMemberExpression(cm) => member::computed_member(cm, ctx),
        Argument::TemplateLiteral(t) => template_expr(t, ctx),
        Argument::BinaryExpression(bin) => binary::binary_expr(bin, ctx),
        Argument::LogicalExpression(log) => logical::logical_expr(log, ctx),
        Argument::ConditionalExpression(c) => unary::conditional_expr(c, ctx),
        Argument::UnaryExpression(un) => unary::unary_expr(un, ctx),
        Argument::TSNonNullExpression(nn) => unary::nonnull_expr(nn, ctx),
        Argument::TSAsExpression(a) => translate_expr(&a.expression, ctx),
        Argument::TSTypeAssertion(t) => translate_expr(&t.expression, ctx),
        Argument::ArrowFunctionExpression(arrow) => arrow_expr(arrow, ctx, false),
        Argument::ParenthesizedExpression(p) => translate_expr(&p.expression, ctx),
        Argument::ThisExpression(_) => super::context::this_expr(ctx),
        Argument::NewExpression(n) => new::new_expr(n, ctx),
        _ => parse_quote!(::core::todo!()),
    }
}

/// Translate a call argument; an object literal borrows its struct name from
/// the callee's declared parameter type (when known). Other arguments fall
/// through to [`translate_argument`].
pub fn translate_argument_init(arg: &Argument, hint: Option<&Type>, ctx: &Ctx<'_>) -> Expr {
    if let Argument::ObjectExpression(obj) = arg {
        return object::object_expr(obj, hint, ctx);
    }
    translate_argument(arg, ctx)
}

/// Translate an initializer; an object literal borrows its struct name from
/// the variable's type annotation (anonymous literals are unsupported yet).
pub fn translate_init(expr: &Expression, ty_hint: Option<&Type>, ctx: &Ctx<'_>) -> Expr {
    if let Expression::ObjectExpression(obj) = expr {
        return object::object_expr(obj, ty_hint, ctx);
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
        Expression::NumericLiteral(_)
            | Expression::StringLiteral(_)
            | Expression::BooleanLiteral(_)
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
                return literals::string_expr(s);
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

fn ident_expr(id: &IdentifierReference) -> Expr {
    let name: &str = &id.name;
    // ES global constants are bare identifiers (`NaN`, `Infinity`), not members
    // — map them to the matching `f64` constant instead of a renamed, undefined
    // local. `-Infinity` lowers via unary `-` on `Infinity`.
    match name {
        "NaN" => parse_quote!(::std::f64::NAN),
        "Infinity" => parse_quote!(::std::f64::INFINITY),
        _ => {
            let ident = bindings::snake(name);
            parse_quote!(#ident)
        }
    }
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

/// The source name of `expr` when it is a plain identifier bound to an
/// `Option<…>` local; `None` otherwise.
pub(in crate::translator) fn option_local_name<'a>(
    expr: &'a Expression,
    ctx: &Ctx<'_>,
) -> Option<&'a str> {
    let Expression::Identifier(id) = expr else {
        return None;
    };
    let name: &str = &id.name;
    if name == "undefined" {
        return None;
    }
    ctx.is_option(&bindings::snake(name).to_string())
        .then_some(name)
}

/// True when `path` names a `HashMap` (the target of a `Record<K, V>`).
pub(in crate::translator) fn is_hashmap(path: &syn::Path) -> bool {
    path.segments.last().is_some_and(|s| s.ident == "HashMap")
}

/// `(x) => expr` → `|x| expr` (expression body only; a block body is unmapped).
/// Parameter type annotations are dropped — Rust infers them at the call site.
/// Translate an arrow to a Rust closure. `borrow_params` wraps each parameter
/// in a `&` pattern (`|&n|`) so the closure body reads owned values — used for
/// `.filter` callbacks, whose closure receives `&Item` even after `.copied()`.
pub(in crate::translator) fn arrow_expr(
    arrow: &ArrowFunctionExpression,
    ctx: &Ctx<'_>,
    borrow_params: bool,
) -> Expr {
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
    let exprs: Vec<Expr> = t
        .expressions
        .iter()
        .map(|e| translate_expr(e, ctx))
        .collect();
    let fmt_lit = syn::LitStr::new(&fmt, Span::call_site());
    parse_quote!(::std::format!(#fmt_lit, #(#exprs),*))
}
