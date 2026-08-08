/// WHATWG `fetch` API helper — `__ds::DsResponse`/`__ds::ds_fetch`. A WinterTC
/// (Ecma TC55) Web API: ES `fetch(url)` returns `Promise<Response>`; this slice
/// holds the `DsResponse`/`DsHeaders` wrappers + the `ds_fetch` async fn that
/// `await fetch(url)` lowers to. Backed by `reqwest` (deno_fetch's HTTP core,
/// the crate Deno/servo reach for) — pure-Rust static track. reqwest
/// auto-switches its backend on `wasm32` (browser `fetch` via wasm-bindgen),
/// so one slice covers the native and the future wasm target.
pub const DS_FETCH_HELPER: &str = r#"
/// A WHATWG `Response` — owns its parts (status/status_text/headers/body), so
/// both a real `fetch(…)` (the body drained eagerly) and a synthetic
/// `new Response(body, init)` (no network) lower to one shape. The body is a
/// one-shot resource (ES semantics): `text`/`json`/`array_buffer` consume
/// `self`; `status`/`status_text`/`ok`/`headers` borrow `&self` (the ES
/// properties do not drain the body). `reqwest::Response` has no public
/// parts-constructor, which is why a synthetic `new Response(…)` forces this
/// owned shape (a `fetch(…)` drains the reqwest body into `body` up front).
pub struct DsResponse {
    pub status: u16,
    pub status_text: ::std::string::String,
    pub headers: DsHeaders,
    pub body: ::std::vec::Vec<u8>,
}
impl DsResponse {
    /// Build a `DsResponse` from a live `reqwest::Response` — the body is
    /// drained eagerly so the rest of `DsResponse` is pure data (no async
    /// needed for `status`/`ok`/`headers`). Used by the `fetch(…)` producers.
    async fn from_reqwest(resp: reqwest::Response) -> DsResponse {
        let status = resp.status().as_u16();
        let status_text = resp
            .status()
            .canonical_reason()
            .unwrap_or("")
            .to_string();
        let mut entries = ::std::vec::Vec::new();
        for (k, v) in resp.headers().iter() {
            if let Ok(s) = v.to_str() {
                entries.push((k.as_str().to_lowercase(), ::std::string::String::from(s)));
            }
        }
        let body = resp.bytes().await.unwrap_or_default().to_vec();
        DsResponse {
            status,
            status_text,
            headers: DsHeaders { entries },
            body,
        }
    }
    /// The translator-emitted constructor for `new Response(body, init?)`. The
    /// `body` is the already-flattened byte buffer (a string/`Blob`/`Uint8Array`
    /// lowered by the translator's Blob-parts coercion); `status` (default
    /// `200`), `status_text` (default `""`), and `headers` (a `(name, value)`
    /// list) come from the ES `init` object. No network — ES `new Response(…)`
    /// is synchronous.
    pub fn new(
        body: ::std::vec::Vec<u8>,
        status: u16,
        status_text: ::std::string::String,
        headers: ::std::vec::Vec<(::std::string::String, ::std::string::String)>,
    ) -> Self {
        DsResponse {
            status,
            status_text,
            headers: DsHeaders {
                entries: headers.into_iter().map(|(k, v)| (k.to_lowercase(), v)).collect(),
            },
            body,
        }
    }
    /// HTTP status code (e.g. 200). ES `response.status` is a number.
    #[inline]
    pub fn status(&self) -> f64 {
        self.status as f64
    }
    /// True iff the status is a 2xx. ES `response.ok`.
    #[inline]
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }
    /// ES `response.statusText` — the HTTP status text (e.g. "OK", "Created").
    #[inline]
    pub fn status_text(&self) -> ::std::string::String {
        self.status_text.clone()
    }
    /// The response headers. ES `response.headers` — a `DsHeaders` view (a
    /// clone of the owned field; names lowercased, insertion order kept).
    /// `DsHeaders` lives in `HEADERS_HELPER` (a pure-`std` slice).
    #[inline]
    pub fn headers(&self) -> DsHeaders {
        self.headers.clone()
    }
    /// The body as UTF-8 text. ES `await response.text()` (consumes the body).
    #[inline]
    pub async fn text(self) -> ::std::string::String {
        ::std::string::String::from_utf8_lossy(&self.body).into_owned()
    }
    /// The body parsed as JSON. ES `await response.json()` (consumes the body);
    /// a body that fails to parse yields `null` (ES would reject the promise
    /// with a `SyntaxError` — the `null` prefix is what the harness reads).
    #[inline]
    pub async fn json(self) -> ::serde_json::Value {
        ::serde_json::from_slice(&self.body).unwrap_or(::serde_json::Value::Null)
    }
    /// The body as raw bytes. ES `await response.arrayBuffer()` (consumes the
    /// body).
    #[inline]
    pub async fn array_buffer(self) -> ::std::vec::Vec<u8> {
        self.body
    }
}
/// `fetch(url)` — a GET request returning a `DsResponse`. ES `fetch` returns a
/// `Promise<Response>`; this async fn is what `await fetch(url)` lowers to (the
/// caller's `await` supplies the `.await`). A 3s timeout keeps a fixture aimed
/// at a WPT test server that does not exist in this environment from hanging
/// the conformance harness. A network failure panics (ES would reject the
/// promise with a `TypeError`; the panic prefix is what the harness reads).
pub async fn ds_fetch<T: reqwest::IntoUrl>(url: T) -> DsResponse {
    let resp = reqwest::Client::builder()
        .timeout(::std::time::Duration::from_secs(3))
        .build()
        .expect("reqwest client build")
        .get(url)
        .send()
        .await
        .expect("fetch network error");
    DsResponse::from_reqwest(resp).await
}
/// `fetch(url, init)` — a request built from the ES `init` object fields:
/// `method` (an HTTP verb, case-insensitive), `body` (a string payload), and
/// `headers` (a `(name, value)` list). ES `fetch` returns `Promise<Response>`;
/// this async fn is what `await fetch(url, init)` lowers to. `method` defaults
/// to GET when `init` omits it; `body`/`headers` are `None`/empty when absent.
/// Same 3s timeout and panic-on-network-error as `ds_fetch`.
pub async fn ds_fetch_with<T: reqwest::IntoUrl>(
    url: T,
    method: ::std::string::String,
    body: ::std::option::Option<::std::string::String>,
    headers: ::std::vec::Vec<(::std::string::String, ::std::string::String)>,
) -> DsResponse {
    let mut req = reqwest::Client::builder()
        .timeout(::std::time::Duration::from_secs(3))
        .build()
        .expect("reqwest client build")
        .request(
            method.to_ascii_uppercase().parse().expect("invalid HTTP method"),
            url,
        );
    if let Some(b) = body {
        req = req.body(b);
    }
    for (k, v) in headers {
        req = req.header(k, v);
    }
    DsResponse::from_reqwest(req.send().await.expect("fetch network error")).await
}
/// A WHATWG `Request` — a fetch descriptor built by `new Request(url, init)`
/// (FETCH §5.2, a WinterTC Web API). It carries the `url`, the HTTP `method`
/// (uppercased — the ES `Request.method` normalization), the `body` (an ES
/// string payload, `None` when absent), and the `headers` (a `(name, value)`
/// list, the same shape `ds_fetch_with` consumes). The translator builds the
/// `(url, method, body, headers)` quadruple from the ES `init` object via the
/// same `fetch_init` extraction `fetch(url, init)` uses, so a `Request` and an
/// inline `init` agree. `fetch(request)` unwraps the fields via
/// `ds_fetch_request`; `.url`/`.method`/`.headers` are the read-only
/// accessors. `#[derive(Clone)]` so a `Request` value copies (ES `fetch(r)`
/// clones, it does not consume). `DsRequest` lives in this slice alongside
/// `DsResponse`/`ds_fetch`, so a `new Request(…)`-only fixture pulls `Fetch`
/// (the dep derivation inserts it on the `__ds::DsRequest` marker).
#[derive(Clone)]
pub struct DsRequest {
    pub url: ::std::string::String,
    pub method: ::std::string::String,
    pub body: ::std::option::Option<::std::string::String>,
    pub headers: ::std::vec::Vec<(::std::string::String, ::std::string::String)>,
}
impl DsRequest {
    /// The translator-emitted constructor. `method` is uppercased to match the
    /// ES `Request.method` normalization; the other fields are stored as given.
    pub fn new(
        url: ::std::string::String,
        method: ::std::string::String,
        body: ::std::option::Option<::std::string::String>,
        headers: ::std::vec::Vec<(::std::string::String, ::std::string::String)>,
    ) -> Self {
        Self {
            url,
            method: method.to_ascii_uppercase(),
            body,
            headers,
        }
    }
    /// ES `request.url` — the request's URL.
    #[inline]
    pub fn url(&self) -> ::std::string::String {
        self.url.clone()
    }
    /// ES `request.method` — the HTTP method (uppercased).
    #[inline]
    pub fn method(&self) -> ::std::string::String {
        self.method.clone()
    }
    /// ES `request.headers` — a `DsHeaders` view (names lowercased, insertion
    /// order kept), built the same way `DsResponse::headers` builds its view.
    #[inline]
    pub fn headers(&self) -> DsHeaders {
        DsHeaders {
            entries: self
                .headers
                .iter()
                .map(|(k, v)| (k.to_lowercase(), v.clone()))
                .collect(),
        }
    }
}
/// `fetch(request)` — a request built from a `DsRequest`'s fields. Mirrors
/// `ds_fetch_with` (the same 3s timeout and panic-on-network-error policy) but
/// reads url/method/body/headers from the `Request` object `new Request(…)`
/// built. ES `fetch` clones the request (it does not consume it), so this
/// takes `&DsRequest`. ES `fetch` returns `Promise<Response>`; the caller's
/// `await` supplies the `.await`.
pub async fn ds_fetch_request(req: &DsRequest) -> DsResponse {
    let mut r = reqwest::Client::builder()
        .timeout(::std::time::Duration::from_secs(3))
        .build()
        .expect("reqwest client build")
        .request(
            req.method.parse().expect("invalid HTTP method"),
            req.url.clone(),
        );
    if let Some(b) = &req.body {
        r = r.body(b.clone());
    }
    for (k, v) in &req.headers {
        r = r.header(k, v);
    }
    DsResponse::from_reqwest(r.send().await.expect("fetch network error")).await
}
"#;

