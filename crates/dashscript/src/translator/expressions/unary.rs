//! Unary, conditional, and non-null expressions.
//! `-`/`!`/`~` → Rust unary; `cond ? a : b` → `if`; `x!` → `unwrap`.

use oxc_ast::ast::{ConditionalExpression, Expression, TSNonNullExpression, UnaryExpression};
use oxc_syntax::operator::{BinaryOperator, UnaryOperator};
use proc_macro2::Span;
use syn::{parse_quote, Expr, Ident, LitStr, UnOp};

use super::super::bindings;
use super::super::builtins;
use super::super::context::Ctx;
use super::translate_expr;

/// Unary `-`/`!`/`~`. (`+` is a no-op; `typeof`, `void`, `delete` are unmapped.)
pub(super) fn unary_expr(un: &UnaryExpression, ctx: &Ctx<'_>) -> Expr {
    let arg = translate_expr(&un.argument, ctx);
    match un.operator {
        // `+x` is ES ToNumber. A string operand needs the full StringToNumber
        // (hex/binary/octal/decimal/Infinity) — `+"0xff"` is 255, not the
        // string "0xff"; a number operand passes through unchanged.
        UnaryOperator::UnaryPlus => {
            if expr_is_string(&un.argument, ctx) {
                builtins::to_number_expr(arg)
            } else {
                arg
            }
        }
        UnaryOperator::UnaryNegation => Expr::Unary(syn::ExprUnary {
            attrs: Vec::new(),
            op: UnOp::Neg(Default::default()),
            expr: Box::new(arg),
        }),
        UnaryOperator::LogicalNot => {
            // `!x` where `x` is a collection/`Option`/number identifier lowers
            // to the negated truthiness check (`x.is_empty()`/`is_none()`/`==
            // 0`), not Rust's `!` — which is E0600 on `Vec`/`HashMap`/`Option`
            // and wrong semantics on `f64`. This reaches inside an `||`/`&&`
            // operand where the shared `condition_expr` only sees the whole
            // logical expression. Falls back to `!arg` for an already-boolean
            // operand (a comparison result, a `bool` local).
            if let Some(e) = super::truthiness(&un.argument, true, ctx) {
                e
            } else {
                Expr::Unary(syn::ExprUnary {
                    attrs: Vec::new(),
                    op: UnOp::Not(Default::default()),
                    expr: Box::new(arg),
                })
            }
        }
        // `~a` → `!ToInt32(a) as f64` (TS `~` is 32-bit bitwise NOT). The
        // operand casts via `bitwise_operand` (f64 → i64 → i32 for the JS
        // `ToInt32` wrap; i64 skips the hop); bound to a local so `as` never
        // binds into a compound operand.
        UnaryOperator::BitwiseNot => {
            let a = super::bitwise_operand(&un.argument, ctx, true);
            parse_quote!({
                let __a = #a;
                (!__a) as f64
            })
        }
        // `typeof x` is a compile-time type query (DashScript is statically
        // typed), so the JS type string is known from the operand's spelling.
        UnaryOperator::Typeof => type_of_expr(&un.argument),
        _ => parse_quote!(::core::todo!()),
    }
}

/// True when `expr` evaluates to a string: a string literal (possibly
/// parenthesized), or an identifier bound to a `string` local. Drives unary
/// `+` to run ToNumber only on a string operand (a number is a no-op).
fn expr_is_string(e: &Expression, ctx: &Ctx<'_>) -> bool {
    match e {
        Expression::StringLiteral(_) => true,
        Expression::ParenthesizedExpression(p) => expr_is_string(&p.expression, ctx),
        Expression::Identifier(id) => {
            let name = bindings::snake(&id.name).to_string();
            ctx.local_type(&name).is_some_and(|p| p.is_ident("String"))
        }
        _ => false,
    }
}

