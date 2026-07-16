//! `Expression` → `syn::Expr`.

use std::collections::HashSet;

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
use quote::quote;
use syn::{parse_quote, parse_str, BinOp, Expr, Ident, Pat, Path, Type, UnOp};

use super::context::Ctx;
use super::{bindings, types};

mod math;
mod methods;

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
        Expression::ArrayExpression(arr) => array_expr(arr, ctx),
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
        // A TS type assertion (`x as T` / `<T>x`) has no runtime effect — the
        // inner expression is passed through unchanged.
        Expression::TSAsExpression(a) => translate_expr(&a.expression, ctx),
        Expression::TSTypeAssertion(t) => translate_expr(&t.expression, ctx),
        Expression::ArrowFunctionExpression(arrow) => arrow_expr(arrow, ctx, false),
        // User-written parens are unwrapped; `prettyplease` re-adds any needed
        // for precedence (e.g. `(a + b) * c` round-trips correctly).
        Expression::ParenthesizedExpression(p) => translate_expr(&p.expression, ctx),
        Expression::ChainExpression(c) => chain_expr(&c.expression, ctx),
        _ => parse_quote!(::core::todo!()),
    }
}

/// Optional chaining `a?.field` → `a.as_ref().map(|__c| __c.field)`. The
/// receiver is an `Option`; the access maps over a reference and yields
/// another `Option`. Only a single optional field access is handled; indexed
/// access, optional calls, and chained `a?.b?.c` fall back to `todo!()`.
fn chain_expr(elem: &oxc_ast::ast::ChainElement, ctx: &Ctx<'_>) -> Expr {
    use oxc_ast::ast::ChainElement;
    match elem {
        ChainElement::StaticMemberExpression(sm) => {
            let obj = translate_expr(&sm.object, ctx);
            let field = bindings::snake(&sm.property.name);
            parse_quote!(#obj.as_ref().map(|__c| __c.#field))
        }
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
        Argument::ArrayExpression(arr) => array_expr(arr, ctx),
        Argument::StaticMemberExpression(sm) => member_expr(sm, ctx),
        Argument::ComputedMemberExpression(cm) => computed_member(cm, ctx),
        Argument::TemplateLiteral(t) => template_expr(t, ctx),
        Argument::BinaryExpression(bin) => binary_expr(bin, ctx),
        Argument::LogicalExpression(log) => logical_expr(log, ctx),
        Argument::ConditionalExpression(c) => conditional_expr(c, ctx),
        Argument::UnaryExpression(un) => unary_expr(un, ctx),
        Argument::TSNonNullExpression(nn) => nonnull_expr(nn, ctx),
        Argument::TSAsExpression(a) => translate_expr(&a.expression, ctx),
        Argument::TSTypeAssertion(t) => translate_expr(&t.expression, ctx),
        Argument::ArrowFunctionExpression(arrow) => arrow_expr(arrow, ctx, false),
        Argument::ParenthesizedExpression(p) => translate_expr(&p.expression, ctx),
        _ => parse_quote!(::core::todo!()),
    }
}

/// Translate a call argument; an object literal borrows its struct name from
/// the callee's declared parameter type (when known). Other arguments fall
/// through to [`translate_argument`].
pub fn translate_argument_init(arg: &Argument, hint: Option<&Type>, ctx: &Ctx<'_>) -> Expr {
    if let Argument::ObjectExpression(obj) = arg {
        return object_expr(obj, hint, ctx);
    }
    translate_argument(arg, ctx)
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
    // `Record<K, V>` (a `HashMap`) → `HashMap::from([(key, value), …])`.
    if is_hashmap(path) {
        return hashmap_literal(obj, ctx);
    }
    if let Some(expr) = variant_construct(obj, path, ctx) {
        return expr;
    }
    // A `…v` spread records a struct-update base (`Struct { …, ..v }`); only an
    // identifier base is supported. If multiple spreads appear, the last wins.
    let optionals = optional_fields_for(path, ctx);
    let mut base: Option<Expr> = None;
    let fields: Vec<syn::FieldValue> = obj
        .properties
        .iter()
        .filter_map(|p| match p {
            ObjectPropertyKind::ObjectProperty(op) => {
                let key = bindings::property_key_name(&op.key)?;
                let mut value = translate_expr(&op.value, ctx);
                // An optional field's supplied value is wrapped in `Some`.
                let key_str = key.to_string();
                if optionals.is_some_and(|s| s.contains(&key_str)) {
                    value = parse_quote!(Some(#value));
                }
                Some(parse_quote!(#key: #value))
            }
            ObjectPropertyKind::SpreadProperty(sp) => {
                base = Some(translate_expr(&sp.argument, ctx));
                None
            }
        })
        .collect();
    match base {
        Some(b) => parse_quote!(#path { #(#fields),*, ..#b }),
        None => {
            let extras = missing_optionals(path, &fields, ctx);
            parse_quote!(#path { #(#fields),*, #(#extras),* })
        }
    }
}

/// The optional (`?:`) field names of the struct named by `path`, if any.
fn optional_fields_for<'a>(path: &syn::Path, ctx: &Ctx<'a>) -> Option<&'a HashSet<String>> {
    let type_name = path.segments.last()?.ident.to_string();
    ctx.struct_optionals(&type_name)
}

/// `None` initializers for optional (`?:`) fields the literal omitted, so a
/// partial struct literal still names every field. Only fields registered as
/// optional on this struct type and absent from `present` are filled.
fn missing_optionals(path: &syn::Path, present: &[syn::FieldValue], ctx: &Ctx<'_>) -> Vec<syn::FieldValue> {
    let Some(type_name) = path.segments.last().map(|s| s.ident.to_string()) else {
        return Vec::new();
    };
    let Some(optionals) = ctx.struct_optionals(&type_name) else {
        return Vec::new();
    };
    let present: HashSet<String> = present
        .iter()
        .filter_map(|f| match &f.member {
            syn::Member::Named(id) => Some(id.to_string()),
            syn::Member::Unnamed(_) => None,
        })
        .collect();
    optionals
        .iter()
        .filter(|name| !present.contains(*name))
        .map(|name| {
            let id = Ident::new(name.as_str(), Span::call_site());
            parse_quote!(#id: None)
        })
        .collect()
}

/// True when `path` names a `HashMap` (the target of a `Record<K, V>`).
fn is_hashmap(path: &syn::Path) -> bool {
    path.segments.last().is_some_and(|s| s.ident == "HashMap")
}

/// `{ a: 1, b: 2 }` as a `HashMap` → `HashMap::from([("a".to_string(), 1.0), …])`.
/// Keys are the `.ds` property names, owned so the map outlives the literal.
fn hashmap_literal(obj: &ObjectExpression, ctx: &Ctx<'_>) -> Expr {
    let entries: Vec<Expr> = obj
        .properties
        .iter()
        .filter_map(|p| {
            let ObjectPropertyKind::ObjectProperty(op) = p else { return None };
            let value = translate_expr(&op.value, ctx);
            let key = if op.computed {
                // `[k]: v` — a dynamic key (an expression, typically a String).
                translate_expr(op.key.as_expression()?, ctx)
            } else {
                let key_str = bindings::property_key_name(&op.key)?.to_string();
                parse_quote!(#key_str.to_string())
            };
            Some(parse_quote!((#key, #value)))
        })
        .collect();
    parse_quote!(::std::collections::HashMap::from([#(#entries),*]))
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

/// `[1, 2, 3]` → `vec![1.0, 2.0, 3.0]`. A spread (`[...xs, 4]`) builds via
/// slice concat: `[xs.as_slice(), &[4.0][..]].concat()`.
fn array_expr(arr: &ArrayExpression, ctx: &Ctx<'_>) -> Expr {
    if arr
        .elements
        .iter()
        .any(|e| matches!(e, ArrayExpressionElement::SpreadElement(_)))
    {
        return spread_array(arr, ctx);
    }
    let elems: Vec<Expr> = arr.elements.iter().filter_map(|e| array_element(e, ctx)).collect();
    parse_quote!(vec![#(#elems),*])
}

/// `[...xs, 4]` → `[xs.as_slice(), &[4.0][..]].concat()`: consecutive literals
/// batch into one `&[..]` slice, each spread into `arg.as_slice()`.
fn spread_array(arr: &ArrayExpression, ctx: &Ctx<'_>) -> Expr {
    let mut segments: Vec<Expr> = Vec::new();
    let mut literals: Vec<Expr> = Vec::new();
    for e in &arr.elements {
        match e {
            ArrayExpressionElement::SpreadElement(sp) => {
                flush_literals(&mut literals, &mut segments);
                let arg = translate_expr(&sp.argument, ctx);
                segments.push(parse_quote!(#arg.as_slice()));
            }
            other => {
                if let Some(expr) = array_element(other, ctx) {
                    literals.push(expr);
                }
            }
        }
    }
    flush_literals(&mut literals, &mut segments);
    parse_quote!([#(#segments),*].concat())
}

/// Flush pending literals into a `&[a, b, ..]` slice segment.
fn flush_literals(literals: &mut Vec<Expr>, segments: &mut Vec<Expr>) {
    if literals.is_empty() {
        return;
    }
    let owned = std::mem::take(literals);
    segments.push(parse_quote!(&[#(#owned),*][..]))
}

fn array_element(elem: &ArrayExpressionElement, ctx: &Ctx<'_>) -> Option<Expr> {
    // A spread element is handled earlier by `spread_array`; an elision (array
    // hole) has no Rust equivalent and is dropped. Any other element is an
    // expression — translate it through the main expression path so an array
    // literal may hold any expression, not just value literals.
    match elem {
        ArrayExpressionElement::SpreadElement(_) | ArrayExpressionElement::Elision(_) => None,
        _ => Some(translate_expr(elem.as_expression()?, ctx)),
    }
}

/// `p.x` → field access. (A `console.log` callee is intercepted earlier.)
fn member_expr(sm: &StaticMemberExpression, ctx: &Ctx<'_>) -> Expr {
    let field_name: &str = &sm.property.name;
    // `Math.PI` / `Math.E` → the corresponding Rust constant.
    if methods::is_ident(&sm.object, "Math") {
        if let Some(p) = math::math_constant(field_name) {
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

/// Base of `**`: a numeric literal gets an `_f64` suffix so `2 ** 3` isn't an
/// ambiguous `{float}` receiver; any other operand translates normally.
fn pow_receiver(expr: &Expression, ctx: &Ctx<'_>) -> Expr {
    if let Expression::NumericLiteral(n) = expr {
        let s = format!("{}_f64", n.value);
        return parse_str(&s).unwrap_or_else(|_| parse_quote!(::core::f64::NAN));
    }
    translate_expr(expr, ctx)
}

/// `arr[i]` → `arr[i as usize]`; `m["k"]` on a `HashMap` →
/// `m.get("k").copied().unwrap()`. A `.ds` index is `f64`; Rust indexes by
/// `usize`, so the Vec/array index is cast. A HashMap key is looked up with
/// `.get` (typed: the key is assumed present, so `unwrap` panics if absent —
/// matching the non-optional type).
fn computed_member(cm: &ComputedMemberExpression, ctx: &Ctx<'_>) -> Expr {
    let obj = translate_expr(&cm.object, ctx);
    if is_hashmap_local(&cm.object, ctx) {
        let key = index_key(&cm.expression, ctx);
        return parse_quote!(#obj.get(#key).copied().unwrap());
    }
    let idx = translate_expr(&cm.expression, ctx);
    let idx = Expr::Cast(syn::ExprCast {
        attrs: Vec::new(),
        expr: Box::new(idx),
        as_token: syn::Token![as](Span::call_site()),
        ty: Box::new(parse_quote!(usize)),
    });
    parse_quote!(#obj[#idx])
}

/// True when `expr` is a local whose type is a `HashMap`.
fn is_hashmap_local(expr: &Expression, ctx: &Ctx<'_>) -> bool {
    let Expression::Identifier(id) = expr else {
        return false;
    };
    let name = bindings::snake(&id.name).to_string();
    ctx.local_type(&name).is_some_and(is_hashmap)
}

/// A HashMap key: a string literal stays bare (a `&str` for `HashMap::get`);
/// any other expression gets `.as_str()`.
fn index_key(expr: &Expression, ctx: &Ctx<'_>) -> Expr {
    if let Expression::StringLiteral(s) = expr {
        let lit = syn::LitStr::new(s.value.as_str(), Span::call_site());
        return parse_quote!(#lit);
    }
    let e = translate_expr(expr, ctx);
    parse_quote!(#e.as_str())
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
    // `"k" in m` → key membership. A `Record`/HashMap uses `contains_key`; an
    // array (`Vec`) treats the left as an index bound: `(i as usize) < len`.
    if matches!(bin.operator, BinaryOperator::In) {
        let key = translate_expr(&bin.left, ctx);
        let right = translate_expr(&bin.right, ctx);
        let is_vec = matches!(&bin.right, Expression::Identifier(id)
            if ctx.local_type(&bindings::snake(&id.name).to_string())
                .and_then(|p| p.segments.last())
                .is_some_and(|s| s.ident == "Vec"));
        return if is_vec {
            parse_quote!((#key as usize) < #right.len())
        } else {
            parse_quote!(#right.contains_key(&#key))
        };
    }
    // Bitwise `&`/`|`/`^` operate on `i32` in both TS and Rust; cast each f64
    // operand down and the result back up to `.ds`'s `number` (`f64`).
    if let Some(expr) = bitwise_expr(bin, ctx) {
        return expr;
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

/// Bitwise `&`/`|`/`^` and shifts `<<`/`>>`/`>>>`: TS applies these to `i32`,
/// so each `f64` operand is cast down, the op applied, and the result cast back
/// to `f64` (`.ds` number). Shifts use `wrapping_shl`/`shr` (which mask the
/// count); `>>>` casts to `u32` first for the zero-fill.
fn bitwise_expr(bin: &BinaryExpression, ctx: &Ctx<'_>) -> Option<Expr> {
    if !matches!(
        bin.operator,
        BinaryOperator::BitwiseAnd
            | BinaryOperator::BitwiseOR
            | BinaryOperator::BitwiseXOR
            | BinaryOperator::ShiftLeft
            | BinaryOperator::ShiftRight
            | BinaryOperator::ShiftRightZeroFill
    ) {
        return None;
    }
    let left = translate_expr(&bin.left, ctx);
    let right = translate_expr(&bin.right, ctx);
    Some(match bin.operator {
        BinaryOperator::BitwiseAnd => parse_quote!(((#left as i32) & (#right as i32)) as f64),
        BinaryOperator::BitwiseOR => parse_quote!(((#left as i32) | (#right as i32)) as f64),
        BinaryOperator::BitwiseXOR => parse_quote!(((#left as i32) ^ (#right as i32)) as f64),
        // `<<`/`>>` use `wrapping_shl`/`shr` (they mask the shift count, so a
        // large `.ds` count won't panic like Rust's plain `<<` would).
        BinaryOperator::ShiftLeft => {
            parse_quote!(((#left as i32).wrapping_shl(#right as u32)) as f64)
        }
        BinaryOperator::ShiftRight => {
            parse_quote!(((#left as i32).wrapping_shr(#right as u32)) as f64)
        }
        // `>>>` is logical (zero-fill): cast to `u32` before the shift.
        BinaryOperator::ShiftRightZeroFill => {
            parse_quote!((((#left as i32) as u32).wrapping_shr(#right as u32)) as f64)
        }
        _ => unreachable!(),
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
    // A `bool` left operand short-circuits as Rust `&&`/`||` (TS `bool || bool`
    // is itself a bool). A value operand uses a truthiness block: TS `a || b`
    // returns `a` when truthy else `b`, so bind `a` once and branch.
    if methods::expr_is_bool(&log.left, ctx) {
        let left = translate_expr(&log.left, ctx);
        let right = translate_expr(&log.right, ctx);
        let op = match log.operator {
            LogicalOperator::And => BinOp::And(Default::default()),
            LogicalOperator::Or => BinOp::Or(Default::default()),
            LogicalOperator::Coalesce => unreachable!(),
        };
        return Expr::Binary(syn::ExprBinary {
            attrs: Vec::new(),
            left: Box::new(left),
            op,
            right: Box::new(right),
        });
    }
    let left = translate_expr(&log.left, ctx);
    let right = translate_expr(&log.right, ctx);
    let cond = methods::truthy_cond(&log.left, ctx);
    match log.operator {
        // `a || b`: a truthy → a, else b
        LogicalOperator::Or => parse_quote!({ let __l = #left; if #cond { __l } else { #right } }),
        // `a && b`: a truthy → b, else a
        LogicalOperator::And => parse_quote!({ let __l = #left; if #cond { #right } else { __l } }),
        LogicalOperator::Coalesce => unreachable!(),
    }
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
    // An optional-chain result is itself an `Option`, so the right side
    // supplies the default via `unwrap_or`.
    if matches!(left, Expression::ChainExpression(_)) {
        let left_val = translate_expr(left, ctx);
        return parse_quote!(#left_val.unwrap_or(#right_val));
    }
    translate_expr(left, ctx)
}

/// Unary `-`/`!`/`~`. (`+` is a no-op; `typeof`, `void`, `delete` are unmapped.)
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
        // `~a` → `!(a as i32) as f64` (TS `~` is 32-bit bitwise NOT).
        UnaryOperator::BitwiseNot => parse_quote!((!(#arg as i32)) as f64),
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

/// The lvalue kind of an assignment's left-hand side. A plain target is any
/// Rust lvalue (`x`, `obj.field`, `arr[i as usize]`); a `m["k"]` on a
/// `HashMap` local is an insert (the map takes the key and value separately).
enum AssignTarget {
    Plain(Expr),
    HashInsert { map: Ident, key: Expr },
}

/// `x = …`, `x += …`, …. Plain targets (`x`, `obj.field`, `arr[i as usize]`)
/// take every compound op; a `m["k"]` HashMap index becomes `m.insert(k, v)`
/// (only `=` — HashMap has no compound-assign semantics).
fn assignment_expr(a: &AssignmentExpression, ctx: &Ctx<'_>) -> Expr {
    let right = translate_expr(&a.right, ctx);
    match assignment_target_kind(&a.left, ctx) {
        Some(AssignTarget::Plain(target)) => {
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
                AssignmentOperator::Exponential => quote!(#target = #target.powf(#right)),
                // Bitwise compound reads & writes the target, so it must be a
                // simple identifier lvalue; the result is cast back to `f64`.
                AssignmentOperator::BitwiseAnd => {
                    quote!(#target = ((#target as i32) & (#right as i32)) as f64)
                }
                AssignmentOperator::BitwiseOR => {
                    quote!(#target = ((#target as i32) | (#right as i32)) as f64)
                }
                AssignmentOperator::BitwiseXOR => {
                    quote!(#target = ((#target as i32) ^ (#right as i32)) as f64)
                }
                AssignmentOperator::ShiftLeft => {
                    quote!(#target = ((#target as i32).wrapping_shl(#right as u32)) as f64)
                }
                AssignmentOperator::ShiftRight => {
                    quote!(#target = ((#target as i32).wrapping_shr(#right as u32)) as f64)
                }
                AssignmentOperator::ShiftRightZeroFill => {
                    quote!(#target = (((#target as i32) as u32).wrapping_shr(#right as u32)) as f64)
                }
                // `x ??= y` on an Option<T>: assign Some(y) when x is None.
                AssignmentOperator::LogicalNullish => {
                    quote!(if #target.is_none() { #target = Some(#right) })
                }
                // `x ||= y` / `x &&= y`: assign y based on x's truthiness.
                AssignmentOperator::LogicalOr => {
                    let truthy = assign_truthy(&a.left, &target, ctx);
                    quote!(if !(#truthy) { #target = #right })
                }
                AssignmentOperator::LogicalAnd => {
                    let truthy = assign_truthy(&a.left, &target, ctx);
                    quote!(if #truthy { #target = #right })
                }
            };
            syn::parse2(tokens).unwrap_or_else(|_| parse_quote!(::core::todo!()))
        }
        Some(AssignTarget::HashInsert { map, key }) => match a.operator {
            AssignmentOperator::Assign => parse_quote!(#map.insert(#key, #right)),
            _ => parse_quote!(::core::todo!()),
        },
        None => parse_quote!(::core::todo!()),
    }
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

/// The truthiness test for an assignment target, picking the check by its
/// declared type (an identifier local) — used by `||=`/`&&=`. Falls back to a
/// numeric `!= 0.0` when the type is unknown or the target isn't an identifier.
fn assign_truthy(left: &AssignmentTarget, target: &Expr, ctx: &Ctx<'_>) -> Expr {
    if let AssignmentTarget::AssignmentTargetIdentifier(id) = left {
        let name = bindings::snake(&id.name).to_string();
        let last = ctx
            .local_type(&name)
            .and_then(|p| p.segments.last())
            .map(|s| s.ident.to_string());
        return match last.as_deref() {
            Some("Vec") | Some("HashMap") | Some("String") => parse_quote!(!#target.is_empty()),
            Some("Option") => parse_quote!(#target.is_some()),
            Some("bool") => parse_quote!(#target),
            _ => parse_quote!(#target != 0.0),
        };
    }
    parse_quote!(#target != 0.0)
}

/// Resolve an assignment's left-hand side to an [`AssignTarget`]. Member
/// targets (`obj.field`, `arr[i]`) become plain Rust lvalues; a `m["k"]` on a
/// `HashMap` local is recognized as an insert.
fn assignment_target_kind(target: &AssignmentTarget, ctx: &Ctx<'_>) -> Option<AssignTarget> {
    match target {
        AssignmentTarget::AssignmentTargetIdentifier(id) => {
            Some(AssignTarget::Plain(ident_expr(id)))
        }
        AssignmentTarget::StaticMemberExpression(sm) => {
            let obj = translate_expr(&sm.object, ctx);
            let field = bindings::snake(&sm.property.name);
            Some(AssignTarget::Plain(parse_quote!(#obj.#field)))
        }
        AssignmentTarget::ComputedMemberExpression(cm) => {
            // `m["k"] = v` on a HashMap → `m.insert(key, v)`.
            if is_hashmap_local(&cm.object, ctx) {
                let Expression::Identifier(id) = &cm.object else {
                    return None;
                };
                let map = bindings::snake(&id.name);
                let key = translate_expr(&cm.expression, ctx);
                return Some(AssignTarget::HashInsert { map, key });
            }
            // `xs[i] = v` → `xs[i as usize] = v`.
            let obj = translate_expr(&cm.object, ctx);
            let idx = translate_expr(&cm.expression, ctx);
            Some(AssignTarget::Plain(parse_quote!(#obj[#idx as usize])))
        }
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
    if let Some(macro_name) = methods::console_method(&call.callee) {
        let vals: Vec<Expr> = call.arguments.iter().map(|a| translate_argument(a, ctx)).collect();
        let placeholders: String = vals.iter().map(|_| "{}").collect::<Vec<_>>().join(" ");
        let fmt = syn::LitStr::new(&placeholders, Span::call_site());
        return parse_quote!(::std::#macro_name!(#fmt, #(#vals),*));
    }
    // `Math.floor(x)` → `x.floor()`; `Math.max(a, b)` → `a.max(b)`.
    if let Expression::StaticMemberExpression(sm) = &call.callee {
        if methods::is_ident(&sm.object, "Math") {
            if let Some(expr) = math::math_method(&sm.property.name, call.arguments.as_slice(), ctx) {
                return expr;
            }
        }
        // `Object.keys(m)` / `Object.values(m)` on a `Record` (a `HashMap`).
        if methods::is_ident(&sm.object, "Object") {
            if let Some(expr) = methods::object_method(&sm.property.name, call.arguments.as_slice(), ctx) {
                return expr;
            }
        }
        // `String.fromCharCode(n)` → a one-char `String`.
        if methods::is_ident(&sm.object, "String") {
            if let Some(expr) = methods::string_static(&sm.property.name, call.arguments.as_slice(), ctx) {
                return expr;
            }
        }
    }
    // Global conversion functions: `String(x)`, `parseInt(s)`, `parseFloat(s)`.
    if let Expression::Identifier(id) = &call.callee {
        if let Some(expr) = methods::global_function(id, call.arguments.as_slice(), ctx) {
            return expr;
        }
    }
    // A method call (`s.toUpperCase()`) maps the method name, not the receiver.
    if let Expression::StaticMemberExpression(sm) = &call.callee {
        if let Some(expr) = methods::array_method(sm, call.arguments.as_slice(), ctx) {
            return expr;
        }
        if let Some(expr) = methods::string_method(sm, call.arguments.as_slice(), ctx) {
            return expr;
        }
        if let Some(expr) = methods::number_method(sm, call.arguments.as_slice(), ctx) {
            return expr;
        }
        if let Some(method) = methods::map_method(&sm.property.name) {
            let obj = translate_expr(&sm.object, ctx);
            let args: Vec<Expr> = call.arguments.iter().map(|a| translate_argument(a, ctx)).collect();
            return parse_quote!(#obj.#method(#(#args),*));
        }
    }
    let callee = translate_expr(&call.callee, ctx);
    // `f({ x, y })` borrows the struct name from `f`'s declared parameter type.
    let hints: Option<&[Option<Path>]> = match &call.callee {
        Expression::Identifier(id) => ctx.function_params(&id.name),
        _ => None,
    };
    let defaults: Option<&[bool]> = match &call.callee {
        Expression::Identifier(id) => ctx.function_defaults(&id.name),
        _ => None,
    };
    let mut args: Vec<Expr> = call
        .arguments
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let hint_ty = hints
                .and_then(|h| h.get(i))
                .and_then(|opt| opt.as_ref())
                .map(|p| -> Type { parse_quote!(#p) });
            let val = translate_argument_init(a, hint_ty.as_ref(), ctx);
            // A non-`Copy` local read elsewhere too is cloned at the call site
            // (TS reference reuse vs Rust move); done before the `Some` wrap.
            let val = clone_owned_local(a, val, ctx);
            // A supplied value for a defaulted parameter wraps in `Some`.
            if defaults.is_some_and(|d| d.get(i) == Some(&true)) {
                parse_quote!(Some(#val))
            } else {
                val
            }
        })
        .collect();
    // Omitted trailing defaulted parameters pass `None`.
    if let Some(h) = hints {
        while args.len() < h.len() {
            args.push(parse_quote!(None));
        }
    }
    parse_quote!(#callee(#(#args),*))
}

/// A bare-local argument passed by value to a user function. TS reference
/// semantics lets the caller reuse the value afterwards, but Rust would move
/// it; when the local is also read elsewhere (use count > 1) and is not
/// `Copy`, clone it at the call site so those later reads still see a value.
/// A scalar is `Copy` (never cloned); a local read only here is moved, which
/// is the idiomatic last use.
fn clone_owned_local(arg: &Argument, val: Expr, ctx: &Ctx<'_>) -> Expr {
    let Argument::Identifier(id) = arg else { return val };
    if id.name.as_str() == "undefined" {
        return val;
    }
    let name = bindings::snake(&id.name).to_string();
    if ctx.use_count(&name) <= 1 {
        return val;
    }
    match ctx.local_type(&name) {
        Some(ty) if types::is_copy_path(ty) => val,
        Some(_) => parse_quote!(#val.clone()),
        None => val,
    }
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