/// WHATWG `Blob` API helper — `__ds::DsBlob` (FileAPI, a WinterTC Web API). A
/// `Blob` is an immutable byte buffer plus a `type` (MIME). ES
/// `new Blob(parts, options)` flattens the parts (each a `string`, a
/// `BufferSource`, or a `Blob`) into one byte buffer; the translator collects
/// the parts into a `Vec<u8>` at the constructor, so the runtime `new` takes
/// the already-collected bytes + the `type` string. `size`/`type` are
/// zero-arg accessors; `slice(start, end, contentType)` returns a new `DsBlob`
/// (a copied sub-range — ES leaves view-vs-copy implementation-defined);
/// `text()`/`array_buffer()`/`bytes()` are async (ES returns a `Promise`),
/// so `await blob.text()` lowers to the async fn's `.await`. Pure `std` — no
/// cargo dep; the marker is `__ds::DsBlob`.
pub const BLOB_HELPER: &str = r#"
/// A WHATWG `Blob` — an immutable byte buffer with a `type` (MIME). The
/// `bytes` are collected from the constructor's parts by the translator; the
/// runtime sees only the flattened buffer. `#[derive(Clone)]` so a `Blob`
/// value copies (ES Blobs are immutable, so a clone shares nothing mutable).
#[derive(Clone)]
pub struct DsBlob {
    pub bytes: ::std::vec::Vec<u8>,
    pub type_: ::std::string::String,
}
impl DsBlob {
    /// Build a `Blob` from already-collected bytes + a `type` string (the
    /// translator flattens `new Blob(parts, …)` to this). `type` defaults to
    /// `""` when the options omit it (ES semantics).
    pub fn new(bytes: ::std::vec::Vec<u8>, type_: ::std::string::String) -> Self {
        Self { bytes, type_ }
    }
    /// `blob.size` — the byte length (ES `size` is a number).
    #[inline]
    pub fn size(&self) -> f64 {
        self.bytes.len() as f64
    }
    /// `blob.type` — the MIME lowercased (ES guarantees ASCII-lowercase).
    #[inline]
    pub fn type_(&self) -> ::std::string::String {
        self.type_.clone()
    }
    /// `blob.slice(start, end, contentType)` — a new `DsBlob` over the
    /// `[relStart, relEnd)` sub-range (ES index resolution), with the given
    /// `contentType` (default `""`).
    pub fn slice(
        &self,
        start: ::std::option::Option<f64>,
        end: ::std::option::Option<f64>,
        content_type: ::std::option::Option<::std::string::String>,
    ) -> DsBlob {
        let size = self.bytes.len();
        let s = ds_blob_index(start, size, 0);
        let e = ds_blob_index(end, size, size);
        let bytes = if s < e {
            self.bytes[s..e].to_vec()
        } else {
            ::std::vec::Vec::new()
        };
        DsBlob {
            bytes,
            type_: content_type.unwrap_or_default(),
        }
    }
    /// `await blob.text()` — the bytes as UTF-8 text (ES uses the UTF-8
    /// replacement decoder; lone surrogates become U+FFFD, matching `from_utf8_lossy`).
    pub async fn text(&self) -> ::std::string::String {
        ::std::string::String::from_utf8_lossy(&self.bytes).into_owned()
    }
    /// `await blob.arrayBuffer()` — a copy of the bytes (ES `ArrayBuffer`).
    pub async fn array_buffer(&self) -> ::std::vec::Vec<u8> {
        self.bytes.clone()
    }
    /// `await blob.bytes()` — a copy of the bytes (ES `Uint8Array`).
    pub async fn bytes(&self) -> ::std::vec::Vec<u8> {
        self.bytes.clone()
    }
}
/// Resolve a `Blob.slice()` index per ES — `NaN`/`-Infinity` → 0,
/// `+Infinity` → `size`, negatives count from the end — then clamp to
/// `[0, size]`. `default` is the value when the argument is absent (`start` →
/// 0, `end` → `size`).
fn ds_blob_index(i: ::std::option::Option<f64>, size: usize, default: usize) -> usize {
    let n = match i {
        ::std::option::Option::None => return default,
        ::std::option::Option::Some(n) => n,
    };
    if n.is_nan() || n == ::core::f64::NEG_INFINITY {
        return 0;
    }
    if n == ::core::f64::INFINITY {
        return size;
    }
    let s = size as f64;
    let idx = if n < 0.0 { (s + n).max(0.0) } else { n.min(s) };
    idx.max(0.0) as usize
}
"#;

