//! Assignment (`x = …`, `x += …`) and update (`i++`) expressions.

use oxc_ast::ast::{
    AssignmentExpression, AssignmentTarget, Expression, SimpleAssignmentTarget, UpdateExpression,
};
use oxc_syntax::operator::{AssignmentOperator, BinaryOperator, UpdateOperator};
use quote::quote;
use syn::{parse_quote, Expr, Ident};

use super::super::bindings;
use super::super::context::Ctx;
use super::logical::assign_truthy;
use super::member::is_hashmap_local;
use super::{ident_expr, translate_expr};

/// The lvalue kind of an assignment's left-hand side. A plain target is any
/// Rust lvalue (`x`, `obj.field`, `arr[i as usize]`); a `m["k"]` on a
/// `HashMap` local is an insert (the map takes the key and value separately).
enum AssignTarget {
    Plain(Expr),
    HashInsert {
        map: Ident,
        key: Expr,
    },
    /// `xs[i] = v` on a `Vec` local — ES `Array` auto-grows on an out-of-range
    /// index, so the store routes through `__ds::array_set` (append or grow),
    /// not a bare `xs[i] = v` (which panics on a Rust `Vec`).
    ArraySet {
        obj: Expr,
        idx: Expr,
        /// The target root is a reference parameter (`c: &mut Vec` binding), so
        /// the `array_set` call reborrows it (`array_set(c, …)`) rather than
        /// taking `&mut` of an owned binding.
        is_ref: bool,
    },
}

