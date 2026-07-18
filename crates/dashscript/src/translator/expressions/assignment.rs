//! Assignment (`x = …`, `x += …`) and update (`i++`) expressions.

use oxc_ast::ast::{
    AssignmentExpression, AssignmentTarget, Expression, SimpleAssignmentTarget, UpdateExpression,
};
use oxc_syntax::operator::{AssignmentOperator, UpdateOperator};
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
    HashInsert { map: Ident, key: Expr },
}

/// `x = …`, `x += …`, …. Plain targets (`x`, `obj.field`, `arr[i as usize]`)
/// take every compound op; a `m["k"]` HashMap index becomes `m.insert(k, v)`
/// (only `=` — HashMap has no compound-assign semantics).
pub(in crate::translator) fn assignment_expr(a: &AssignmentExpression, ctx: &Ctx<'_>) -> Expr {
    let right = translate_expr(&a.right, ctx);
    match assignment_target_kind(&a.left, ctx) {
        Some(AssignTarget::Plain(target)) => {
            let tokens = match a.operator {
                AssignmentOperator::Assign => quote!(#target = #right),
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

/// `i++` / `i--` → `i += 1_f64` / `i -= 1_f64`. Statement-context only: TS returns
/// the old value, which we don't preserve — fine for `i++;` but not `return i++`.
/// The step is `1_f64` because `.ds` `number` is `f64`; an integer step would be a
/// type error against an `f64` target.
pub(super) fn update_expr(u: &UpdateExpression) -> Expr {
    let Some(target) = simple_target(&u.argument) else {
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