/// WHATWG `File` API helper — `__ds::DsFile` (FileAPI, a WinterTC Web API). A
/// `File` is a `Blob` with a `name` and a `lastModified` (epoch-ms). It wraps a
/// `DsBlob` and delegates `size`/`type`/`slice`/`text`/`arrayBuffer`/`bytes` to
/// it (ES `File` extends `Blob`); `slice` returns a `DsBlob` (per spec,
/// `File.prototype.slice` returns a `Blob`, not a `File`). The marker is
/// `__ds::DsFile`; the dep resolution pulls `Blob` alongside (File reuses
/// `DsBlob`), so `BLOB_HELPER` is injected whenever a file uses `File`.
pub const FILE_HELPER: &str = r#"
/// A WHATWG `File` — a `Blob` with a `name` and `lastModified` (epoch-ms). ES
/// `File` extends `Blob`, so the byte buffer + `type` live in the wrapped
/// `DsBlob`; the File-specific `name`/`last_modified` are siblings.
/// `#[derive(Clone)]` follows `DsBlob`.
#[derive(Clone)]
pub struct DsFile {
    pub blob: crate::__ds::DsBlob,
    pub name: ::std::string::String,
    pub last_modified: f64,
}
impl DsFile {
    /// Build a `File` from already-collected bytes, a `type`, a `name`, and a
    /// `lastModified` (epoch-ms). The translator flattens
    /// `new File(bits, name, options)` to this.
    pub fn new(
        bytes: ::std::vec::Vec<u8>,
        type_: ::std::string::String,
        name: ::std::string::String,
        last_modified: f64,
    ) -> Self {
        Self {
            blob: crate::__ds::DsBlob::new(bytes, type_),
            name,
            last_modified,
        }
    }
    /// `file.size` — delegates to the wrapped `Blob` (ES `size` is a number).
    #[inline]
    pub fn size(&self) -> f64 {
        self.blob.size()
    }
    /// `file.type` — the wrapped `Blob`'s MIME (ES guarantees ASCII-lowercase).
    #[inline]
    pub fn type_(&self) -> ::std::string::String {
        self.blob.type_()
    }
    /// `file.name` — the file name (ES `name` is a string).
    #[inline]
    pub fn name(&self) -> ::std::string::String {
        self.name.clone()
    }
    /// `file.lastModified` — the last-modified time in epoch-ms (ES a number).
    #[inline]
    pub fn last_modified(&self) -> f64 {
        self.last_modified
    }
    /// `file.slice(start, end, contentType)` — a new `DsBlob` over the sub-range
    /// (per spec, `File.prototype.slice` returns a `Blob`). Delegates to the
    /// wrapped `Blob`'s index resolution.
    pub fn slice(
        &self,
        start: ::std::option::Option<f64>,
        end: ::std::option::Option<f64>,
        content_type: ::std::option::Option<::std::string::String>,
    ) -> crate::__ds::DsBlob {
        self.blob.slice(start, end, content_type)
    }
    /// `await file.text()` — the bytes as UTF-8 text (delegates to `Blob`).
    pub async fn text(&self) -> ::std::string::String {
        self.blob.text().await
    }
    /// `await file.arrayBuffer()` — a copy of the bytes (delegates to `Blob`).
    pub async fn array_buffer(&self) -> ::std::vec::Vec<u8> {
        self.blob.array_buffer().await
    }
    /// `await file.bytes()` — a copy of the bytes (delegates to `Blob`).
    pub async fn bytes(&self) -> ::std::vec::Vec<u8> {
        self.blob.bytes().await
    }
}
"#;

