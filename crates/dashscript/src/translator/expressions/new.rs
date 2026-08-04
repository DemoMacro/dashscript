//! `new Foo(args)` → `Foo::new(args)`.
use oxc_ast::ast::{
    Argument, ArrayExpressionElement, Expression, NewExpression, ObjectExpression,
    ObjectPropertyKind, PropertyKey,
};
use quote::format_ident;
use syn::{parse_quote, Expr, Ident};

use super::super::bindings;
use super::super::builtins;
use super::super::context::Ctx;
use super::super::globals::error_ctor_name;
use super::super::types;
use super::{array_elem_arg, array_elem_expr, array_owned_expr};

/// `new Foo(args)` → `Foo::new(args)`. Only an identifier callee (a user class)
/// maps; `new foo.Bar()` or `new (factory())()` fall back to `todo!()`.
///
/// `new Map()` / `new Set()` are special-cased to empty Rust collections — the
/// no-arg form only; `new Map(entries)` needs a `(K, V)` pair iterable (not yet
/// supported), so it falls through to `Map::new(…)` and surfaces as a `cargo
/// check` error honestly.
pub(super) fn new_expr(n: &NewExpression, ctx: &Ctx<'_>) -> Expr {
    // `new Temporal.<Type>(isoFields…)` → `temporal_rs::<Type>::new(…)`. The
    // member callee `Temporal.<Type>` resolves to a Temporal ISO-field
    // constructor (PlainDate/PlainDateTime/PlainTime/PlainYearMonth);
    // `builtins::temporal_new` casts the fields and unwraps the Result.
    // Intercepted before the Identifier arm and the generic `Foo::new` path
    // (which would emit `Temporal::<Type>::new` — E0425, `Temporal` is not a
    // Rust identifier in scope).
    if let Some(e) = builtins::temporal_new(&n.callee, &n.arguments, ctx) {
        return e;
    }
    // `new RegExp("pat"[, flags])` — the ES RegExp constructor, lowered to the
    // same `__ds::regex` helper as `/pat/` literals. Intercepted before the
    // generic `Foo::new` lowering, which would emit `RegExp::new` (E0425).
    if let Expression::Identifier(id) = &n.callee {
        if id.name.as_str() == "RegExp" {
            if let Some(e) = builtins::reg_exp_constructor(&n.arguments, ctx) {
                return e;
            }
        }
        // `new Promise((resolve, reject) => { … })` — the ES `Promise`
        // constructor (T3 stage 2c). The executor runs synchronously under a
        // clonable `DsResolver`; `resolve(x)`/`reject(reason)` settle a shared
        // cell the returned `DsPromise<T>` polls. Intercepted before the generic
        // `Foo::new` path (which would emit `Promise::new(…)` — E0433, no such
        // Rust type). The `Promise` runtime dep is flagged by the
        // `__ds::DsPromise` marker probe; a non-function/async executor returns
        // `None` and falls through honestly. `new Promise` reaches `new` outside
        // reflection fixtures, so the classify `Identifier => Mapped` arm covers
        // it (no engine degrade — unlike `Date`, `Promise` now has a static ctor).
        if id.name.as_str() == "Promise" {
            if let Some(e) = builtins::promise_ctor(&n.arguments, ctx) {
                return e;
            }
        }
        // `new Worker(handler)` — a Web Worker isolate (Direction D, D1): spawns a
        // thread running `handler` for each message received. Lowered before
        // the generic `Foo::new` path (which would emit `Worker::new` — E0425,
        // `Worker` is the runtime type, not a user class). File-based
        // `new Worker('./w.ts')` (worker-entry translation + build-time dep
        // scan) is a later batch reusing this runtime.
        if id.name.as_str() == "Worker" {
            if let Some(arg) = n.arguments.first() {
                let handler = array_elem_arg(arg, ctx);
                return worker_ctor(arg, handler);
            }
        }
        // `new <TypedArray>(n)` / `<TypedArray>([1, 2, 3])` — an ES typed array
        // of a fixed-width Rust scalar (`Int8Array`→`i8`, …, `Float64Array`→`f64`;
        // see `typed_array_elem_type`). Two constructor forms lower: a numeric
        // length `new Int32Array(n)` → `vec![0_i32; n as usize]` (n zeroed
        // elements), and `new Int32Array([1, 2, 3])` → a copy with each element
        // cast to the elem type (the typed-array-from-array case). An empty
        // `new Int32Array()` is an empty vec. `Float16Array` (no stable Rust
        // `f16`) and the BigInt arrays (DashScript has no BigInt literal) are
        // not mapped — they fall through to the generic `Foo::new(…)` path and
        // surface at `cargo check` honestly.
        if let Some(elem) = super::typed_array_elem_type(id.name.as_str()) {
            let ty: Ident = format_ident!("{}", elem);
            if n.arguments.is_empty() {
                return parse_quote!(::std::vec::Vec::<#ty>::new());
            }
            if n.arguments.len() == 1 {
                let arg = &n.arguments[0];
                let e = array_elem_arg(arg, ctx);
                // A from-iterable source: an array literal
                // (`new Int32Array([1, 2, 3])`), a member access
                // (`new Uint8Array(t.bytes)` on a `Vec<f64>` property), or a
                // local known to be a `Vec<T>` (`new Uint8Array(buf)`). Each
                // element is cast to the typed array's elem type. The
                // numeric-length path (`vec![0; n as usize]`) is for a numeric
                // arg — a `NumericLiteral`, a `BinaryExpression` like
                // `h.length + 4`, or a known-number/unknown local — so a `Vec`
                // arg never takes `(Vec) as usize` (E0605).
                let from_iterable = match arg.as_expression() {
                    Some(Expression::ArrayExpression(_)) => true,
                    Some(expr) if expr.as_member_expression().is_some() => true,
                    Some(Expression::Identifier(id)) => {
                        let name = bindings::snake(&id.name).to_string();
                        ctx.local_type(&name)
                            .is_some_and(|p| p.segments.last().is_some_and(|s| s.ident == "Vec"))
                    }
                    _ => false,
                };
                if from_iterable {
                    return parse_quote!(
                        (#e).into_iter().map(|x| x as #ty).collect::<::std::vec::Vec<#ty>>()
                    );
                }
                // `new Int32Array(n)` — n zeroed elements. The `0_<ty>` literal
                // is parsed as a single suffixed literal (not `0_` + ident) so
                // prettyplease prints `0_u8`/`0_f64`/…, matching the u8 path.
                let zero: Expr =
                    syn::parse_str(&format!("0_{elem}")).expect("typed-array zero literal");
                return parse_quote!(::std::vec![#zero; (#e) as usize]);
            }
        }
        // `new TextEncoder()` / `new TextDecoder()` — the WHATWG Encoding API
        // (a WinterTC Web API). Stateless global constructors; `.encode`/
        // `.decode` map to the `__ds::TextEncoder`/`__ds::TextDecoder` impls
        // (UTF-8). Intercepted before the generic `Foo::new` path, which would
        // emit `TextEncoder::new` (E0425 — no such Rust type). The `Encoding`
        // runtime dep is flagged by the `__ds::TextEncoder` marker probe, which
        // injects both struct defs into `__ds.rs`.
        if builtins::encoding_ctor_type(id.name.as_str()).is_some() {
            // `new TextDecoder(label?, options?)` carries a label and
            // `{ fatal, ignoreBOM }` options; `new TextEncoder()` is argless.
            if id.name.as_str() == "TextDecoder" {
                return text_decoder_ctor(n.arguments.as_slice(), ctx);
            }
            let name = bindings::type_ident(&id.name);
            return parse_quote!(crate::__ds::#name::new());
        }
        // `new URLSearchParams(...)` / `new URL(...)` — the WHATWG URL API
        // (a WinterTC Web API). `URLSearchParams` parses a query string (or
        // empty); `URL` parses a full URL, optionally against a base.
        // Intercepted before the generic `Foo::new` path (which would emit
        // `URLSearchParams::new`/`Url::new` — E0433, no such Rust type). The
        // `Url` runtime dep is flagged by the `__ds::DsUrl`/`__ds::DsUrlSearchParams`
        // marker probe, which injects both struct defs into `__ds.rs`.
        // Instance methods (`.get`/`.has`/…) lower verbatim — each struct's
        // inherent methods already carry ES-matching signatures.
        if builtins::url_ctor_type(id.name.as_str()).is_some() {
            return match id.name.as_str() {
                "URL" => url_ctor(n.arguments.as_slice(), ctx),
                _ => url_search_params_ctor(n.arguments.as_slice(), ctx),
            };
        }
        // `new URLPattern(input[, baseURL])` — the WHATWG URLPattern API (a
        // WinterTC Web API). A string `input` is a constructor string; an
        // undefined/absent input is the empty pattern; any other expression
        // (`new URL(…)`, a variable) is ToString'd. Intercepted before the
        // generic `Foo::new` path (which would emit `URLPattern::new` — E0433).
        // The `URLPattern` runtime dep is flagged by the `__ds::DsURLPattern`
        // marker probe; a pattern that fails to compile panics a `TypeError`.
        if builtins::urlpattern_ctor_type(id.name.as_str()).is_some() {
            return urlpattern_ctor(n.arguments.as_slice(), ctx);
        }
        // `new EventTarget()` / `new Event(type[, init])` — the WHATWG DOM
        // Events API (a WinterTC Web API). `EventTarget` is argless; `Event`
        // takes a type string + an optional `{ bubbles, cancelable }` init.
        // Intercepted before the generic `Foo::new` path (which would emit
        // `EventTarget::new`/`Event::new` — E0433, no such Rust types). The
        // `EventTarget` runtime dep is flagged by the `__ds::DsEvent` marker
        // probe, which injects `DsEventTarget`/`DsEvent`/`DsEventInit` into
        // `__ds.rs`. Instance methods (`addEventListener`/`dispatchEvent`/…)
        // dispatch in the call path; event properties (`.type`/`.bubbles`/…) in
        // the member path.
        if builtins::event_target_ctor_type(id.name.as_str()).is_some() {
            return match id.name.as_str() {
                "EventTarget" => parse_quote!(crate::__ds::DsEventTarget::new()),
                "Event" => event_ctor(n.arguments.as_slice(), ctx),
                "AbortController" => parse_quote!(crate::__ds::DsAbortController::new()),
                _ => unreachable!(),
            };
        }
        // `new Headers(init?)` — the WHATWG FETCH `Headers` API (a WinterTC Web
        // API). `init` may be absent, a Record `{ name: value, … }`, or a
        // `[[name, value], …]` sequence; each name/value is ToString-coerced.
        // Intercepted before the generic `Foo::new` path (which would emit
        // `Headers::new` — E0433). The `Headers` runtime dep is flagged by the
        // `__ds::DsHeaders` marker probe; a non-record/non-sequence init panics
        // the `TypeError` ES throws. Instance methods (`.get`/`.set`/…) dispatch
        // in the call path.
        if builtins::headers_ctor_type(id.name.as_str()).is_some() {
            return builtins::headers_ctor(n.arguments.as_slice(), ctx);
        }
        // `new Blob(parts?, options?)` — the WHATWG FileAPI `Blob` API (a
        // WinterTC Web API). `parts` is a sequence of `string`/`BufferSource`/
        // `Blob`; `options.type` carries the MIME. Intercepted before the
        // generic `Foo::new` path (which would emit `Blob::new` — E0433). The
        // `Blob` runtime dep is flagged by the `__ds::DsBlob` marker probe;
        // instance methods (`slice`/`text`/`arrayBuffer`/`bytes`) and
        // `size`/`type` accessors dispatch in the call/member paths.
        if builtins::blob_ctor_type(id.name.as_str()).is_some() {
            return builtins::blob_ctor(n.arguments.as_slice(), ctx);
        }
        // `new File(bits, name, options?)` — the WHATWG FileAPI `File` API (a
        // WinterTC Web API). A `File` is a `Blob` with a `name` and a
        // `lastModified`; `bits` reuses the `Blob` parts collector. Intercepted
        // before the generic `Foo::new` path (which would emit `File::new` —
        // E0433). The `File` runtime dep is flagged by the `__ds::DsFile` marker
        // probe (and pulls `Blob` alongside, since `DsFile` wraps `DsBlob`); the
        // inherited `Blob` methods/accessors dispatch via `is_blob_local`
        // (widened to accept a `DsFile`), and `name`/`lastModified` dispatch in
        // the member path.
        if builtins::file_ctor_type(id.name.as_str()).is_some() {
            return builtins::file_ctor(n.arguments.as_slice(), ctx);
        }
        // `new FormData()` — the WHATWG FETCH `FormData` API (a WinterTC Web
        // API). The no-arg form builds an empty ordered `(name, value)` list
        // (`crate::__ds::DsFormData`); the ES `new FormData(form)` (an HTML
        // `form` element) has no static lowering. Intercepted before the generic
        // `Foo::new` path (which would emit `FormData::new` — E0433). The
        // `FormData` runtime dep is flagged by the `__ds::DsFormData` marker
        // probe (and pulls `File` → `Blob` alongside, since a value may be a
        // `File`); instance methods (`append`/`has`/`delete`/`set`) dispatch in
        // the call path.
        if builtins::form_data_ctor_type(id.name.as_str()).is_some() {
            return builtins::form_data_ctor(n.arguments.as_slice(), ctx);
        }
        // `new ReadableStream([{ start(controller) { … } }])` — the WHATWG
        // Streams API (a WinterTC Web API). The push-source form maps
        // (`controller.enqueue`/`.close` + `getReader` + `await reader.read()`);
        // any other shape degrades to an empty stream (an honest partial).
        // Intercepted before the generic `Foo::new` path (which would emit
        // `ReadableStream::new` — E0433). The `Streams` runtime dep is flagged
        // by the `__ds::DsReadableStream` marker probe; instance methods
        // (`getReader`/`read`/`enqueue`/`close`) dispatch in the call path.
        if builtins::streams_ctor_type(id.name.as_str()).is_some() {
            return builtins::readable_stream_ctor(n.arguments.as_slice(), ctx);
        }
        // `new CompressionStream(format)` — the WHATWG Streams compression API
        // (a WinterTC Web API). The transform is internal (`flate2`), so the
        // `writable`/`readable` sides lower as plain field access + generic
        // method calls; only the constructor needs a dispatch arm. A
        // `gzip`/`deflate`/`deflate-raw` literal maps; `brotli` or a non-literal
        // format returns `None` and falls through to the generic `Foo::new`
        // path (E0433 — honest unsupported). Intercepted before the generic path
        // (which would emit `CompressionStream::new` — E0433).
        if builtins::compression_ctor_type(id.name.as_str()).is_some() {
            if let Some(ctor) =
                builtins::compression_stream_ctor(id.name.as_str(), n.arguments.as_slice())
            {
                return ctor;
            }
        }
        // `new DOMException(message[, name])` — the WinterTC/HTML `DOMException`
        // (a Web API, never the engine). Unlike `new Error(msg)` — where `name`
        // derives from the constructor — a `DOMException`'s `name` is the SECOND
        // argument and defaults to `"Error"` when absent. `name` must be a string
        // literal (the legacy DOMException name set: `"NetworkError"`/
        // `"NotFoundError"`/…) to lower to the `&'static str` `DsError::new`
        // expects; it reuses the `DsError` value model `new Error` maps to, so as
        // a value `e.name`/`e.message`/`e.toString()` work (a later `e.name` reads
        // the `DsError` field via Rust type inference). A non-literal `name` or a
        // spread message arg has no static form and falls through to the generic
        // `Foo::new` path (E0433 — honest). Intercepted before `error_ctor_name`
        // (DOMException is not an Error subclass) and the generic path.
        if id.name.as_str() == "DOMException" {
            let name: Option<syn::LitStr> = match n.arguments.get(1) {
                None => Some(syn::LitStr::new("Error", proc_macro2::Span::call_site())),
                Some(Argument::StringLiteral(s)) => {
                    let v = s.value.to_string();
                    Some(syn::LitStr::new(&v, proc_macro2::Span::call_site()))
                }
                Some(_) => None,
            };
            let msg_is_spread = matches!(n.arguments.first(), Some(Argument::SpreadElement(_)));
            if let (Some(name), false) = (name, msg_is_spread) {
                let msg: Expr = match n.arguments.first() {
                    // A string-literal message lowers verbatim (`"m"`: `&str` is
                    // `impl Into<String>`); any other expression is ToString'd.
                    Some(Argument::StringLiteral(s)) => {
                        let lit =
                            syn::LitStr::new(s.value.as_str(), proc_macro2::Span::call_site());
                        parse_quote!(#lit)
                    }
                    Some(arg) => {
                        let e = array_elem_arg(arg, ctx);
                        parse_quote!((#e).to_string())
                    }
                    None => parse_quote!(::std::string::String::new()),
                };
                return parse_quote!(crate::__ds::DsError::new(#name, #msg));
            }
        }
        // `new Error("msg")` / `new TypeError(msg)` / `new Test262Error(msg)` —
        // an ES native Error constructor (or the test262 harness's
        // `Test262Error`). `throw new <X>(<literal>)` is intercepted earlier by
        // `thrown_error` (→ `panic_any(DsError)`); a throw with a dynamic message
        // and any `new <X>(…)` used as a value (`var e = new TypeError("x")`)
        // reach here. Lowered to a `DsError` value — `name`/`message` fields plus
        // `Display`, so `e.message`/`e.name`/`e.toString()` work. The message arg
        // (any type — ES stringifies it) becomes `.to_string()`; no arg is "".
        // Intercepted before the generic `Foo::new` path, which would emit
        // `Error::new(…)`/`Test262Error::new(…)` — E0433, no such Rust type.
        if let Some(ctor) = error_ctor_name(id.name.as_str()) {
            let msg: Expr = match n.arguments.first() {
                Some(arg) => {
                    let e = array_elem_arg(arg, ctx);
                    parse_quote!((#e).to_string())
                }
                None => parse_quote!(::std::string::String::new()),
            };
            let ctor_lit = syn::LitStr::new(ctor, proc_macro2::Span::call_site());
            return parse_quote!(crate::__ds::DsError::new(#ctor_lit, #msg));
        }
    }
    let Some(name) = class_name(&n.callee) else {
        return parse_quote!(::core::todo!());
    };
    if n.arguments.is_empty() {
        // `WeakMap`/`WeakSet` lower to the same strong-collection backing as
        // `Map`/`Set` — DashScript has no GC-precise weak refs (a `WeakMap`
        // keyed by `Uint8Array` is a `HashMap<Vec<u8>, V>`). The constructor's
        // type arguments carry over as a turbofish so an unannotated binding
        // (`let m = new Map<string, T>()`) infers its type.
        let targs = n.type_arguments.as_deref();
        match name.to_string().as_str() {
            "Map" | "WeakMap" => match targs.map(|a| &a.params).filter(|p| p.len() == 2) {
                Some(p) => {
                    let k = types::translate_type(&p[0]);
                    let v = types::translate_type(&p[1]);
                    if types::is_f64_type(&k) {
                        return parse_quote!(::std::collections::HashMap::<crate::__ds::DsF64Key, #v>::new());
                    }
                    return parse_quote!(::std::collections::HashMap::<#k, #v>::new());
                }
                None => return parse_quote!(::std::collections::HashMap::new()),
            },
            "Set" | "WeakSet" => match targs.and_then(|a| a.params.first()) {
                Some(e) => {
                    let e = types::translate_type(e);
                    if types::is_f64_type(&e) {
                        return parse_quote!(
                            ::std::collections::HashSet::<crate::__ds::DsF64Key>::new()
                        );
                    }
                    return parse_quote!(::std::collections::HashSet::<#e>::new());
                }
                None => return parse_quote!(::std::collections::HashSet::new()),
            },
            _ => {}
        }
    }
    // `new Map([[k, v], …])` → `HashMap::from([(k, v), …])` — a literal initial
    // map of [key, value] pairs, the common module-constant case. ES Map
    // accepts any iterable of pairs; a spread or non-array arg falls through to
    // the generic `Map::new(…)` path (a `cargo check` error honestly),
    // matching `new Set([a, b, …])`.
    if name.to_string().as_str() == "Map" {
        if let Some(e) = map_from_array_arg(&n.arguments, ctx) {
            return e;
        }
    }
    // `new Set([a, b, …])` → `HashSet::from([a, b, …])` — a literal initial set
    // of scalar values, the common module-constant case. ES Set accepts any
    // iterable; a spread or a non-array arg falls through to the generic
    // `Set::new(…)` path (a `cargo check` error honestly), matching `new Map()`.
    if name.to_string().as_str() == "Set" {
        if let Some(e) = set_from_array_arg(&n.arguments, ctx) {
            return e;
        }
    }
    // A class field typed `number` is `f64`, so the synthesized `new` takes
    // `f64` parameters — a flavor-promoted `i64` argument (`new Point3D(i, …)`
    // where `i` is an `i64` counter) is site-cast via `array_elem_arg`.
    let args: Vec<Expr> = n.arguments.iter().map(|a| array_elem_arg(a, ctx)).collect();
    parse_quote!(#name::new(#(#args),*))
}

/// `new TextDecoder(label?, options?)` →
/// `crate::__ds::TextDecoder::new(label, fatal, ignore_bom)`. The label
/// (default `"utf-8"`) resolves at runtime via `encoding_rs::for_label`; the
/// `options` object's `fatal`/`ignoreBOM` BooleanLiteral fields lower to the
/// `bool` ctor params (absent or non-literal → `false`, the ES default). A
/// non-string-literal label is ToString'd so a variable label still resolves.
fn text_decoder_ctor(args: &[Argument], ctx: &Ctx<'_>) -> Expr {
    let label: Expr = match args.first() {
        None => parse_quote!(::std::string::String::from("utf-8")),
        Some(Argument::StringLiteral(s)) => {
            let lit = syn::LitStr::new(s.value.as_str(), proc_macro2::Span::call_site());
            parse_quote!(::std::string::String::from(#lit))
        }
        Some(arg) => {
            let e = array_elem_arg(arg, ctx);
            parse_quote!((#e).to_string())
        }
    };
    let (fatal, ignore_bom) = match args.get(1) {
        Some(Argument::ObjectExpression(obj)) => decode_options(obj),
        _ => (parse_quote!(false), parse_quote!(false)),
    };
    parse_quote!(crate::__ds::TextDecoder::new(#label, #fatal, #ignore_bom))
}

/// `{ fatal: bool, ignoreBOM: bool }` → `(fatal, ignore_bom)` bool exprs. Only
/// BooleanLiteral field values lower statically (the common fixture shape); a
/// non-literal value or absent field defaults to `false`.
fn decode_options(obj: &ObjectExpression) -> (Expr, Expr) {
    let mut fatal: Expr = parse_quote!(false);
    let mut ignore_bom: Expr = parse_quote!(false);
    for kind in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(p) = kind else {
            continue;
        };
        let name = match &p.key {
            PropertyKey::StaticIdentifier(id) => id.name.as_str(),
            PropertyKey::StringLiteral(s) => s.value.as_str(),
            _ => continue,
        };
        let value = match &p.value {
            Expression::BooleanLiteral(b) => b.value,
            _ => continue,
        };
        let lit = syn::LitBool::new(value, proc_macro2::Span::call_site());
        match name {
            "fatal" => fatal = parse_quote!(#lit),
            "ignoreBOM" => ignore_bom = parse_quote!(#lit),
            _ => {}
        }
    }
    (fatal, ignore_bom)
}

/// `new Worker(handler)` constructor selection (Direction D).
///
/// - D1 one-way: a 1-arg handler `(msg) => { … }` → `Worker::new`.
/// - D2 bidirectional: a 2-arg handler `(msg, reply) => { reply.send(v); }` →
///   `Worker::new_with_reply`, so the worker can reply and main reads it via
///   `recv`.
///
/// The first param's type annotation is threaded through as a turbofish
/// `new_with_reply::<A, _>`: the worker deserializes each incoming message to
/// `A`, but the closure body alone may not pin `A` (e.g. `reply.send(msg * 2)`
/// — the generic `send` does not anchor `msg`'s type), so the `: number`
/// annotation is the anchor. An untyped 2-arg handler falls back to
/// `new_with_reply` and surfaces at `cargo check` if `A` stays ambiguous. Only
/// an inline arrow's arity is inspected; a named-function handler (an
/// identifier) defaults to one-way.
fn worker_ctor(arg: &Argument, handler: Expr) -> Expr {
    let Argument::ArrowFunctionExpression(a) = arg else {
        return parse_quote!(crate::__ds::Worker::new(#handler));
    };
    if a.params.items.len() < 2 {
        return parse_quote!(crate::__ds::Worker::new(#handler));
    }
    let msg_ty = a
        .params
        .items
        .first()
        .and_then(|p| p.type_annotation.as_deref())
        .map(|ta| types::translate_type(&ta.type_annotation));
    match msg_ty {
        Some(ty) => parse_quote!(crate::__ds::Worker::new_with_reply::<#ty, _>(#handler)),
        None => parse_quote!(crate::__ds::Worker::new_with_reply(#handler)),
    }
}

/// `new URLSearchParams(init?)` → `crate::__ds::DsUrlSearchParams::from_query
/// (init)` (one arg) or `::new()` (no arg). The init is coerced via ES
/// `ToString` (`es_to_string_arg`, same as the instance methods): a `number`/
/// `null`/`undefined` argument becomes its string form, a `String`/`&str`
/// passes through — `from_query` is generic over `AsRef<str>`. A
/// record/sequence/`URLSearchParams` init (ES also accepts those) is not yet
/// lowered; it falls through to the generic `Foo::new` path and surfaces at
/// `cargo check` honestly.
fn url_search_params_ctor(args: &[Argument], ctx: &Ctx<'_>) -> Expr {
    match args.first() {
        Some(arg) => {
            let init = builtins::es_to_string_arg(arg, ctx);
            parse_quote!(crate::__ds::DsUrlSearchParams::from_query(#init))
        }
        None => parse_quote!(crate::__ds::DsUrlSearchParams::new()),
    }
}

/// `new URL(input[, base])` → `crate::__ds::DsUrl::parse(input)` or
/// `::parse_with_base(input, base)`. Each arg may be a `String` or a `&str`
/// literal — both `parse`/`parse_with_base` are generic over `AsRef<str>`. ES
/// `new URL()` with no args throws `TypeError`; the no-arg case panics with
/// the same class name (the WPT verdict reads the panic prefix), rather than
/// emitting a phantom `Url::new` (E0433).
fn url_ctor(args: &[Argument], ctx: &Ctx<'_>) -> Expr {
    match (args.first(), args.get(1)) {
        (Some(input), Some(base)) => {
            let i = array_elem_arg(input, ctx);
            let b = array_elem_arg(base, ctx);
            parse_quote!(crate::__ds::DsUrl::parse_with_base(#i, #b))
        }
        (Some(input), None) => {
            let i = array_elem_arg(input, ctx);
            parse_quote!(crate::__ds::DsUrl::parse(#i))
        }
        (None, _) => parse_quote!(::core::panic!(
            "TypeError: URL constructor requires at least 1 argument"
        )),
    }
}

/// `new URLPattern(input[, baseURL])` → `crate::__ds::DsURLPattern::from_str
/// (input)` (a string input) or `::empty()` (undefined/absent). Any non-string
/// first arg (`new URL(…)`, a variable) is ToString'd, then `from_str` — ES
/// coerces the input, and a `URL` is its href. A pattern that fails to compile
/// (an unclosed group) panics a `TypeError` inside `from_str`. The optional
/// `baseURL` (arg 1) is dropped — a base only matters for a relative pattern,
/// and the common case is an absolute one (YAGNI until a fixture needs it).
fn urlpattern_ctor(args: &[Argument], ctx: &Ctx<'_>) -> Expr {
    use oxc_ast::ast::Argument as Arg;
    let Some(arg0) = args.first() else {
        return parse_quote!(crate::__ds::DsURLPattern::empty());
    };
    match arg0 {
        Arg::Identifier(id) if id.name.as_str() == "undefined" => {
            parse_quote!(crate::__ds::DsURLPattern::empty())
        }
        Arg::StringLiteral(s) => {
            let lit = syn::LitStr::new(s.value.as_str(), proc_macro2::Span::call_site());
            parse_quote!(crate::__ds::DsURLPattern::from_str(#lit))
        }
        _ => {
            // Any other expression (`new URL(…)`, a variable, …) — ToString,
            // then `from_str` (a `URL` is its href; the rest ES-coerces).
            let e = array_elem_arg(arg0, ctx);
            parse_quote!(crate::__ds::DsURLPattern::from_str(&(#e).to_string()))
        }
    }
}

/// `new Event(type[, init])` → `crate::__ds::DsEvent::new(type, init)`. The type
/// is ToString-coerced (`es_to_string_arg`, so a non-string type still satisfies
/// `AsRef<str>`); the optional `init` object's `bubbles`/`cancelable`
/// BooleanLiteral fields lower to `DsEventInit` (absent or non-literal → `false`,
/// the ES default), and a missing `init` lowers to `DsEventInit::default()`. ES
/// `new Event()` (no args) throws `TypeError`; the no-arg case panics with the
/// same class name (the WPT verdict reads the prefix), rather than emitting a
/// phantom `Event::new` (E0433).
fn event_ctor(args: &[Argument], ctx: &Ctx<'_>) -> Expr {
    let Some(type_arg) = args.first() else {
        return parse_quote!(::core::panic!(
            "TypeError: Event constructor requires at least 1 argument"
        ));
    };
    let type_ = builtins::es_to_string_arg(type_arg, ctx);
    let init = match args.get(1).and_then(|a| a.as_expression()) {
        Some(Expression::ObjectExpression(obj)) => builtins::event_init(obj),
        _ => parse_quote!(crate::__ds::DsEventInit::default()),
    };
    parse_quote!(crate::__ds::DsEvent::new(#type_, #init))
}
/// map of [key, value] pairs. Each element must be a 2-element array literal;
/// `None` otherwise (spread / non-array / wrong arity), so anything else falls
/// through to the generic `Map::new(…)` path. A numeric key (detected from the
/// first pair's first element) wraps each key in `DsF64Key` — `f64` lacks
/// `Eq`/`Hash`, so the SameValueZero newtype is the only way to house one in a
/// `HashMap`.
fn map_from_array_arg(args: &[Argument], ctx: &Ctx<'_>) -> Option<Expr> {
    use oxc_ast::ast::{ArrayExpressionElement, Expression};
    if args.len() != 1 {
        return None;
    }
    let Expression::ArrayExpression(arr) = args[0].as_expression()? else {
        return None;
    };
    // A `Map<number, _>` (first pair's key is a numeric literal) wraps each key
    // in `DsF64Key` so the `HashMap` compiles.
    let f64key = matches!(
        arr.elements.first(),
        Some(ArrayExpressionElement::ArrayExpression(inner))
            if matches!(
                inner.elements.first(),
                Some(ArrayExpressionElement::NumericLiteral(_))
            )
    );
    let mut pairs: Vec<Expr> = Vec::with_capacity(arr.elements.len());
    for el in &arr.elements {
        let Expression::ArrayExpression(inner) = el.as_expression()? else {
            return None;
        };
        if inner.elements.len() != 2 {
            return None;
        }
        let k = array_elem_expr(inner.elements[0].as_expression()?, ctx);
        let v = array_elem_expr(inner.elements[1].as_expression()?, ctx);
        pairs.push(if f64key {
            parse_quote!((crate::__ds::DsF64Key(#k), #v))
        } else {
            parse_quote!((#k, #v))
        });
    }
    Some(parse_quote!(
        ::std::collections::HashMap::from([#(#pairs),*])
    ))
}

/// `new Set([a, b, …])` → `HashSet::from([a, b, …])` — a literal initial set of
/// scalar values, the common module-constant case. `None` unless the sole arg
/// is a spread-free array literal, so anything else falls through to the
/// generic `Set::new(…)` path.
fn set_from_array_arg(args: &[Argument], ctx: &Ctx<'_>) -> Option<Expr> {
    if args.len() != 1 {
        return None;
    }
    let Expression::ArrayExpression(arr) = args[0].as_expression()? else {
        return None;
    };
    let arr_expr = array_owned_expr(arr, ctx)?;
    // A number-element literal `new Set([1, 2, …])` would infer `HashSet<f64>`,
    // but `f64` lacks `Eq`/`Hash` — wrap each element in `DsF64Key` (SameValueZero)
    // so the set compiles. Detected by the first element being a numeric literal.
    if arr
        .elements
        .first()
        .is_some_and(|e| matches!(e, ArrayExpressionElement::NumericLiteral(_)))
    {
        return Some(parse_quote!(
            #arr_expr
                .iter()
                .copied()
                .map(crate::__ds::DsF64Key)
                .collect::<::std::collections::HashSet<crate::__ds::DsF64Key>>()
        ));
    }
    Some(parse_quote!(::std::collections::HashSet::from(#arr_expr)))
}

/// The class type name when `callee` is a plain identifier (`Foo`).
fn class_name(callee: &Expression) -> Option<Ident> {
    let Expression::Identifier(id) = callee else {
        return None;
    };
    Some(bindings::type_ident(&id.name))
}
