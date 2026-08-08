//! Per-function engine degradation: a TS function the static translator cannot
//! lower keeps its Rust signature, but the body marshals its arguments to
//! `serde_json::Value` and dispatches to the embedded QuickJS engine via
//! `__ds::engine::call_fn` / `call_module_fn`. Extracted from `functions/mod.rs`.

use oxc_ast::ast::{
    ArrowFunctionExpression, FormalParameters, Function, Statement, TSTypeAnnotation,
};
use quote::{format_ident, quote};
use syn::{parse_quote, Block, Expr, FnArg, Ident, ItemFn, ReturnType, Stmt};

use super::super::context::{Ctx, Locals, Narrow};
use super::super::name_table::NameTable;
use super::super::registry::TypeRegistry;
use super::super::{analysis, bindings, expressions, types};

// Function-skeleton helpers kept in `mod.rs` (a child module reaches its
// parent's private items, so no visibility change is needed there).
use super::{
    arrow_expression_block, arrow_return_type, body_locals, fn_output, module_mode, return_path_of,
    translate_body, translate_params,
};

/// Whether a nested `function` declaration should lower to a closure rather
/// than a Rust nested fn item. A nested fn that captures an outer local has
/// closure semantics a Rust fn item cannot express (`fn helper() { …x }` is
/// E0434 — a fn item cannot close over its environment), so it must become
/// `let name = |..| { .. };`. A non-capturing nested fn stays a fn item —
/// zero-cost, and recursive (a closure cannot name itself). `async`/
/// `generator`/generic nested fns stay fn items (closures carry no generic
/// params; rare in the test262/WPT helper convention regardless).
pub(super) fn nested_fn_should_be_closure(
    func: &Function,
    outer: &Locals,
    registry: &TypeRegistry,
    names: &NameTable<'_>,
) -> bool {
    if func.r#async || func.generator || func.type_parameters.is_some() {
        return false;
    }
    let Some(body) = func.body.as_deref() else {
        return false;
    };
    let analysis = analysis::analyze(
        &body.statements,
        names,
        &registry.mut_methods,
        &registry.ref_params,
    );
    // Captures an outer local: a referenced name that resolves in the
    // enclosing function's bindings (not the nested fn's own params/locals). A
    // Rust fn item can close over neither a read nor a write, so `use_counts`
    // (reads) and `mutated`/`member_mutated` (writes) are all checked — a
    // pure-write capture like `function h() { x = 1; }` (the WPT
    // `addEventListener` handler pattern) is E0434 too. `bindings` (not
    // `types`) is the capture set — a `var x;` / `let n = 0` with no derivable
    // type path is still a binding a nested fn closes over.
    let captures_outer = analysis
        .use_counts
        .keys()
        .chain(analysis.mutated.iter())
        .chain(analysis.member_mutated.iter())
        .any(|k| outer.bindings.contains(k));
    if !captures_outer {
        return false;
    }
    // A self-referential `let name = |..| { name(..) };` cannot compile (the
    // binding is not in scope inside its own initializer), so a recursive
    // capturing fn stays a fn item and surfaces E0434 honestly.
    if let Some(id) = &func.id {
        let self_name = names.of_binding(id).to_string();
        if analysis.use_counts.contains_key(&self_name) {
            return false;
        }
    }
    true
}

