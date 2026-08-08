//! Type-path inference for `new` expressions and callee/method-call results:
//! the `syn::Path` a constructor or call initializer lowers to, recorded on a
//! local so a later member access or element write dispatches through the right
//! type. Extracted from `functions/mod.rs`.

use oxc_ast::ast::Expression;
use quote::format_ident;
use syn::{parse_quote, Path, Type};

use super::super::bindings;
use super::super::context::Locals;
use super::super::registry::TypeRegistry;
use super::super::types;

/// `new <TypedArray>(…)` → `Vec<elem>` (Int8Array→Vec<i8>, …, Float64Array→
/// Vec<f64>), so an unannotated `let x = new Int32Array(3)` records `Vec<i32>`
/// and a later `x[0] = v` stores the value with an `i32` cast. `ArrayBuffer`
/// stays `Vec<u8>` (a raw byte buffer). Mirrors the constructor's type mapping
/// (`typed_array_elem_type`); `None` for any other `new` callee.
pub(super) fn typed_array_path(new_expr: &oxc_ast::ast::NewExpression) -> Option<Path> {
    let Expression::Identifier(id) = &new_expr.callee else {
        return None;
    };
    if id.name.as_str() == "ArrayBuffer" {
        return Some(parse_quote!(Vec<u8>));
    }
    let elem = super::super::expressions::typed_array_elem_type(id.name.as_str())?;
    let ty = format_ident!("{}", elem);
    Some(parse_quote!(Vec<#ty>))
}

/// `new Set(…)` / `new Map(…)` → the inferred `HashSet<E>` / `HashMap<K, V>`
/// path (reusing module-global inference), so an unannotated `let s = new
/// Set([1])` records its type and a later `s.add(…)` / `s.has(…)` resolves the
/// receiver. `None` for a non-collection `new`.
pub(super) fn collection_local_path(new_expr: &oxc_ast::ast::NewExpression) -> Option<Path> {
    let ty = super::lazy_static::new_collection_return_type(new_expr)?;
    types::type_path(&ty).cloned()
}

/// `new URLSearchParams(...)` → `crate::__ds::DsUrlSearchParams`, so an
/// unannotated `let params = new URLSearchParams("a=b")` records the type and a
/// later `params.size` lowers to `.len()`. Only the `URLSearchParams` callee
/// maps; any other `new` yields `None`.
pub(super) fn url_search_params_path(new_expr: &oxc_ast::ast::NewExpression) -> Option<Path> {
    let Expression::Identifier(id) = &new_expr.callee else {
        return None;
    };
    if id.name.as_str() == "URLSearchParams" {
        Some(parse_quote!(crate::__ds::DsUrlSearchParams))
    } else {
        None
    }
}

/// `new URL(...)` → `crate::__ds::DsUrl`, so an unannotated `let u = new
/// URL("…")` records the type and a later `u.href`/`u.origin`/… lowers to the
/// matching accessor. Only the `URL` callee maps; any other `new` yields `None`.
pub(super) fn url_path(new_expr: &oxc_ast::ast::NewExpression) -> Option<Path> {
    let Expression::Identifier(id) = &new_expr.callee else {
        return None;
    };
    if id.name.as_str() == "URL" {
        Some(parse_quote!(crate::__ds::DsUrl))
    } else {
        None
    }
}

/// `new <ErrorCtor>(…)` / `new DOMException(…)` → `DsError`. DashScript lowers
/// every Error variant (Error/TypeError/RangeError/…) and DOMException to the
/// one `DsError` value, so an unannotated `let e = new TypeError("…")` records
/// `DsError` and a later `e instanceof TypeError` folds to `true` statically
/// (both sides are DsError). Any other `new` callee yields `None`.
pub(super) fn error_path(new_expr: &oxc_ast::ast::NewExpression) -> Option<Path> {
    let Expression::Identifier(id) = &new_expr.callee else {
        return None;
    };
    let name = id.name.as_str();
    if super::super::globals::error_ctor_name(name).is_some() || name == "DOMException" {
        Some(parse_quote!(crate::__ds::DsError))
    } else {
        None
    }
}

/// `new TextEncoder()` / `new TextDecoder(…)` → the `__ds::Text*` Rust type, so
/// an unannotated `let d = new TextDecoder("…")` records the type and a later
/// `d.decode(…)` dispatches through `text_decoder_method` (the receiver resolves
/// to `crate::__ds::TextDecoder`). Either encoding ctor maps; any other `new`
/// yields `None`.
pub(super) fn encoding_ctor_path(new_expr: &oxc_ast::ast::NewExpression) -> Option<Path> {
    let Expression::Identifier(id) = &new_expr.callee else {
        return None;
    };
    match id.name.as_str() {
        "TextEncoder" => Some(parse_quote!(crate::__ds::TextEncoder)),
        "TextDecoder" => Some(parse_quote!(crate::__ds::TextDecoder)),
        _ => None,
    }
}

/// `new EventTarget()` / `new Event(…)` → the `__ds::DsEvent*` Rust type, so an
/// unannotated `let et = new EventTarget()` (or `let e = new Event("x")`)
/// records the type and a later `et.addEventListener`/`et.dispatchEvent`
/// dispatches through `event_target_method` (the receiver resolves to
/// `DsEventTarget`), and `event.type`/`.defaultPrevented`/… through the event
/// member dispatch. Either ctor maps; any other `new` yields `None`.
pub(super) fn event_target_path(new_expr: &oxc_ast::ast::NewExpression) -> Option<Path> {
    let Expression::Identifier(id) = &new_expr.callee else {
        return None;
    };
    match id.name.as_str() {
        "EventTarget" => Some(parse_quote!(crate::__ds::DsEventTarget)),
        "Event" => Some(parse_quote!(crate::__ds::DsEvent)),
        // `DsCustomEvent` (no `<T>` — the detail-payload type is inferred at the
        // call site; the last-segment match routes `ev.detail`/`.type`/…).
        "CustomEvent" => Some(parse_quote!(crate::__ds::DsCustomEvent)),
        _ => None,
    }
}

/// `new Headers(init?)` → `crate::__ds::DsHeaders`, so an unannotated
/// `let h = new Headers(…)` records the type and a later `h.get`/`h.set`/…
/// dispatches through `headers_method` (the receiver resolves to `DsHeaders`).
/// Any other `new` yields `None`.
pub(super) fn headers_path(new_expr: &oxc_ast::ast::NewExpression) -> Option<Path> {
    let Expression::Identifier(id) = &new_expr.callee else {
        return None;
    };
    match id.name.as_str() {
        "Headers" => Some(parse_quote!(crate::__ds::DsHeaders)),
        _ => None,
    }
}

/// `new Blob(parts?, options?)` → `crate::__ds::DsBlob`, so an unannotated
/// `let b = new Blob(…)` records the type and a later `b.size`/`b.type`/
/// `b.slice(…)`/`b.text()` dispatches through `blob_method`/the accessors (the
/// receiver resolves to `DsBlob`). Only the `Blob` callee maps; any other `new`
/// yields `None`.
pub(super) fn blob_path(new_expr: &oxc_ast::ast::NewExpression) -> Option<Path> {
    let Expression::Identifier(id) = &new_expr.callee else {
        return None;
    };
    match id.name.as_str() {
        "Blob" => Some(parse_quote!(crate::__ds::DsBlob)),
        _ => None,
    }
}

/// `new File(bits, name, options?)` → `crate::__ds::DsFile`, so an unannotated
/// `let f = new File(…)` records a `DsFile` local and a later `f.size`/
/// `f.slice(…)`/`await f.text()`/`f.name` resolves its receiver (a `File` is a
/// `Blob`, so the `Blob` accessors/methods dispatch on it via `is_blob_local`,
/// widened to accept a `DsFile`). Only the `File` callee maps; any other `new`
/// yields `None`.
pub(super) fn file_path(new_expr: &oxc_ast::ast::NewExpression) -> Option<Path> {
    let Expression::Identifier(id) = &new_expr.callee else {
        return None;
    };
    match id.name.as_str() {
        "File" => Some(parse_quote!(crate::__ds::DsFile)),
        _ => None,
    }
}

/// `new FormData()` → `crate::__ds::DsFormData`, so an unannotated
/// `let fd = new FormData()` records a `DsFormData` local and a later
/// `fd.append(…)`/`fd.has(…)`/`fd.delete(…)`/`fd.set(…)` resolves its receiver.
/// Only the `FormData` callee maps; any other `new` yields `None`.
pub(super) fn form_data_path(new_expr: &oxc_ast::ast::NewExpression) -> Option<Path> {
    let Expression::Identifier(id) = &new_expr.callee else {
        return None;
    };
    match id.name.as_str() {
        "FormData" => Some(parse_quote!(crate::__ds::DsFormData)),
        _ => None,
    }
}

/// `new Request(url, init?)` → `crate::__ds::DsRequest`, so an unannotated
/// `let r = new Request(…)` records a `DsRequest` local and a later
/// `fetch(r)` resolves its argument type and `r.url`/`r.method`/`r.headers`
/// resolve the receiver. Only the `Request` callee maps; any other `new`
/// yields `None`.
pub(super) fn request_path(new_expr: &oxc_ast::ast::NewExpression) -> Option<Path> {
    let Expression::Identifier(id) = &new_expr.callee else {
        return None;
    };
    match id.name.as_str() {
        "Request" => Some(parse_quote!(crate::__ds::DsRequest)),
        _ => None,
    }
}

/// `new Response(…)` → `crate::__ds::DsResponse`, so an unannotated `let r =
/// new Response(…)` records a `DsResponse` local and a later `.status`/
/// `.statusText`/`.ok`/`.headers` (member accessors) and `await r.text()`/
/// `.json()`/`.arrayBuffer()` (call path) resolve the receiver. Only the
/// `Response` callee maps; any other `new` yields `None`.
pub(super) fn response_path(new_expr: &oxc_ast::ast::NewExpression) -> Option<Path> {
    let Expression::Identifier(id) = &new_expr.callee else {
        return None;
    };
    match id.name.as_str() {
        "Response" => Some(parse_quote!(crate::__ds::DsResponse)),
        _ => None,
    }
}

/// `new Promise(…)` → `crate::__ds::DsPromise<T>`, so an unannotated `let p =
/// new Promise(…)` records a `DsPromise` local and a later `p.then(…)` /
/// `await p` resolves the receiver. The value type `T` is inferred from the
/// executor's `resolve(value)` call site; `is_ds_promise_local` keys only off
/// the last path segment, so a placeholder `<serde_json::Value>` (matching the
/// `Promise.resolve`/`Promise.all` record in [`callee_return_path`]) keeps the
/// path a valid Rust type without over-committing `T`. Only the `Promise`
/// callee maps; any other `new` yields `None`.
pub(super) fn promise_path(new_expr: &oxc_ast::ast::NewExpression) -> Option<Path> {
    let Expression::Identifier(id) = &new_expr.callee else {
        return None;
    };
    match id.name.as_str() {
        "Promise" => Some(parse_quote!(crate::__ds::DsPromise<serde_json::Value>)),
        _ => None,
    }
}

/// `new ReadableStream(…)` → `crate::__ds::DsReadableStream`, so an unannotated
/// `let rs = new ReadableStream(…)` records the type and a later
/// `rs.getReader()` dispatches through `streams_method` (the receiver resolves
/// to `DsReadableStream`). The chunk type `T` is inferred at the call site, so
/// no generic arg is recorded here (the predicate matches on the segment name).
pub(super) fn streams_path(new_expr: &oxc_ast::ast::NewExpression) -> Option<Path> {
    let Expression::Identifier(id) = &new_expr.callee else {
        return None;
    };
    if id.name.as_str() == "ReadableStream" {
        Some(parse_quote!(crate::__ds::DsReadableStream))
    } else if matches!(
        id.name.as_str(),
        "CompressionStream" | "DecompressionStream"
    ) {
        Some(parse_quote!(crate::__ds::DsCompressionStream))
    } else {
        None
    }
}

/// `new AbortController()` → `crate::__ds::DsAbortController`, so an unannotated
/// `let c = new AbortController()` records the type and a later `c.signal`/
/// `c.abort()` resolves the receiver. Only the `AbortController` callee maps;
/// any other `new` yields `None`.
pub(super) fn abort_path(new_expr: &oxc_ast::ast::NewExpression) -> Option<Path> {
    let Expression::Identifier(id) = &new_expr.callee else {
        return None;
    };
    match id.name.as_str() {
        "AbortController" => Some(parse_quote!(crate::__ds::DsAbortController)),
        _ => None,
    }
}

/// `controller.signal` → `crate::__ds::DsAbortSignal`, so an unannotated
/// `let s = controller.signal` records the signal type and a later
/// `s.aborted`/`s.addEventListener(…)` resolves the receiver. The init must be a
/// `.signal` member access on a `DsAbortController` local; any other shape
/// yields `None` (a chained `controller.signal.aborted` needs no binding — the
/// member dispatch's `is_abort_signal_receiver` matches it inline).
pub(super) fn abort_signal_access_path(
    init: &oxc_ast::ast::Expression,
    locals: &Locals,
) -> Option<Path> {
    let Expression::StaticMemberExpression(sm) = init else {
        return None;
    };
    if sm.property.name.as_str() != "signal" {
        return None;
    }
    let Expression::Identifier(id) = &sm.object else {
        return None;
    };
    let ctrl_path = locals.get(&bindings::snake(id.name.as_str()).to_string())?;
    let is_controller = ctrl_path
        .segments
        .last()
        .is_some_and(|s| s.ident == "DsAbortController");
    if !is_controller {
        return None;
    }
    Some(parse_quote!(crate::__ds::DsAbortSignal))
}

/// `<blob>.slice(…)` where `blob` is a tracked `DsBlob` (or `DsFile`) local →
/// `DsBlob`, so an unannotated `let s = b.slice(0, 5)` records the type and a
/// later `s.size`/`s.slice(…)`/`await s.text()` resolves its receiver (a WHATWG
/// `Blob.slice`/`File.slice` returns a new `Blob`, never a `File`). Returns
/// `None` for any other call shape (the declarator's `CallExpression` arm
/// reaches this after `callee_return_path`).
pub(super) fn blob_slice_path(
    call: &oxc_ast::ast::CallExpression,
    locals: &Locals,
) -> Option<Path> {
    let Expression::StaticMemberExpression(sm) = &call.callee else {
        return None;
    };
    if sm.property.name.as_str() != "slice" {
        return None;
    }
    let Expression::Identifier(id) = &sm.object else {
        return None;
    };
    let b_path = locals.get(&bindings::snake(id.name.as_str()).to_string())?;
    let is_blob = b_path
        .segments
        .last()
        .is_some_and(|s| s.ident == "DsBlob" || s.ident == "DsFile");
    if !is_blob {
        return None;
    }
    Some(parse_quote!(crate::__ds::DsBlob))
}

/// `arr[i]` where `arr` is a tracked `Vec<T>` (or `Option<Vec<T>>`) local → `T`,
/// so an unannotated `let element = elements[i]` records `Element` and a later
/// `element.text` access resolves its struct. Works on the source AST — the
/// emitted `elements[i as usize].clone()` is plain `elements[i]` here, since the
/// cast and `.clone()` are added only at emit time. Returns `None` for any other
/// initializer shape.
pub(super) fn vec_index_elem_path(
    init: &oxc_ast::ast::Expression,
    locals: &Locals,
) -> Option<Path> {
    let Expression::ComputedMemberExpression(cm) = init else {
        return None;
    };
    let Expression::Identifier(id) = &cm.object else {
        return None;
    };
    let arr_path = locals.get(&bindings::snake(id.name.as_str()).to_string())?;
    let outer = arr_path.segments.last()?;
    // The `Vec<…>` segment: directly when `arr` is `Vec<T>`, or the inner type
    // argument when `arr` is `Option<Vec<T>>` (an optional `T[] | undefined`).
    let vec_seg = if outer.ident == "Vec" {
        outer
    } else if outer.ident == "Option" {
        first_type_arg_seg(outer).filter(|s| s.ident == "Vec")?
    } else {
        return None;
    };
    // The element type `T` is the `Vec`'s first type-argument segment
    // (`Element`, `__DsUnion…`); reconstruct a single-segment path from it.
    let elem_ident = first_type_arg_seg(vec_seg)?.ident.clone();
    Some(parse_quote!(#elem_ident))
}

/// The first `syn::PathSegment` inside a path segment's first generic type
/// argument — `Vec<Element>` → the `Element` segment, `Option<Vec<T>>`'s
/// `Option` → the `Vec<T>` segment. `None` when the segment has no generic type
/// argument or it is not a plain path.
fn first_type_arg_seg(seg: &syn::PathSegment) -> Option<&syn::PathSegment> {
    let syn::PathArguments::AngleBracketed(args) = &seg.arguments else {
        return None;
    };
    let first_ty = args.args.iter().find_map(|g| match g {
        syn::GenericArgument::Type(t) => Some(t),
        _ => None,
    })?;
    match first_ty {
        syn::Type::Path(tp) => tp.path.segments.last(),
        _ => None,
    }
}

/// The declared return-type path of a `fn_name(…)` call's callee, when the
/// callee is a bare identifier naming a function with an annotated return type.
pub(super) fn callee_return_path(
    call: &oxc_ast::ast::CallExpression,
    registry: &TypeRegistry,
    locals: &Locals,
) -> Option<Path> {
    match &call.callee {
        Expression::Identifier(id) => {
            // `fetch(url)` → `crate::__ds::DsResponse` (a WinterTC Web API).
            // `fetch` is a global, never in `function_returns`, so an
            // unannotated `let r = fetch(url)` — and `let r = await fetch(url)`
            // via the `AwaitExpression` arm in `register_declarator` — records
            // the type and a later `r.status`/`.ok`/`.headers` lowers to the
            // wrapper's accessors.
            if id.name.as_str() == "fetch" {
                return Some(parse_quote!(crate::__ds::DsResponse));
            }
            registry
                .function_returns
                .get(id.name.as_str())
                .cloned()
                .flatten()
        }
        // `JSON.parse(s)` → `serde_json::Value` (the dynamic parse result), so
        // an unannotated `var v = JSON.parse(...)` records its type and a later
        // `console.log(v)` routes through `__ds::inspect` (rendering the parsed
        // value the way Node prints it) instead of `Value`'s JSON `Display`,
        // which would double-quote a string (`"abc"` vs Node's `abc`).
        Expression::StaticMemberExpression(sm)
            if super::super::builtins::is_ident(&sm.object, "JSON")
                && sm.property.name == "parse" =>
        {
            Some(parse_quote!(serde_json::Value))
        }
        // `Promise.resolve(x)` / `Promise.all([..])` → `crate::__ds::DsPromise<T>`
        // (the static track, T3 stage 2a), so a `let p = Promise.resolve(x)`
        // records a `DsPromise` local and a later `p.then(…)` dispatches on the
        // receiver type. The element type `T` varies per call site;
        // `is_ds_promise_local` keys only off the last path segment, so a
        // placeholder `<serde_json::Value>` keeps the path a valid Rust type
        // without over-committing the inferred `T` (the `.then` closure's
        // parameter type is inferred from the receiver, not this path).
        Expression::StaticMemberExpression(sm)
            if super::super::builtins::is_ident(&sm.object, "Promise")
                && matches!(sm.property.name.as_str(), "resolve" | "all") =>
        {
            Some(parse_quote!(crate::__ds::DsPromise<serde_json::Value>))
        }
        // `crypto.subtle.importKey(…)` → `crate::__ds::DsCryptoKey` (the WinterTC
        // WebCrypto HMAC subset), so an unannotated `let k = crypto.subtle.importKey(…)`
        // — and `let k = await crypto.subtle.importKey(…)` via the `AwaitExpression`
        // arm in `register_declarator` — records the type, and a later
        // `crypto.subtle.sign(algo, k, …)`/`.verify(…)` passes the key through as a
        // `DsCryptoKey` arg (the callee `crypto.subtle` is detected by the shared
        // predicate, mirroring `crypto_method`'s two-level chain guard).
        Expression::StaticMemberExpression(sm)
            if sm.property.name.as_str() == "importKey"
                && super::super::builtins::is_crypto_subtle_member(&sm.object) =>
        {
            Some(parse_quote!(crate::__ds::DsCryptoKey))
        }
        // `crypto.subtle.generateKey(…)` → `crate::__ds::DsCryptoKey` (the
        // WinterTC WebCrypto key factory), so an unannotated
        // `let k = crypto.subtle.generateKey(…)` — and `await …` via the
        // `AwaitExpression` arm — records the same type as `importKey`, and a
        // later `sign`/`encrypt` passes the key through.
        Expression::StaticMemberExpression(sm)
            if sm.property.name.as_str() == "generateKey"
                && super::super::builtins::is_crypto_subtle_member(&sm.object) =>
        {
            Some(parse_quote!(crate::__ds::DsCryptoKey))
        }
        // `crypto.subtle.deriveKey(…)` → `crate::__ds::DsCryptoKey` (the WinterTC
        // WebCrypto derived key), so an unannotated
        // `let k = crypto.subtle.deriveKey(…)` — and `await …` via the
        // `AwaitExpression` arm — records the same type as `importKey`/
        // `generateKey`, and a later `encrypt`/`exportKey` passes the key through.
        Expression::StaticMemberExpression(sm)
            if sm.property.name.as_str() == "deriveKey"
                && super::super::builtins::is_crypto_subtle_member(&sm.object) =>
        {
            Some(parse_quote!(crate::__ds::DsCryptoKey))
        }
        // `crypto.subtle.deriveBits(…)` → `Vec<u8>` (the WinterTC WebCrypto PBKDF2
        // derived bytes), so an unannotated `let dk = crypto.subtle.deriveBits(…)`
        // — and `await …` via the `AwaitExpression` arm — records the same
        // `Vec<u8>` as `sign`/`encrypt`.
        Expression::StaticMemberExpression(sm)
            if sm.property.name.as_str() == "deriveBits"
                && super::super::builtins::is_crypto_subtle_member(&sm.object) =>
        {
            Some(parse_quote!(Vec<u8>))
        }
        // `crypto.subtle.exportKey(…)` → `Vec<u8>` (the WinterTC WebCrypto raw
        // key bytes — the inverse of `importKey`), so an unannotated
        // `let raw = crypto.subtle.exportKey("raw", key)` (and `await …` via the
        // `AwaitExpression` arm) records the same `Vec<u8>` `new Uint8Array(…)`
        // records (the only statically modeled format is `"raw"`).
        Expression::StaticMemberExpression(sm)
            if sm.property.name.as_str() == "exportKey"
                && super::super::builtins::is_crypto_subtle_member(&sm.object) =>
        {
            Some(parse_quote!(Vec<u8>))
        }
        // `crypto.subtle.sign(…)` → `Vec<u8>` (the WinterTC WebCrypto HMAC tag
        // bytes), so an unannotated `let sig = crypto.subtle.sign(…)` (and `await
        // …` via the `AwaitExpression` arm) records the same `Vec<u8>` `new
        // Uint8Array(…)` records. A later `crypto.subtle.verify(algo, key, sig,
        // …)` then recognizes `sig` as a byte vector — the signature arg's
        // `digest_data_arg` coercion keys off the `Vec` path segment and passes it
        // through, instead of applying the Blob string coercion (`sig.to_string()`
        // would require `Vec<u8>: Display`).
        Expression::StaticMemberExpression(sm)
            if sm.property.name.as_str() == "sign"
                && super::super::builtins::is_crypto_subtle_member(&sm.object) =>
        {
            Some(parse_quote!(Vec<u8>))
        }
        // `crypto.subtle.encrypt(…)`/`.decrypt(…)` → `Vec<u8>` (the WinterTC
        // WebCrypto AES-GCM ciphertext/plaintext bytes), so an unannotated
        // `let ct = crypto.subtle.encrypt(…)` (and `await …`) records the same
        // `Vec<u8>` `new Uint8Array(…)` records. A later `crypto.subtle.decrypt(
        // algo, key, ct, …)` then recognizes `ct` as a byte vector — the data
        // arg's `digest_data_arg` coercion passes it through instead of the Blob
        // string coercion (the same reason `sign` is registered).
        Expression::StaticMemberExpression(sm)
            if (sm.property.name.as_str() == "encrypt"
                || sm.property.name.as_str() == "decrypt")
                && super::super::builtins::is_crypto_subtle_member(&sm.object) =>
        {
            Some(parse_quote!(Vec<u8>))
        }
        // `crypto.subtle.wrapKey(…)` → `Vec<u8>` (the WinterTC WebCrypto AES-KW
        // wrapped-key bytes), so an unannotated `let wrapped =
        // crypto.subtle.wrapKey(…)` (and `await …`) records the same `Vec<u8>`
        // `new Uint8Array(…)` records — mirroring `encrypt`/`sign`.
        Expression::StaticMemberExpression(sm)
            if sm.property.name.as_str() == "wrapKey"
                && super::super::builtins::is_crypto_subtle_member(&sm.object) =>
        {
            Some(parse_quote!(Vec<u8>))
        }
        // `crypto.subtle.unwrapKey(…)` → `crate::__ds::DsCryptoKey` (the WinterTC
        // WebCrypto key rebuilt from the AES-KW-unwrapped bytes; the call site's
        // `await` drives the async future). Records the same `DsCryptoKey` as
        // `importKey`/`generateKey`/`deriveKey`.
        Expression::StaticMemberExpression(sm)
            if sm.property.name.as_str() == "unwrapKey"
                && super::super::builtins::is_crypto_subtle_member(&sm.object) =>
        {
            Some(parse_quote!(crate::__ds::DsCryptoKey))
        }
        // `cs.writable.getWriter()` → `DsCompressionWriter` /
        // `cs.readable.getReader()` → `DsCompressionReader` (a WinterTC Web API),
        // so an unannotated `let writer = cs.writable.getWriter()` — receiver is
        // the `writable`/`readable` field of a `DsCompressionStream` local —
        // records the type and a later `writer.write(…)`/`.close()`/
        // `reader.read()` dispatches through `compression_method`. Scoped before
        // the `ReadableStream` `getReader` arm: a `cs.readable.getReader()`
        // receiver is a field access (`cs.readable`), not an Identifier, so it
        // never matched that arm's `DsReadableStream` local check anyway.
        Expression::StaticMemberExpression(sm)
            if sm.property.name.as_str() == "getWriter"
                && is_compression_field(&sm.object, "writable", locals) =>
        {
            Some(parse_quote!(crate::__ds::DsCompressionWriter))
        }
        Expression::StaticMemberExpression(sm)
            if sm.property.name.as_str() == "getReader"
                && is_compression_field(&sm.object, "readable", locals) =>
        {
            Some(parse_quote!(crate::__ds::DsCompressionReader))
        }
        // `rs.getReader()` → `crate::__ds::DsReadableStreamDefaultReader`
        // (a WinterTC Web API), so an unannotated `let reader = rs.getReader()`
        // — on a `DsReadableStream` local — records the type and a later
        // `reader.read()` dispatches through `streams_method`. Only a
        // `DsReadableStream` receiver qualifies; the chunk type `T` is inferred
        // at the call site, so no generic arg is recorded (the predicate matches
        // on the segment name).
        Expression::StaticMemberExpression(sm) if sm.property.name.as_str() == "getReader" => {
            let Expression::Identifier(id) = &sm.object else {
                return None;
            };
            let name = bindings::snake(id.name.as_str()).to_string();
            if locals.get(&name).is_some_and(|p| {
                p.segments
                    .last()
                    .is_some_and(|s| s.ident == "DsReadableStream")
            }) {
                Some(parse_quote!(crate::__ds::DsReadableStreamDefaultReader))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// True when `expr` is `<DsCompressionStream local>.<side>` (the `writable`/
/// `readable` field), so `cs.writable.getWriter()`/`cs.readable.getReader()`
/// records the writer/reader return type. Used by `callee_return_path`.
fn is_compression_field(expr: &oxc_ast::ast::Expression, side: &str, locals: &Locals) -> bool {
    let Expression::StaticMemberExpression(f) = expr else {
        return false;
    };
    if f.property.name.as_str() != side {
        return false;
    }
    let Expression::Identifier(id) = &f.object else {
        return false;
    };
    let name = bindings::snake(id.name.as_str()).to_string();
    locals.get(&name).is_some_and(|p| {
        p.segments
            .last()
            .is_some_and(|s| s.ident == "DsCompressionStream")
    })
}

/// Extract the path of a `Type::Path`, if any.
pub(super) fn path_of(ty: &Type) -> Option<syn::Path> {
    if let Type::Path(tp) = ty {
        Some(tp.path.clone())
    } else {
        None
    }
}