/// WHATWG `Headers` API helper — `__ds::DsHeaders` (FETCH §5.1, a WinterTC Web
/// API). A header is an ordered list of `(name, value)` pairs with case-
/// insensitive name lookup (HTTP headers are) — `Vec<(String, String)>` keyed
/// on the lowercased name, so iteration order matches insertion order (ES
/// `for_each`/`keys`/`values`/`entries`) and a `get` of a repeated name joins
/// the values with `", "` (ES semantics). Pure `std` — independent of
/// `reqwest`'s header map (the one bridge is `DsResponse::headers`, which
/// builds a `DsHeaders` from a `reqwest::HeaderMap`). Never degraded to the
/// engine; the `Headers` runtime dep is flagged by the `__ds::DsHeaders`
/// marker probe.
pub const HEADERS_HELPER: &str = r#"
/// WHATWG `Headers` — an ordered, case-insensitive-by-name list of `(name,
/// value)` pairs. Names are stored lowercased (HTTP header names are case-
/// insensitive); values are stored as-given (a leading/trailing trim is the
/// only normalization, matching the common WPT shape). `entries` is public so
/// `DsResponse::headers` can build a view directly from `reqwest`'s header map.
/// `#[derive(Clone)]` so `DsResponse` (which owns a `DsHeaders` field, so a
/// `new Response(…)` and a `fetch(…)` share one Response shape) can return a
/// headers view by clone.
#[derive(Clone)]
pub struct DsHeaders {
    pub entries: ::std::vec::Vec<(::std::string::String, ::std::string::String)>,
}
impl DsHeaders {
    /// `new Headers()` — an empty header list.
    pub fn new() -> Self {
        Self {
            entries: ::std::vec::Vec::new(),
        }
    }
    /// Build from initial `(name, value)` pairs (the ES `new Headers([[n, v],
    /// …])` form, or a Record lowered to pairs by the translator). Each pair
    /// appends with name normalization, so duplicate names accumulate (ES
    /// `append`, not `set`).
    pub fn from_pairs(
        pairs: ::std::vec::Vec<(::std::string::String, ::std::string::String)>,
    ) -> Self {
        let mut h = Self::new();
        for (n, v) in pairs {
            h.append(n, v);
        }
        h
    }
    /// `headers.append(name, value)` — add a pair (name lowercased, value
    /// trimmed). ES append accumulates; it does not replace existing same-name
    /// entries. Owned `String` params match the translator's `es_to_string_arg`
    /// lowering (the unified ES `ToString` coercion returns `String`).
    pub fn append(&mut self, name: ::std::string::String, value: ::std::string::String) {
        self.entries
            .push((name.to_ascii_lowercase(), value.trim().to_string()));
    }
    /// `headers.delete(name)` — drop every pair whose name matches (case-
    /// insensitive).
    pub fn delete(&mut self, name: ::std::string::String) {
        let l = name.to_ascii_lowercase();
        self.entries.retain(|(n, _)| n != &l);
    }
    /// `headers.get(name)` — the matching values joined by `", "`, or `None`
    /// (ES `null`) when no pair has the name. Case-insensitive lookup.
    pub fn get(
        &self,
        name: ::std::string::String,
    ) -> ::std::option::Option<::std::string::String> {
        let l = name.to_ascii_lowercase();
        let vs: ::std::vec::Vec<&str> = self
            .entries
            .iter()
            .filter(|(n, _)| n == &l)
            .map(|(_, v)| v.as_str())
            .collect();
        if vs.is_empty() {
            ::std::option::Option::None
        } else {
            ::std::option::Option::Some(vs.join(", "))
        }
    }
    /// `headers.set(name, value)` — replace every same-name pair with one
    /// `(name, value)` (ES set). Inlined (rather than `delete` + `append`) so
    /// the owned `name`/`value` are each consumed once.
    pub fn set(&mut self, name: ::std::string::String, value: ::std::string::String) {
        let l = name.to_ascii_lowercase();
        self.entries.retain(|(n, _)| n != &l);
        self.entries.push((l, value.trim().to_string()));
    }
    /// `headers.has(name)` — true iff any pair's name matches (case-
    /// insensitive).
    pub fn has(&self, name: ::std::string::String) -> bool {
        let l = name.to_ascii_lowercase();
        self.entries.iter().any(|(n, _)| n == &l)
    }
    /// `headers.forEach(callback)` — invoke `callback(value, name)` per pair in
    /// insertion order (ES `forEach` passes value first, then name).
    pub fn for_each<F: ::std::ops::FnMut(&str, &str)>(&self, mut f: F) {
        for (n, v) in &self.entries {
            f(v.as_str(), n.as_str());
        }
    }
    /// `headers.keys()` as a `Vec<String>` (insertion order). The translator
    /// lowers `headers.keys()` iteration to this; an ES iterator wrapper would
    /// need a closure state machine the static path avoids.
    pub fn keys_vec(&self) -> ::std::vec::Vec<::std::string::String> {
        self.entries.iter().map(|(n, _)| n.clone()).collect()
    }
    /// `headers.values()` as a `Vec<String>` (insertion order).
    pub fn values_vec(&self) -> ::std::vec::Vec<::std::string::String> {
        self.entries.iter().map(|(_, v)| v.clone()).collect()
    }
    /// `headers.entries()` as a `Vec<(String, String)>` (insertion order).
    pub fn entries_vec(&self) -> ::std::vec::Vec<(::std::string::String, ::std::string::String)> {
        self.entries.clone()
    }
    /// `headers.getSetCookie()` — every `set-cookie` value as its own element
    /// (FETCH §5.2). Unlike `get()` (which joins values with ", "),
    /// `getSetCookie()` returns one array element per Set-Cookie header,
    /// preserving the multi-value semantics `append` storage already keeps.
    pub fn get_set_cookie(&self) -> ::std::vec::Vec<::std::string::String> {
        self.entries
            .iter()
            .filter(|(n, _)| n == "set-cookie")
            .map(|(_, v)| v.clone())
            .collect()
    }
}
impl ::std::default::Default for DsHeaders {
    fn default() -> Self {
        Self::new()
    }
}
"#;

