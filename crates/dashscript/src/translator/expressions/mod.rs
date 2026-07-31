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
pub(in crate::translator) mod call;
mod fmt_merge;
pub(in crate::translator) mod literals;
mod logical;
mod member;
mod new;
mod object;
mod unary;

// Re-exports only for callers outside this module's dispatch: `builtins` reads
// `bool_expr`/`string_expr` via `super::super::expressions::…`, and `functions`
// reads `array_slice_expr`, and `new` reads `array_owned_expr`. Sibling
// families use fully-qualified paths (`super::logical::assign_truthy`)
// instead, so they need no re-export.
pub(in crate::translator) use array::{array_owned_expr, array_slice_expr};
pub(in crate::translator) use assignment::assignment_expr;
pub(in crate::translator) use literals::{bool_expr, string_expr};
pub(in crate::translator) use member::{
    is_hashmap_local, is_hashset_local, is_vec_u8_local, option_unwrap_object,
};
pub(in crate::translator) use unary::typeof_operand_is_runtime;

use oxc_ast::ast::{
    Argument, ArrowFunctionExpression, Expression, FunctionBody, IdentifierReference,
    SequenceExpression, Statement, TemplateLiteral,
};
use proc_macro2::Span;
use syn::{parse_quote, Expr, Ident, Pat, Stmt, Type};

use super::context::{is_option_path, Ctx, Narrow};
use super::{bindings, types};

