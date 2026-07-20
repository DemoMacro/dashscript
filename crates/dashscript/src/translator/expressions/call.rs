//! Call expressions: `console.log` → `println!`, built-in static/instance
//! methods, global conversions, and plain user calls.

use oxc_ast::ast::{
    Argument, CallExpression, Expression, IdentifierReference, StaticMemberExpression,
};
use proc_macro2::Span;
use syn::{parse_quote, Expr, Type};

use super::super::bindings;
use super::super::builtins;
use super::super::context::Ctx;
use super::super::flavor::NumberFlavor;
use super::super::types;
use super::fmt_merge;
use super::{
    array_elem_arg, is_number_arg, translate_argument, translate_argument_init, translate_expr,
    translate_number_to,
};

/// `/pat/.test(s)` / `r.test(s)` — ES `RegExp.prototype.test`. The receiver
/// is a regex literal (compiled inline via `__ds::regex`) or a local bound to
/// one (`let r = /pat/`, whose inferred type is `regress::Regex`); both lower
/// to regress `Regex::find(...).is_some()`. Returns `None` for any other
/// receiver so a non-regex `.test` falls through to a plain call.
fn regex_test(sm: &StaticMemberExpression, args: &[Argument], ctx: &Ctx<'_>) -> Option<Expr> {
    let arg = builtins::str_method_arg(args.first()?, ctx);
    match &sm.object {
        Expression::RegExpLiteral(re) => {
            let re = super::regex_literal_expr(re);
            Some(parse_quote!(#re.find(#arg).is_some()))
        }
        Expression::Identifier(_) if is_regex_local(&sm.object, ctx) => {
            let r = translate_expr(&sm.object, ctx);
            Some(parse_quote!(#r.find(#arg).is_some()))
        }
        // `RegExp("pat").test(s)` / `new RegExp("pat").test(s)` — the
        // constructor lowers to `__ds::regex` (a `regress::Regex`), so `.test`
        // maps to `.find(s).is_some()` on the constructed value (no local).
        Expression::CallExpression(_) | Expression::NewExpression(_)
            if is_reg_exp_ctor(&sm.object) =>
        {
            let r = translate_expr(&sm.object, ctx);
            Some(parse_quote!(#r.find(#arg).is_some()))
        }
        _ => None,
    }
}

/// `/pat/.exec(s)` (non-global) → `Option<DsMatch>` (ES: the match result or
/// `null`). Mirrors `String.prototype.match` but the receiver is the regex. A
/// literal receiver re-compiles via `regex_match`; a regex local
/// (`let r = /pat/; r.exec(s)`) reuses the already-compiled `regress::Regex`
/// and converts its `Match` to a `DsMatch`.
fn regex_exec(sm: &StaticMemberExpression, args: &[Argument], ctx: &Ctx<'_>) -> Option<Expr> {
    let arg = builtins::str_method_arg(args.first()?, ctx);
    match &sm.object {
        Expression::RegExpLiteral(re) => {
            let (pat, fl) = super::regex_lit_parts(re);
            Some(parse_quote!(crate::__ds::regex_match(#pat, #fl, #arg)))
        }
        // `r.exec(s)` on a regex local — `r` is an already-compiled
        // `regress::Regex`, so lower to `r.find(s)` and convert the `Match` to
        // a `DsMatch` (mirrors `regex_match`, which re-compiles from a literal).
        Expression::Identifier(_) if is_regex_local(&sm.object, ctx) => {
            let r = translate_expr(&sm.object, ctx);
            Some(parse_quote!({
                let __t = #arg;
                #r.find(__t).map(|__m| crate::__ds::ds_match_from(__t, &__m))
            }))
        }
        // `RegExp("pat").exec(s)` / `new RegExp("pat").exec(s)` — the
        // constructor lowers to `__ds::regex`, so reuse the constructed value
        // (no local) and convert its `Match` to a `DsMatch`.
        Expression::CallExpression(_) | Expression::NewExpression(_)
            if is_reg_exp_ctor(&sm.object) =>
        {
            let r = translate_expr(&sm.object, ctx);
            Some(parse_quote!({
                let __t = #arg;
                #r.find(__t).map(|__m| crate::__ds::ds_match_from(__t, &__m))
            }))
        }
        _ => None,
    }
}

/// Whether `expr` is a local whose inferred type is `regress::Regex` (a
/// `let r = /pat/` binding) — so `.test` on it lowers to the regress method
/// rather than a plain field call.
fn is_regex_local(expr: &Expression, ctx: &Ctx<'_>) -> bool {
    let Expression::Identifier(id) = expr else {
        return false;
    };
    let name = bindings::snake(&id.name).to_string();
    ctx.local_type(&name)
        .is_some_and(|ty| ty.segments.last().is_some_and(|s| s.ident == "Regex"))
}

/// Whether `expr` is a `RegExp(...)` call or `new RegExp(...)` — both lower to
/// `__ds::regex` (a `regress::Regex`), so `.test`/`.exec` on the constructed
/// value dispatch like a regex local, without an intervening binding.
fn is_reg_exp_ctor(expr: &Expression) -> bool {
    let callee = match expr {
        Expression::CallExpression(c) => &c.callee,
        Expression::NewExpression(n) => &n.callee,
        _ => return false,
    };
    matches!(callee, Expression::Identifier(id) if id.name.as_str() == "RegExp")
}

/// Whether a `console.log` argument evaluates to `Option<DsMatch>` — an ES
/// `RegExp.prototype.exec` result — so it must route through
/// `__ds::fmt_option_match` (Node's match-array inspect form) rather than `{}`
/// (which fails to compile: `Option<DsMatch>` has no `Display`, blocked by the
/// orphan rule since `Option` is std's).
fn is_match_arg(arg: &Argument, ctx: &Ctx<'_>) -> bool {
    match arg {
        Argument::CallExpression(c) => is_match_call(&c.callee, c.arguments.as_slice(), ctx),
        Argument::Identifier(id) => is_match_local(id, ctx),
        Argument::ParenthesizedExpression(p) => is_match_call_expr(&p.expression, ctx),
        _ => false,
    }
}

/// A call whose result is `Option<DsMatch>`: `.exec` on a regex (always), or
/// `s.match(/pat/)` without the global flag (with `g` it lowers to
/// `Vec<String>`). A variable pattern's flags are not visible at translate
/// time, so only a literal pattern's `.match` is recognized.
fn is_match_call(callee: &Expression, args: &[Argument], ctx: &Ctx<'_>) -> bool {
    let Expression::StaticMemberExpression(sm) = callee else {
        return false;
    };
    match sm.property.name.as_str() {
        "exec" => {
            matches!(sm.object, Expression::RegExpLiteral(_)) || is_regex_local(&sm.object, ctx)
        }
        "match" => match args.first().and_then(|a| a.as_expression()) {
            Some(Expression::RegExpLiteral(re)) => {
                let (_pat, fl) = super::regex_lit_parts(re);
                !fl.value().contains('g')
            }
            _ => false,
        },
        _ => false,
    }
}

fn is_match_call_expr(expr: &Expression, ctx: &Ctx<'_>) -> bool {
    match expr {
        Expression::CallExpression(c) => is_match_call(&c.callee, c.arguments.as_slice(), ctx),
        Expression::Identifier(id) => is_match_local(id, ctx),
        Expression::ParenthesizedExpression(p) => is_match_call_expr(&p.expression, ctx),
        _ => false,
    }
}

/// A local whose inferred type is `Option<DsMatch>` (a `let m = /pat/.exec(s)`
/// binding) — so `console.log(m)` routes to the match-array formatter. Reuses
/// [`super::member::is_option_ds_match`], which walks `Option`'s generic
/// argument for `DsMatch` (a plain last-segment check would miss it).
fn is_match_local(id: &IdentifierReference, ctx: &Ctx<'_>) -> bool {
    let name = bindings::snake(&id.name).to_string();
    ctx.local_type(&name)
        .is_some_and(super::member::is_option_ds_match)
}

/// `console.log(x)` → `println!("{}", x)`; any other call maps the callee and
/// its arguments to a plain Rust call expression.
pub(super) fn translate_call(call: &CallExpression, ctx: &Ctx<'_>) -> Expr {
    if let Some(macro_name) = builtins::console_method(&call.callee) {
        // String-literal args fold into the format string as literal text
        // (labels); every other arg is a `{}` placeholder. This emits
        // `println!("a {}", v)` instead of `println!("{}", "a".to_string(), v)`
        // — no needless `.to_string()` and no empty-format-string lint.
        let mut fmt = String::new();
        let mut vals: Vec<Expr> = Vec::new();
        for (i, a) in call.arguments.iter().enumerate() {
            if i > 0 {
                fmt.push(' ');
            }
            match a {
                Argument::StringLiteral(s) => {
                    // Escape `{`/`}` so a literal brace isn't a placeholder.
                    for ch in s.value.chars() {
                        fmt.push(ch);
                        if ch == '{' || ch == '}' {
                            fmt.push(ch);
                        }
                    }
                }
                _ if is_number_arg(a, ctx) => {
                    // An ES `Number::toString`: Rust's `f64` `Display` differs
                    // from ECMAScript (`1e21` → `1e+21`, `1e-7`, `-0` → `0`), so
                    // route any numeric argument — a literal, a numeric local,
                    // arithmetic, `Math.*`, `.length` — through the `__ds` helper
                    // (ryu_js). Its presence in the output flags `needs_ryu_js`.
                    // `__ds::number_to_string` takes `f64`; a flavor-promoted
                    // `i64` local (`console.log(total)` where `total` is `i64`)
                    // is site-cast here so the call compiles.
                    let e = if let Some(expr) = a.as_expression() {
                        translate_number_to(expr, NumberFlavor::F64, ctx)
                    } else {
                        translate_argument(a, ctx)
                    };
                    let wrapped: Expr = parse_quote!(crate::__ds::number_to_string(#e));
                    fmt.push_str("{}");
                    vals.push(wrapped);
                }
                _ if is_match_arg(a, ctx) => {
                    // `console.log(/pat/.exec(s))` — `Option<DsMatch>` has no
                    // `Display` (orphan rule on `Option`), so render via the
                    // Node match-array formatter instead of `{}`.
                    let e = translate_argument(a, ctx);
                    let wrapped: Expr = parse_quote!(crate::__ds::fmt_option_match(#e));
                    fmt.push_str("{}");
                    vals.push(wrapped);
                }
                _ => {
                    let e = translate_argument(a, ctx);
                    match fmt_merge::inline_arg(e) {
                        fmt_merge::Inlined::Format { fmt: ifmt, args } => {
                            fmt.push_str(&fmt_merge::renumber_format(&ifmt, vals.len()));
                            vals.extend(args);
                        }
                        fmt_merge::Inlined::Display(e) => {
                            fmt.push_str("{}");
                            vals.push(e);
                        }
                    }
                }
            }
        }
        let fmt_lit = syn::LitStr::new(&fmt, Span::call_site());
        return parse_quote!(::std::#macro_name!(#fmt_lit, #(#vals),*));
    }
    // `String.prototype.trim.call(x)` — the JS idiom of borrowing a prototype
    // method via `.call`. Lower `String.prototype.<m>.call(r, ...)` to
    // `String(r).<m>(...)` (ToString the receiver, then the mapped method).
    // A plain prototype access without `.call` stays unmapped; `cargo check`
    // rejects it honestly.
    if let Some((builtin, method)) = prototype_method_call(&call.callee) {
        if builtin == "String" && !call.arguments.is_empty() {
            let obj = to_string_expr(&call.arguments[0], ctx);
            // First the adapted methods (includes/indexOf/slice/pad/...), then
            // the name-mapped passthroughs (trim/toUpperCase/toLowerCase/...).
            if let Some(expr) =
                builtins::string_method_on(obj.clone(), method, &call.arguments[1..], ctx)
            {
                return expr;
            }
            if let Some(m) = builtins::map_method(method) {
                let args: Vec<Expr> = call.arguments[1..]
                    .iter()
                    .map(|a| translate_argument(a, ctx))
                    .collect();
                return parse_quote!(#obj.#m(#(#args),*));
            }
        }
        // `Array.prototype.<m>.call(recv, …)` — borrow an Array prototype method
        // via `.call`. Only a `Vec` receiver is lowered (`array_method_on`
        // returns `None` otherwise); an array-like receiver has no mapping.
        if builtin == "Array" && !call.arguments.is_empty() {
            if let Some(expr) =
                builtins::array_method_on(&call.arguments[0], method, &call.arguments[1..], ctx)
            {
                return expr;
            }
        }
    }
    // `Math.floor(x)` → `x.floor()`; `Math.max(a, b)` → `a.max(b)`.
    if let Expression::StaticMemberExpression(sm) = &call.callee {
        if builtins::is_ident(&sm.object, "Math") {
            if let Some(expr) =
                builtins::math_method(&sm.property.name, call.arguments.as_slice(), ctx)
            {
                return expr;
            }
        }
        // `Object.keys(m)` / `Object.values(m)` on a `Record` (a `HashMap`).
        if builtins::is_ident(&sm.object, "Object") {
            if let Some(expr) =
                builtins::object_method(&sm.property.name, call.arguments.as_slice(), ctx)
            {
                return expr;
            }
        }
        // `Array.of(…)` / `Array.isArray(x)` / `Array.from(…)`.
        if builtins::is_ident(&sm.object, "Array") {
            if let Some(expr) = builtins::array_static(sm, call.arguments.as_slice(), ctx) {
                return expr;
            }
        }
        // `String.fromCharCode(n)` → a one-char `String`.
        if builtins::is_ident(&sm.object, "String") {
            if let Some(expr) =
                builtins::string_static(&sm.property.name, call.arguments.as_slice(), ctx)
            {
                return expr;
            }
        }
        // `Number.isNaN(x)` / `Number.isFinite(x)` / `Number.isInteger(x)`.
        if builtins::is_ident(&sm.object, "Number") {
            if let Some(expr) =
                builtins::number_static(&sm.property.name, call.arguments.as_slice(), ctx)
            {
                return expr;
            }
        }
        // `JSON.parse(s)` / `JSON.stringify(x)` (inlines `serde_json::`).
        if builtins::is_ident(&sm.object, "JSON") {
            if let Some(expr) =
                builtins::json_static(&sm.property.name, call.arguments.as_slice(), ctx)
            {
                return expr;
            }
        }
        // `RegExp.escape(s)` (TC39 Stage 3).
        if builtins::is_ident(&sm.object, "RegExp") {
            if let Some(expr) =
                builtins::reg_exp_static(&sm.property.name, call.arguments.as_slice(), ctx)
            {
                return expr;
            }
        }
    }
    // `Temporal.PlainDate.from(s)` → temporal_rs. The callee is a nested
    // `Temporal.<Type>.<method>` static member (its object is itself a member).
    if let Expression::StaticMemberExpression(sm) = &call.callee {
        if let Expression::StaticMemberExpression(type_me) = &sm.object {
            if builtins::is_ident(&type_me.object, "Temporal") {
                if let Some(expr) = builtins::temporal_static(
                    type_me.property.name.as_str(),
                    sm.property.name.as_str(),
                    call.arguments.as_slice(),
                    ctx,
                ) {
                    return expr;
                }
            }
        }
    }
    // Global conversion functions: `String(x)`, `parseInt(s)`, `parseFloat(s)`.
    if let Expression::Identifier(id) = &call.callee {
        if let Some(expr) = builtins::global_function(id, call.arguments.as_slice(), ctx) {
            return expr;
        }
    }
    // A method call (`s.toUpperCase()`) maps the method name, not the receiver.
    if let Expression::StaticMemberExpression(sm) = &call.callee {
        // `/pat/.test(s)` / `r.test(s)` on a regex (ES RegExp.prototype.test).
        if sm.property.name.as_str() == "test" {
            if let Some(expr) = regex_test(sm, call.arguments.as_slice(), ctx) {
                return expr;
            }
        }
        // `/pat/.exec(s)` on a regex literal (ES RegExp.prototype.exec, non-
        // global) → the first match as `Option<DsMatch>`.
        if sm.property.name.as_str() == "exec" {
            if let Some(expr) = regex_exec(sm, call.arguments.as_slice(), ctx) {
                return expr;
            }
        }
        if let Some(expr) = builtins::array_method(sm, call.arguments.as_slice(), ctx) {
            return expr;
        }
        if let Some(expr) = builtins::string_method(sm, call.arguments.as_slice(), ctx) {
            return expr;
        }
        if let Some(expr) = builtins::number_method(sm, call.arguments.as_slice(), ctx) {
            return expr;
        }
        // `m.set(k, v)` / `s.add(v)` / `m.has(k)` on a Map/Set (HashMap/HashSet
        // local) — dispatched on the receiver's resolved type.
        if let Some(expr) = builtins::collection_method(sm, call.arguments.as_slice(), ctx) {
            return expr;
        }
        if let Some(method) = builtins::map_method(&sm.property.name) {
            let obj = translate_expr(&sm.object, ctx);
            // `push` (the only `map_method` name with an argument) writes into a
            // `Vec<f64>`, so a flavor-promoted `i64` arg is coerced to `f64`.
            let args: Vec<Expr> = call
                .arguments
                .iter()
                .map(|a| array_elem_arg(a, ctx))
                .collect();
            return parse_quote!(#obj.#method(#(#args),*));
        }
    }
    let callee = translate_expr(&call.callee, ctx);
    // `f({ x, y })` borrows the struct name from `f`'s declared parameter type.
    let hints: Option<&[Option<syn::Path>]> = match &call.callee {
        Expression::Identifier(id) => ctx.function_params(&id.name),
        _ => None,
    };
    let defaults: Option<&[bool]> = match &call.callee {
        Expression::Identifier(id) => ctx.function_defaults(&id.name),
        _ => None,
    };
    // Per-parameter reference-parameter (`&mut`) flags — a call borrows a
    // bare-identifier argument in place (`&mut arg`) at those positions instead
    // of cloning, so the callee's `c[i] = v` is visible here (ES reference
    // semantics for arrays/objects).
    let ref_flags: Option<&[bool]> = match &call.callee {
        Expression::Identifier(id) => ctx.function_ref_params(&id.name),
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
            // A `number` parameter is `f64` (Phase 1 keeps cross-function
            // flavor out of scope), so a flavor-promoted `i64` argument
            // (`compute(i)` where `i` is an `i64` counter) is site-cast to
            // match the callee's `f64` parameter type. A non-number argument
            // keeps the struct-hint / default-aware path.
            let val = if is_number_arg(a, ctx) {
                match a.as_expression() {
                    Some(e) => translate_number_to(e, NumberFlavor::F64, ctx),
                    None => translate_argument_init(a, hint_ty.as_ref(), ctx),
                }
            } else {
                translate_argument_init(a, hint_ty.as_ref(), ctx)
            };
            // A reference-parameter position borrows a bare-identifier argument
            // in place (`&mut arg`): the callee's mutation is then visible
            // here, and the value is neither moved nor cloned. Any other
            // argument shape keeps the owned/clone path (rare; cargo check
            // backstops a literal/expression passed for mutation).
            let val = if ref_flags.is_some_and(|f| f.get(i) == Some(&true))
                && matches!(a, Argument::Identifier(_))
            {
                parse_quote!(&mut #val)
            } else {
                clone_owned_local(a, val, ctx)
            };
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
    let Argument::Identifier(id) = arg else {
        return val;
    };
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

/// Detect `Builtin.prototype.<method>.call(...)` — the JS idiom of borrowing a
/// prototype method via `.call`. Returns `(builtin, method)`; the caller reads
/// the receiver/args straight from the `CallExpression` (an `Argument` slice
/// would drag in oxc's arena lifetime). Only builtins DashScript can lower are
/// matched (`String` today); a bare prototype access without `.call` is left
/// for the fallback path.
pub(in crate::translator) fn prototype_method_call<'a>(
    callee: &'a Expression,
) -> Option<(&'static str, &'a str)> {
    let Expression::StaticMemberExpression(call_me) = callee else {
        return None;
    };
    if call_me.property.name.as_str() != "call" {
        return None;
    }
    let Expression::StaticMemberExpression(method_me) = &call_me.object else {
        return None;
    };
    let method = method_me.property.name.as_str();
    let Expression::StaticMemberExpression(proto_me) = &method_me.object else {
        return None;
    };
    if proto_me.property.name.as_str() != "prototype" {
        return None;
    }
    let Expression::Identifier(builtin) = &proto_me.object else {
        return None;
    };
    let builtin = match builtin.name.as_str() {
        "String" => "String",
        "Array" => "Array",
        _ => return None,
    };
    Some((builtin, method))
}

/// ToString-coerce a `.call(receiver)` argument to a `String`, matching TS
/// `String(x)`: a scalar via `format!`; `null`/`undefined` to the literal
/// `"null"`/`"undefined"` (they lower to `None`, which has no `Display`).
/// An array/object receiver uses `format!` too — approximate, since JS joins
/// an array's items while DashScript prints Rust's `Vec` form (a known gap).
fn to_string_expr(arg: &Argument, ctx: &Ctx<'_>) -> Expr {
    match arg {
        Argument::NullLiteral(_) => parse_quote!("null".to_string()),
        Argument::Identifier(id) if id.name.as_str() == "undefined" => {
            parse_quote!("undefined".to_string())
        }
        _ => {
            let e = translate_argument(arg, ctx);
            parse_quote!(::std::format!("{}", #e))
        }
    }
}