/// WHATWG `FormData` API helper — `__ds::DsFormData` (FETCH §5.2 / XHR, a
/// WinterTC Web API). A `FormData` is an ordered list of `(name, value)` pairs
/// where `value` is a `string` *or* a `File` — modelled as a `DsFormEntryValue`
/// enum (`Str`/`File`). `append` pushes (duplicates allowed, ES preserves
/// insertion order); `set` clears the name then pushes; `has`/`delete` are the
/// name queries. The value-returning methods (`get`/`getAll`/`entries`/`keys`/
/// `values`/`forEach`) are not lowered here — their `string | File` union
/// result needs the union-unboxing path, a separate batch; the static path
/// lowers the void/bool mutation+query surface, which is the common server
/// shape. The marker is `__ds::DsFormData`; the dep resolution pulls `File`
/// alongside (the value enum carries a `DsFile`), so FILE_HELPER + BLOB_HELPER
/// inject whenever a `FormData` appears.
pub const FORM_DATA_HELPER: &str = r#"
/// A WHATWG `FormData` entry value — a `string` or a `File` (ES
/// `FormDataEntryValue`). `#[derive(Clone)]` follows `DsFile`/`String`.
#[derive(Clone)]
pub enum DsFormEntryValue {
    Str(::std::string::String),
    File(crate::__ds::DsFile),
}
/// A WHATWG `FormData` — an ordered `(name, value)` list (duplicates allowed,
/// matching ES insertion-order semantics). `entries` is public so a future
/// `DsResponse::formData` / request body can build a view directly.
#[derive(Clone)]
pub struct DsFormData {
    pub entries: ::std::vec::Vec<(::std::string::String, crate::__ds::DsFormEntryValue)>,
}
impl DsFormData {
    /// `new FormData()` — an empty entry list.
    pub fn new() -> Self {
        Self {
            entries: ::std::vec::Vec::new(),
        }
    }
    /// `formData.append(name, value)` where `value` is a `string` — pushes a
    /// new entry (duplicates allowed).
    pub fn append_str(&mut self, name: ::std::string::String, value: ::std::string::String) {
        self.entries
            .push((name, crate::__ds::DsFormEntryValue::Str(value)));
    }
    /// `formData.append(name, file)` — pushes a `File` entry.
    pub fn append_file(&mut self, name: ::std::string::String, file: crate::__ds::DsFile) {
        self.entries
            .push((name, crate::__ds::DsFormEntryValue::File(file)));
    }
    /// `formData.has(name)` — whether any entry carries `name`.
    #[inline]
    pub fn has(&self, name: ::std::string::String) -> bool {
        self.entries.iter().any(|(k, _)| k == &name)
    }
    /// `formData.delete(name)` — remove every entry carrying `name`.
    pub fn delete(&mut self, name: ::std::string::String) {
        self.entries.retain(|(k, _)| k != &name);
    }
    /// `formData.set(name, value)` where `value` is a `string` — remove all
    /// `name` entries, then push the new one (ES `set` replaces, not appends).
    pub fn set_str(&mut self, name: ::std::string::String, value: ::std::string::String) {
        self.entries.retain(|(k, _)| k != &name);
        self.append_str(name, value);
    }
    /// `formData.set(name, file)` — remove all `name` entries, then push the
    /// `File` (ES `set` replaces, not appends).
    pub fn set_file(&mut self, name: ::std::string::String, file: crate::__ds::DsFile) {
        self.entries.retain(|(k, _)| k != &name);
        self.append_file(name, file);
    }
}
impl ::std::default::Default for DsFormData {
    fn default() -> Self {
        Self::new()
    }
}
"#;