/// Wrap `expr` in `Some(..)` when the target type is `Option<T>` but the value
/// is a plain `T` — `.ts` implicitly widens `T` to `T | undefined` at a return
/// or argument boundary, but Rust needs an explicit `Some`. Only a bare
/// identifier qualifies, and only when its known type is a plain `T` or is
/// unregistered (e.g. a `for`-of loop variable whose iterable was not an inline
/// array literal, so its type was never recorded): both are non-`Option`
/// values. An identifier already typed `Option<T>`, or any non-identifier
/// expression (a field access, call, or literal), keeps its own spelling —
/// `cargo check` backstops the rest so a wrong wrap fails loudly, not silently.
pub(in crate::translator) fn implicit_some(
    arg: &Expression,
    expr: Expr,
    target_ty: Option<&Type>,
    ctx: &Ctx<'_>,
) -> Expr {
    let Some(ty) = target_ty else {
        return expr;
    };
    let Type::Path(tp) = ty else {
        return expr;
    };
    if !is_option_path(&tp.path) {
        return expr;
    }
    if let Expression::Identifier(id) = arg {
        // `undefined` / `null` lower to `None` (already an `Option` value), so
        // wrapping would produce `Some(None)`; anything else whose type is not
        // a known `Option<T>` is a plain `T` → wrap.
        let is_none_literal = matches!(id.name.as_str(), "undefined" | "null");
        if !is_none_literal && !ctx.is_option(&bindings::snake(&id.name).to_string()) {
            return parse_quote!(::std::option::Option::Some(#expr));
        }
    }
    expr
}

/// Translate `expr` and cast it to number-flavor `to`. A numeric literal is
/// re-emitted at the target flavor (no cast); any other expression is
/// translated then cast if its own flavor differs. Used at arithmetic /
/// comparison operand sites so an `i64` counter and an `f64` literal meet at
/// one type.
pub(in crate::translator) fn translate_number_to(
    expr: &Expression,
    to: super::flavor::NumberFlavor,
    ctx: &Ctx<'_>,
) -> Expr {
    use super::flavor::{expr_flavor, NumberFlavor};
    // A bare numeric literal — or its negation (`-1000`) — is re-emitted at the
    // target flavor directly. Without this, a negated literal at `i64` would
    // slip through the generic cast path: the unary emitter is f64-only
    // (`-1000_f64`), but `expr_flavor` reports `I64`, so the `(I64, I64)` arm
    // returns that f64 emit verbatim and `let i: i64 = -1000_f64` mismatches.
    if let Some(v) = literal_value(expr) {
        return match to {
            NumberFlavor::I64 => literals::numeric_expr_i64(v),
            NumberFlavor::F64 => literals::numeric_expr(v),
        };
    }
    let e = translate_expr(expr, ctx);
    match (expr_flavor(expr, ctx), to) {
        (NumberFlavor::F64, NumberFlavor::I64) => cast_as(e, parse_quote!(i64)),
        (NumberFlavor::I64, NumberFlavor::F64) => cast_as(e, parse_quote!(f64)),
        _ => e,
    }
}

/// Cast `e` to `ty`, parenthesizing a compound operand. `as` (precedence 7)
/// outranks arithmetic (9) but does not bind tightly enough to wrap a binary
/// expression on its left, so `(i - j) as f64` is required — a bare
/// `i - j as f64` parses as `i - (j as f64)` (a type mismatch). A simple
/// operand (path/literal/call/…) needs no parens, so call/assign sites stay
/// free of `unused_parens`.
fn cast_as(e: Expr, ty: Type) -> Expr {
    let simple = matches!(
        &e,
        Expr::Path(_)
            | Expr::Lit(_)
            | Expr::Paren(_)
            | Expr::Call(_)
            | Expr::MethodCall(_)
            | Expr::Field(_)
            | Expr::Index(_)
            | Expr::Tuple(_)
            | Expr::Cast(_)
    );
    if simple {
        parse_quote!(#e as #ty)
    } else {
        parse_quote!((#e) as #ty)
    }
}

/// Cast a bitwise-operator operand to `i32` (signed) or `u32` (unsigned),
/// matching ES `ToInt32`/`ToUint32` (mod-2³² wrap). The cast is *not* a plain
/// `f64 as i32`: Rust saturates out-of-range floats to `i32::MAX`/`MIN`, but
/// `ToInt32` wraps (mod 2³²). An `f64` operand goes through `i64` first —
/// `f64 as i64` is exact below 2⁵³, then `i64 as i32` truncates with the same
/// wrap semantics as `ToInt32` (verified against ECMA-262: `i64 as i32` is the
/// low-32-bits-as-signed, which is step 4–5 of the abstract operation). An
/// `i64` operand skips the hop — `i64 as i32` already wraps. A numeric literal
/// re-emits at its own flavor so `1 << i` binds `1_i64`, not `1_f64`.
pub(in crate::translator) fn bitwise_operand(e: &Expression, ctx: &Ctx<'_>, signed: bool) -> Expr {
    use super::flavor::{expr_flavor, NumberFlavor};
    let flavor = expr_flavor(e, ctx);
    let base = if is_number_expr(e, ctx) {
        translate_number_to(e, flavor, ctx)
    } else {
        translate_expr(e, ctx)
    };
    let target: Type = if signed {
        parse_quote!(i32)
    } else {
        parse_quote!(u32)
    };
    match flavor {
        // i64 → i32/u32: a single truncation, same mod-2³² wrap as ToInt32.
        NumberFlavor::I64 => cast_as(base, target),
        // f64 → i64 → i32/u32: the i64 hop is load-bearing (f64 as i32 saturates).
        NumberFlavor::F64 => {
            let via_i64 = cast_as(base, parse_quote!(i64));
            cast_as(via_i64, target)
        }
    }
}

/// The numeric value of a literal expression: a `NumericLiteral`, or its
/// negation (`-1000` → `-1000.0`). `None` for anything else — the generic cast
/// path handles it. `-0` is reported as `-0.0`; callers reaching `i64` for it
/// would be a flavor-inference bug (a `-0` binding is forced `f64`), caught by
/// the conformance gate.
fn literal_value(expr: &Expression) -> Option<f64> {
    use oxc_syntax::operator::UnaryOperator;
    match expr {
        Expression::NumericLiteral(n) => Some(n.value),
        Expression::UnaryExpression(u)
            if matches!(
                u.operator,
                UnaryOperator::UnaryNegation | UnaryOperator::UnaryPlus
            ) =>
        {
            if let Expression::NumericLiteral(n) = &u.argument {
                Some(if u.operator == UnaryOperator::UnaryNegation {
                    -n.value
                } else {
                    n.value
                })
            } else {
                None
            }
        }
        Expression::ParenthesizedExpression(p) => literal_value(&p.expression),
        _ => None,
    }
}

/// Translate a condition operand (`if (cond)`, `while (cond)`, `cond ? a : b`)
/// to a Rust `bool`. A bare collection/`Option` identifier lowers to an
/// emptiness/`is_some` check (ES truthiness); `!x` flips to the negated form;
/// anything else translates as a plain boolean expression. Shared by statement
/// control flow (`functions::control_flow`) and the ternary (`unary`) so both
/// apply the same truthiness rule.
pub(in crate::translator) fn condition_expr(test: &Expression, ctx: &Ctx<'_>) -> Expr {
    use oxc_syntax::operator::UnaryOperator;
    if let Some(e) = truthiness(test, false, ctx) {
        return e;
    }
    if let Expression::UnaryExpression(un) = test {
        if matches!(un.operator, UnaryOperator::LogicalNot) {
            // `!x` → `!(condition(x))` — recurse so `!opts.indent` lowers to
            // `!__ds::truthy(&opts.indent)` rather than E0600 (`!` on a String).
            // The negated truthiness fast path (a bare identifier of known type)
            // is tried first; falling through recurses so a member access inside
            // `!` still gets `__ds::truthy`.
            if let Some(e) = truthiness(&un.argument, true, ctx) {
                return e;
            }
            let inner = condition_expr(&un.argument, ctx);
            return parse_quote!(!(#inner));
        }
    }
    if needs_truthy_wrapper(test) {
        let e = translate_expr(test, ctx);
        return parse_quote!(crate::__ds::truthy(&(#e)));
    }
    translate_expr(test, ctx)
}

/// Whether `test` needs an `__ds::truthy(&expr)` wrapper in condition position.
/// Returns `true` for anything not obviously a Rust `bool`: a bare identifier
/// of unknown type (an `Element` value), a member access, a call, a cast, an
/// arithmetic binary, logical `&&`/`||` (which lower to ES value semantics, not
/// `bool`). Returns `false` only for what is already `bool` — a comparison
/// operator (`==`/`!=`/`===`/`!==`/`<`/`<=`/`>`/`>=`) or a boolean literal.
/// The Rust compiler then picks the `DsTruthy` impl by inferred type, so the
/// translator needs no type inference (the root cause of the
/// `let opts = normalize_options(…)` case — `opts` is `_`-typed to the translator).
fn needs_truthy_wrapper(test: &Expression) -> bool {
    use oxc_syntax::operator::BinaryOperator;
    match test {
        Expression::BinaryExpression(bin) => !matches!(
            bin.operator,
            BinaryOperator::Equality
                | BinaryOperator::Inequality
                | BinaryOperator::StrictEquality
                | BinaryOperator::StrictInequality
                | BinaryOperator::LessThan
                | BinaryOperator::LessEqualThan
                | BinaryOperator::GreaterThan
                | BinaryOperator::GreaterEqualThan
        ),
        Expression::BooleanLiteral(_) => false,
        _ => true,
    }
}

/// The Rust boolean form of an ES truthiness test. A numeric literal folds to
/// its compile-time truthiness (nonzero and non-NaN); a bare identifier of a
/// number (`f64`/integer), collection (`Vec`/`String`), or `Option` type maps
/// to the matching runtime check. `negated` selects the falsy side (`== 0`/
/// `is_nan`/`is_empty`/`is_none`) vs the truthy side. Anything else returns
/// `None` (the caller treats the expression as already boolean).
fn truthiness(expr: &Expression, negated: bool, ctx: &Ctx<'_>) -> Option<Expr> {
    // A numeric literal's ES truthiness is known at translate time, so a
    // `while (1)` / `do { … } while (0)` folds to a Rust `bool` literal
    // instead of emitting `!(1_f64)` (E0600: `!` on f64).
    if let Expression::NumericLiteral(n) = expr {
        let v = n.value;
        let truthy = v != 0.0 && !v.is_nan();
        let b = if negated { !truthy } else { truthy };
        return Some(if b {
            parse_quote!(true)
        } else {
            parse_quote!(false)
        });
    }
    let Expression::Identifier(id) = expr else {
        return None;
    };
    // A delayed-binding mutable global's accessor returns `Option<T>` (it is
    // `RefCell<Option<T>>` seeded `None`), so its ES truthiness is presence:
    // `if (x)` → `x().is_some()`, `if (!x)` → `x().is_none()`. It is not a
    // local, so the `local_type` path below does not reach it.
    if ctx.names().is_optional_mutable_static(id) {
        let getter = ctx.names().of_reference(id);
        return Some(if negated {
            parse_quote!(#getter().is_none())
        } else {
            parse_quote!(#getter().is_some())
        });
    }
    let ident = ctx.names().of_reference(id);
    let last = ctx
        .local_type(&ident.to_string())?
        .segments
        .last()?
        .ident
        .to_string();
    match last.as_str() {
        // ES `Boolean(f64)`: nonzero and non-NaN. NaN is falsy (`!NaN === true`),
        // so the negated form ORs the two falsy cases.
        "f64" => Some(if negated {
            parse_quote!(#ident == 0.0 || #ident.is_nan())
        } else {
            parse_quote!(#ident != 0.0 && !#ident.is_nan())
        }),
        // Integer scalars have no NaN; truthiness is simply != 0.
        "i64" | "i32" | "usize" | "u64" | "u32" | "u16" | "u8" | "i16" | "i8" => Some(if negated {
            parse_quote!(#ident == 0)
        } else {
            parse_quote!(#ident != 0)
        }),
        "Vec" | "String" | "HashMap" | "HashSet" => Some(if negated {
            parse_quote!(#ident.is_empty())
        } else {
            parse_quote!(!#ident.is_empty())
        }),
        "Option" => Some(if negated {
            parse_quote!(#ident.is_none())
        } else {
            parse_quote!(#ident.is_some())
        }),
        // A bare `bool` is already a Rust `bool` — no conversion. The caller
        // uses the expr as-is (`if b`, `b && c`) or applies `!` itself (`!b`),
        // both of which typecheck without help.
        "bool" => None,
        // Any other identifier type (a user struct/enum, a union, an opaque
        // type) is an ES object — always truthy. This avoids a `DsTruthy` impl
        // per user type and matches ES: only the falsy primitives
        // (`0`/`NaN`/`""`/`null`/`undefined`/`false`) are falsy; objects are
        // always truthy.
        _ => Some(if negated {
            parse_quote!(false)
        } else {
            parse_quote!(true)
        }),
    }
}

/// Translate an expression to its `syn::Expr` form.
///
/// Every `Expression` variant is matched explicitly (no `_` wildcard): a future
/// oxc variant lands as a `cargo check` error here rather than silently emitting
/// `todo!()`. Mapped variants lower to Rust; unmapped ones (JSX, `super`,
/// dynamic `import()`, `await`/`yield`, bigints, function/class/sequence
/// expressions) lower to a `todo!()` placeholder that `check` flags as
/// unsupported before emit.
pub fn translate_expr(expr: &Expression, ctx: &Ctx<'_>) -> Expr {
    match expr {
        // Literals & identifiers.
        Expression::StringLiteral(s) => literals::string_expr(s),
        Expression::NumericLiteral(n) => literals::numeric_expr(n.value),
        Expression::BooleanLiteral(b) => literals::bool_expr(b.value),
        Expression::NullLiteral(_) => parse_quote!(None),
        Expression::RegExpLiteral(re) => regex_literal_expr(re),
        Expression::Identifier(id) => ident_or_undefined(id, ctx),
        // Compound expressions.
        Expression::CallExpression(call) => call::translate_call(call, ctx),
        Expression::ArrayExpression(arr) => array::array_expr(arr, ctx),
        Expression::ObjectExpression(obj) => object::object_expr(obj, None, ctx),
        Expression::TemplateLiteral(t) => template_expr(t, ctx),
        // Member access (the three `MemberExpression` variants oxc flattens in).
        Expression::StaticMemberExpression(sm) => member::member_expr(sm, ctx),
        Expression::ComputedMemberExpression(cm) => member::computed_member(cm, ctx),
        Expression::PrivateFieldExpression(_) => unsupported_expr(),
        // Operators.
        Expression::BinaryExpression(bin) => binary::binary_expr(bin, ctx),
        Expression::LogicalExpression(log) => logical::logical_expr(log, ctx),
        Expression::ConditionalExpression(c) => unary::conditional_expr(c, ctx),
        Expression::UnaryExpression(un) => unary::unary_expr(un, ctx),
        Expression::AssignmentExpression(a) => assignment::assignment_expr(a, ctx),
        Expression::UpdateExpression(u) => assignment::update_expr(u, ctx),
        // TS type-layer constructs with no runtime effect — passthrough.
        Expression::TSNonNullExpression(nn) => unary::nonnull_expr(nn, ctx),
        Expression::TSAsExpression(a) => translate_expr(&a.expression, ctx),
        Expression::TSSatisfiesExpression(s) => translate_expr(&s.expression, ctx),
        Expression::TSTypeAssertion(t) => translate_expr(&t.expression, ctx),
        Expression::TSInstantiationExpression(i) => translate_expr(&i.expression, ctx),
        // Functions, sequences, references.
        Expression::ArrowFunctionExpression(arrow) => arrow_expr(arrow, ctx, false),
        Expression::SequenceExpression(s) => sequence_expr(s, ctx),
        // User-written parens are unwrapped; `prettyplease` re-adds any needed
        // for precedence (e.g. `(a + b) * c` round-trips correctly).
        Expression::ParenthesizedExpression(p) => translate_expr(&p.expression, ctx),
        Expression::ChainExpression(c) => member::chain_expr(&c.expression, ctx),
        // `this` inside a class method → the receiver (`self`/`__ds_self`);
        // outside a method → a `compile_error!`.
        Expression::ThisExpression(_) => super::context::this_expr(ctx),
        Expression::NewExpression(n) => new::new_expr(n, ctx),
        // Unsupported ES/TS constructs. `check` flags these before emit; these
        // explicit arms (vs a `_` wildcard) keep dispatch exhaustive so a
        // future oxc variant lands as a `cargo check` error, never silently.
        Expression::Super(_)
        | Expression::ImportMeta(_)
        | Expression::NewTarget(_)
        | Expression::ImportExpression(_)
        | Expression::AwaitExpression(_)
        | Expression::YieldExpression(_)
        | Expression::PrivateInExpression(_)
        | Expression::JSXElement(_)
        | Expression::JSXFragment(_)
        | Expression::V8IntrinsicExpression(_)
        | Expression::BigIntLiteral(_)
        | Expression::FunctionExpression(_)
        | Expression::ClassExpression(_)
        | Expression::TaggedTemplateExpression(_) => unsupported_expr(),
    }
}

/// `a, b, c` → `{ a; b; c }` — a Rust block expression: each but the last is a
/// statement, the last is the block's value, matching ES left-to-right
/// evaluation and the sequence's value being the final expression.
fn sequence_expr(s: &SequenceExpression, ctx: &Ctx<'_>) -> Expr {
    let last = s.expressions.len() - 1;
    let head: Vec<Stmt> = s.expressions[..last]
        .iter()
        .map(|e| {
            let x = translate_expr(e, ctx);
            parse_quote!(#x;)
        })
        .collect();
    let tail = translate_expr(&s.expressions[last], ctx);
    parse_quote!({ #(#head)* #tail })
}

/// A `todo!()` placeholder for an unmapped expression kind. `check` flags these
/// as `unsupported` before emit, so reaching one at runtime is a translator/
/// check mismatch (loud, not silent). Exists to keep [`translate_expr`]
/// exhaustive without a `_` wildcard.
fn unsupported_expr() -> Expr {
    parse_quote!(::core::todo!())
}

/// `/pattern/flags` → a compiled `regress::Regex` via `__ds::regex`. The
/// `regress` crate implements ES regex semantics (backreferences, lookaround,
/// unicode) the `regex` crate cannot express; oxc parses the literal upfront,
/// so an invalid pattern never reaches runtime. Flags are reconstructed as an
/// ES flag string ("gimsuydv") from oxc's bitflag set.
/// `(pattern, flags)` literals for an ES RegExp literal — shared by the
/// literal lowering (`__ds::regex`) and the string-method lowering
/// (`String.prototype.match` → `__ds::regex_match`). Flags are reconstructed
/// as an ES flag string ("gimsuydv") from oxc's bitflag set.
pub(in crate::translator) fn regex_lit_parts(
    re: &oxc_ast::ast::RegExpLiteral,
) -> (syn::LitStr, syn::LitStr) {
    use oxc_ast::ast::RegExpFlags;
    let f = re.regex.flags;
    let mut flags = String::new();
    if f.contains(RegExpFlags::G) {
        flags.push('g');
    }
    if f.contains(RegExpFlags::I) {
        flags.push('i');
    }
    if f.contains(RegExpFlags::M) {
        flags.push('m');
    }
    if f.contains(RegExpFlags::S) {
        flags.push('s');
    }
    if f.contains(RegExpFlags::U) {
        flags.push('u');
    }
    if f.contains(RegExpFlags::Y) {
        flags.push('y');
    }
    if f.contains(RegExpFlags::D) {
        flags.push('d');
    }
    if f.contains(RegExpFlags::V) {
        flags.push('v');
    }
    let pat = syn::LitStr::new(re.regex.pattern.text.as_str(), Span::call_site());
    let fl = syn::LitStr::new(&flags, Span::call_site());
    (pat, fl)
}

/// `/pat/gi.flags` / `.source` / `.global` / `.ignoreCase` / `.multiline` /
/// `.dotAll` / `.unicode` / `.unicodeSets` / `.sticky` / `.hasIndices` on a
/// regex literal — the property is fully known at translate time (oxc parsed
/// the literal), so it lowers to a bare literal, not a runtime `Regex` field.
/// `.source` follows ES's empty-pattern rule (`"(?:)"`); `.unicode` is true
/// under either the `u` or `v` flag. Returns `None` for any other name.
pub(in crate::translator) fn regex_literal_property(
    re: &oxc_ast::ast::RegExpLiteral,
    name: &str,
) -> Option<Expr> {
    use oxc_ast::ast::RegExpFlags;
    let f = re.regex.flags;
    let bool_expr = |set: bool| -> Expr {
        if set {
            parse_quote!(true)
        } else {
            parse_quote!(false)
        }
    };
    match name {
        "global" => Some(bool_expr(f.contains(RegExpFlags::G))),
        "ignoreCase" => Some(bool_expr(f.contains(RegExpFlags::I))),
        "multiline" => Some(bool_expr(f.contains(RegExpFlags::M))),
        "dotAll" => Some(bool_expr(f.contains(RegExpFlags::S))),
        "unicode" => Some(bool_expr(
            f.contains(RegExpFlags::U) || f.contains(RegExpFlags::V),
        )),
        "unicodeSets" => Some(bool_expr(f.contains(RegExpFlags::V))),
        "sticky" => Some(bool_expr(f.contains(RegExpFlags::Y))),
        "hasIndices" => Some(bool_expr(f.contains(RegExpFlags::D))),
        "flags" => {
            let (_, fl) = regex_lit_parts(re);
            Some(parse_quote!(#fl))
        }
        "source" => {
            let pat = re.regex.pattern.text.as_str();
            let src = if pat.is_empty() { "(?:)" } else { pat };
            let lit = syn::LitStr::new(src, Span::call_site());
            Some(parse_quote!(#lit))
        }
        _ => None,
    }
}

fn regex_literal_expr(re: &oxc_ast::ast::RegExpLiteral) -> Expr {
    let (pat, fl) = regex_lit_parts(re);
    parse_quote!(crate::__ds::regex(#pat, #fl))
}

/// Translate a call argument — [`Argument`] inherits the `Expression` variants.
pub fn translate_argument(arg: &Argument, ctx: &Ctx<'_>) -> Expr {
    match arg {
        Argument::StringLiteral(s) => literals::string_expr(s),
        Argument::NumericLiteral(n) => literals::numeric_expr(n.value),
        Argument::BooleanLiteral(b) => literals::bool_expr(b.value),
        Argument::NullLiteral(_) => parse_quote!(None),
        Argument::Identifier(id) => ident_or_undefined(id, ctx),
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
        Argument::RegExpLiteral(re) => regex_literal_expr(re),
        // An anonymous object literal argument lowers to a `HashMap` (no
        // parameter type hint at a call site) — same as an unannotated object
        // binding. Fixes `Object.assign(target, { a: 2 })`, where the source
        // previously fell through to `todo!()`.
        Argument::ObjectExpression(obj) => object::object_expr(obj, None, ctx),
        Argument::AssignmentExpression(a) => assignment::assignment_expr(a, ctx),
        Argument::UpdateExpression(u) => assignment::update_expr(u, ctx),
        Argument::ChainExpression(c) => member::chain_expr(&c.expression, ctx),
        // TS type-layer constructs with no runtime effect — passthrough.
        Argument::TSSatisfiesExpression(s) => translate_expr(&s.expression, ctx),
        Argument::TSInstantiationExpression(i) => translate_expr(&i.expression, ctx),
        Argument::SequenceExpression(s) => sequence_expr(s, ctx),
        // Unsupported ES/TS constructs — explicit arms keep dispatch exhaustive
        // (no `_` wildcard), so a future oxc variant lands as a `cargo check`
        // error rather than silent `todo!()`.
        Argument::Super(_)
        | Argument::ImportMeta(_)
        | Argument::NewTarget(_)
        | Argument::ImportExpression(_)
        | Argument::AwaitExpression(_)
        | Argument::YieldExpression(_)
        | Argument::PrivateInExpression(_)
        | Argument::PrivateFieldExpression(_)
        | Argument::JSXElement(_)
        | Argument::JSXFragment(_)
        | Argument::V8IntrinsicExpression(_)
        | Argument::BigIntLiteral(_)
        | Argument::FunctionExpression(_)
        | Argument::ClassExpression(_)
        | Argument::TaggedTemplateExpression(_)
        | Argument::SpreadElement(_) => unsupported_expr(),
    }
}

/// Translate a call argument; an object literal borrows its struct name from
/// the callee's declared parameter type (when known). An optional-field read
/// whose parameter type is the field's inner type unwraps
/// (`f(obj.opt_field)` → `f(obj.opt_field.as_ref().unwrap().clone())`). Other
/// arguments fall through to [`translate_argument`].
pub fn translate_argument_init(arg: &Argument, hint: Option<&Type>, ctx: &Ctx<'_>) -> Expr {
    if let Argument::ObjectExpression(obj) = arg {
        return object::object_expr(obj, hint, ctx);
    }
    if let Some(expr) = arg.as_expression() {
        if let Some(unwrapped) = unwrap_optional_field_read(expr, hint, ctx) {
            return unwrapped;
        }
    }
    translate_argument(arg, ctx)
}

/// Box a value into the matching variant of a union enum (a return type or a
/// `let`/field's declared union). A scalar literal maps to its conventional
/// variant — a string to `Str(..)`, a number to `Num(..)`, a boolean to
/// `Bool(..)`, `undefined` to `Undef`, `null` to `Null` — but only when the
/// enum actually has that variant (a named mixed union may spell them
/// differently). A bare variable maps to the arm whose inner type matches the
/// variable's declared type. Falls back to translating the value as-is when no
/// arm matches, so cargo check surfaces the gap. The union analogue of
/// [`object::box_union_value`] (a HashMap value literal), generalized to read
/// the enum's actual variants so a named mixed union boxes to the right arm.
fn box_to_union(value: &Expression, union_ident: &Ident, ctx: &Ctx<'_>) -> Expr {
    let Some(item) = ctx.registry().union_enums.get(union_ident) else {
        return translate_expr(value, ctx);
    };
    let has = |name: &str| item.variants.iter().any(|v| v.ident == name);
    match value {
        Expression::StringLiteral(s) if has("Str") => {
            let v = literals::string_expr(s);
            parse_quote!(crate::#union_ident::Str(#v))
        }
        Expression::NumericLiteral(n) if has("Num") => {
            let v = literals::numeric_expr(n.value);
            parse_quote!(crate::#union_ident::Num(#v))
        }
        Expression::BooleanLiteral(b) if has("Bool") => {
            let v = b.value;
            parse_quote!(crate::#union_ident::Bool(#v))
        }
        Expression::Identifier(id) if id.name.as_str() == "undefined" && has("Undef") => {
            parse_quote!(crate::#union_ident::Undef)
        }
        Expression::NullLiteral(_) if has("Null") => parse_quote!(crate::#union_ident::Null),
        Expression::Identifier(id) => {
            let var = bindings::snake(&id.name);
            box_variable_to_union(&var, union_ident, item, ctx)
                .unwrap_or_else(|| translate_expr(value, ctx))
        }
        _ => translate_expr(value, ctx),
    }
}

/// Box a variable into the union arm whose inner type matches the variable's
/// declared type — `value: String` into `__DsUnionNumStr::Str(value)` when the
/// enum has a `Str(String)` arm. Compares the arm's tuple-field type's last
/// path segment to the variable's recorded type. `None` when the variable has
/// no known type or no arm matches.
fn box_variable_to_union(
    var: &Ident,
    union_ident: &Ident,
    item: &syn::ItemEnum,
    ctx: &Ctx<'_>,
) -> Option<Expr> {
    let ty = ctx.local_type(&var.to_string())?;
    let want = ty.segments.last()?.ident.to_string();
    for v in &item.variants {
        let syn::Fields::Unnamed(unnamed) = &v.fields else {
            continue;
        };
        let Some(syn::Type::Path(tp)) = unnamed.unnamed.first().map(|f| &f.ty) else {
            continue;
        };
        if tp.path.segments.last().is_some_and(|s| s.ident == want) {
            let variant = &v.ident;
            return Some(parse_quote!(crate::#union_ident::#variant(#var.clone())));
        }
    }
    None
}

/// If `expr` is `obj.field` where `field` is an optional (`?:`) field of
/// `obj`'s struct type and `ty_hint` is the field's inner (non-`Option`) type,
/// emit a clone-and-unwrap read: TS `element.elements` (an optional field)
/// flowing into a `Vec<Element>` parameter assumes the value present, so the
/// read lowers to `element.elements.as_ref().unwrap().clone()`. `None` for any
/// other shape (a non-field expr, a non-optional field, a hint that does not
/// match the field's inner type), so a genuine `Option<T>` flow keeps its own
/// translation. When the inner type is a union enum and `ty_hint` is `String`,
/// the read unwraps then coerces via the union's `Display` impl (`element.text`
/// → `…to_string()`). An `Option<Struct>` receiver (`element` where
/// `element: Option<Element>`) unwraps the receiver first, then the field.
fn unwrap_optional_field_read(
    expr: &Expression,
    ty_hint: Option<&Type>,
    ctx: &Ctx<'_>,
) -> Option<Expr> {
    let Expression::StaticMemberExpression(sm) = expr else {
        return None;
    };
    let Expression::Identifier(obj_id) = &sm.object else {
        return None;
    };
    let hint_path = ty_hint.and_then(types::type_path)?;
    let hint_last = hint_path.segments.last()?.ident.to_string();
    let obj_name = bindings::snake(&obj_id.name);
    let obj_ty = ctx.local_type(&obj_name.to_string())?;
    let last_seg = obj_ty.segments.last()?;
    // `Option<Struct>` receiver (`element` where `element: Option<Element>`)
    // unwraps to the inner `Struct`; a bare `Struct` receiver reads its field
    // directly. Either way the field must be an optional `?:` field whose inner
    // type matches `ty_hint`.
    let (struct_name, obj_is_option) = if last_seg.ident == "Option" {
        let syn::PathArguments::AngleBracketed(args) = &last_seg.arguments else {
            return None;
        };
        let inner = args.args.iter().find_map(|a| match a {
            syn::GenericArgument::Type(syn::Type::Path(tp)) => {
                tp.path.segments.last().map(|s| s.ident.to_string())
            }
            _ => None,
        })?;
        (inner, true)
    } else {
        (last_seg.ident.to_string(), false)
    };
    let field = bindings::snake(&sm.property.name);
    let inner = ctx.field_type(&struct_name, &field.to_string())?;
    let inner_last = types::type_path(inner)?.segments.last()?.ident.to_string();
    // Both paths below require an optional (`?:`) field — its Rust type is
    // `Option<T>`. `struct_optionals` stores the original `.ts` field spelling,
    // not the snake-cased Rust name, so look the property up by its source name.
    if !ctx
        .struct_optionals(&struct_name)
        .is_some_and(|s| s.contains(sm.property.name.as_str()))
    {
        return None;
    }
    // Case 1: the field's inner type matches `ty_hint` → a plain unwrap.
    if inner_last == hint_last {
        if obj_is_option {
            return Some(
                parse_quote!((#obj_name.as_ref().unwrap().#field.as_ref().unwrap().clone())),
            );
        }
        return Some(parse_quote!((#obj_name.#field.as_ref().unwrap().clone())));
    }
    // Case 2: the field's inner type is a union enum whose `Display` impl
    // coerces to `String` — TS `element.text` (`string | number | boolean`)
    // flowing into a `String` renders via `to_string()`.
    if hint_last == "String" && ctx.is_union_enum(&inner_last) {
        if obj_is_option {
            return Some(
                parse_quote!((#obj_name.as_ref().unwrap().#field.as_ref().unwrap().to_string())),
            );
        }
        return Some(parse_quote!((#obj_name.#field.as_ref().unwrap().to_string())));
    }
    None
}

/// Translate an initializer; an object literal borrows its struct name from
/// the variable's type annotation (anonymous literals are unsupported yet).
pub fn translate_init(expr: &Expression, ty_hint: Option<&Type>, ctx: &Ctx<'_>) -> Expr {
    if let Expression::ObjectExpression(obj) = expr {
        return object::object_expr(obj, ty_hint, ctx);
    }
    // A value flowing into a union return/let type boxes into the matching
    // variant (`return value` where `value: String` into
    // `__DsUnionNumStr::Str`). `null`/`undefined` map to the `Null`/`Undef`
    // unit variants here, ahead of the `Option<T>` `None` rule below.
    if let Some(id) = ty_hint
        .and_then(types::type_path)
        .and_then(|p| p.segments.last())
        .map(|s| &s.ident)
        .filter(|id| ctx.registry().union_enums.contains_key(id))
    {
        return box_to_union(expr, id, ctx);
    }
    // An optional struct field read into its inner (non-`Option`) type unwraps
    // — `write_elements(js.elements)` where `elements: Option<Vec<Element>>`
    // lowers to `js.elements.as_ref().unwrap().clone()` (TS assumes the field
    // present). A genuine `Option<T>` hint keeps the field as-is.
    if let Some(unwrapped) = unwrap_optional_field_read(expr, ty_hint, ctx) {
        return unwrapped;
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
    // A `number` literal into an `i64`-flavored binding anchors to `_i64` so
    // `let i: i64 = 0` emits `0_i64` (not `0_f64`, a type mismatch). Other
    // contexts keep `_f64` — a bare literal must stay a valid method receiver
    // (`5.is_finite()`).
    if let Expression::NumericLiteral(n) = expr {
        if ty_hint.is_some_and(is_i64_type) {
            return literals::numeric_expr_i64(n.value);
        }
    }
    // A non-literal number expression into a number-typed binding casts to
    // the binding's flavor: `return i` where `i: i64` into `-> f64` needs
    // `i as f64`; a same-flavor binding is an identity no-op.
    if ty_hint.is_some_and(is_number_type) && is_number_expr(expr, ctx) {
        let to = if ty_hint.is_some_and(is_i64_type) {
            super::flavor::NumberFlavor::I64
        } else {
            super::flavor::NumberFlavor::F64
        };
        return translate_number_to(expr, to, ctx);
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

/// True when `ty` is `i64` — a flavor-promoted integer binding.
fn is_i64_type(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Path(tp) if tp.path.segments.last().is_some_and(|s| s.ident == "i64")
    )
}

/// True when `ty` is a numeric scalar (`f64` or `i64`).
fn is_number_type(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Path(tp) if tp.path.segments.last().is_some_and(|s| s.ident == "f64" || s.ident == "i64")
    )
}

fn ident_expr(id: &IdentifierReference, ctx: &Ctx<'_>) -> Expr {
    // ES global constants are bare identifiers (`NaN`, `Infinity`), not members
    // — map them to the matching `f64` constant instead of a renamed, undefined
    // local. `-Infinity` lowers via unary `-` on `Infinity`. Every other
    // identifier resolves its Rust name through the per-symbol `NameTable`
    // (not the lossy `snake(name)` fold), so two `.ts` bindings that collapse to
    // the same snake-name (e.g. `N` and `n`) read as distinct Rust idents.
    match id.name.as_str() {
        "NaN" => parse_quote!(::std::f64::NAN),
        "Infinity" => parse_quote!(::std::f64::INFINITY),
        _ => {
            let ident = ctx.names().of_reference(id);
            // A module-level non-const-expr `const` lowered to a lazy static is
            // read through its accessor fn (`name()` → `&'static T`), not a
            // bare identifier. A mutable module-global `let` lowered to a
            // thread-local `RefCell` (B3-2) is read the same way (`name()` → T,
            // a clone) — the get accessor shares the binding name.
            if ctx.names().is_lazy_static(id) || ctx.names().is_mutable_static(id) {
                parse_quote!(#ident())
            } else {
                parse_quote!(#ident)
            }
        }
    }
}

/// `undefined` (a global identifier in TS) maps to `None`; any other
/// identifier is a plain reference.
fn ident_or_undefined(id: &IdentifierReference, ctx: &Ctx<'_>) -> Expr {
    if id.name.as_str() == "undefined" {
        return parse_quote!(None);
    }
    ident_expr(id, ctx)
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

/// True when `path` names a `HashMap` (the target of a `Record<K, V>` / `Map`).
pub(in crate::translator) fn is_hashmap(path: &syn::Path) -> bool {
    path.segments.last().is_some_and(|s| s.ident == "HashMap")
}

/// True when `path` names a `HashSet` (the target of an ES `Set<T>`).
pub(in crate::translator) fn is_hashset(path: &syn::Path) -> bool {
    path.segments.last().is_some_and(|s| s.ident == "HashSet")
}

/// True when `path` names a `Vec<u8>` (the target of a `Uint8Array` byte
/// buffer). Only the bare `Vec<u8>` form matches — a `Vec<f64>` (an ES `Array`)
/// or any other element type does not.
pub(in crate::translator) fn is_vec_u8(path: &syn::Path) -> bool {
    let Some(seg) = path.segments.last() else {
        return false;
    };
    if seg.ident != "Vec" {
        return false;
    }
    let syn::PathArguments::AngleBracketed(args) = &seg.arguments else {
        return false;
    };
    args.args.iter().any(|a| {
        matches!(
            a,
            syn::GenericArgument::Type(syn::Type::Path(tp)) if tp.path.is_ident("u8")
        )
    })
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
    // An expression-body arrow (`() => expr`) lowers its single expression
    // inline. A block-body arrow (`() => { … }`) is a function body in
    // everything but syntax: its statements translate via `translate_body`
    // with the same per-body `Locals` a named `fn` gets (params registered,
    // mutations + number flavors analyzed), built once in `body_locals` — the
    // shared source of truth, so an arrow body and an `fn` body never diverge.
    let body: Expr = if arrow.expression {
        single_expression_body(&arrow.body)
            .map(|e| translate_expr(e, ctx))
            .unwrap_or_else(|| parse_quote!(::core::todo!()))
    } else {
        let mut locals = super::functions::body_locals(
            &arrow.params,
            Some(&*arrow.body),
            ctx.registry(),
            ctx.names(),
        );
        let block = super::functions::translate_body(
            &arrow.body.statements[..],
            &mut locals,
            ctx.registry(),
            &Narrow::default(),
            None,
            ctx.names(),
        );
        parse_quote!(#block)
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
        .map(|e| {
            let translated = translate_expr(e, ctx);
            // A numeric interpolation routes through `__ds::number_to_string`
            // so `${1e21}` is "1e+21", not Rust's "1000000000000000000000".
            // Coerced to `f64` so a flavor-promoted `i64` local compiles.
            if is_number_expr(e, ctx) {
                let n = translate_number_to(e, super::flavor::NumberFlavor::F64, ctx);
                parse_quote!(crate::__ds::number_to_string(#n))
            } else {
                // A non-number interpolation routes through `__ds::display` so an
                // `Option` (ES `undefined`) renders as "undefined" and a user
                // object as "[object Object]" — ES coercion, not Rust `Display`
                // (which is E0277 on `Option`/user types). The `__ds::display`
                // marker auto-flags the `Display` dep and its per-type impls.
                parse_quote!(crate::__ds::display(&(#translated)))
            }
        })
        .collect();
    let fmt_lit = syn::LitStr::new(&fmt, Span::call_site());
    parse_quote!(::std::format!(#fmt_lit, #(#exprs),*))
}

/// Whether `expr` evaluates to an `f64` (DashScript `number`). The number→
/// string emit points use this to route a value through `__ds::number_to_string`
/// (ryu-js) instead of Rust's `Display`, which differs from ECMAScript (`1e21`,
/// `1e-7`, `-0`). Conservative: only patterns unambiguously numeric return
/// `true`; an untracked call returns `false` and falls back to `Display`.
pub(in crate::translator) fn is_number_expr(e: &Expression, ctx: &Ctx<'_>) -> bool {
    use oxc_syntax::operator::UnaryOperator;
    match e {
        Expression::NumericLiteral(_) => true,
        Expression::ParenthesizedExpression(p) => is_number_expr(&p.expression, ctx),
        Expression::TSAsExpression(a) => is_number_expr(&a.expression, ctx),
        Expression::TSTypeAssertion(t) => is_number_expr(&t.expression, ctx),
        Expression::UnaryExpression(u) => {
            matches!(
                u.operator,
                UnaryOperator::UnaryNegation | UnaryOperator::UnaryPlus
            ) && is_number_expr(&u.argument, ctx)
        }
        Expression::BinaryExpression(b) => {
            is_arith_operator(&b.operator)
                && is_number_expr(&b.left, ctx)
                && is_number_expr(&b.right, ctx)
        }
        Expression::Identifier(id) => match id.name.as_str() {
            "NaN" | "Infinity" => true,
            _ => is_number_local(id, ctx),
        },
        Expression::CallExpression(c) => is_number_call(&c.callee),
        // `.length` is numeric (array/string length); other members are not
        // tracked, so they fall back to `Display`.
        Expression::StaticMemberExpression(sm) => sm.property.name.as_str() == "length",
        // `++x` / `x++` / `--x` / `x--` — an ES update expression always yields
        // a number (the operator coerces), so `${++i}` routes through
        // number_to_string, not Display.
        Expression::UpdateExpression(_) => true,
        _ => false,
    }
}

/// Whether a call argument evaluates to an `f64` — [`is_number_expr`] over the
/// parallel `Argument` enum. oxc models `Argument` and `Expression` separately;
/// an `Argument`'s sub-expressions are `Expression`, so this delegates inward
/// to [`is_number_expr`].
pub(in crate::translator) fn is_number_arg(arg: &Argument, ctx: &Ctx<'_>) -> bool {
    use oxc_syntax::operator::UnaryOperator;
    match arg {
        Argument::NumericLiteral(_) => true,
        Argument::ParenthesizedExpression(p) => is_number_expr(&p.expression, ctx),
        Argument::TSAsExpression(a) => is_number_expr(&a.expression, ctx),
        Argument::TSTypeAssertion(t) => is_number_expr(&t.expression, ctx),
        Argument::UnaryExpression(u) => {
            matches!(
                u.operator,
                UnaryOperator::UnaryNegation | UnaryOperator::UnaryPlus
            ) && is_number_expr(&u.argument, ctx)
        }
        Argument::BinaryExpression(b) => {
            is_arith_operator(&b.operator)
                && is_number_expr(&b.left, ctx)
                && is_number_expr(&b.right, ctx)
        }
        Argument::Identifier(id) => match id.name.as_str() {
            "NaN" | "Infinity" => true,
            _ => is_number_local(id, ctx),
        },
        Argument::CallExpression(c) => is_number_call(&c.callee),
        Argument::StaticMemberExpression(sm) => sm.property.name.as_str() == "length",
        _ => false,
    }
}

/// Coerce a number expression to `f64` for writing into a `Vec<f64>` (an ES
/// array's element type is `number`). A flavor-promoted `i64` scalar — `i` in
/// `arr.push(i)` where `i` is an `i64` counter, or an element of a `[i, j]`
/// literal — would otherwise mismatch `Vec<f64>::push` / `vec![i64; …]`. A
/// non-number expression translates unchanged (cargo backstops it: TS forbids
/// a number in a `string[]`, so a number never lands in a `Vec<String>`).
pub(in crate::translator) fn array_elem_expr(e: &Expression, ctx: &Ctx<'_>) -> Expr {
    if is_number_expr(e, ctx) {
        translate_number_to(e, super::flavor::NumberFlavor::F64, ctx)
    } else {
        translate_expr(e, ctx)
    }
}

/// [`array_elem_expr`] over a call argument — the write site for `arr.push(arg)`
/// / `arr.unshift(arg)` / `arr.fill(arg)` / `splice(…, items)` / `with(i, arg)`
/// / `Array.of(…, arg)`.
pub(in crate::translator) fn array_elem_arg(arg: &Argument, ctx: &Ctx<'_>) -> Expr {
    if is_number_arg(arg, ctx) {
        if let Some(e) = arg.as_expression() {
            return translate_number_to(e, super::flavor::NumberFlavor::F64, ctx);
        }
    }
    translate_argument(arg, ctx)
}

/// The arithmetic binary operators whose `f64 × f64 → f64` result is numeric.
/// `+` is included: when both operands are numeric (checked by the caller) it
/// is addition, not string concatenation.
fn is_arith_operator(op: &oxc_syntax::operator::BinaryOperator) -> bool {
    use oxc_syntax::operator::BinaryOperator;
    matches!(
        op,
        BinaryOperator::Addition
            | BinaryOperator::Subtraction
            | BinaryOperator::Multiplication
            | BinaryOperator::Division
            | BinaryOperator::Remainder
            | BinaryOperator::Exponential
    )
}

/// True when `id` is a numeric local (`f64` or `i64`) — so a number→string
/// coercion routes through `__ds::number_to_string`. ES rendering applies to
/// integers too, not just doubles.
fn is_number_local(id: &IdentifierReference, ctx: &Ctx<'_>) -> bool {
    // A top-level `const` promoted to a crate-level `const` item (escape
    // promotion, A3) is an `f64` value, but it is not a per-body local — so
    // consult the shared name table, not the body-local type map.
    if ctx.names().is_number_const(id) {
        return true;
    }
    let name = bindings::snake(&id.name).to_string();
    ctx.local_type(&name).is_some_and(|p| {
        p.segments
            .last()
            .is_some_and(|s| s.ident == "f64" || s.ident == "i64")
    })
}

/// True when `callee` is a known-numeric call: `Math.<anything>(…)`, or the
/// `parseInt`/`parseFloat`/`Number` globals.
fn is_number_call(callee: &Expression) -> bool {
    match callee {
        Expression::StaticMemberExpression(sm) => {
            matches!(&sm.object, Expression::Identifier(id) if id.name.as_str() == "Math")
        }
        Expression::Identifier(id) => {
            matches!(id.name.as_str(), "parseInt" | "parseFloat" | "Number")
        }
        _ => false,
    }
}