/// `x = …`, `x += …`, …. Plain targets (`x`, `obj.field`, `arr[i as usize]`)
/// take every compound op; a `m["k"]` HashMap index becomes `m.insert(k, v)`
/// (only `=` — HashMap has no compound-assign semantics).
pub(in crate::translator) fn assignment_expr(a: &AssignmentExpression, ctx: &Ctx<'_>) -> Expr {
    let right = translate_expr(&a.right, ctx);
    match assignment_target_kind(&a.left, ctx) {
        Some(AssignTarget::Plain(target)) => {
            let tokens = match a.operator {
                // `s = s + "lit"` / `s = "lit" + s` → `s.push_str("lit")` (amortized
                // O(1) append) instead of rebuilding the whole string via
                // `format!`. Restricted to a `String`-typed local in the
                // self-plus-one-literal shape; anything else keeps the general
                // `target = right` lowering.
                AssignmentOperator::Assign => {
                    match self_plus_string_literal(&a.left, &a.right, ctx) {
                        Some(lit) => {
                            let lit_token = syn::LitStr::new(lit, proc_macro2::Span::call_site());
                            quote!(#target.push_str(#lit_token))
                        }
                        None => quote!(#target = #right),
                    }
                }
                // `s += "lit"` is string append (String has no AddAssign) → `push_str`.
                AssignmentOperator::Addition => match &a.right {
                    Expression::StringLiteral(s) => {
                        let lit =
                            syn::LitStr::new(s.value.as_str(), proc_macro2::Span::call_site());
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
                // Each operand is parenthesized before `as i64` so the cast
                // never binds into a compound right-hand side, and the `i64`
                // hop matches JS `ToInt32`/`ToUint32` wrap (not Rust's
                // saturating `f64 as i32`) — see `binary::bitwise_expr`.
                AssignmentOperator::BitwiseAnd => {
                    quote!(#target = (((#target) as i64) as i32 & ((#right) as i64) as i32) as f64)
                }
                AssignmentOperator::BitwiseOR => {
                    quote!(#target = (((#target) as i64) as i32 | ((#right) as i64) as i32) as f64)
                }
                AssignmentOperator::BitwiseXOR => {
                    quote!(#target = (((#target) as i64) as i32 ^ ((#right) as i64) as i32) as f64)
                }
                AssignmentOperator::ShiftLeft => {
                    quote!(#target = (((#target) as i64) as i32).wrapping_shl(((#right) as i64) as u32) as f64)
                }
                AssignmentOperator::ShiftRight => {
                    quote!(#target = (((#target) as i64) as i32).wrapping_shr(((#right) as i64) as u32) as f64)
                }
                AssignmentOperator::ShiftRightZeroFill => {
                    quote!(#target = (((#target) as i64) as u32).wrapping_shr(((#right) as i64) as u32) as f64)
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
        Some(AssignTarget::ArraySet { obj, idx, is_ref }) => match a.operator {
            // `xs[i] = v` → `__ds::array_set(&mut xs, i, v)` (ES auto-grow). A
            // bare `xs[i as usize] = v` would panic on a Rust `Vec` when `i` is
            // out of range; ES grows the array instead. Compound assign on an
            // array index (`xs[i] += v`) is rare and needs a read-modify-write
            // through the same helper — left as a TODO until a real fixture
            // needs it. The `__ds::array_set` token in the output flags
            // `needs_array_helper`.
            AssignmentOperator::Assign => {
                // Bind the value first so an RHS that reads the same array
                // (`arr[i] = arr[j]`) cannot collide with the `&mut` borrow
                // `array_set` takes — the immutable borrow ends at the `let`
                // before the mutable one starts. (A function-call argument's
                // `&mut` borrow activates immediately, unlike the two-phase
                // borrow a direct `arr[i] = arr[j]` gets.) A reference-parameter
                // target (`c: &mut Vec`) drops the `&mut` — `array_set(c, …)`
                // auto-reborrows the binding.
                if is_ref {
                    parse_quote!({
                        let __ds_v = #right;
                        crate::__ds::array_set(#obj, #idx, __ds_v);
                    })
                } else {
                    parse_quote!({
                        let __ds_v = #right;
                        crate::__ds::array_set(&mut #obj, #idx, __ds_v);
                    })
                }
            }
            _ => parse_quote!(::core::todo!()),
        },
        None => parse_quote!(::core::todo!()),
    }
}

/// `i++` / `i--` → `i += 1_f64` / `i -= 1_f64`. Statement-context only: TS returns
/// the old value, which we don't preserve — fine for `i++;` but not `return i++`.
/// The step is `1_f64` because `.ds` `number` is `f64`; an integer step would be a
/// type error against an `f64` target.
pub(super) fn update_expr(u: &UpdateExpression, ctx: &Ctx<'_>) -> Expr {
    let Some(target) = simple_target(&u.argument, ctx) else {
        return parse_quote!(::core::todo!());
    };
    let tokens = match u.operator {
        UpdateOperator::Increment => quote!(#target += 1_f64),
        UpdateOperator::Decrement => quote!(#target -= 1_f64),
    };
    syn::parse2(tokens).unwrap_or_else(|_| parse_quote!(::core::todo!()))
}

/// Resolve an assignment's left-hand side to an [`AssignTarget`]. Member
/// targets (`obj.field`, `arr[i]`) become plain Rust lvalues; a `m["k"]` on a
/// `HashMap` local is recognized as an insert.
fn assignment_target_kind(target: &AssignmentTarget, ctx: &Ctx<'_>) -> Option<AssignTarget> {
    match target {
        AssignmentTarget::AssignmentTargetIdentifier(id) => {
            Some(AssignTarget::Plain(ident_expr(id, ctx)))
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
            // `xs[i] = v` → ES `Array` auto-grow via `__ds::array_set` (a bare
            // `xs[i as usize] = v` would panic on a Rust `Vec` when `i` is out
            // of range; ES grows the array instead). The `__ds::array_set` token
            // in the output flags `needs_array_helper`.
            let obj = translate_expr(&cm.object, ctx);
            let idx = translate_expr(&cm.expression, ctx);
            let is_ref = is_ref_param_target(&cm.object, ctx);
            Some(AssignTarget::ArraySet { obj, idx, is_ref })
        }
        _ => None,
    }
}

fn simple_target(target: &SimpleAssignmentTarget, ctx: &Ctx<'_>) -> Option<Expr> {
    match target {
        SimpleAssignmentTarget::AssignmentTargetIdentifier(id) => Some(ident_expr(id, ctx)),
        _ => None,
    }
}

/// Whether a computed-member assignment target's root is a reference parameter
/// of the current function (`c` in `c[i] = v` where `c: &mut Vec`), so the
/// `array_set` call reborrows the binding instead of taking `&mut` of an owned
/// value. Only a bare-identifier root matches; a nested/indirect target keeps
/// the owned path.
fn is_ref_param_target(object: &Expression, ctx: &Ctx<'_>) -> bool {
    let Expression::Identifier(id) = object else {
        return false;
    };
    let name = ctx.names().of_reference(id).to_string();
    ctx.is_ref_param(&name)
}

/// `s = s + "lit"` / `s = "lit" + s` lowers to `s.push_str("lit")` — an
/// amortized-O(1) in-place append — instead of rebuilding the string via
/// `format!`. Only the self-plus-one-string-literal shape on a `String`-typed
/// local qualifies; any other RHS (two variables, a chain, a non-literal) keeps
/// the general `target = right` lowering.
fn self_plus_string_literal<'a>(
    left: &AssignmentTarget,
    right: &'a Expression,
    ctx: &Ctx<'_>,
) -> Option<&'a str> {
    let AssignmentTarget::AssignmentTargetIdentifier(id) = left else {
        return None;
    };
    let name = id.name.as_str();
    // `push_str` needs a mutable owned `String`; restrict to `String`-typed
    // locals so a `&str` param keeps the familiar assignment error instead of a
    // confusing `push_str not found` one.
    let rust_name = bindings::snake(name).to_string();
    let is_string = ctx
        .local_type(&rust_name)
        .and_then(|p| p.segments.last())
        .is_some_and(|s| s.ident == "String");
    if !is_string {
        return None;
    }
    let bin = match right {
        Expression::BinaryExpression(b) if matches!(b.operator, BinaryOperator::Addition) => b,
        _ => return None,
    };
    let left_is_self = matches!(&bin.left, Expression::Identifier(i) if i.name.as_str() == name);
    let right_is_self = matches!(&bin.right, Expression::Identifier(i) if i.name.as_str() == name);
    match (left_is_self, right_is_self) {
        (true, false) => match &bin.right {
            Expression::StringLiteral(s) => Some(s.value.as_str()),
            _ => None,
        },
        (false, true) => match &bin.left {
            Expression::StringLiteral(s) => Some(s.value.as_str()),
            _ => None,
        },
        _ => None,
    }
}