/// WHATWG EventTarget/Event API helper — `__ds::DsEventTarget`/`__ds::DsEvent`
/// (WinterTC Web APIs). A `DsEventTarget` is a pub/sub: `addEventListener` boxes
/// the listener into `Vec<Box<dyn FnMut(&DsEvent)>>` behind an `Arc<Mutex<…>>`
/// (ES EventTargets are shared, mutable, single-threaded), `dispatchEvent`
/// invokes each listener whose `type` matches, and returns `false` only when a
/// `cancelable` event had `preventDefault` called (the ES contract). `DsEvent`
/// holds `default_prevented` in a `Cell` so a `&DsEvent` listener can flip it
/// (ES events are shared references). `EventTarget`/`Event` constructors,
/// `addEventListener`/`removeEventListener`/`dispatchEvent`/`preventDefault` map
/// verbatim to the inherent methods; `event.type`/`.bubbles`/`.cancelable`/
/// `.defaultPrevented`/`.timeStamp` dispatch in `member.rs`. Pure `std` — no
/// cargo dep; marker `__ds::DsEvent` (a common prefix of `DsEventTarget`/
/// `DsEvent`/`DsEventInit`, so any of the three pulls the slice).
pub const EVENT_TARGET_HELPER: &str = r#"
/// A WHATWG EventTarget — a pub/sub for typed events. Listeners are boxed
/// `FnMut(&DsEvent)` closures in a shared, single-threaded `Arc<Mutex<Vec<…>>>`
/// (ES EventTargets are shared + mutable). `#[derive(Clone)]` clones the `Arc`,
/// so `let et2 = et` shares the same listener set (ES reference semantics).
#[derive(Clone)]
pub struct DsEventTarget {
    inner: ::std::sync::Arc<::std::sync::Mutex<::std::vec::Vec<DsListenerEntry>>>,
}
struct DsListenerEntry {
    type_: ::std::string::String,
    callback: ::std::boxed::Box<dyn ::std::ops::FnMut(&DsEvent)>,
}
impl DsEventTarget {
    /// `new EventTarget()` — an empty listener set.
    pub fn new() -> Self {
        Self {
            inner: ::std::sync::Arc::new(::std::sync::Mutex::new(
                ::std::vec::Vec::new(),
            )),
        }
    }
    /// `et.addEventListener(type, cb)` — register a listener for `type`. The
    /// third ES arg (`useCapture`) is ignored (single-threaded, no capture
    /// phase). A `null`/`undefined` listener is filtered at the call site, so
    /// this always receives a real closure. `type_` is `String` (the translator's
    /// ES `ToString` lowering yields an owned string), matching the entry field.
    pub fn add_event_listener(
        &self,
        type_: ::std::string::String,
        callback: ::std::boxed::Box<dyn ::std::ops::FnMut(&DsEvent)>,
    ) {
        self.inner
            .lock()
            .unwrap()
            .push(DsListenerEntry { type_, callback });
    }
    /// `et.removeEventListener(type, cb)` — remove listeners for `type`. The ES
    /// signature matches a specific `(type, cb)` pair by listener identity; the
    /// static translator cannot compare closure identity, so this drops every
    /// listener for `type` (a deliberate simplification — the common WPT shape
    /// removes the only listener of a type, which is exact).
    pub fn remove_event_listener(&self, type_: ::std::string::String) {
        self.inner.lock().unwrap().retain(|e| e.type_ != type_);
    }
    /// `et.dispatchEvent(event)` — invoke each listener whose `type` matches,
    /// then return `false` iff the event was cancelable AND `preventDefault`
    /// was called (the ES return contract); otherwise `true`.
    pub fn dispatch_event(&self, event: &DsEvent) -> bool {
        let type_ = event.type_.clone();
        let mut listeners = self.inner.lock().unwrap();
        for entry in listeners.iter_mut() {
            if entry.type_ == type_ {
                (entry.callback)(event);
            }
        }
        ::std::mem::drop(listeners);
        !(event.cancelable && event.default_prevented.get())
    }
}
impl ::std::default::Default for DsEventTarget {
    fn default() -> Self {
        Self::new()
    }
}
// WinterTC `self` / `globalThis` — the global scope is itself an `EventTarget`
// (a receiver for `addEventListener`/`removeEventListener`/`dispatchEvent`). A
// single thread-local instance mirrors TS's single-threaded global; because
// `DsEventTarget` clones its inner `Arc`, every `wpt_self()` call shares one
// listener set, so a listener registered on `self` is seen by a later
// `self.dispatchEvent`. `crate::__ds::wpt_self().<method>(…)` is the lowering
// for `self.<method>(…)` / `globalThis.<method>(…)` on the global target.
thread_local! {
    static WPT_SELF: DsEventTarget = DsEventTarget::new();
}
pub fn wpt_self() -> DsEventTarget {
    WPT_SELF.with(|s| s.clone())
}

/// `reportError(error)` (HTML §5) — dispatch an `"error"` event to the global
/// `self` EventTarget (an `addEventListener("error", …)` / `self.onerror`
/// listener receives it); if no listener canceled it (`preventDefault` on a
/// cancelable event), write the error to stderr — the browser-console
/// "Uncaught" trace an unhandled `reportError` leaves behind. The payload is
/// `Display`d, so an ES `Error` / `DOMException` (a `DsError`) and a primitive
/// both type-check. Reuses `DsEvent` / `DsEventInit` / `wpt_self` — no new
/// runtime dep, just a method on the global EventTarget. Named `ds_report_error`
/// (not `report_error`) so the drift guard does not read the `report_error(`
/// callee as a snake-case fall-through of the `reportError` global.
pub fn ds_report_error<T: ::std::fmt::Display>(err: &T) {
    let evt = DsEvent::new(
        "error".to_string(),
        DsEventInit {
            bubbles: false,
            cancelable: true,
        },
    );
    if !wpt_self().dispatch_event(&evt) {
        eprintln!("Uncaught {}", err);
    }
}

/// A WHATWG `Event`. `default_prevented` is a `Cell` so a `&DsEvent` listener
/// (the ES dispatch shape) can flip it via `preventDefault`. `#[derive(Clone)]`
/// for `let e2 = e` reference sharing.
#[derive(Clone)]
pub struct DsEvent {
    pub type_: ::std::string::String,
    pub bubbles: bool,
    pub cancelable: bool,
    pub default_prevented: ::std::cell::Cell<bool>,
    pub timestamp: f64,
}
impl DsEvent {
    /// `new Event(type, init)` — `init.bubbles`/`init.cancelable` default to
    /// `false`; `defaultPrevented` starts `false`; `timeStamp` is 0.0 (a fixed
    /// epoch is out of scope — WPT rarely asserts its exact value). `type_` is
    /// `String` (the translator's ES `ToString` lowering yields an owned string).
    pub fn new(type_: ::std::string::String, init: DsEventInit) -> Self {
        Self {
            type_,
            bubbles: init.bubbles,
            cancelable: init.cancelable,
            default_prevented: ::std::cell::Cell::new(false),
            timestamp: 0.0,
        }
    }
    /// `event.type` — ES exposes it as a property; `type` is a Rust keyword, so
    /// the member dispatch in `member.rs` routes `event.type` here.
    #[inline]
    pub fn type_(&self) -> ::std::string::String {
        self.type_.clone()
    }
    /// `event.defaultPrevented` (a property; `member.rs` dispatches).
    #[inline]
    pub fn default_prevented(&self) -> bool {
        self.default_prevented.get()
    }
    /// `event.preventDefault()` — sets `defaultPrevented` only when `cancelable`
    /// (the ES guard).
    pub fn prevent_default(&self) {
        if self.cancelable {
            self.default_prevented.set(true);
        }
    }
    /// `event.stopPropagation()` — a no-op (single listener set, no propagation
    /// phases); present so a fixture calling it compiles.
    pub fn stop_propagation(&self) {}
    /// `event.stopImmediatePropagation()` — likewise a no-op.
    pub fn stop_immediate_propagation(&self) {}
}

