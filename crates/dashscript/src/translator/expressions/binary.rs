//! Binary operators: arithmetic, comparison, bitwise, `in`, and `+` string concat.

use oxc_ast::ast::{BinaryExpression, ChainElement, Expression};
use oxc_syntax::operator::BinaryOperator;
use proc_macro2::Span;
use syn::{parse_quote, parse_str, BinOp, Expr, Type};

use super::super::bindings;
use super::super::context::Ctx;
use super::super::flavor::{expr_flavor, NumberFlavor};
use super::bitwise_operand;
use super::fmt_merge;
use super::is_number_expr;
use super::option_local_name;
use super::translate_expr;
use super::translate_number_to;

/// Binary ops. TS `==`/`===` collapse to Rust `==` (Rust has no coercive `==`);
/// likewise `!=`/`!==`. `**`, bitwise, shifts, `in`, `instanceof` are unmapped.
///
/// A `+` chain that contains a string literal is TS string concatenation and is
/// mapped to `format!` — Rust's `+` does not apply to `String`.
///
/// We build `syn::Expr::Binary` directly (not `quote!` tokens) so `prettyplease`
/// adds parentheses by precedence instead of emitting a redundant pair around
/// every sub-expression.
/// `String` instance methods whose result is a `String` — recognizing one as a
/// `+` operand (alongside its string receiver) lets `(cond ? "a" : "") +
/// spaces.repeat(n)` fold into `format!` instead of Rust's `+`, which does not
/// apply to `String`.
const STRING_RETURNING_METHODS: &[&str] = &[
    "repeat",
    "toString",
    "toLowerCase",
    "toUpperCase",
    "trim",
    "trimStart",
    "trimEnd",
    "replace",
    "replaceAll",
    "substring",
    "substr",
    "slice",
    "padStart",
    "padEnd",
    "concat",
    "normalize",
    "toLocaleLowerCase",
    "toLocaleUpperCase",
];

