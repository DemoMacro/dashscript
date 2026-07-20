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
pub(in crate::translator) use member::{is_hashmap_local, is_hashset_local};

use oxc_ast::ast::{
    Argument, ArrowFunctionExpression, Expression, FunctionBody, IdentifierReference, Statement,
    TemplateLiteral,
};
use proc_macro2::Span;
use syn::{parse_quote, Expr, Pat, Type};

use super::context::Ctx;
use super::{bindings, types};

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
        Expression::Identifier(id) => ident_or_undefined(id, ctx),
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
        Expression::UpdateExpression(u) => assignment::update_expr(u, ctx),
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
        Expression::RegExpLiteral(re) => regex_literal_expr(re),
        _ => parse_quote!(::core::todo!()),
    }
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
    // (not the lossy `snake(name)` fold), so two `.ds` bindings that collapse to
    // the same snake-name (e.g. `N` and `n`) read as distinct Rust idents.
    match id.name.as_str() {
        "NaN" => parse_quote!(::std::f64::NAN),
        "Infinity" => parse_quote!(::std::f64::INFINITY),
        _ => {
            let ident = ctx.names().of_reference(id);
            parse_quote!(#ident)
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
        .map(|e| {
            let translated = translate_expr(e, ctx);
            // A numeric interpolation routes through `__ds::number_to_string`
            // so `${1e21}` is "1e+21", not Rust's "1000000000000000000000".
            // Coerced to `f64` so a flavor-promoted `i64` local compiles.
            if is_number_expr(e, ctx) {
                let n = translate_number_to(e, super::flavor::NumberFlavor::F64, ctx);
                parse_quote!(crate::__ds::number_to_string(#n))
            } else {
                translated
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
