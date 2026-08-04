//! Call expressions: `console.log` → `println!`, built-in static/instance
//! methods, global conversions, and plain user calls.

use oxc_ast::ast::{
    Argument, CallExpression, Expression, IdentifierReference, StaticMemberExpression,
};
use proc_macro2::Span;
use syn::{parse_quote, Arm, Expr, Ident, Type};

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
/// The string argument to `.test`/`.exec`: ES coerces a *missing* argument via
/// ToString to `"undefined"` (so `re.test()` searches for the literal
/// "undefined"), while a present argument follows the normal string-method
/// coercion. Without this, a no-arg call would fail to lower (regress `find`
/// needs a `&str`) and emit a phantom `.test()`.
fn regex_str_arg(args: &[Argument], ctx: &Ctx<'_>) -> Expr {
    match args.first() {
        Some(a) => builtins::str_method_arg(a, ctx),
        None => parse_quote!("undefined"),
    }
}

fn regex_test(sm: &StaticMemberExpression, args: &[Argument], ctx: &Ctx<'_>) -> Option<Expr> {
    let arg = regex_str_arg(args, ctx);
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
    let arg = regex_str_arg(args, ctx);
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
/// A `console.log` argument that is a container Rust `Display` cannot reach —
/// a `Vec`/`HashMap`/`HashSet`/`Option` (std collections have no `Display`, so
/// `println!("{}", vec)` would not compile). Routed through `__ds::inspect`
/// (Node's `console.log` inspect format). Only an identifier whose type is
/// known via `Ctx::local_type`; inline container expressions widen later.
fn is_container_arg(arg: &Argument, ctx: &Ctx<'_>) -> bool {
    match arg {
        // A local whose type is a non-primitive container (`Vec`/`HashMap`/
        // `HashSet`/`Option`) or a `serde_json::Value` (a `JSON.parse` result)
        // — none have a Rust `Display` matching Node's console.log format.
        Argument::Identifier(id) => {
            let name = bindings::snake(&id.name).to_string();
            ctx.local_type(&name).is_some_and(|p| {
                p.segments.last().is_some_and(|s| {
                    matches!(
                        s.ident.to_string().as_str(),
                        "Vec" | "HashMap" | "HashSet" | "Option" | "Value"
                    )
                })
            })
        }
        // An inline `JSON.parse(...)` call returns `serde_json::Value` — route
        // through `inspect` so the parsed value renders as Node prints it
        // (a string verbatim, an array `[ a, 'b' ]`, …) rather than via the
        // `Value`'s JSON `Display`.
        Argument::CallExpression(c) => is_json_parse_call(&c.callee),
        _ => false,
    }
}

/// True when `callee` is the `JSON.parse` member expression.
fn is_json_parse_call(callee: &Expression) -> bool {
    let Expression::StaticMemberExpression(sm) = callee else {
        return false;
    };
    builtins::is_ident(&sm.object, "JSON") && sm.property.name == "parse"
}

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
    // A delayed-binding mutable global holds `Option<fn …>` behind its accessor;
    // calling it as `x(args)` lowers to `(x().expect("x"))(args)` — ES throws on
    // a nullish call, here it panics with the source name (the same fail-loud
    // contract as a deref of `None`). Checked before namespace/builtin dispatch
    // so the callee path is one branch.
    if let Expression::Identifier(id) = &call.callee {
        if ctx.names().is_optional_mutable_static(id) {
            let getter = ctx.names().of_reference(id);
            let label_lit = syn::LitStr::new(id.name.as_str(), Span::call_site());
            let args: Vec<Expr> = call
                .arguments
                .iter()
                .map(|a| translate_argument(a, ctx))
                .collect();
            return parse_quote!((#getter().expect(#label_lit))(#(#args),*));
        }
    }
    // `ns.foo(…)` where `ns` is a namespace import → the free function
    // `ns::foo(…)`. Guarded before any builtin / method dispatch so a member
    // name that collides with a mapped method (`ns.push`) is not mis-routed to a
    // Vec method by name alone. The callee path reuses `member_expr`'s namespace
    // branch via `translate_expr`, so read and call forms share one lowering.
    if let Expression::StaticMemberExpression(sm) = &call.callee {
        if let Expression::Identifier(id) = &sm.object {
            if ctx.names().is_namespace(id) {
                let callee = translate_expr(&call.callee, ctx);
                let args: Vec<Expr> = call
                    .arguments
                    .iter()
                    .map(|a| translate_argument(a, ctx))
                    .collect();
                return parse_quote!(#callee(#(#args),*));
            }
        }
    }
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
                    // An ES `Number::toString` — route through `__ds` (ryu_js),
                    // not Rust `Display` (`1e21`, `1e-7`, `-0` differ). See
                    // `number_arg_to_es_string` for the flavor-promoted `i64`
                    // site-cast and the `needs_ryu_js` flag.
                    vals.push(number_arg_to_es_string(a, ctx));
                    fmt.push_str("{}");
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
                _ if is_container_arg(a, ctx) => {
                    // A container (`Vec`/`HashMap`/`HashSet`/`Option`) has no
                    // Rust `Display` — route through `__ds::inspect`, Node's
                    // console.log inspect format (`[ a, 'b' ]`, `{ k: v }`).
                    let e = translate_argument(a, ctx);
                    let wrapped: Expr = parse_quote!(crate::__ds::inspect(&(#e)));
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
        // `Promise.resolve(x)` / `Promise.all([...])` — static combinators (T3
        // stage 2a). Every other `Promise.<method>` (race/any/allSettled/then)
        // and bare `Promise`/`new Promise` degrade to the engine — `classify`
        // pulls only `resolve`/`all` out, so they reach here.
        if builtins::is_ident(&sm.object, "Promise") {
            if let Some(expr) =
                builtins::promise_static(&sm.property.name, call.arguments.as_slice(), ctx)
            {
                return expr;
            }
        }
        // `assert.sameValue(a, b)` / `assert.notSameValue(a, b)` — the test262
        // harness (a host object). Reflection asserts (`throws`/`compareArray`/
        // `verifyProperty`/…) are routed to the engine by `classify` before
        // dispatch reaches here, so an unmapped name surfaces honestly.
        if builtins::is_ident(&sm.object, "assert") {
            if let Some(expr) =
                builtins::assert_method(&sm.property.name, call.arguments.as_slice(), ctx)
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
        // Bare `assert(mustBeTrue[, message])` — test262's truth assert, lowered
        // to `assert_same_value(mustBeTrue, true)` (see `assert_call`). Dispatched
        // before `global_function` so the bare-callee form does not fall through
        // to a phantom `assert` binding (E0425).
        if id.name.as_str() == "assert" {
            if let Some(expr) = builtins::assert_call(call.arguments.as_slice(), ctx) {
                return expr;
            }
        }
        // WPT testharness globals — `test()`/`assert_equals`/`assert_true`/…
        // (the web-platform analogue of test262's `assert`). Dispatched before
        // `global_function` so the bare-callee form does not fall through to a
        // phantom binding (E0425). WinterTC is static-only: these lower to
        // `__ds::wpt_*` Rust helpers, never to the engine.
        if let Some(expr) = builtins::testharness_function(id, call.arguments.as_slice(), ctx) {
            return expr;
        }
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
        // `performance.now()` — the WinterTC (W3C hr-time) High Resolution
        // Time API. The receiver is the global `performance` object, optionally
        // via the WinterTC `self` alias (`self.performance.now()`); both lower
        // to `__ds::perf_now()`. Dispatched before the array/string/… method
        // tables: a global-object receiver never matches a local-typed one, so
        // the order is safe.
        if let Some(expr) = builtins::perf_method(sm) {
            return expr;
        }
        // `crypto.randomUUID()` / `crypto.getRandomValues(buf)` — WinterTC
        // (W3C WebCrypto) methods on the global `crypto` object (optionally via
        // the WinterTC `self` alias); lower to `__ds::crypto_*` helpers.
        if let Some(expr) = builtins::crypto_method(sm, call.arguments.as_slice(), ctx) {
            return expr;
        }
        // `URL.canParse(url, base?)` / `URL.parse(url, base?)` — the WinterTC
        // WHATWG URL static methods on the `URL` constructor object. The callee
        // is the `URL` identifier (not a local), so this dispatch fires before
        // the local-typed method tables; a non-`URL` receiver falls through.
        // `URL.parse` lowers to `Option<DsUrl>` (ES `null` on parse failure).
        if let Some(expr) = builtins::url_static_method(sm, call.arguments.as_slice(), ctx) {
            return expr;
        }
        // `b.slice(…)` / `await b.text()` / `await b.arrayBuffer()` /
        // `await b.bytes()` on a `DsBlob` local (`new Blob(…)` binding) — the
        // WinterTC WHATWG FileAPI `Blob` API. Dispatched BEFORE the name-based
        // `string_method` below: `Blob` shares the `slice` method name with
        // `String`, and the string lowering keys off the method name alone, so a
        // `DsBlob` receiver must be claimed first (the gate is `is_blob_local`;
        // a real string still falls through to `string_method`). `slice` returns
        // a new `DsBlob`; the async methods return a `Future` the caller's
        // `await` drives (engines return a `Promise`).
        if let Some(expr) = builtins::blob_method(sm, call.arguments.as_slice(), ctx) {
            return expr;
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
        // `params.get/has/set/append/delete/getAll/sort/toString(...)` on a
        // `URLSearchParams` (`DsUrlSearchParams` local) — a WinterTC Web API.
        // Dispatched after `collection_method` (a HashMap/HashSet receiver is
        // not a `DsUrlSearchParams`); each name/value arg is coerced via ES
        // `ToString`, so a numeric value type-checks against `AsRef<str>`.
        if let Some(expr) = builtins::url_search_params_method(sm, call.arguments.as_slice(), ctx) {
            return expr;
        }
        // `url.searchParams.<method>(...)` — a URLSearchParams method through a
        // DsUrl's live `searchParams` view (the receiver is `<DsUrl>.searchParams`,
        // not a DsUrlSearchParams local). Dispatched right after the local form:
        // a DsUrlSearchParams local receiver never matches the `<DsUrl>.searchParams`
        // chain, so the two are mutually exclusive.
        if let Some(expr) =
            builtins::url_search_params_on_url_method(sm, call.arguments.as_slice(), ctx)
        {
            return expr;
        }
        // `buf.set(source, offset)` on a `Uint8Array` (`Vec<u8>` local) — a
        // byte-buffer copy. Dispatched after `collection_method` so a `Map.set`
        // (a HashMap receiver) is handled first; only a `Vec<u8>` receiver
        // lands here, and `map_method` does not map `set`, so the order is safe.
        if let Some(expr) = builtins::typed_array_method(sm, call.arguments.as_slice(), ctx) {
            return expr;
        }
        // `decoder.decode(bytes[, options])` on a `TextDecoder` local (a
        // WinterTC Web API). The ES `decode` second arg `{ stream }` (a
        // streaming instance buffer) is dropped — the static decode is
        // stateless per call. Dispatched after `typed_array_method` (a
        // `Vec<u8>` receiver is not a `TextDecoder`); `decode` is not a
        // collection method, so the order is safe.
        if let Some(expr) = builtins::text_decoder_method(sm, call.arguments.as_slice(), ctx) {
            return expr;
        }
        // `encoder.encode()` / `encoder.encode(undefined)` on a `TextEncoder`
        // (a local or an inline `new TextEncoder()`) — the ES `encode(input =
        // "")` default lowers to an empty byte sequence. A supplied value
        // falls through to a plain call. Dispatched after `typed_array_method`
        // and `text_decoder_method`; `encode` is not a collection method.
        if let Some(expr) = builtins::text_encoder_method(sm, call.arguments.as_slice(), ctx) {
            return expr;
        }
        // `et.addEventListener(type, cb[, useCapture|options])` /
        // `removeEventListener(type, cb[, options])` / `dispatchEvent(event)` on
        // a `DsEventTarget` local (`new EventTarget()` binding) — the WinterTC
        // WHATWG DOM Events API. Dispatched after the local-typed method tables
        // (a `DsEventTarget` receiver never matches `Vec`/`HashMap`/…); the
        // listener callback is wrapped in a discard-return adapter so any
        // callback shape type-checks against `Box<dyn FnMut(&DsEvent)>`.
        if let Some(expr) = builtins::event_target_method(sm, call.arguments.as_slice(), ctx) {
            return expr;
        }
        // `controller.abort()` / `signal.addEventListener("abort", cb)` /
        // `signal.removeEventListener(…)` / `signal.dispatchEvent(…)` on a
        // DsAbortController / DsAbortSignal value (a local or a chained
        // `controller.signal`) — the WinterTC WHATWG DOM Abort API. Dispatched
        // right after the EventTarget table (a signal receiver never matches a
        // DsEventTarget); the abort listener reuses the EventTarget callback
        // adapter so any callback shape type-checks against `Box<dyn FnMut(&DsEvent)>`.
        if let Some(expr) = builtins::abort_method(sm, call.arguments.as_slice(), ctx) {
            return expr;
        }
        // `h.get(name)` / `h.has(name)` / `h.set(name, value)` / `h.append(…)` /
        // `h.delete(name)` / `h.forEach(cb)` / `h.keys()` / `h.values()` /
        // `h.entries()` on a `DsHeaders` local (`new Headers(…)` binding) — the
        // WinterTC WHATWG FETCH `Headers` API. Dispatched after the local-typed
        // method tables (a `DsHeaders` receiver never matches `Vec`/`HashMap`/…);
        // each name/value arg is ToString-coerced; iteration returns a `Vec`.
        if let Some(expr) = builtins::headers_method(sm, call.arguments.as_slice(), ctx) {
            return expr;
        }
        // `s.getReader()` / `r.read()` / `c.enqueue(v)` / `c.close()` on a
        // `DsReadableStream` / `DsReadableStreamDefaultReader` /
        // `DsReadableStreamController` local — the WinterTC WHATWG Streams API
        // (push-source read loop). Dispatched on the receiver's resolved type;
        // an unmapped receiver or name falls through. `reader.read()` returns a
        // pinned future awaited by `await` (the await-gate drives it).
        if let Some(expr) = builtins::streams_method(sm, call.arguments.as_slice(), ctx) {
            return expr;
        }
        // `cs.writable.getWriter()` / `writer.write(chunk)` / `writer.close()` /
        // `cs.readable.getReader()` / `reader.read()` on a `DsCompressionStream`
        // / `DsCompressionWriter` / `DsCompressionReader` local — the WinterTC
        // WHATWG Streams compression API (one-shot `flate2` transform). The
        // `writable`/`readable` receiver is a field access on a
        // `DsCompressionStream` local; the writer/reader receiver is the local
        // `callee_return_path` typed. Dispatched after `streams_method` (a
        // compression receiver never matches the `DsReadableStream`/… locals).
        if let Some(expr) = builtins::compression_method(sm, call.arguments.as_slice(), ctx) {
            return expr;
        }
        // `d.toString()` / `d.toJSON()` / `d.equals(o)` on a `Temporal.<Type>`
        // local (`temporal_rs::<Type>`). Dispatched on the receiver's resolved
        // type; a non-Temporal receiver or unmapped name falls through.
        if let Some(expr) = builtins::temporal_method(sm, call.arguments.as_slice(), ctx) {
            return expr;
        }
        // `p.then(onFulfilled)` on a `DsPromise<T>` receiver — a `Promise`
        // instance method (T3 stage 2b). Dispatched on the receiver (a
        // resolved `DsPromise` local or a `Promise.resolve(..)`/`.all([..])`
        // call); a non-Promise receiver or an unmapped name falls through.
        if let Some(expr) = builtins::promise_instance_method(sm, call.arguments.as_slice(), ctx) {
            return expr;
        }
        if let Some(method) = builtins::map_method(&sm.property.name) {
            // `obj.opt_field.push(..)` — the field is `Option<Vec<..>>`; route
            // through `get_or_insert_with(Default::default)` so the method lands
            // on the inner `Vec`. ES guarantees the field is non-undefined here
            // (a prior `obj.opt_field = []`), so the insert is a no-op in
            // practice; `get_or_insert_with` keeps it sound if not.
            if let Expression::StaticMemberExpression(inner) = &sm.object {
                let inner_field = bindings::snake(&inner.property.name);
                if super::member::static_member_is_optional_field(&inner.object, &inner_field, ctx)
                {
                    let inner_obj = translate_expr(&inner.object, ctx);
                    let args: Vec<Expr> = call
                        .arguments
                        .iter()
                        .map(|a| clone_owned_local(a, array_elem_arg(a, ctx), ctx))
                        .collect();
                    return parse_quote!(
                        #inner_obj.#inner_field
                            .get_or_insert_with(::core::default::Default::default)
                            .#method(#(#args),*)
                    );
                }
            }
            let obj = translate_expr(&sm.object, ctx);
            // `push` (the only `map_method` name with an argument) writes into a
            // `Vec<f64>`, so a flavor-promoted `i64` arg is coerced to `f64`.
            let args: Vec<Expr> = call
                .arguments
                .iter()
                .map(|a| clone_owned_local(a, array_elem_arg(a, ctx), ctx))
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
        // An engine-degraded callee marshals its arguments by value, so none of
        // its parameters is borrowed in place — `&mut arg` would mismatch the
        // by-value signature (and the engine's mutation never crosses back).
        Expression::Identifier(id) if !ctx.is_dynamic_fn(&id.name) => {
            ctx.function_ref_params(&id.name)
        }
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
            // Widen an `Option<Small>` argument to a wider scalar-union parameter
            // (`writeText(element.text)` where `text?: string | number | boolean`
            // meets `text: … | null | undefined`). Returns `None` for any other
            // argument, so the default path below is unchanged.
            if let Some(widened) = widen_union_arg(a, hint_ty.as_ref(), ctx) {
                return widened;
            }
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
                // The callee takes this parameter by `&mut`. If the argument is
                // itself a `&mut` ref-param local (a recursive call forwarding
                // an accumulator, e.g. `collect_deep(.., result)`), it is
                // already the right type — pass it as-is so Rust reborrows,
                // instead of layering a `&mut &mut` (or E0596 on a non-`mut`
                // binding). An owned local still gets `&mut` to borrow in place.
                if let Argument::Identifier(id) = a {
                    if ctx.is_ref_param(&bindings::snake(&id.name).to_string()) {
                        val
                    } else {
                        parse_quote!(&mut #val)
                    }
                } else {
                    parse_quote!(&mut #val)
                }
            } else {
                clone_owned_local(a, val, ctx)
            };
            // A supplied value for a defaulted parameter wraps in `Some` (the
            // parameter is `Option<T>` and a value was given). A non-defaulted
            // `Option<T>` parameter passed a plain `T` (e.g. `find(child)` where
            // the callee takes `Option<Element>`) also needs `Some`.
            if defaults.is_some_and(|d| d.get(i) == Some(&true)) {
                parse_quote!(Some(#val))
            } else if let Some(e) = a.as_expression() {
                super::implicit_some(e, val, hint_ty.as_ref(), ctx)
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
    // A closure callee (`(() => …)()` / `(function () { … })()`) needs parens —
    // a bare `|| …(args)` parses as a closure whose body is the call, not a
    // call on the closure (E0618 on the IIFE's returned-`!` body). Other
    // callees (idents, member access, a returned closure `f()()`) are fine
    // unparenthesized.
    let callee = if matches!(callee, syn::Expr::Closure(_)) {
        parse_quote!((#callee))
    } else {
        callee
    };
    parse_quote!(#callee(#(#args),*))
}

/// A bare-local argument passed by value to a user function. TS reference
/// semantics lets the caller reuse the value afterwards, but Rust would move
/// it; when the local is also read elsewhere (use count > 1) and is not
/// `Copy`, clone it at the call site so those later reads still see a value.
/// A scalar is `Copy` (never cloned); a local read only here is moved, which
/// is the idiomatic last use.
fn clone_owned_local(arg: &Argument, val: Expr, ctx: &Ctx<'_>) -> Expr {
    // A bare local passed by value: clone it when it is non-`Copy` and read
    // again later (use count > 1), so a later read still sees a value.
    if let Argument::Identifier(id) = arg {
        if id.name.as_str() == "undefined" {
            return val;
        }
        let name = bindings::snake(&id.name).to_string();
        if ctx.use_count(&name) <= 1 {
            return val;
        }
        return match ctx.local_type(&name) {
            Some(ty) if types::is_copy_path(ty) => val,
            Some(_) => parse_quote!(#val.clone()),
            None => val,
        };
    }
    // A `obj.field` field access passed by value: moving `field` out partially
    // moves `obj`, so when `obj` is a reused local (use count > 1) and the field
    // is non-`Copy`, clone the field to keep `obj` whole for its later reads.
    if let Some(Expression::StaticMemberExpression(sm)) = arg.as_expression() {
        if let Expression::Identifier(obj) = &sm.object {
            let obj_name = bindings::snake(obj.name.as_str()).to_string();
            if ctx.use_count(&obj_name) > 1 {
                if let Some(struct_name) = ctx
                    .local_type(&obj_name)
                    .and_then(|p| p.segments.last())
                    .map(|s| s.ident.to_string())
                {
                    let field = bindings::snake(sm.property.name.as_str()).to_string();
                    let non_copy = ctx
                        .field_type(&struct_name, &field)
                        .and_then(types::type_path)
                        .map(|p| !types::is_copy_path(p))
                        .unwrap_or(true);
                    if non_copy {
                        return parse_quote!((#val).clone());
                    }
                }
            }
        }
    }
    val
}

/// Widen an `Option<Small>` argument into a wider scalar-union parameter `Big`
/// whose variant set is a name-superset of `Small`'s. TS lets a narrower union
/// flow into a wider one at a call boundary — `writeText(element.text)` where
/// `text?: string | number | boolean` (an `Option<Small>`) meets `text:
/// string | number | boolean | undefined | null` (a wider `Big`). Rust's nominal
/// enums need an explicit `match` to convert; `None` (an absent optional field
/// = `undefined`) lands on `Big`'s `Undef` variant, or `Null` when there is no
/// `Undef`. Returns `None` for any non-matching argument so the caller keeps its
/// default translation (cargo check backstops a real mismatch).
fn widen_union_arg(arg: &Argument, param_ty: Option<&Type>, ctx: &Ctx<'_>) -> Option<Expr> {
    use std::collections::HashSet;

    // The parameter must be a scalar-union enum (`Big`).
    let big_name = last_type_ident(param_ty?)?;
    let big = ctx
        .registry()
        .union_enums
        .get(&Ident::new(&big_name, Span::call_site()))?;
    // The argument is `obj.field` where `field` is an optional scalar union —
    // the emitted field is `Option<Small>`.
    let sm = arg.as_expression().and_then(|e| match e {
        Expression::StaticMemberExpression(sm) => Some(sm),
        _ => None,
    })?;
    let Expression::Identifier(obj) = &sm.object else {
        return None;
    };
    let obj_name = bindings::snake(obj.name.as_str()).to_string();
    let struct_name = ctx
        .local_type(&obj_name)?
        .segments
        .last()?
        .ident
        .to_string();
    let field = bindings::snake(sm.property.name.as_str()).to_string();
    if !ctx.field_optional(&struct_name, &field) {
        return None;
    }
    let small_name = last_type_ident(ctx.field_type(&struct_name, &field)?)?;
    if small_name == big_name {
        return None;
    }
    let small = ctx
        .registry()
        .union_enums
        .get(&Ident::new(&small_name, Span::call_site()))?;
    // `Small`'s variants must be a name-subset of `Big`'s — each variant wraps
    // one TS scalar keyword with a fixed Rust type, so a same-name variant
    // carries the identical payload and the conversion is loss-free.
    let big_names: HashSet<String> = big.variants.iter().map(|v| v.ident.to_string()).collect();
    if !small
        .variants
        .iter()
        .all(|v| big_names.contains(&v.ident.to_string()))
    {
        return None;
    }
    // `None` (undefined) maps to `Big`'s `Undef`, else `Null`.
    let none_name = big_names
        .iter()
        .find(|n| n.as_str() == "Undef")
        .or_else(|| big_names.iter().find(|n| n.as_str() == "Null"))?;
    let small_ident = Ident::new(&small_name, Span::call_site());
    let big_ident = Ident::new(&big_name, Span::call_site());
    let none_ident = Ident::new(none_name, Span::call_site());
    let arg_expr = translate_expr(arg.as_expression()?, ctx);
    let arms: Vec<Arm> = small
        .variants
        .iter()
        .map(|v| {
            let vid = &v.ident;
            match &v.fields {
                syn::Fields::Unnamed(_) => {
                    parse_quote!(::std::option::Option::Some(crate::#small_ident::#vid(x)) => crate::#big_ident::#vid(x))
                }
                _ => parse_quote!(::std::option::Option::Some(crate::#small_ident::#vid) => crate::#big_ident::#vid),
            }
        })
        .collect();
    Some(parse_quote!(
        match #arg_expr {
            #(#arms,)*
            None => crate::#big_ident::#none_ident,
        }
    ))
}

/// The last path-segment identifier of a `syn::Type` that is a plain path
/// (`__DsUnion…`, an interface name), or `None` for any other shape.
fn last_type_ident(ty: &Type) -> Option<String> {
    types::type_path(ty)?
        .segments
        .last()
        .map(|s| s.ident.to_string())
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

/// Render a numeric argument as an ES `Number::toString` string (ryu_js),
/// routing around Rust's `f64` `Display`, which differs from ECMAScript
/// (`1e21` → `1e+21`, `1e-7`, `-0` → `0`). Shared by the console.log /
/// template format points and the `String.prototype.method.call(n)` idiom's
/// `ToString(n)` step. Its presence in the output flags `needs_ryu_js`.
fn number_arg_to_es_string(arg: &Argument, ctx: &Ctx<'_>) -> Expr {
    let e = if let Some(expr) = arg.as_expression() {
        translate_number_to(expr, NumberFlavor::F64, ctx)
    } else {
        translate_argument(arg, ctx)
    };
    parse_quote!(crate::__ds::number_to_string(#e))
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
        // A numeric receiver (`String.prototype.trim.call(1e21)`) is ES
        // `Number::toString`, not Rust `Display` — see `number_arg_to_es_string`.
        _ if is_number_arg(arg, ctx) => number_arg_to_es_string(arg, ctx),
        _ => {
            let e = translate_argument(arg, ctx);
            parse_quote!(::std::format!("{}", #e))
        }
    }
}