pub(super) fn binary_expr(bin: &BinaryExpression, ctx: &Ctx<'_>) -> Expr {
    // `x === null` / `x !== null` → `x.is_none()` / `x.is_some()` when `x` is an
    // Option-typed local; any other comparison returns `None` and falls through.
    if let Some(expr) = null_equality(bin, ctx) {
        return expr;
    }
    // `v === undefined` / `v !== null` when `v` is an inline scalar-union enum
    // local → `matches!(v, <Enum>::Undef)` / `!matches!(v, <Enum>::Null)` — the
    // union's `Undef`/`Null` variant is the runtime tag, not Rust `None`.
    if let Some(expr) = union_null_equality(bin, ctx) {
        return expr;
    }
    // `obj.field == value` where `field` is an optional `?:` struct member —
    // the field reads as `Option<T>`, so a bare `==`/`!=` against `T` would
    // not compile. Lower to an `as_deref()`/`Some(…)` comparison.
    if let Some(expr) = option_field_equality(bin, ctx) {
        return expr;
    }
    if matches!(bin.operator, BinaryOperator::Addition) && concat_is_string(bin, ctx) {
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
        let right = translate_expr(&bin.right, ctx);
        let is_vec = matches!(&bin.right, Expression::Identifier(id)
            if ctx.local_type(&bindings::snake(&id.name).to_string())
                .and_then(|p| p.segments.last())
                .is_some_and(|s| s.ident == "Vec"));
        return if is_vec {
            let key = translate_expr(&bin.left, ctx);
            parse_quote!((#key as usize) < #right.len())
        } else {
            // A string-literal key borrows as `&str` directly (a `HashMap` keys
            // it via `Borrow<str>`); avoid the needless `.to_string()`.
            match &bin.left {
                Expression::StringLiteral(s) => {
                    let lit = syn::LitStr::new(s.value.as_str(), Span::call_site());
                    parse_quote!(#right.contains_key(#lit))
                }
                _ => {
                    let key = translate_expr(&bin.left, ctx);
                    parse_quote!(#right.contains_key(&#key))
                }
            }
        };
    }
    // Bitwise `&`/`|`/`^` operate on `i32` in both TS and Rust; cast each f64
    // operand down and the result back up to `.ts`'s `number` (`f64`).
    if let Some(expr) = bitwise_expr(bin, ctx) {
        return expr;
    }
    // Flavor-aware operand emit: an `i64` counter mixed with an `f64` literal
    // would be a Rust type error, so both operands emit at a common flavor.
    // ES arithmetic is infectious-f64 (one double operand → whole op `f64`);
    // `/` is always floating-point. Comparison ops match operands the same
    // way. `**`, string `+`, and bitwise already returned above.
    let combine = if matches!(bin.operator, BinaryOperator::Division) {
        NumberFlavor::F64
    } else {
        expr_flavor(&bin.left, ctx).combine(expr_flavor(&bin.right, ctx))
    };
    let left = super::translate_number_to(&bin.left, combine, ctx);
    let right = super::translate_number_to(&bin.right, combine, ctx);
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
/// to `f64` (`.ts` number). Shifts use `wrapping_shl`/`shr` (which mask the
/// count); `>>>` casts to `u32` first for the zero-fill.
///
/// The cast must go through `i64`, not directly to `i32`: Rust's `f64 as i32`
/// *saturates* (out-of-range → `i32::MAX`/`MIN`), but JS `ToInt32` *wraps*
/// (mod 2³²). A bit-vector algorithm like Myers–Levenshtein routinely lets an
/// operand grow past the i32 range (`(eq & pv) + pv` can reach ~2³²), where the
/// two diverge — saturating turns the wrong bit pattern into the result. `f64
/// as i64` is exact for finite values below 2⁵³, and `i64 as i32` then truncates
/// with the same wrap semantics as `ToInt32`. (±Inf/NaN, which `ToInt32` maps
/// to 0, are an unhandled edge here.)
pub(super) fn bitwise_expr_to(
    bin: &BinaryExpression,
    ctx: &Ctx<'_>,
    result_ty: Type,
) -> Option<Expr> {
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
    // Each operand is cast via `bitwise_operand`, which routes an `f64` through
    // `i64` (so the `i64 as i32` truncation matches JS `ToInt32` *wrap*, not
    // Rust's saturating `f64 as i32`) and lets an `i64` operand skip the hop.
    // The cast result is bound to a local first — never inlined against a
    // compound expression, where `as` would bind to its right subtree
    // (`1 << i as i64` parsing as `1 << (i as i64)`).
    let a_i32 = bitwise_operand(&bin.left, ctx, true);
    let b_i32 = bitwise_operand(&bin.right, ctx, true);
    let a_u32 = bitwise_operand(&bin.left, ctx, false);
    let b_u32 = bitwise_operand(&bin.right, ctx, false);
    Some(match bin.operator {
        BinaryOperator::BitwiseAnd => parse_quote!({
            let __a = #a_i32;
            let __b = #b_i32;
            (__a & __b) as #result_ty
        }),
        BinaryOperator::BitwiseOR => parse_quote!({
            let __a = #a_i32;
            let __b = #b_i32;
            (__a | __b) as #result_ty
        }),
        BinaryOperator::BitwiseXOR => parse_quote!({
            let __a = #a_i32;
            let __b = #b_i32;
            (__a ^ __b) as #result_ty
        }),
        // `<<`/`>>` use `wrapping_shl`/`shr` (they mask the shift count, so a
        // large `.ts` count won't panic like Rust's plain `<<` would).
        BinaryOperator::ShiftLeft => parse_quote!({
            let __a = #a_i32;
            let __b = #b_u32;
            __a.wrapping_shl(__b) as #result_ty
        }),
        BinaryOperator::ShiftRight => parse_quote!({
            let __a = #a_i32;
            let __b = #b_u32;
            __a.wrapping_shr(__b) as #result_ty
        }),
        // `>>>` is logical (zero-fill): `ToUint32` the left operand (→ u32)
        // before the shift.
        BinaryOperator::ShiftRightZeroFill => parse_quote!({
            let __a = #a_u32;
            let __b = #b_u32;
            __a.wrapping_shr(__b) as #result_ty
        }),
        _ => unreachable!(),
    })
}

/// The number-context bitwise emitter — the masked result rounds back to `f64`
/// (a `.ts` `number`). Index sites use [`bitwise_expr_to`] with `usize` to skip
/// that hop (see `member::index_expr`), which both saves a conversion per
/// access and keeps the `& mask` range visible to LLVM so the `Vec` bounds
/// check can be elided.
fn bitwise_expr(bin: &BinaryExpression, ctx: &Ctx<'_>) -> Option<Expr> {
    bitwise_expr_to(bin, ctx, parse_quote!(f64))
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

/// `null` or the `undefined` global.
fn is_nullish(expr: &Expression) -> bool {
    matches!(expr, Expression::NullLiteral(_))
        || matches!(expr, Expression::Identifier(id) if id.name.as_str() == "undefined")
}

/// `v === undefined` / `v !== null` (or the mirror sides) when `v` is an inline
/// scalar-union enum local → `matches!(v, <Enum>::Undef)` /
/// `!matches!(v, <Enum>::Null)`. The union's `Undef`/`Null` variant is the
/// runtime tag for an absent value (a plain Rust `==`/`!=` would not compile —
/// the enum has no `None`, and the `undefined`/`null` literal lowers to one).
/// Returns `None` unless the non-null side is exactly such a union local whose
/// enum has the matching variant, so anything else falls through loudly.
fn union_null_equality(bin: &BinaryExpression, ctx: &Ctx<'_>) -> Option<Expr> {
    let negate = match bin.operator {
        BinaryOperator::Equality | BinaryOperator::StrictEquality => false,
        BinaryOperator::Inequality | BinaryOperator::StrictInequality => true,
        _ => return None,
    };
    let (left_n, right_n) = (is_nullish(&bin.left), is_nullish(&bin.right));
    let (union_side, nullish) = if right_n {
        (&bin.left, &bin.right)
    } else if left_n {
        (&bin.right, &bin.left)
    } else {
        return None;
    };
    let variant_tag = match nullish {
        Expression::NullLiteral(_) => "Null",
        Expression::Identifier(i) if i.name.as_str() == "undefined" => "Undef",
        _ => return None,
    };
    let Expression::Identifier(id) = union_side else {
        return None;
    };
    let name = bindings::snake(&id.name).to_string();
    let enum_ident = ctx
        .local_type(&name)
        .and_then(|p| p.segments.last().map(|s| &s.ident))?;
    let item = ctx.registry().union_enums.get(enum_ident)?;
    if !item.variants.iter().any(|v| v.ident == variant_tag) {
        return None;
    }
    let local = bindings::snake(&id.name);
    let variant = proc_macro2::Ident::new(variant_tag, proc_macro2::Span::call_site());
    Some(if negate {
        parse_quote!(!matches!(#local, crate::#enum_ident::#variant))
    } else {
        parse_quote!(matches!(#local, crate::#enum_ident::#variant))
    })
}

/// `obj.field == value` / `!=` when `field` is an optional (`?:`) member of
/// `obj`'s struct — the field reads as `Option<T>`, so a bare `==`/`!=`
/// against a `T` would not compile. Lower to `obj.field.as_deref() ==
/// Option::Some(value.as_str())` (a string field against a string value is the
/// common case — `child.name == name`). Returns `None` unless one side is
/// exactly such a field access and the other is a string identifier, so
/// anything else falls through loudly to `cargo check`.
fn option_field_equality(bin: &BinaryExpression, ctx: &Ctx<'_>) -> Option<Expr> {
    let negate = match bin.operator {
        BinaryOperator::Equality | BinaryOperator::StrictEquality => false,
        BinaryOperator::Inequality | BinaryOperator::StrictInequality => true,
        _ => return None,
    };
    // One side is `obj.field` (a static/optional member access); the other is
    // a bare identifier value.
    let (member_side, value_side): (&Expression, &Expression) = match (&bin.left, &bin.right) {
        (l, r) if is_member_access(l) && matches!(r, Expression::Identifier(_)) => (l, r),
        (l, r) if is_member_access(r) && matches!(l, Expression::Identifier(_)) => (r, l),
        _ => return None,
    };
    // Pull `(object identifier, property name)` out of the member access.
    let (obj_id, prop) = match member_side {
        Expression::StaticMemberExpression(sm) => {
            let Expression::Identifier(id) = &sm.object else {
                return None;
            };
            (id, &sm.property)
        }
        Expression::ChainExpression(c) => match &c.expression {
            ChainElement::StaticMemberExpression(sm) => {
                let Expression::Identifier(id) = &sm.object else {
                    return None;
                };
                (id, &sm.property)
            }
            _ => return None,
        },
        _ => return None,
    };
    let obj_path = ctx.local_type(&bindings::snake(&obj_id.name).to_string())?;
    let struct_name = obj_path.segments.last()?.ident.to_string();
    if struct_name == "Option" {
        return None;
    }
    let field = bindings::snake(prop.name.as_str()).to_string();
    if !ctx.field_optional(&struct_name, &field) {
        return None;
    }
    if !operand_is_string(value_side, ctx) {
        return None;
    }
    let field_expr = translate_expr(member_side, ctx);
    let value_expr = translate_expr(value_side, ctx);
    Some(if negate {
        parse_quote!((#field_expr.as_deref() != ::std::option::Option::Some(#value_expr.as_str())))
    } else {
        parse_quote!((#field_expr.as_deref() == ::std::option::Option::Some(#value_expr.as_str())))
    })
}

/// True when `e` is a static member access (`obj.field`) or an optional-chain
/// member (`obj?.field`) — the shapes whose field may be an optional `?:`
/// struct member.
fn is_member_access(e: &Expression) -> bool {
    matches!(
        e,
        Expression::StaticMemberExpression(_) | Expression::ChainExpression(_)
    )
}

/// True when a `+` chain is string concatenation: any leaf operand is a string
/// literal. TS makes the entire chain a string concat as soon as one operand is
/// a string, so this syntactic check is sound — and the only unhandled case
/// (`stringVar + stringVar`, no literal) fails loudly under `cargo check`.
fn concat_is_string(bin: &BinaryExpression, ctx: &Ctx<'_>) -> bool {
    operand_is_string(&bin.left, ctx) || operand_is_string(&bin.right, ctx)
}

fn operand_is_string(expr: &Expression, ctx: &Ctx<'_>) -> bool {
    match expr {
        Expression::StringLiteral(_) => true,
        // A `string`-typed identifier (param/local) is string concatenation
        // too — `greeting + name` where `name: string`, not just `"lit" +`.
        Expression::Identifier(id) => {
            let name = bindings::snake(&id.name).to_string();
            ctx.local_type(&name)
                .and_then(|p| p.segments.last())
                .is_some_and(|s| s.ident == "String")
        }
        Expression::BinaryExpression(inner)
            if matches!(inner.operator, BinaryOperator::Addition) =>
        {
            concat_is_string(inner, ctx)
        }
        // `(string_expr)` — parens carry no type change.
        Expression::ParenthesizedExpression(p) => operand_is_string(&p.expression, ctx),
        // `str.method(...)` where the receiver is a string and the method
        // returns a string (repeat/toLowerCase/trim/…). Without this, `x +
        // spaces.repeat(n)` falls through to Rust's `+`, which fails on
        // `String`; the method call folds into `format!` as a `{}` leaf.
        Expression::CallExpression(c) => {
            if let Expression::StaticMemberExpression(sm) = &c.callee {
                if STRING_RETURNING_METHODS.contains(&sm.property.name.as_str())
                    && operand_is_string(&sm.object, ctx)
                {
                    return true;
                }
            }
            false
        }
        _ => false,
    }
}

/// Flatten a `+` chain to its leaf operands (left to right) and emit a single
/// `format!(…)`. String-literal leaves fold into the format string as literal
/// text; every other leaf is a `{}` placeholder — so `"a" + x + "b"` becomes
/// `format!("a{}b", x)` with no needless `.to_string()`.
fn string_concat(bin: &BinaryExpression, ctx: &Ctx<'_>) -> Expr {
    let mut leaves: Vec<&Expression> = Vec::new();
    collect_leaves(&bin.left, &mut leaves);
    collect_leaves(&bin.right, &mut leaves);
    let mut fmt = String::new();
    let mut parts: Vec<Expr> = Vec::new();
    for leaf in leaves {
        match leaf {
            Expression::StringLiteral(s) => {
                for ch in s.value.chars() {
                    fmt.push(ch);
                    if ch == '{' || ch == '}' {
                        fmt.push(ch);
                    }
                }
            }
            _ => {
                // A number leaf routes through `__ds::number_to_string` so
                // `s + 1e21` is "1e+21" and `s + -0` is "0" — Rust `Display`
                // gives the long integer form / "-0". Other leaves keep the
                // inline merge (string/bool `Display` is already ES-correct).
                if is_number_expr(leaf, ctx) {
                    let n = translate_number_to(leaf, NumberFlavor::F64, ctx);
                    fmt.push_str("{}");
                    parts.push(parse_quote!(crate::__ds::number_to_string(#n)));
                } else {
                    let e = translate_expr(leaf, ctx);
                    match fmt_merge::inline_arg(e) {
                        fmt_merge::Inlined::Format { fmt: ifmt, args } => {
                            fmt.push_str(&fmt_merge::renumber_format(&ifmt, parts.len()));
                            parts.extend(args);
                        }
                        fmt_merge::Inlined::Display(e) => {
                            fmt.push_str("{}");
                            parts.push(e);
                        }
                    }
                }
            }
        }
    }
    let fmt_lit = syn::LitStr::new(&fmt, Span::call_site());
    parse_quote!(::std::format!(#fmt_lit, #(#parts),*))
}

/// Flatten a `+` chain to its leaf operands (borrows, untranslated). A non-`+`
/// sub-expression (e.g. `a * b` inside a concat) is one leaf.
fn collect_leaves<'a>(expr: &'a Expression<'a>, leaves: &mut Vec<&'a Expression<'a>>) {
    if let Expression::BinaryExpression(bin) = expr {
        if matches!(bin.operator, BinaryOperator::Addition) {
            collect_leaves(&bin.left, leaves);
            collect_leaves(&bin.right, leaves);
            return;
        }
    }
    leaves.push(expr);
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
