//! Call expressions: `console.log` → `println!`, built-in static/instance
//! methods, global conversions, and plain user calls.

use oxc_ast::ast::{Argument, CallExpression, Expression};
use proc_macro2::Span;
use syn::{parse_quote, Expr, Type};

use super::super::bindings;
use super::super::builtins;
use super::super::context::Ctx;
use super::super::types;
use super::fmt_merge;
use super::{translate_argument, translate_argument_init, translate_expr};

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
    }
    // Global conversion functions: `String(x)`, `parseInt(s)`, `parseFloat(s)`.
    if let Expression::Identifier(id) = &call.callee {
        if let Some(expr) = builtins::global_function(id, call.arguments.as_slice(), ctx) {
            return expr;
        }
    }
    // A method call (`s.toUpperCase()`) maps the method name, not the receiver.
    if let Expression::StaticMemberExpression(sm) = &call.callee {
        if let Some(expr) = builtins::array_method(sm, call.arguments.as_slice(), ctx) {
            return expr;
        }
        if let Some(expr) = builtins::string_method(sm, call.arguments.as_slice(), ctx) {
            return expr;
        }
        if let Some(expr) = builtins::number_method(sm, call.arguments.as_slice(), ctx) {
            return expr;
        }
        if let Some(method) = builtins::map_method(&sm.property.name) {
            let obj = translate_expr(&sm.object, ctx);
            let args: Vec<Expr> = call
                .arguments
                .iter()
                .map(|a| translate_argument(a, ctx))
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
fn prototype_method_call<'a>(callee: &'a Expression) -> Option<(&'static str, &'a str)> {
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
