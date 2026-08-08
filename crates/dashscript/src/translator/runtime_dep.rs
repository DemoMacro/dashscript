//! `RuntimeDep` — the runtime-dependency enum a translated file may pull in
//! (an extra crate, a helper module, or the embedded QuickJS engine), plus its
//! table-driven metadata methods (`marker` / `cargo` / `helper` / `engine_builtin`).
//! Extracted from `translator/mod.rs`

use super::helpers::*;

/// A runtime dependency a translated file may pull in — an extra crate, a
/// helper module, or the embedded QuickJS engine. Each variant carries its own
/// metadata ([`RuntimeDep::marker`] / [`RuntimeDep::cargo`] / [`RuntimeDep::helper`]),
/// so adding a new runtime dep is one variant plus one arm per method — not a
/// new field threaded through every construction site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RuntimeDep {
    /// An emit point routes an `f64` through `__ds::number_to_string`, so the
    /// crate needs `ryu_js` (ES `Number::toString`) and the `__ds` helper.
    RyuJs,
    /// A `JSON.parse`/`JSON.stringify` call inlines a `serde_json::` call (no
    /// helper module — the calls are direct).
    SerdeJson,
    /// ES dynamic reflection (`Object.defineProperty`/`getOwnPropertyDescriptor`/
    /// `create`/`getPrototypeOf`, accessor properties, …) the static translator
    /// cannot lower; the whole program runs under an embedded QuickJS engine
    /// via `__ds::engine` (the `rquickjs` crate). A gated compat fallback — the
    /// body is never lowered, so it carries no text marker.
    Engine,
    /// tc39 test262 `$262.agent` API — the bottom-layer agent surface
    /// (`start`/`broadcast`/`getReport`/`sleep`/`monotonicNow` main-side;
    /// `report`/`leaving`/`receiveBroadcast` agent-side) that drives true
    /// cross-thread `Atomics.wait`/`notify`. Mirrors QuickJS's own
    /// `run-test262.c` agent model: each agent is an independent `Runtime` +
    /// `JS_SetCanBlock(true)` + own OS thread; the `SharedArrayBuffer` is
    /// shared by raw backing pointer; broadcast sync uses a `Mutex`+`Condvar`.
    /// Registered as an engine builtin (`register_atomics_agent`) under
    /// `wire_web_apis`, so an atomics fixture that reaches `$262.agent.*`
    /// degrades per-function and the agent threads run inside the production
    /// binary. No text marker (the body degrades to the engine, like `Engine`);
    /// no cargo dep (pure `std` + the `rquickjs` `Engine` already pulls).
    Atomics,
    /// An indexed assignment `xs[i] = v` that may auto-grow; routes through
    /// `__ds::array_set`. Pure `std` — no cargo dep.
    ArrayHelper,
    /// An ES RegExp literal `/pat/flags` lowered to a `regress::Regex` (ES
    /// regex semantics — backreferences, lookaround, which the `regex` crate
    /// cannot express). Routes through `__ds::regex`; flags the `regress` dep.
    Regress,
    /// A `Temporal.*` API call (`Temporal.PlainDate.from`, `.toString`, …)
    /// lowered to the `temporal_rs` crate (boa-dev/temporal-rs — the Rust
    /// implementation of ECMAScript Temporal). Routes through `temporal_rs::`
    /// directly; no `__ds` helper slice.
    Temporal,
    /// A Web Worker–style isolate (Direction D, D1): `new Worker(handler)` spawns a
    /// thread running `handler` for each message the main thread sends via
    /// `postMessage`. Pure Rust stack — no JS engine in the worker (decision
    /// point 10 MVP); messages cross the thread boundary as JSON (`serde_json`),
    /// so any `Serialize`/`DeserializeOwned` value works. Routes through
    /// `__ds::Worker`; reuses `serde_json` (no separate crate).
    Worker,
    /// ES truthiness for a value used in condition position (`if (x)`, `while
    /// (x)`). The translator emits `__ds::truthy(&expr)` for any non-boolean
    /// condition whose truthiness it cannot lower without a type checker (a
    /// member access like `opts.indent`, a numeric cast, a call); the Rust
    /// compiler picks the matching `DsTruthy` impl by inferred type. Pure `std`
    /// — no cargo dep. Routes through `__ds::truthy`.
    Truthy,
    /// ES string rendering for a value interpolated into a template literal
    /// (`${opts.field}`) or concatenated — the translator emits
    /// `__ds::display(&expr)` for any non-numeric interpolation, and the Rust
    /// compiler picks the matching `DsDisplay` impl by inferred type. ES
    /// semantics: `undefined`/`null` (an `Option`'s `None`) → `"undefined"`,
    /// a boolean → `"true"`/`"false"`, an array → elements joined by `,`, an
    /// object → `"[object Object]"`. Pure `std` — no cargo dep.
    Display,
    /// A WHATWG Encoding API constructor (`new TextEncoder()` / `new
    /// TextDecoder(…)` — a WinterTC Web API. `TextEncoder` is UTF-8 only (the
    /// sole encoding the Encoding API guarantees for encode);
    /// `__ds::TextEncoder::encode` is `String::into_bytes`. `TextDecoder`
    /// resolves its `label` to a `encoding_rs::Encoding` and decodes through
    /// it (BOM handling + `fatal`/`ignoreBOM` options); backed by the
    /// `encoding_rs` crate. The marker is `__ds::Text` (a common prefix of
    /// `__ds::TextEncoder`/`__ds::TextDecoder`), so the helper slice defining
    /// both structs is injected whenever a file uses either.
    Encoding,
    /// A WHATWG URL API `URLSearchParams` (a WinterTC Web API). `new
    /// URLSearchParams("a=b&c=d")` parses an `application/x-www-form-urlencoded`
    /// string into an ordered name/value list; `.get`/`.has`/`.set`/`.append`/
    /// `.delete`/`.getAll`/`.sort`/`.toString`/`.size` map to the
    /// `__ds::DsUrlSearchParams` impl (a `Vec<(String, String)>`). Routes
    /// through `form_urlencoded::parse`/`Serializer` (the WHATWG spec reference
    /// parser — the same one servo/url uses). Flags the `form_urlencoded` dep.
    /// The marker `__ds::DsUrlSearchParams` is emitted at the constructor (a
    /// `new URLSearchParams(...)` always precedes any method call), so the
    /// helper slice is injected once per file that builds one.
    Url,
    /// An ECMAScript error object lowered through `panic!`/`catch_unwind` —
    /// `throw new RangeError("msg")` panics a `DsError`, and `catch (e)`
    /// downcasts it back. Carries the error class `name` + `message`, so
    /// `e.constructor.name`/`e.name`/`e.message`/`e.toString()`/`instanceof`
    /// work without string-matching panic messages. Pure `std` — no cargo
    /// dep. Routes through `__ds::DsError`.
    Error,
    /// Node-inspect rendering for a `console.log` argument Rust's `Display`
    /// cannot reach — a `Vec`/`HashMap`/`HashSet`/`Option` (std collections
    /// have no `Display`, so `println!("{}", vec)` would not compile) — plus
    /// the nested Node format (`[ a, 'b' ]`, `{ k: v }`, `Set { … }`). The
    /// translator emits `__ds::inspect(&expr)` for a non-primitive console.log
    /// argument; the Rust compiler picks the matching `DsInspect` impl by
    /// inferred type. Distinct from `DsDisplay` (ES `ToString`: objects →
    /// "[object Object]"): `console.log` inspects, `${obj}` displays. Flags
    /// `ryu-js` (an `f64` element formats via `ryu_js`).
    Inspect,
    /// test262 `assert.sameValue(a, b)` / `notSameValue` — lowers to a Rust
    /// SameValue check (`__ds::assert_same_value`) that panics a `Test262Error`
    /// on mismatch. Pure `std` — no cargo dep. Reflection asserts
    /// (`throws`/`compareArray`/`verifyProperty`/…) degrade to the engine
    /// (`RuntimeDep::Engine`), where the test262 harness runs natively.
    Assert,
    /// WPT (web-platform-tests) testharness asserts — `assert_equals`/
    /// `not_equals`/`assert_throws_dom`/`assert_throws_js`/`assert_unreached`
    /// lower to Rust helpers (`__ds::wpt_*`) that panic an `AssertionError` on
    /// failure. The WinterTC conformance path is static-first with per-function
    /// degrade (same model as test262) — so these share `ASSERT_HELPER`
    /// with test262's asserts (same `DsSameValue` core). `assert_true`/
    /// `assert_false` route through `wpt_assert_equals` against `&true`/`&false`;
    /// `test(fn, name)` lowers to an immediate closure call (no helper). Pure
    /// `std` — no cargo dep; marker `__ds::wpt_`. Pulls `Error` (for
    /// `wpt_assert_throws`'s `catch_quiet`).
    WptAssert,
    /// A `number` as a `Set`/`Map` key. ES `Set`/`Map` compare keys by
    /// SameValueZero, but Rust `f64` lacks `Eq`/`Hash` (NaN breaks reflexivity),
    /// so `Set<number>`/`Map<number, _>` wrap each key in `DsF64Key` — a
    /// `#[derive(Clone, Copy)]` newtype implementing SameValueZero `Eq`+`Hash`
    /// (+0 === -0, NaN === NaN). Pure `std`; marker `__ds::DsF64Key`.
    CollectionKey,
    /// A `replace`/`replaceAll` whose replacement string carries ES
    /// `GetSubstitution` `$` patterns (`$$`→`$`, `$&`→the match, `` $` ``→before,
    /// `$'`→after; `$n`/`$<…>` literal for a string search). Rust's
    /// `str::replace`/`replacen` treat the replacement literally, so such a call
    /// routes through `__ds::ds_replace`/`__ds::ds_replace_all`. A replacement
    /// with no `$` (the common case) stays on the fast path — Rust's native
    /// `replace`. Pure `std` — no cargo dep.
    StringReplace,
    /// `Math.max`/`Math.min` — ES semantics differ from Rust `f64::max`/`min`
    /// on two edges: any `NaN` argument yields `NaN` (Rust returns the other
    /// operand), and `+0`/`-0` are ordered (`Math.max(-0, +0)` = `+0`,
    /// `Math.min(-0, +0)` = `-0`; Rust returns the left operand when they
    /// compare equal). Variadic `max`/`min` folds `__ds::ds_f64_max`/
    /// `ds_f64_min` left to right. Pure `std` — no cargo dep.
    F64MaxMin,
    /// `atob(s)`/`btoa(s)` — the WinterTC (Ecma TC55) base64 globals (WHATWG
    /// Encoding/Infra). `btoa` Latin-1 encodes a string's ≤U+00FF code units;
    /// `atob` strips ASCII whitespace then forgiving-decodes. Backed by the
    /// `base64` crate; marker `__ds::b64_` (the common prefix of `b64_encode`/
    /// `b64_decode`, so either global pulls the slice).
    Base64,
    /// `performance.now()` — the WinterTC (W3C hr-time) High Resolution Time
    /// API. Returns a monotonic DOMHighResTimeStamp (ms since timeOrigin).
    /// The receiver is the global `performance` object, optionally via the
    /// WinterTC `self` alias (`self.performance.now()`); both lower to
    /// `__ds::perf_now()` (a `static OnceLock<Instant>` epoch — pure `std`).
    HrTime,
    /// WinterTC WebCrypto — `crypto.randomUUID()` (an RFC 4122 v4 UUID string);
    /// `crypto`/`self.crypto` both lower to `__ds::crypto_random_uuid` (the
    /// `uuid` crate's `new_v4`, pure-Rust — never degraded).
    Crypto,
    /// WinterTC WebCrypto `SubtleCrypto` — `crypto.subtle.digest(algo, data)`
    /// (the one-shot hash; the no-key bulk of the WPT `WebCryptoAPI/digest`
    /// fixtures). `algo` is the ES name (`"SHA-1"`/`"SHA-256"`/`"SHA-384"`/
    /// `"SHA-512"`); `data` is a `BufferSource` (`Vec<u8>`); the result is the
    /// digest bytes. Backed by the RustCrypto `sha1`/`sha2` crates (pure-Rust —
    /// never degraded). `async` (ES returns a `Promise<ArrayBuffer>`); the
    /// `await` drives the future and the `Tokio` dep is pulled transitively by
    /// the async entry. The HMAC key-bearing subset (`importKey`/`sign`/`verify`,
    /// backed by `hmac`) is mapped alongside; `encrypt`/`decrypt`/`generateKey`/
    /// `deriveBits` need a wider key model and land later. The marker
    /// `__ds::crypto_subtle_` is the common prefix, so any SubtleCrypto call
    /// flags the dep (a key-only fixture reaches the slice too).
    SubtleCrypto,
    /// WHATWG URLPattern — `new URLPattern(input[, baseURL])` (a WinterTC Web
    /// API). A string `input` is a constructor string; an undefined/absent input
    /// is the empty pattern; `new URLPattern(new URL(…))` uses the URL's href.
    /// A pattern that fails to compile (an unclosed group) panics a `TypeError`.
    /// Backed by the `urlpattern` crate (denoland's WHATWG reference); marker
    /// `__ds::DsURLPattern`. Pulls `Error` (for the `DsError` the helper panics).
    /// Instance methods (`test`/`exec`) are not yet lowered.
    URLPattern,
    /// A `tokio` async runtime — the point WinterTC's asynchronous Web APIs
    /// (`fetch`/`setTimeout`/Streams/`SubtleCrypto`) and ES `async`/`await`/
    /// `Promise` introduce a runtime and a thread model. A `.ts` file whose
    /// top level awaits lowers its implicit entry to `#[tokio::main] async fn
    /// main`; `async fn` items and `.await` map to native Rust. Pure-Rust
    /// static track. Flags `tokio` (macros + rt, single-thread) and
    /// `futures`; no `__ds` helper slice (the runtime is `#[tokio::main]`, not
    /// a helper module). The marker is the `#[tokio::main]` attribute, which
    /// only the async entry emits.
    Tokio,
    /// `Promise.resolve(x)` / `Promise.all([...])` — the static track for ES
    /// `Promise` combinators (T3 stage 2a). A `Promise<T>` lowers to a boxed,
    /// single-threaded `Future<Output = T>` (`DsPromise<T>`), so every Promise
    /// site shares one Rust type (each `futures` combinator has a distinct
    /// anonymous type — boxing unifies them). `Promise.resolve` →
    /// `futures::future::ready`; `Promise.all` → `futures::future::join_all`.
    /// Reflection-driven Promise usage (Symbol.species, thenable `await`,
    /// prototype chains) is not lowered — those fixtures stay on the engine
    /// (test262) or `unsupported` (WinterTC). Flags `futures` (also pulled by
    /// `Tokio` — `append_dep` dedupes the overlap); marker `__ds::ds_promise_`.
    Promise,
    /// WHATWG `fetch(url)` — a WinterTC (Ecma TC55) Web API. ES `fetch` returns
    /// `Promise<Response>`; `await fetch(url)` lowers to `__ds::ds_fetch(url)`
    /// (a `DsResponse`), the caller's `await` driving it. `Response` properties
    /// (`status`/`ok`/`headers`/`text`) map to `DsResponse` methods. Backed by
    /// `reqwest` (rustls-tls, pure-Rust TLS — no system OpenSSL), the same HTTP
    /// core deno_fetch uses. Flags `reqwest`;
    /// marker `__ds::ds_fetch`.
    Fetch,
    /// WHATWG `EventTarget`/`Event` — the WinterTC (Ecma TC55) DOM Events API
    /// (`new EventTarget()`, `new Event(type, init)`, `addEventListener`/
    /// `removeEventListener`/`dispatchEvent`, `preventDefault`). A pub/sub backed
    /// by `Arc<Mutex<Vec<Box<dyn FnMut(&DsEvent)>>>>` (ES EventTargets are shared,
    /// mutable, single-threaded); `event.type`/`.bubbles`/`.cancelable`/
    /// `.defaultPrevented` map to `DsEvent` accessors. Pure `std` — never
    /// degraded; marker `__ds::DsEvent` (common prefix of `DsEventTarget`/
    /// `DsEvent`/`DsEventInit`, so any one pulls the slice).
    EventTarget,
    /// WHATWG `Headers` (FETCH §5.1, a WinterTC Web API) — an ordered, case-
    /// insensitive-by-name `(name, value)` list (`DsHeaders`). Pure `std`;
    /// never degraded; marker `__ds::DsHeaders`. Also pulled by `Fetch`
    /// (`DsResponse::headers` returns a `DsHeaders`).
    Headers,
    /// WHATWG `setTimeout`/`setInterval`/`clearTimeout`/`clearInterval` (HTML
    /// §8.6 timers, a WinterTC Web API) — the event loop's task queue, modeled
    /// as a `thread_local` drain run at the entry's end. Pure `std`; never
    /// degraded; marker `__ds::wpt_set_` (common prefix of `wpt_set_timeout`/
    /// `wpt_set_interval`).
    Timers,
    /// WHATWG `ReadableStream` (Streams standard, a WinterTC Web API) — the
    /// readable side. `new ReadableStream({ start(c) { c.enqueue(…); c.close() }
    /// })` + `getReader()` + `await reader.read()` → `{ done, value }`. A push-
    /// source baseline backed by an `Arc<Mutex<…>>` chunk queue (mirroring
    /// `DsResolver`); pure `std`, never degraded; marker `__ds::DsReadableStream`.
    /// `pull`/`cancel`/`tee`/BYOB are out of scope (honest partials).
    Streams,
    /// WHATWG `CompressionStream` (Streams standard, a WinterTC Web API) — the
    /// compression side. `new CompressionStream(format)` + `cs.writable.getWriter()
    /// .write(bytes)`/`.close()` + `cs.readable.getReader()` + `await reader.read()`.
    /// The transform is **internal** (`flate2`, never a user closure — this avoids
    /// the `'static`-capture blocker that gates a general `WritableStream` user
    /// sink); a one-shot model buffers writes, compresses on `close()`, and reads
    /// one chunk. Backed by `flate2`; pure-Rust static track, never degraded;
    /// marker `__ds::DsCompressionStream`. `DecompressionStream`/`brotli`/true
    /// streaming are out of scope (honest partials).
    Compression,
    /// WHATWG `AbortController`/`AbortSignal` (DOM standard, a WinterTC Web API)
    /// — `new AbortController()` + `controller.signal`/`.abort()` +
    /// `signal.aborted`/`.addEventListener("abort", cb)`. A shared
    /// `Arc<Mutex<bool>>` flag (the `aborted` state) + an embedded
    /// `DsEventTarget` (an `AbortSignal` extends `EventTarget`); pure `std`,
    /// never degraded; marker `__ds::DsAbort` (common prefix of
    /// `DsAbortController`/`DsAbortSignal`). The dep resolution pulls
    /// `EventTarget` alongside (the signal reuses `DsEventTarget`/`DsEvent`).
    AbortController,
    /// WHATWG `Blob` API (FileAPI, a WinterTC Web API) — `new Blob(parts, options?)`
    /// flattens the parts into one byte buffer (each `string` → UTF-8 bytes,
    /// `number` → `number_to_string` then bytes, a `Uint8Array`/`Blob` local →
    /// its bytes); `blob.size`/`blob.type` are zero-arg accessors and
    /// `blob.slice(start, end, contentType)` returns a new `Blob`. The async
    /// methods `text()`/`arrayBuffer()`/`bytes()` are `async fn`s (an `await`
    /// at the call site adds the `.await`). Pure `std` — no cargo dep; the
    /// marker is `__ds::DsBlob`.
    Blob,
    /// WHATWG `File` API (FileAPI, a WinterTC Web API) — `new File(bits, name,
    /// options?)`. A `File` is a `Blob` with a `name` and a `lastModified`
    /// (epoch-ms), so `DsFile` wraps a `DsBlob` and delegates `size`/`type`/
    /// `slice`/`text`/`arrayBuffer`/`bytes` to it (ES `File extends Blob`); a
    /// `File.prototype.slice` returns a `Blob` (not a `File`). `name`/`type` and
    /// `lastModified` (default `Date.now()`) come from the ctor; `instanceof
    /// Blob` holds for a `File` (subtype). Pure `std` — no cargo dep of its own;
    /// the marker is `__ds::DsFile`, and the dep resolution pulls `Blob`
    /// alongside (File reuses `DsBlob`).
    File,
    /// WHATWG `FormData` API (FETCH §5.2 / XHR, a WinterTC Web API) —
    /// `new FormData()` + `append(name, value)`/`append(name, file)` +
    /// `has(name)`/`delete(name)`/`set(name, value)`/`set(name, file)`. An
    /// ordered `(name, value)` list where `value` is a `string` or a `File`
    /// (a `DsFormEntryValue` enum); the value-returning `get`/`getAll`/
    /// `entries`/`forEach` stay unmapped (their `string | File` union result
    /// needs the union-unboxing path, a separate batch). Pure `std` — no cargo
    /// dep of its own; the marker is `__ds::DsFormData`, and the dep resolution
    /// pulls `File` alongside (the value enum carries a `DsFile`).
    FormData,
    /// ES legacy URI globals (ECMA-262 §B.2.1) — `encodeURI`/`decodeURI`/
    /// `encodeURIComponent`/`decodeURIComponent`, the percent-escape/unescape
    /// functions every JS host ships. Pure `std` (UTF-8 byte percent-encode/
    /// decode); the marker is `__ds::uri_` (common prefix of `uri_encode`/
    /// `uri_decode`/`uri_encode_component`/`uri_decode_component`), so a
    /// fixture using any one pulls URI_HELPER (sibling free fns in the slice).
    Uri,
    /// ES2025 `Math.sumPrecise` — exact summation of a `number` array/iterable,
    /// rounded to nearest-even. The finite path delegates to the `xsum` crate
    /// (`__ds::sum_precise_exact` wraps `XsumAuto`); the NaN/±∞/−0 state
    /// machine stays inline at the call site. Cargo dep `xsum`; marker
    /// `__ds::sum_precise`.
    SumPrecise,
}