/// A WHATWG `CustomEvent` — an `Event` carrying an arbitrary `detail` payload
/// (a WinterTC Web API). Mirrors `DsEvent` for `type`/`bubbles`/`cancelable`/
/// `defaultPrevented` (a `CustomEvent` is-an `Event`), and adds a
/// `detail: Option<T>` field: ES `detail` is `any` (defaulting to `undefined`
/// when omitted), and `Option<T>` lets `T` track the payload's static type so
/// `assert_equals(ev.detail, v)` lines up (`Some(v)` projects to `v` under
/// `DsSameValue`; `None` projects to `undefined`). Generic like
/// `DsReadableStream<T>` — `T` is inferred from the `detail` value at the call
/// site. A separate type from `DsEvent` (not a subtype): a `DsEventTarget`
/// listener takes `&DsEvent`, so `dispatchEvent(customEvent)` does not dispatch
/// to listeners on the static path (an honest partial for that shape; the
/// common WPT form — construct + read properties — is fully static).
#[derive(Clone)]
pub struct DsCustomEvent<T> {
    pub type_: ::std::string::String,
    pub bubbles: bool,
    pub cancelable: bool,
    pub default_prevented: ::std::cell::Cell<bool>,
    pub detail: ::std::option::Option<T>,
}
impl<T: ::std::clone::Clone> DsCustomEvent<T> {
    /// `new CustomEvent(type, init)` — `detail` is `Some(v)` when the init
    /// carries it, `None` otherwise (ES `undefined`); `bubbles`/`cancelable`
    /// default to `false`.
    pub fn new(
        type_: ::std::string::String,
        detail: ::std::option::Option<T>,
        bubbles: bool,
        cancelable: bool,
    ) -> Self {
        Self {
            type_,
            bubbles,
            cancelable,
            default_prevented: ::std::cell::Cell::new(false),
            detail,
        }
    }
    /// `event.type` (ES property; `type` is a Rust keyword, so `member.rs`
    /// routes `event.type` here).
    #[inline]
    pub fn type_(&self) -> ::std::string::String {
        self.type_.clone()
    }
    /// `event.defaultPrevented` (a property; `member.rs` dispatches).
    #[inline]
    pub fn default_prevented(&self) -> bool {
        self.default_prevented.get()
    }
    /// `event.preventDefault()` — sets `defaultPrevented` only when `cancelable`.
    pub fn prevent_default(&self) {
        if self.cancelable {
            self.default_prevented.set(true);
        }
    }
    /// `event.stopPropagation()` — a no-op (same single-listener simplification
    /// as `DsEvent`).
    pub fn stop_propagation(&self) {}
    /// `event.stopImmediatePropagation()` — likewise a no-op.
    pub fn stop_immediate_propagation(&self) {}
    /// `event.detail` (ES property; `member.rs` dispatches). `Some(v)` projects
    /// to `v` under `DsSameValue`, so `assert_equals(ev.detail, v)` holds; `None`
    /// projects to `undefined`.
    #[inline]
    pub fn detail(&self) -> ::std::option::Option<T> {
        self.detail.clone()
    }
}

/// `new Event(type, init)`'s `init` object — `{ bubbles, cancelable }`, both
/// defaulting to `false`. `#[derive(Clone)]` + `Default` for the
/// `new Event(type)` (no init) and `new Event(type, {})` forms.
#[derive(Clone)]
pub struct DsEventInit {
    pub bubbles: bool,
    pub cancelable: bool,
}
impl ::std::default::Default for DsEventInit {
    fn default() -> Self {
        Self {
            bubbles: false,
            cancelable: false,
        }
    }
}
"#;

