//! Binary operators: arithmetic, comparison, bitwise, `in`, and `+` string concat.

use oxc_ast::ast::{BinaryExpression, Expression};
use oxc_syntax::operator::BinaryOperator;
use proc_macro2::Span;
use syn::{parse_quote, parse_str, BinOp, Expr};

use super::super::bindings;
use super::super::context::Ctx;
use super::super::flavor::{expr_flavor, NumberFlavor};
use super::fmt_merge;
use super::option_local_name;
use super::translate_expr;

/// Binary ops. TS `==`/`===` collapse to Rust `==` (Rust has no coercive `==`);
/// likewise `!=`/`!==`. `**`, bitwise, shifts, `in`, `instanceof` are unmapped.
///
/// A `+` chain that contains a string literal is TS string concatenation and is
/// mapped to `format!` — Rust's `+` does not apply to `String`.
///
/// We build `syn::Expr::Binary` directly (not `quote!` tokens) so `prettyplease`
/// adds parentheses by precedence instead of emitting a redundant pair around
/// every sub-expression.
pub(super) fn binary_expr(bin: &BinaryExpression, ctx: &Ctx<'_>) -> Expr {
    // `x === null` / `x !== null` → `x.is_none()` / `x.is_some()` when `x` is an
    // Option-typed local; any other comparison returns `None` and falls through.
    if let Some(expr) = null_equality(bin, ctx) {
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
    // operand down and the result back up to `.ds`'s `number` (`f64`).
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
/// to `f64` (`.ds` number). Shifts use `wrapping_shl`/`shr` (which mask the
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
        // Each operand is bound to a local first, then `as i64 as i32` (or
        // `as u32` for `>>>`'s left) is applied to that local — never inline
        // against a compound expression, where `as` would bind to its right
        // subtree (`1 << i as i64` parsing as `1 << (i as i64)`). The `i64` hop
        // matches JS `ToInt32`/`ToUint32` *wrap*, not Rust's saturating
        // `f64 as i32`; see the doc comment above.
        BinaryOperator::BitwiseAnd => parse_quote!({
            let __a = (#left) as i64 as i32;
            let __b = (#right) as i64 as i32;
            (__a & __b) as f64
        }),
        BinaryOperator::BitwiseOR => parse_quote!({
            let __a = (#left) as i64 as i32;
            let __b = (#right) as i64 as i32;
            (__a | __b) as f64
        }),
        BinaryOperator::BitwiseXOR => parse_quote!({
            let __a = (#left) as i64 as i32;
            let __b = (#right) as i64 as i32;
            (__a ^ __b) as f64
        }),
        // `<<`/`>>` use `wrapping_shl`/`shr` (they mask the shift count, so a
        // large `.ds` count won't panic like Rust's plain `<<` would).
        BinaryOperator::ShiftLeft => parse_quote!({
            let __a = (#left) as i64 as i32;
            let __b = (#right) as i64 as u32;
            __a.wrapping_shl(__b) as f64
        }),
        BinaryOperator::ShiftRight => parse_quote!({
            let __a = (#left) as i64 as i32;
            let __b = (#right) as i64 as u32;
            __a.wrapping_shr(__b) as f64
        }),
        // `>>>` is logical (zero-fill): `ToUint32` the left operand (i64 → u32)
        // before the shift.
        BinaryOperator::ShiftRightZeroFill => parse_quote!({
            let __a = (#left) as i64 as u32;
            let __b = (#right) as i64 as u32;
            __a.wrapping_shr(__b) as f64
        }),
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

/// `null` or the `undefined` global.
fn is_nullish(expr: &Expression) -> bool {
    matches!(expr, Expression::NullLiteral(_))
        || matches!(expr, Expression::Identifier(id) if id.name.as_str() == "undefined")
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