impl RuntimeDep {
    /// All variants in declaration order — the order helper slices and cargo
    /// deps are emitted, so output stays deterministic.
    pub(super) const ALL: [RuntimeDep; 38] = [
        RuntimeDep::RyuJs,
        RuntimeDep::SerdeJson,
        RuntimeDep::Engine,
        RuntimeDep::Atomics,
        RuntimeDep::ArrayHelper,
        RuntimeDep::Regress,
        RuntimeDep::Temporal,
        RuntimeDep::Worker,
        RuntimeDep::Truthy,
        RuntimeDep::Display,
        RuntimeDep::Encoding,
        RuntimeDep::Url,
        RuntimeDep::Error,
        RuntimeDep::Inspect,
        RuntimeDep::Assert,
        RuntimeDep::WptAssert,
        RuntimeDep::CollectionKey,
        RuntimeDep::StringReplace,
        RuntimeDep::F64MaxMin,
        RuntimeDep::Base64,
        RuntimeDep::HrTime,
        RuntimeDep::Crypto,
        RuntimeDep::SubtleCrypto,
        RuntimeDep::URLPattern,
        RuntimeDep::Tokio,
        RuntimeDep::Promise,
        RuntimeDep::Fetch,
        RuntimeDep::EventTarget,
        RuntimeDep::Headers,
        RuntimeDep::Timers,
        RuntimeDep::Streams,
        RuntimeDep::Compression,
        RuntimeDep::AbortController,
        RuntimeDep::Blob,
        RuntimeDep::File,
        RuntimeDep::FormData,
        RuntimeDep::Uri,
        RuntimeDep::SumPrecise,
    ];