/// Lower a nested `function` declaration to `let [mut] name = |params| -> ret
/// { body };` — a closure that captures its outer locals. Calls resolve
/// unchanged (`name(args)`). `mut` is added when the closure mutates a
/// captured binding (FnMut), so the binding is callable; a non-mutating
/// closure (Fn) takes a plain `let`.
pub(super) fn nested_fn_closure(
    func: &Function,
    outer: &Locals,
    registry: &TypeRegistry,
    names: &NameTable<'_>,
) -> Stmt {
    let name = func
        .id
        .as_ref()
        .map_or_else(|| format_ident!("__ds_anon"), |id| names.of_binding(id));
    let mut locals = body_locals(&func.params, func.body.as_deref(), registry, names);
    let inputs: Vec<FnArg> = translate_params(&func.params, &locals, registry, names);
    let output = fn_output(func, registry);
    let return_path = func.return_type.as_deref().and_then(return_path_of);
    let defaults: Vec<Stmt> = func
        .params
        .items
        .iter()
        .filter_map(|fp| {
            let init = fp.initializer.as_deref()?;
            let pname = names.of_pattern(&fp.pattern);
            let default = expressions::translate_expr(
                init,
                &Ctx::new(&locals, registry, &Narrow::default(), names),
            );
            Some(parse_quote!(let #pname = #pname.unwrap_or(#default);))
        })
        .collect();
    let body_stmts: &[Statement] = func.body.as_deref().map_or(&[], |b| &b.statements[..]);
    let mut block = translate_body(
        body_stmts,
        &mut locals,
        registry,
        &Narrow::default(),
        return_path.as_ref(),
        names,
    );
    if !defaults.is_empty() {
        let mut stmts = defaults;
        stmts.extend(block.stmts);
        block.stmts = stmts;
    }
    // A closure that mutates a captured binding is FnMut and needs a `mut`
    // binding to call; one that only reads captures is Fn (plain `let`).
    let analysis = analysis::analyze(
        body_stmts,
        names,
        &registry.mut_methods,
        &registry.ref_params,
    );
    let needs_mut = analysis
        .mutated
        .iter()
        .chain(analysis.member_mutated.iter())
        .any(|k| outer.get(k).is_some());
    // Free-fn params are `FnArg::Typed(name: type)`; the `name: type` PatType
    // is a valid closure `Pat`.
    let pats: Vec<syn::Pat> = inputs
        .into_iter()
        .map(|a| match a {
            FnArg::Typed(t) => syn::Pat::Type(t),
            FnArg::Receiver(_) => unreachable!("nested fn has no `self`"),
        })
        .collect();
    if needs_mut {
        parse_quote!(let mut #name = |#(#pats),*| #output #block;)
    } else {
        parse_quote!(let #name = |#(#pats),*| #output #block;)
    }
}

/// A per-function engine degradation site: keep the Rust signature (params,
/// return type, generics) but replace the body with a `__ds::engine::call_fn`
/// invocation. Each argument is marshaled to `serde_json::Value` (every emitted
/// struct/enum derives `Serialize`/`Deserialize` in this mode), and a non-unit
/// return is marshaled back. `__DS_MODULE_JS` is the whole module's
/// annotation-stripped JS, eval'd per call so the function's helper
/// dependencies are in scope (a dynamic function usually leans on other module
/// functions; the engine defines all of them before the call).
pub(super) fn engine_fn_item(
    name: &Ident,
    ts_name: &str,
    func: &Function,
    registry: &TypeRegistry,
    names: &NameTable<'_>,
) -> ItemFn {
    engine_fn_item_from_sig(
        name,
        ts_name,
        &func.params,
        func.return_type.as_deref(),
        func.r#async,
        func.type_parameters.as_deref(),
        registry,
        names,
    )
}

/// The const-arrow variant of [`engine_fn_item`]: same degraded stub, but the
/// signature comes from an `ArrowFunctionExpression` (no `id`/`generator`; the
/// const binding already named the function). Shared via
/// [`engine_fn_item_from_sig`] so a const-arrow fn whose signature carries an
/// unmappable type (`<T>(data, type): OutputByType[T]` — B6d #312) degrades
/// the same way a `function` declaration does.
pub(super) fn engine_arrow_fn_item(
    name: &Ident,
    ts_name: &str,
    arrow: &ArrowFunctionExpression,
    registry: &TypeRegistry,
    names: &NameTable<'_>,
) -> ItemFn {
    engine_fn_item_from_sig(
        name,
        ts_name,
        &arrow.params,
        arrow.return_type.as_deref(),
        arrow.r#async,
        arrow.type_parameters.as_deref(),
        registry,
        names,
    )
}

/// The shared core of [`engine_fn_item`] / [`engine_arrow_fn_item`]: emit a
/// degraded Rust `fn` whose body marshals its arguments to `serde_json::Value`
/// and dispatches to `__ds::engine::call_fn` / `call_module_fn`. A signature
/// type the static translator cannot express becomes `serde_json::Value` (the
/// marshal type), so the signature is concrete rather than `_`. Both
/// [`Function`] and [`ArrowFunctionExpression`] expose the same field shape
/// (`params` / `return_type` / `r#async` / `type_parameters`), so a `function`
/// declaration and a const-arrow fn lower to the same stub shape.
#[allow(clippy::too_many_arguments)]
pub(super) fn engine_fn_item_from_sig(
    name: &Ident,
    ts_name: &str,
    params: &FormalParameters,
    return_type: Option<&TSTypeAnnotation>,
    is_async: bool,
    type_parameters: Option<&oxc_ast::ast::TSTypeParameterDeclaration>,
    registry: &TypeRegistry,
    names: &NameTable<'_>,
) -> ItemFn {
    // Degraded signature: a param/return type the static translator cannot
    // express (unknown/indexed access/…) becomes `serde_json::Value` — the
    // marshal type — so the signature is concrete rather than `_`. An
    // expressible type maps normally, so a degraded function mixing the two
    // keeps the expressible params concrete.
    let inputs: Vec<FnArg> = params
        .items
        .iter()
        .map(|fp| {
            let pname = names.of_pattern(&fp.pattern);
            let mut ty = fp.type_annotation.as_deref().map_or_else(
                || parse_quote!(::serde_json::Value),
                |ta| types::translate_type_degraded_for_signature(&ta.type_annotation, registry),
            );
            // An optional (`?:`) or default-initialized param is `Option<T>`,
            // mirroring translate_params — a static caller passes an
            // `Option<Js2XmlOptions>` for an `options?` param, so the degraded
            // signature must accept `Option<_>` or the call site mismatches.
            if fp.optional || fp.initializer.is_some() {
                ty = parse_quote!(Option<#ty>);
            }
            parse_quote!(#pname: #ty)
        })
        .collect();
    let output: ReturnType = if is_async {
        // An `async fn` degraded to QuickJS returns a JS `Promise`, which
        // cannot marshal across the serde boundary to Rust's `DsPromise<T>` (a
        // `Pin<Box<dyn Future>>` — not `DeserializeOwned`). The degraded stub
        // keeps the `async` keyword (see below) so the entry's injected
        // `__ds_main().await` resolves, but drops the JS return type: the stub
        // is an `async fn` wrapping a sync `__ds::engine::call_fn`, returning
        // `impl Future<Output = ()>`; the JS Promise the body returns resolves
        // inside QuickJS's event loop during that sync `call_fn`, unreachable
        // (and unneeded) from Rust. The static [`fn_output`] path unwraps
        // `Promise<T>` to `T` for the body's actual return; the degraded stub
        // has no body return to unwrap. (A static caller that `.await`s a
        // degraded async fn and uses the value is an inherent async-over-
        // degrade contradiction; the conformance entry `main` never does.)
        ReturnType::Default
    } else {
        return_type.map_or(ReturnType::Default, |rt| {
            let ty = types::translate_type_degraded_for_signature(&rt.type_annotation, registry);
            parse_quote!(-> #ty)
        })
    };
    let generics: Vec<Ident> = type_parameters.map_or_else(Vec::new, |tp| {
        tp.params
            .iter()
            .map(|p| bindings::type_ident(&p.name.name))
            .collect()
    });
    // Marshal each argument to `serde_json::Value` (Serialize is derived on
    // every emitted struct/enum in per-function mode).
    let args: Vec<Expr> = params
        .items
        .iter()
        .map(|fp| {
            let pname = names.of_pattern(&fp.pattern);
            parse_quote!(serde_json::to_value(&#pname).unwrap_or(serde_json::Value::Null))
        })
        .collect();
    let ts_lit = syn::LitStr::new(ts_name, proc_macro2::Span::call_site());
    // Module mode: the file's annotation-stripped JS carries ESM imports, so
    // `call_fn`'s script-mode `eval` cannot run it (ESM imports are not parsed
    // in script mode) — route to `call_module_fn` (the module loader resolves
    // the imports), keyed by the file's import specifier. Script-eval mode
    // keeps `call_fn` with the `__DS_MODULE_JS` const.
    let call: syn::Expr = if module_mode() {
        let spec = crate::translator::imports::current_module_specifier()
            .unwrap_or_else(|| "__ds_entry".to_string());
        let spec_lit = syn::LitStr::new(&spec, proc_macro2::Span::call_site());
        parse_quote!(crate::__ds::engine::call_module_fn(#spec_lit, #ts_lit, &__ds_args))
    } else {
        parse_quote!(crate::__ds::engine::call_fn(#ts_lit, __DS_MODULE_JS, &__ds_args))
    };
    // A unit/void return discards the engine's `Value`; a typed return
    // deserializes it back to the signature's Rust type.
    let block: Block = match &output {
        ReturnType::Default => parse_quote!({
            let __ds_args: Vec<serde_json::Value> = vec![#(#args),*];
            let _ = #call;
        }),
        ReturnType::Type(_, ret_ty) => parse_quote!({
            let __ds_args: Vec<serde_json::Value> = vec![#(#args),*];
            let __ds_ret = #call;
            serde_json::from_value::<#ret_ty>(__ds_ret)
                .expect("engine return value did not deserialize to the declared return type")
        }),
    };
    // Keep the `async` keyword on a degraded `async fn` so the entry's
    // injected `__ds_main().await` resolves: the stub is an `async fn` wrapping
    // a sync `call_fn`, returning `impl Future<Output = ()>` (immediately ready
    // — `call_fn` is synchronous), and `.await` yields `()`. Without the
    // keyword the stub is a sync `fn` returning `()`, and the injected
    // `.await` fails (`() is not a future`).
    let async_kw: Option<proc_macro2::TokenStream> = is_async.then(|| quote!(async));
    if generics.is_empty() {
        parse_quote! {
            #async_kw fn #name(#(#inputs),*) #output #block
        }
    } else {
        parse_quote! {
            #async_kw fn #name<#(#generics),*>(#(#inputs),*) #output #block
        }
    }
}

/// `export const name = <T>(params): ret => body` → a `fn name<T>(params) -> ret
/// { body }`. The const binding names the function; the arrow supplies the
/// generic type parameters, params, and return type. A type predicate
/// (`arg is X`) returns `bool` — the runtime shape of a TS type guard. An
/// expression body (`=> expr`) becomes the block's trailing expression; a block
/// body (`=> { … }`) maps through [`translate_body`]. Mirrors
/// [`translate_function`] so a const arrow compiles identically to a `function`
/// declaration.
pub(super) fn translate_const_arrow_to_fn(
    name: Ident,
    arrow: &ArrowFunctionExpression,
    registry: &TypeRegistry,
    names: &NameTable<'_>,
) -> ItemFn {
    let mut locals = body_locals(&arrow.params, Some(arrow.body.as_ref()), registry, names);
    let inputs = translate_params(&arrow.params, &locals, registry, names);
    let output = arrow_return_type(arrow.return_type.as_deref());
    let return_path = arrow.return_type.as_deref().and_then(return_path_of);
    let block = if arrow.expression {
        arrow_expression_block(
            &arrow.body,
            &locals,
            registry,
            &output,
            return_path.as_ref(),
            names,
        )
    } else {
        translate_body(
            &arrow.body.statements[..],
            &mut locals,
            registry,
            &Narrow::default(),
            return_path.as_ref(),
            names,
        )
    };
    let generics: Vec<Ident> = arrow
        .type_parameters
        .as_deref()
        .map_or_else(Vec::new, |tp| {
            tp.params
                .iter()
                .map(|p| bindings::type_ident(&p.name.name))
                .collect()
        });
    if generics.is_empty() {
        parse_quote! {
            fn #name(#(#inputs),*) #output #block
        }
    } else {
        parse_quote! {
            fn #name<#(#generics),*>(#(#inputs),*) #output #block
        }
    }
}