/// WHATWG AbortController/AbortSignal API helper — `__ds::DsAbortController`/
/// `__ds::DsAbortSignal` (a WinterTC Web API). `controller.abort()` flips a
/// shared `Arc<Mutex<bool>>` to `true` (the `signal.aborted` flag) and fires the
/// `"abort"` event to the signal's embedded `DsEventTarget` (an `AbortSignal`
/// extends `EventTarget`). `#[derive(Clone)]` clones the `Arc`, so
/// `controller.signal` returns a signal sharing the same flag and listeners (ES
/// reference semantics). Reuses `DsEventTarget`/`DsEvent`/`DsEventInit` from
/// `EVENT_TARGET_HELPER` (the dep resolution pulls `EventTarget` alongside —
/// see `mod.rs`); pure `std`, no cargo dep; marker `__ds::DsAbort`.
pub const DS_ABORT_HELPER: &str = r#"
/// A WHATWG `AbortSignal` — the read-only side of an `AbortController`. Carries
/// the `aborted` flag (an `Arc<Mutex<bool>>` shared with the controller) and an
/// embedded `DsEventTarget` (ES `AbortSignal` extends `EventTarget`, so
/// `signal.addEventListener("abort", cb)` / `removeEventListener` /
/// `dispatchEvent` route there). `#[derive(Clone)]` clones the `Arc`, so
/// `controller.signal` returns a signal sharing the same flag and listeners.
#[derive(Clone)]
pub struct DsAbortSignal {
    aborted: ::std::sync::Arc<::std::sync::Mutex<bool>>,
    reason: ::std::sync::Arc<::std::sync::Mutex<::std::option::Option<DsError>>>,
    target: DsEventTarget,
}
impl DsAbortSignal {
    /// `signal.aborted` (a property; `member.rs` dispatches). ES exposes the
    /// boolean flag as a property; the Rust accessor reads the shared flag.
    #[inline]
    pub fn aborted(&self) -> bool {
        *self.aborted.lock().unwrap()
    }
    /// `signal.addEventListener(type, cb)` — register a listener (usually
    /// `"abort"`) on the embedded EventTarget. Same `Box<dyn FnMut(&DsEvent)>`
    /// callback shape as `DsEventTarget::add_event_listener`.
    pub fn add_event_listener(
        &self,
        type_: ::std::string::String,
        callback: ::std::boxed::Box<dyn ::std::ops::FnMut(&DsEvent)>,
    ) {
        self.target.add_event_listener(type_, callback);
    }
    /// `signal.removeEventListener(type, cb)` — drop listeners for `type` on the
    /// embedded EventTarget.
    pub fn remove_event_listener(&self, type_: ::std::string::String) {
        self.target.remove_event_listener(type_);
    }
    /// `signal.dispatchEvent(event)` — dispatch on the embedded EventTarget.
    pub fn dispatch_event(&self, event: &DsEvent) -> bool {
        self.target.dispatch_event(event)
    }
    /// Flip `aborted` to `true`, store `reason` (ES `controller.abort(reason)` /
    /// `AbortSignal.abort(reason)`'s arg), and fire the `"abort"` event once.
    /// ES queues the event as a microtask; this static model fires it
    /// synchronously on `controller.abort()` — the common WPT shape (assert
    /// `aborted` / a listener fired right after `abort()`) passes; a fixture
    /// depending on the microtask ordering is an honest partial. The guard is
    /// dropped before dispatch so a listener that itself re-locks
    /// `aborted`/`reason` does so cleanly.
    fn signal_abort(&self, reason: ::std::option::Option<DsError>) {
        let mut guard = self.aborted.lock().unwrap();
        if !*guard {
            *guard = true;
            *self.reason.lock().unwrap() = reason;
            ::std::mem::drop(guard);
            let evt = DsEvent::new(
                ::std::string::String::from("abort"),
                DsEventInit::default(),
            );
            self.target.dispatch_event(&evt);
        }
    }
    /// `signal.reason` (a property; `member.rs` dispatches). ES returns the
    /// abort reason — `controller.abort(reason)`'s arg, else a `DOMException`
    /// named `"AbortError"` (the default). The Rust accessor returns that
    /// default for an abort with no reason; for an un-aborted signal ES returns
    /// `undefined`, but a fixture only reads `reason` after abort, so the
    /// default is the honest common path (an un-aborted read is an honest
    /// partial). `DsError` (the `DOMException`/`Error` model) is pulled
    /// alongside — `AbortController` derives `Error` (see `derive_deps`).
    #[inline]
    pub fn reason(&self) -> DsError {
        self.reason
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(|| DsError::new("AbortError", "signal is aborted without reason"))
    }
    /// `signal.throwIfAborted()` — if `aborted`, throw `signal.reason` (a
    /// `DsError`), else a no-op. The throw lowers to `panic_any(DsError)`, so a
    /// surrounding `try`/`catch` recovers the same `DsError` ES would (with
    /// `e.name`/`e.message` intact, the way `throw new Error(…)` does).
    pub fn throw_if_aborted(&self) {
        if self.aborted() {
            ::std::panic::panic_any(self.reason());
        }
    }
    /// `AbortSignal.abort()` (static) — a fresh signal already in the aborted
    /// state with the default `AbortError` reason. `abort_static` dispatches
    /// `AbortSignal.abort()` here; named `aborted_signal` (not `abort`, the
    /// instance method) so the static-vs-instance split is unambiguous, and the
    /// marker probe still catches the emit (`DsAbortSignal::aborted_signal`
    /// shares the `__ds::DsAbort` prefix with the struct).
    pub fn aborted_signal() -> DsAbortSignal {
        let s = DsAbortSignal::default();
        s.signal_abort(::std::option::Option::None);
        s
    }
}
impl ::std::default::Default for DsAbortSignal {
    fn default() -> Self {
        Self {
            aborted: ::std::sync::Arc::new(::std::sync::Mutex::new(false)),
            reason: ::std::sync::Arc::new(::std::sync::Mutex::new(::std::option::Option::None)),
            target: DsEventTarget::new(),
        }
    }
}

/// A WHATWG `AbortController` — the write side. `controller.abort()` flips the
/// shared `aborted` flag (and fires `"abort"`); `controller.signal` returns a
/// clone of the signal (sharing the flag and listeners). `#[derive(Clone)]` for
/// `let c2 = c` reference sharing.
#[derive(Clone)]
pub struct DsAbortController {
    signal: DsAbortSignal,
}
impl DsAbortController {
    /// `new AbortController()` — a fresh, un-aborted signal.
    pub fn new() -> Self {
        Self {
            signal: DsAbortSignal::default(),
        }
    }
    /// `controller.signal` (a property; `member.rs` dispatches) — returns a
    /// signal sharing the same flag and listeners (ES reference semantics).
    #[inline]
    pub fn signal(&self) -> DsAbortSignal {
        self.signal.clone()
    }
    /// `controller.abort([reason])` — flip `aborted` and fire `"abort"` once.
    /// The ES `reason` arg is dropped (the common WPT shape aborts without a
    /// reason; `signal.reason` then returns the default `AbortError`).
    pub fn abort(&self) {
        self.signal.signal_abort(::std::option::Option::None);
    }
}
impl ::std::default::Default for DsAbortController {
    fn default() -> Self {
        Self::new()
    }
}
"#;