    /// The emitted-text marker that signals this dep was pulled in. `None` for
    /// `Engine` — it is set explicitly when the translator detects a reflection
    /// construct (the body is never lowered, so there is no text to scan).
    pub(super) fn marker(self) -> Option<&'static str> {
        match self {
            RuntimeDep::RyuJs => Some("__ds::number_to_string"),
            RuntimeDep::SerdeJson => Some("serde_json::"),
            RuntimeDep::ArrayHelper => Some("__ds::array_set"),
            RuntimeDep::Regress => Some("__ds::regex"),
            RuntimeDep::Temporal => Some("temporal_rs::"),
            RuntimeDep::Worker => Some("__ds::Worker"),
            RuntimeDep::Truthy => Some("__ds::truthy"),
            RuntimeDep::Display => Some("__ds::display"),
            // Common prefix of `__ds::TextEncoder` and `__ds::TextDecoder`, so
            // a fixture using either the encoder or the decoder pulls
            // ENCODING_HELPER (both structs live in the same slice) — a
            // TextDecoder-only fixture must still inject the slice, or
            // `__ds::TextDecoder` is an undefined type (E0433).
            RuntimeDep::Encoding => Some("__ds::Text"),
            // Common prefix of `__ds::DsUrl` and `__ds::DsUrlSearchParams`, so
            // a fixture using either the WHATWG URL parser (`new URL(…)`) or the
            // query params list pulls URL_HELPER (the `DsUrl` wrapper + the
            // `DsUrlSearchParams` list live in the same slice).
            RuntimeDep::Url => Some("__ds::DsUrl"),
            RuntimeDep::Error => Some("__ds::DsError"),
            RuntimeDep::Inspect => Some("__ds::inspect"),
            // Common prefix of `assert_same_value`/`assert_not_same_value`/
            // `assert_throws`, so a fixture using any one `assert.*` form pulls
            // ASSERT_HELPER (each is a sibling free fn in the slice).
            RuntimeDep::Assert => Some("__ds::assert_"),
            // Common prefix of `wpt_assert_equals`/`wpt_assert_not_equals`/
            // `wpt_assert_throws`/`wpt_assert_unreached`, so a fixture using any
            // WPT assert pulls ASSERT_HELPER (sibling free fns in the slice).
            RuntimeDep::WptAssert => Some("__ds::wpt_"),
            RuntimeDep::CollectionKey => Some("__ds::DsF64Key"),
            RuntimeDep::StringReplace => Some("__ds::ds_replace"),
            RuntimeDep::F64MaxMin => Some("__ds::ds_f64_max"),
            RuntimeDep::Base64 => Some("__ds::b64_"),
            // Common prefix of `perf_now`/`perf_time_origin`, so a fixture using
            // `performance.now()` (call) or `performance.timeOrigin` (member)
            // pulls PERF_HELPER (sibling free fns in the slice).
            RuntimeDep::HrTime => Some("__ds::perf_"),
            // Common prefix of `crypto_random_uuid`/`crypto_get_random_values`,
            // so a fixture using either WebCrypto method (`randomUUID`/
            // `getRandomValues`) pulls CRYPTO_HELPER (sibling free fns in the
            // slice) and the `uuid` + `getrandom` crates.
            RuntimeDep::Crypto => Some("__ds::crypto_"),
            // `__ds::crypto_subtle_` is the common prefix of `digest`/
            // `import_key`/`sign`/`verify`, so any SubtleCrypto call flags the
            // dep (and pulls `sha1`/`sha2`/`hmac`) — a key-only fixture
            // (`importKey`/`sign`/`verify`, no `digest`) reaches the slice too.
            RuntimeDep::SubtleCrypto => Some("__ds::crypto_subtle_"),
            RuntimeDep::URLPattern => Some("__ds::DsURLPattern"),
            // The `#[tokio::main]` attribute only the async entry emits — a
            // `.ts` source cannot produce it any other way, so its presence in
            // the generated text is the signal that this crate pulls tokio.
            // `current_thread` flavor matches JS's single-thread event loop and
            // needs no `Send` bound on futures (a `promise_test` body capturing
            // any value type compiles).
            RuntimeDep::Tokio => Some("#[tokio::main(flavor = \"current_thread\")]"),
            RuntimeDep::Promise => Some("__ds::ds_promise_"),
            RuntimeDep::Fetch => Some("__ds::ds_fetch"),
            // Common prefix of `__ds::DsEventTarget`, `__ds::DsEvent`, and
            // `__ds::DsEventInit`, so a fixture using any one of the three
            // (`new EventTarget()`, `new Event(…)`, or a `{ bubbles, cancelable }`
            // init literal) pulls EVENT_TARGET_HELPER (all three structs live in
            // the same slice).
            RuntimeDep::EventTarget => Some("__ds::DsEvent"),
            // `__ds::DsHeaders` — the WHATWG `Headers` model (FETCH §5.1).
            // Also pulled in by `Fetch` (`DsResponse::headers` returns a
            // `DsHeaders`), but a `new Headers()` constructor emits this marker
            // directly, so a Headers-only fixture injects the slice.
            RuntimeDep::Headers => Some("__ds::DsHeaders"),
            // Common prefix of `__ds::wpt_set_timeout` and
            // `__ds::wpt_set_interval`, so a fixture registering either timer
            // pulls TIMERS_HELPER (both the scheduling fns and the drain live in
            // the same slice). `wpt_done`/`wpt_clear_timer`/`wpt_run_timers` do
            // not by themselves prove a timer was registered (so they do not
            // pull the slice on their own).
            RuntimeDep::Timers => Some("__ds::wpt_set_"),
            RuntimeDep::Streams => Some("__ds::DsReadableStream"),
            RuntimeDep::Compression => Some("__ds::DsCompressionStream"),
            // Common prefix of `__ds::DsAbortController` and `__ds::DsAbortSignal`,
            // so a fixture using either the controller (`new AbortController()`)
            // or the signal (`controller.signal` / an annotated binding) pulls
            // DS_ABORT_HELPER (both structs live in the same slice). The signal
            // reuses `DsEventTarget`/`DsEvent` from EVENT_TARGET_HELPER, which
            // the dep resolution pulls alongside (see the `AbortController` arm
            // after the marker probe) — without it, `DsEventTarget` is E0433.
            RuntimeDep::AbortController => Some("__ds::DsAbort"),
            RuntimeDep::Blob => Some("__ds::DsBlob"),
            // Common prefix of `__ds::DsFile` and `__ds::DsBlob` — a fixture
            // using `File` pulls both FILE_HELPER (DsFile) and BLOB_HELPER
            // (DsFile wraps DsBlob). The marker `__ds::DsFile` is unique, but
            // the dep derivation below also inserts `Blob` so the wrapped type
            // is defined.
            RuntimeDep::File => Some("__ds::DsFile"),
            RuntimeDep::FormData => Some("__ds::DsFormData"),
            // Common prefix of `uri_encode`/`uri_decode`/`uri_encode_component`/
            // `uri_decode_component`, so any of the four ES URI globals pulls
            // URI_HELPER (sibling free fns in the slice).
            RuntimeDep::Uri => Some("__ds::uri_"),
            // `__ds::sum_precise_exact` — the Math.sumPrecise finite path.
            RuntimeDep::SumPrecise => Some("__ds::sum_precise"),
            RuntimeDep::Engine => None,
            // `$262.agent` is engine-only — the body degrades to `__ds::engine`
            // (like `Engine`), so no static text marker.
            RuntimeDep::Atomics => None,
        }
    }

    /// The cargo dependencies to append, if this dep needs any crate(s). A slice
    /// because one runtime dep can pull more than one crate (`Worker` needs both
    /// `serde` — the trait bounds `Serialize`/`DeserializeOwned` — and
    /// `serde_json` for the actual marshal). `append_dep` is idempotent, so an
    /// overlap with another dep (or a user-declared `cargo:serde_json`) is a
    /// no-op, not a duplicate. `None` for `ArrayHelper` (pure `std`).
    pub(super) fn cargo(self) -> Option<&'static [(&'static str, &'static str)]> {
        match self {
            // The crates.io package is `ryu-js` (hyphen); Rust exposes it as
            // `ryu_js` (underscore) in `use`, so the Cargo.toml key uses the
            // package name.
            RuntimeDep::RyuJs => Some(&[("ryu-js", "\"1.0\"")]),
            RuntimeDep::SerdeJson => Some(&[("serde_json", "\"1\"")]),
            // `rquickjs` bundles QuickJS-NG C sources (compiled via `cc`), so
            // it is only emitted for programs that opt into the engine path.
            // `serde_json` is the per-function degradation marshal layer
            // (`call_fn` marshals args/return as `serde_json::Value`).
            RuntimeDep::Engine => Some(&[
                // `loader` enables `Loader`/`Resolver` so the engine can load
                // multi-file `.js` ESM modules (npm packages with sibling
                // `import`s) at runtime, not just single-file `ctx.eval`.
                (
                    "rquickjs",
                    "{ version = \"0.12\", features = [\"loader\"] }",
                ),
                ("serde", "{ version = \"1\", features = [\"derive\"] }"),
                ("serde_json", "\"1\""),
            ]),
            // `$262.agent` reuses the engine's `rquickjs` (already pulled by
            // `Engine` — atomics fixtures degrade the whole body, so `Engine`
            // is always set alongside). No extra crate.
            RuntimeDep::Atomics => None,
            RuntimeDep::Regress => Some(&[("regress", "\"0.11\"")]),
            // `temporal_rs` (boa-dev/temporal-rs) — the Rust implementation of
            // ECMAScript Temporal. Default features embed time-zone data
            // (`compiled_data`) so `Temporal.Now`/`ZonedDateTime` work standalone.
            RuntimeDep::Temporal => Some(&[("temporal_rs", "\"0.2\"")]),
            // `Worker` marshals messages as JSON. The handler's message type is
            // bounded `serde::Serialize`/`DeserializeOwned` (the trait, for
            // type-safe `from_value`/`to_value`), so `serde` is needed alongside
            // `serde_json` — the default crate exposes the traits (no `derive`
            // feature: the helper bounds generic params, it does not derive).
            RuntimeDep::Worker => Some(&[("serde", "\"1\""), ("serde_json", "\"1\"")]),
            RuntimeDep::ArrayHelper => None,
            RuntimeDep::Truthy => None,
            RuntimeDep::Display => None,
            // `encoding_rs` (Mozilla) — the WHATWG Encoding Standard reference
            // implementation `TextDecoder` resolves labels through and decodes
            // (UTF-8/UTF-16/single-byte/multi-byte/CJK). `TextEncoder` is UTF-8
            // only and stays `String::into_bytes` (no crate path).
            RuntimeDep::Encoding => Some(&[("encoding_rs", "\"0.8\"")]),
            // `url` (servo/url) — the WHATWG URL reference parser `DsUrl`
            // wraps. `serde` provides the `Serialize` trait for
            // `JSON.stringify(url)` / `url.toJSON()`. `form_urlencoded` is the
            // `application/x-www-form-urlencoded` parser/serializer
            // `DsUrlSearchParams` routes through (cached locally as a transitive
            // dep of the workspace `url` crate).
            RuntimeDep::Url => Some(&[
                ("url", "\"2\""),
                ("serde", "\"1\""),
                ("form_urlencoded", "\"1.2\""),
            ]),
            RuntimeDep::Error => None,
            RuntimeDep::Assert => None,
            RuntimeDep::WptAssert => None,
            RuntimeDep::Inspect => Some(&[("ryu-js", "\"1.0\""), ("serde_json", "\"1\"")]),
            RuntimeDep::CollectionKey => None,
            RuntimeDep::StringReplace => None,
            RuntimeDep::F64MaxMin => None,
            RuntimeDep::Base64 => Some(&[("base64", "\"0.22\"")]),
            RuntimeDep::HrTime => None,
            // `uuid` (uuid-rs/uuid) — `crypto.randomUUID()` is RFC 4122 v4.
            // The `v4` feature enables `Uuid::new_v4` (backed by `getrandom`).
            // `getrandom` (rust-random/getrandom) — `crypto.getRandomValues(buf)`
            // fills a byte buffer from the system CSPRNG (0.2 — the same major
            // version `uuid::new_v4` pulls, so the two never fork the source).
            RuntimeDep::Crypto => Some(&[
                ("uuid", "{ version = \"1\", features = [\"v4\"] }"),
                ("getrandom", "\"0.2\""),
            ]),
            // `sha1`/`sha2` (RustCrypto) — `crypto.subtle.digest` one-shot hash
            // (pure-Rust). SHA-1 (20 bytes) + the SHA-2 family (256/384/512).
            // The `Tokio` runtime the async `digest` needs is pulled transitively
            // by the `await`-driven async entry, not here.
            RuntimeDep::SubtleCrypto => Some(&[
                ("sha1", "\"0.10\""),
                ("sha2", "\"0.10\""),
                // HMAC `sign`/`verify` (the key-bearing SubtleCrypto subset).
                ("hmac", "\"0.12\""),
                // AES-GCM `encrypt`/`decrypt` (the authenticated-encryption
                // subset). WebCrypto's AES-GCM output is `ciphertext || tag`,
                // byte-compatible with `aead::Aead::encrypt`. Keyed by length
                // (128/256; 192 is not statically modeled — `aes-gcm` does not
                // export `Aes192Gcm` by default). `aead` is named directly so
                // `::aead::{Aead, KeyInit, Key, Nonce}` resolves regardless of
                // re-export.
                ("aes-gcm", "\"0.10\""),
                ("aead", "\"0.5\""),
                // AES-CBC `encrypt`/`decrypt` (the unauthenticated block-cipher
                // subset). `cbc::Encryptor`/`Decryptor` over `aes::Aes128`/
                // `Aes256`, PKCS7 padding (the only padding WebCrypto uses). 192
                // is not statically modeled (mirrors AES-GCM). `cbc` re-exports
                // the `cipher` traits (`KeyIvInit`/`BlockEncryptMut`/
                // `BlockDecryptMut`), so `cbc::cipher::*` resolves without a
                // separate `cipher` dep; `aes` is named directly for the
                // `Aes128`/`Aes256` block-cipher types. The `alloc` +
                // `block-padding` features unlock `encrypt_padded_vec_mut`/
                // `decrypt_padded_vec_mut` (the allocating, PKCS7-padded API).
                ("aes", "\"0.8\""),
                ("cbc", "{ version = \"0.1\", features = [\"alloc\", \"block-padding\"] }"),
                // `getrandom` — `crypto.subtle.generateKey(…)` fills the fresh
                // key with cryptographically random bytes (the same source
                // `crypto.getRandomValues` uses, listed under `Crypto`).
                ("getrandom", "\"0.2\""),
            ]),
            // `urlpattern` (denoland/rust-urlpattern) — the WHATWG URLPattern
            // reference. `new URLPattern(…)` wraps `urlpattern::UrlPattern`; a
            // pattern that fails to compile panics a `TypeError` (ES error class).
            // `regex` is named explicitly because `parse_constructor_string<R:
            // RegExp>` needs a turbofish (`::<regex::Regex>`) — urlpattern 0.6
            // binds the default engine `R` to `regex::Regex` but does not re-export
            // the crate, so it must be a direct dep of the emitted project.
            RuntimeDep::URLPattern => Some(&[("urlpattern", "\"0.6\""), ("regex", "\"1\"")]),
            // `tokio` (tokio-rs/tokio) — the async runtime `#[tokio::main]`
            // expands against. `macros` (the attribute) + `rt` (a single-thread
            // scheduler — `flavor = "current_thread"`). `futures` for the
            // `Future` trait/`FutureExt` a `Promise` mapping composes over.
            RuntimeDep::Tokio => Some(&[
                (
                    "tokio",
                    "{ version = \"1\", features = [\"macros\", \"rt\"] }",
                ),
                ("futures", "\"0.3\""),
            ]),
            // `futures` — `Promise.resolve`/`all` compose over `ready`/`join_all`.
            // Also pulled by `Tokio` (`append_dep` dedupes the overlap).
            RuntimeDep::Promise => Some(&[("futures", "\"0.3\"")]),
            // `reqwest` — the WHATWG fetch HTTP core (deno_fetch's engine).
            // `rustls-tls` uses pure-Rust TLS (rustls + webpki-roots), so the
            // emitted project needs no system OpenSSL/schannel and builds the
            // same on every target. `charset` lets `Response::text()` honor a
            // Content-Type charset; `http2` is the default ALPN. reqwest pulls
            // tokio itself for its async runtime (the fixture already depends
            // on tokio via `Tokio`; `append_dep` dedupes the overlap).
            RuntimeDep::Fetch => Some(&[
                ("reqwest", "{ version = \"0.12\", default-features = false, features = [\"rustls-tls\", \"charset\", \"http2\"] }"),
                // `Response.json()` parses the body via `serde_json::Value`.
                ("serde_json", "\"1\""),
            ]),
            // EventTarget/Event is pure `std` (`Arc<Mutex<Vec<…>>>` pub/sub +
            // `Cell` interior mutability) — no cargo dep.
            RuntimeDep::EventTarget => None,
            // Pure `std` (`Vec<(String, String)>`); no crate. A `reqwest`
            // header map is the input only when `DsResponse::headers` builds a
            // view, and `reqwest` is then flagged by `Fetch`, not `Headers`.
            RuntimeDep::Headers => None,
            // Pure `std` (a `Vec` + `Instant`); no crate.
            RuntimeDep::Timers => None,
            // Pure `std` (the `Arc<Mutex<VecDeque<…>>>` queue + `Waker`); no
            // crate. The boxed `read()` future is spelled inline (`Pin<Box<dyn
            // Future>>`), so a Streams-only fixture pulls no `futures`/Promise.
            // Pure `std` (the `Arc<Mutex<VecDeque<…>>>` queue + `Waker`); no
            // crate. The boxed `read()` future is spelled inline (`Pin<Box<dyn
            // Future>>`), so a Streams-only fixture pulls no `futures`/Promise.
            RuntimeDep::Streams => None,
            // `flate2` (rust-lang/flate2) — the WHATWG compression core
            // (`gzip`/`deflate`/`deflate-raw`). Pure-Rust DEFLATE (miniz_oxide by
            // default, no C); the one-shot `GzEncoder`/`ZlibEncoder`/
            // `DeflateEncoder` `ds_compress` composes over.
            RuntimeDep::Compression => Some(&[("flate2", "\"1\"")]),
            // AbortController/AbortSignal is pure `std` (`Arc<Mutex<bool>>` +
            // the embedded `DsEventTarget`); no crate. `DsEventTarget` comes
            // from EVENT_TARGET_HELPER, pulled by the dep resolution.
            RuntimeDep::AbortController => None,
            RuntimeDep::Blob => None,
            // File is pure `std` (wraps `DsBlob`); no crate of its own. The
            // wrapped `DsBlob` comes from BLOB_HELPER, pulled by derivation.
            RuntimeDep::File => None,
            // FormData is pure `std` (an ordered list + a value enum); no crate.
            // The `DsFile` its value enum carries comes from FILE_HELPER, pulled
            // by derivation.
            RuntimeDep::FormData => None,
            // ES URI globals are pure `std` (UTF-8 byte percent-encode/decode);
            // no cargo dep.
            RuntimeDep::Uri => None,
            // Math.sumPrecise's exact summation delegates to the `xsum` crate
            // (Radford Neal's superaccumulator); the NaN/±∞/−0 state machine
            // stays inline at the call site.
            RuntimeDep::SumPrecise => Some(&[("xsum", "\"0.1\"")]),
        }
    }

    /// The `__ds` helper source slice this dep contributes, if any.
    pub(super) fn helper(self) -> Option<&'static str> {
        match self {
            RuntimeDep::RyuJs => Some(RYUJS_HELPERS),
            RuntimeDep::ArrayHelper => Some(ARRAY_HELPER),
            RuntimeDep::Regress => Some(REGRESS_HELPERS),
            RuntimeDep::SerdeJson
            | RuntimeDep::Engine
            | RuntimeDep::Temporal
            | RuntimeDep::Atomics => None,
            RuntimeDep::Worker => Some(WORKER_HELPER),
            RuntimeDep::Truthy => Some(TRUTHY_HELPER),
            RuntimeDep::Display => Some(DISPLAY_HELPER),
            RuntimeDep::Encoding => Some(ENCODING_HELPER),
            RuntimeDep::Url => Some(URL_HELPER),
            RuntimeDep::Error => Some(ERROR_HELPER),
            RuntimeDep::Assert => Some(ASSERT_HELPER),
            // WPT asserts share ASSERT_HELPER (same DsSameValue core). A
            // WPT-only fixture still pulls the slice's test262 asserts (unused
            // but must type-check) — the same asymmetry Assert has with
            // `assert_throws` (which needs ERROR_HELPER, pulled below).
            RuntimeDep::WptAssert => Some(ASSERT_HELPER),
            RuntimeDep::Inspect => Some(INSPECT_HELPER),
            RuntimeDep::CollectionKey => Some(COLLECTION_KEY_HELPER),
            RuntimeDep::StringReplace => Some(STRING_REPLACE_HELPER),
            RuntimeDep::F64MaxMin => Some(F64_MAXMIN_HELPER),
            RuntimeDep::Base64 => Some(BASE64_HELPER),
            RuntimeDep::HrTime => Some(PERF_HELPER),
            RuntimeDep::Crypto => Some(CRYPTO_HELPER),
            RuntimeDep::SubtleCrypto => Some(SUBTLE_HELPER),
            RuntimeDep::URLPattern => Some(URLPATTERN_HELPER),
            // The runtime is `#[tokio::main]`, not a helper module — no slice.
            RuntimeDep::Tokio => None,
            RuntimeDep::Promise => Some(DS_PROMISE_HELPER),
            RuntimeDep::Fetch => Some(DS_FETCH_HELPER),
            RuntimeDep::EventTarget => Some(EVENT_TARGET_HELPER),
            RuntimeDep::Headers => Some(HEADERS_HELPER),
            RuntimeDep::Timers => Some(TIMERS_HELPER),
            RuntimeDep::Streams => Some(DS_STREAMS_HELPER),
            RuntimeDep::Compression => Some(DS_COMPRESSION_HELPER),
            RuntimeDep::AbortController => Some(DS_ABORT_HELPER),
            RuntimeDep::Blob => Some(BLOB_HELPER),
            RuntimeDep::File => Some(FILE_HELPER),
            RuntimeDep::FormData => Some(FORM_DATA_HELPER),
            RuntimeDep::Uri => Some(URI_HELPER),
            RuntimeDep::SumPrecise => Some(SUM_PRECISE_HELPER),
        }
    }

    /// The engine-path Web API builtin for this dep — `(register_call,
    /// fn_source)` — when this is a WinterTC Web API the engine should register
    /// via `wire_web_apis` so a degraded function calling it finds it in the
    /// QuickJS global. `None` for non-Web-API deps and for Web APIs whose engine
    /// builtin isn't wired yet (added per the Javy pattern — JS shim + native
    /// `Function::new` delegating to the same `__ds::` impl the static path
    /// uses). `register_call` is stamped into `wire_web_apis`'s body;
    /// `fn_source` (defining `fn <register_call>(ctx)`) is appended to the
    /// engine module. Emitted only when `Engine` is also active, so a non-engine
    /// fixture never references these.
    pub(super) fn engine_builtin(self) -> Option<(&'static str, &'static str)> {
        match self {
            RuntimeDep::Encoding => {
                Some(("register_text_encoding(ctx)", TEXT_ENCODING_ENGINE_BUILTIN))
            }
            RuntimeDep::HrTime => Some(("register_perf_now(ctx)", PERF_ENGINE_BUILTIN)),
            RuntimeDep::Base64 => Some(("register_base64(ctx)", BASE64_ENGINE_BUILTIN)),
            RuntimeDep::Crypto => Some(("register_crypto(ctx)", CRYPTO_ENGINE_BUILTIN)),
            // `assert.sameValue`/`throws`/`notSameValue` + `Test262Error` — the
            // test262 harness assert family. Pure-JS shim (no native fn): the
            // static path's `assert_same_value<A: DsSameValue>` is generic over
            // concrete Rust types, unreachable from a dynamic `rquickjs::Value`,
            // but QuickJS already has ES `Object.is` (SameValue) + `Error`/
            // `try-catch`, so the assert family runs faithfully in JS. One
            // contract (a mismatch throws `Test262Error`), two delivery paths.
            RuntimeDep::Assert => Some(("register_assert(ctx)", ASSERT_ENGINE_BUILTIN)),
            // WPT testharness sync subset (`assert_equals`/`true`/`approx`/… +
            // `AssertionError`). Same pure-JS reasoning as `Assert`;
            // `promise_test`/`async_test` need an async runtime the engine
            // lacks — those fixtures honestly degrade to `EngineLimitation`.
            RuntimeDep::WptAssert => Some(("register_wpt_assert(ctx)", WPT_ASSERT_ENGINE_BUILTIN)),
            // `EventTarget`/`AbortSignal`/`AbortController` — the WHATWG
            // abort/event family. A single `register_abort` defines all three
            // (`AbortSignal extends EventTarget`, and the controller holds a
            // signal). Mapped only under `EventTarget`: an `AbortController`
            // dep derives `EventTarget` (see `derive_deps`), so this stamps
            // `register_abort(ctx)` exactly once regardless of which of the
            // three a degraded body reaches. Pure-JS shim (no native fn) —
            // the static `DsAbortSignal` carries `Arc<Mutex<…>>` + callback
            // boxes that cannot cross the serde boundary, but the ES semantics
            // are a small state machine the shim runs faithfully.
            RuntimeDep::EventTarget => Some(("register_abort(ctx)", ABORT_ENGINE_BUILTIN)),
            // `$262.agent` — the tc39 test262 agent API for true cross-thread
            // `Atomics.wait`/`notify`. Each agent is an independent `Runtime` +
            // own OS thread (QuickJS's `run-test262.c` model); the SAB is
            // shared by raw backing pointer, broadcast sync via `Mutex`+
            // `Condvar`. No native `__ds::` delegation — the whole agent state
            // machine lives in the builtin (it owns the threads + shared
            // state); `atomicsHelper.js`'s high-level `safeBroadcast`/`waitUntil`
            // ride on top unchanged.
            RuntimeDep::Atomics => Some(("register_atomics_agent(ctx)", AGENT_262_ENGINE_BUILTIN)),
            _ => None,
        }
    }
}