/// `typeof x` — the JS type string, known at translate time from the
/// operand's spelling (DashScript is statically typed, so this is a compile-
/// time query, not a runtime check). `typeof <number>` → `"number"`,
/// `<string>` → `"string"`, `<boolean>` → `"boolean"`, `typeof null` →
/// `"object"` (the JS quirk), `typeof Math.<const>`/`Number.<const>` →
/// `"number"`, `typeof Math.<method>`/`Number.<method>` → `"function"` (a
/// function reference), `typeof Array`/`Object`/… → `"function"` (a global
/// builtin constructor is callable). Anything else falls back to `"object"`.
/// Returned as a Rust `String`.
fn type_of_expr(arg: &Expression) -> Expr {
    let s: &str = match arg {
        Expression::NumericLiteral(_) => "number",
        Expression::StringLiteral(_) => "string",
        Expression::BooleanLiteral(_) => "boolean",
        // JS `typeof null === "object"` — the famous bug, kept for conformance.
        Expression::NullLiteral(_) => "object",
        Expression::StaticMemberExpression(sm) if builtins::is_ident(&sm.object, "Math") => {
            // `Math.<constant>` is a number; `Math.<method>` is a function ref.
            if builtins::math_constant(&sm.property.name).is_some() {
                "number"
            } else {
                "function"
            }
        }
        Expression::StaticMemberExpression(sm) if builtins::is_ident(&sm.object, "Number") => {
            // `Number.<constant>` (MAX_VALUE/EPSILON/…) is a number;
            // `Number.<method>` (isInteger/parseInt/…) is a function ref.
            if builtins::number_constant(&sm.property.name).is_some() {
                "number"
            } else {
                "function"
            }
        }
        Expression::FunctionExpression(_) | Expression::ArrowFunctionExpression(_) => "function",
        // A global builtin constructor is callable (`typeof Array === "function"`).
        // Namespace objects (`Math`/`JSON`/`Reflect`/`Atomics`/`Intl`/`globalThis`)
        // are not — `typeof === "object"`; a user identifier also falls back to
        // "object" (a precise answer for a user symbol needs type inference).
        Expression::Identifier(id) => match id.name.as_str() {
            // Namespace objects — not callable, `typeof === "object"`.
            "Math" | "JSON" | "Reflect" | "Atomics" | "Intl" | "globalThis" => "object",
            // Global constructors — callable, `typeof === "function"`.
            "Array"
            | "Object"
            | "String"
            | "Number"
            | "Boolean"
            | "Symbol"
            | "Function"
            | "Date"
            | "RegExp"
            | "Error"
            | "TypeError"
            | "RangeError"
            | "SyntaxError"
            | "ReferenceError"
            | "EvalError"
            | "URIError"
            | "AggregateError"
            | "SuppressedError"
            | "Promise"
            | "Map"
            | "Set"
            | "WeakMap"
            | "WeakSet"
            | "WeakRef"
            | "FinalizationRegistry"
            | "ArrayBuffer"
            | "SharedArrayBuffer"
            | "DataView"
            | "BigInt"
            | "Proxy"
            | "Int8Array"
            | "Uint8Array"
            | "Uint8ClampedArray"
            | "Int16Array"
            | "Uint16Array"
            | "Int32Array"
            | "Uint32Array"
            | "Float32Array"
            | "Float64Array"
            | "BigInt64Array"
            | "BigUint64Array" => "function",
            _ => "object",
        },
        _ => "object",
    };
    let lit = LitStr::new(s, Span::call_site());
    parse_quote!(#lit.to_string())
}

/// `cond ? a : b` → `if cond { a } else { b }` — Rust's `if` is an expression.
/// A `typeof v === "string" ? f(v) : v` ternary (where `v` is an inline
/// scalar-union enum) lowers to a `match` that re-binds `v` to the variant's
/// inner value in the matching arm, so the `then` branch sees `v` as the
/// scalar (a `String`) and the `else` branch sees the whole enum.
pub(super) fn conditional_expr(c: &ConditionalExpression, ctx: &Ctx<'_>) -> Expr {
    if let Some(expr) = union_typeof_conditional(c, ctx) {
        return expr;
    }
    let test = super::condition_expr(&c.test, ctx);
    let then = translate_expr(&c.consequent, ctx);
    let els = translate_expr(&c.alternate, ctx);
    parse_quote!(if #test { #then } else { #els })
}

/// `typeof v === "string" ? then : els` (where `v` is an inline scalar-union
/// enum local) → `match v.clone() { Enum::Str(v) => then.to_string(), v =>
/// els.to_string() }`. The clone owns the scrutinee so the variant arm re-binds
/// `v` to the inner scalar (so `then`, which uses `v` as a `String`,
/// type-checks), and the catch-all keeps `v` as the enum. Both arms render via
/// `to_string` (the enum through its `Display` impl) so the `match` is a single
/// `String` — a `typeof` ternary almost always discriminates to stringify the
/// value (escape/interpolate), so the `String` result is the common case; a
/// numeric-context `typeof` ternary would surface as a `cargo check` error
/// rather than silently miscompile. Returns `None` unless the test is exactly
/// that shape over a union local, so a plain ternary falls through to `if`.
fn union_typeof_conditional(c: &ConditionalExpression, ctx: &Ctx<'_>) -> Option<Expr> {
    let (local, enum_ident, variant, negate) = typeof_union_test(&c.test, ctx)?;
    let then = translate_expr(&c.consequent, ctx);
    let els = translate_expr(&c.alternate, ctx);
    Some(if negate {
        parse_quote!(match ::std::clone::Clone::clone(&#local) {
            #enum_ident::#variant(#local) => ::std::string::ToString::to_string(&(#els)),
            #local => ::std::string::ToString::to_string(&(#then)),
        })
    } else {
        parse_quote!(match ::std::clone::Clone::clone(&#local) {
            #enum_ident::#variant(#local) => ::std::string::ToString::to_string(&(#then)),
            #local => ::std::string::ToString::to_string(&(#els)),
        })
    })
}

/// Deconstruct a `typeof <id> === "<scalar>"` (or `!==`, or the sides swapped)
/// test against an inline scalar-union enum local: the local's snake name, the
/// enum ident, the matching variant (`Str`/`Num`/`Bool`), and whether the
/// operator is negated. `None` for any other shape.
fn typeof_union_test(test: &Expression, ctx: &Ctx<'_>) -> Option<(Ident, Ident, Ident, bool)> {
    let Expression::BinaryExpression(bin) = test else {
        return None;
    };
    let negate = match bin.operator {
        BinaryOperator::Equality | BinaryOperator::StrictEquality => false,
        BinaryOperator::Inequality | BinaryOperator::StrictInequality => true,
        _ => return None,
    };
    let (id_name, scalar_str) = match (typeof_ident(&bin.left), str_literal(&bin.right)) {
        (Some(n), Some(s)) => (n, s),
        _ => match (typeof_ident(&bin.right), str_literal(&bin.left)) {
            (Some(n), Some(s)) => (n, s),
            _ => return None,
        },
    };
    let variant_tag = match scalar_str.as_str() {
        "string" => "Str",
        "number" => "Num",
        "boolean" => "Bool",
        _ => return None,
    };
    let local = bindings::snake(&id_name);
    let enum_ident = ctx
        .local_type(&local.to_string())
        .and_then(|p| p.segments.last().map(|s| s.ident.clone()))?;
    let item = ctx.registry().union_enums.get(&enum_ident)?;
    if !item.variants.iter().any(|v| v.ident == variant_tag) {
        return None;
    }
    let variant = Ident::new(variant_tag, Span::call_site());
    Some((local, enum_ident, variant, negate))
}

/// `typeof <id>` → that identifier's name, else `None`.
fn typeof_ident(expr: &Expression) -> Option<String> {
    let Expression::UnaryExpression(un) = expr else {
        return None;
    };
    if !matches!(un.operator, UnaryOperator::Typeof) {
        return None;
    }
    let Expression::Identifier(id) = &un.argument else {
        return None;
    };
    Some(id.name.to_string())
}

/// A string-literal expression's value, else `None`.
fn str_literal(expr: &Expression) -> Option<String> {
    let Expression::StringLiteral(s) = expr else {
        return None;
    };
    Some(s.value.to_string())
}

/// `x!` (TS non-null assertion) → `x.unwrap()`. The author asserts non-null, so
/// a panic on `None` is their explicit choice, not an implicit assumption.
pub(super) fn nonnull_expr(nn: &TSNonNullExpression, ctx: &Ctx<'_>) -> Expr {
    // Inside an `if (opt)` narrowing, `opt!` reads the bound inner value
    // directly — no `Option::unwrap` after an `is_some` check.
    if let Expression::Identifier(id) = &nn.expression {
        if ctx.is_narrowed_some(&bindings::snake(&id.name).to_string()) {
            return translate_expr(&nn.expression, ctx);
        }
    }
    let inner = translate_expr(&nn.expression, ctx);
    parse_quote!(#inner.unwrap())
}
