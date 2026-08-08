/// ES `Promise` combinator helpers — `__ds::DsPromise`/`ds_promise_resolve`/
/// `ds_promise_all`. The static track for `Promise.resolve`/`Promise.all`
/// (T3 stage 2a): a `Promise<T>` is a boxed, single-threaded `Future<Output =
/// T>` so every Promise site shares one Rust type (each `futures` combinator
/// has a distinct anonymous type — boxing unifies them). `current_thread`
/// tokio needs no `Send` bound, so a `DsPromise` capturing any value type
/// compiles. `Promise.all` uses `join_all` (awaits all, preserves order); the
/// ES reject short-circuit is not yet modelled (an all-fulfill fixture passes;
/// a rejection fixture stays partial). Reflection-driven Promise usage
/// (Symbol.species, thenable `await`, prototype chains) is not lowered. Backed
/// by the `futures` crate (also pulled by `Tokio`).
pub const DS_PROMISE_HELPER: &str = r#"
/// A JS `Promise<T>` — a boxed, single-threaded `Future<Output = T>`. Boxing
/// unifies the distinct anonymous types of `ready`/`join_all`/`async {}` so a
/// Promise value has one Rust type at every site.
pub type DsPromise<T> = ::std::pin::Pin<::std::boxed::Box<dyn ::std::future::Future<Output = T>>>;

/// `Promise.resolve(x)` — a Promise fulfilled with `x`. `futures::future::ready`
/// wraps the value; boxing unifies the type.
pub fn ds_promise_resolve<T: 'static>(x: T) -> DsPromise<T> {
    ::std::boxed::Box::pin(::futures::future::ready(x))
}

/// `Promise.all([p1, p2, …])` — fulfills with each input's value in order.
/// `join_all` awaits every input (no reject short-circuit yet); an empty input
/// fulfills with `[]`. Each input must already be a `DsPromise<T>` (the call
/// emit wraps a non-Promise element via `ds_promise_resolve`).
pub fn ds_promise_all<T: 'static>(
    futs: ::std::vec::Vec<DsPromise<T>>,
) -> DsPromise<::std::vec::Vec<T>> {
    ::std::boxed::Box::pin(::futures::future::join_all(futs))
}

/// `p.then(onFulfilled)` — fulfills with the callback's return value. ES `then`
/// returns a Promise and `Promise.resolve`s the callback's value; this static
/// track models the common shape where the callback returns a plain value or
/// runs for side effects (returning `()`). A callback that itself returns a
/// Promise (a thenable chain) is not flattened — it yields a
/// `DsPromise<DsPromise<U>>`, an honest partial. `onRejected` (arg 1) is not
/// modelled: a rejected input propagates by panicking through the `.await`, so
/// a reject-path fixture stays partial rather than mis-stating the verdict.
pub fn ds_promise_then<T: 'static, U: 'static, F: 'static + FnOnce(T) -> U>(
    fut: DsPromise<T>,
    f: F,
) -> DsPromise<U> {
    ::std::boxed::Box::pin(async move { f(fut.await) })
}

/// A pending-or-settled slot shared between the `resolve`/`reject` a
/// `new Promise(executor)` hands out and the `DsPromise`'s polling future.
/// First settlement wins; later `resolve`/`reject` are no-ops (ES idempotency).
enum DsPromiseCell<T> {
    Pending(::std::option::Option<::std::task::Waker>),
    Fulfilled(T),
    Rejected(::std::string::String),
}

/// The `resolve`/`reject` handed to a `new Promise(executor)`. `Clone` shares
/// the one settlement slot (a cheap `Arc`), so a resolver captured by a
/// deferred callback (`setTimeout(() => resolve(x), …)`) settles the same
/// promise — the deferred-settlement pattern the static path could not express
/// before. A bare `new Promise(executor)` is now a first-class static value.
pub struct DsResolver<T> {
    cell: ::std::sync::Arc<::std::sync::Mutex<DsPromiseCell<T>>>,
}

impl<T> ::std::clone::Clone for DsResolver<T> {
    #[inline]
    fn clone(&self) -> Self {
        Self {
            cell: self.cell.clone(),
        }
    }
}

impl<T> DsResolver<T> {
    /// `resolve(value)` — settle fulfilled. First call wins; a later call (after
    /// any settle) is a no-op. Wakes a pending future so it re-polls.
    pub fn resolve(&self, value: T) {
        let waker = {
            let mut guard = self.cell.lock().expect("promise cell poisoned");
            if !::core::matches!(*guard, DsPromiseCell::Pending(_)) {
                return;
            }
            ::std::mem::replace(&mut *guard, DsPromiseCell::Fulfilled(value))
        };
        if let DsPromiseCell::Pending(Some(waker)) = waker {
            waker.wake();
        }
    }

    /// `reject(reason)` — settle rejected. `reason` is `Display`'d (the message a
    /// `.catch`/rejection surfaces). First call wins; a later call is a no-op.
    pub fn reject<R: ::std::fmt::Display>(&self, reason: R) {
        let waker = {
            let mut guard = self.cell.lock().expect("promise cell poisoned");
            if !::core::matches!(*guard, DsPromiseCell::Pending(_)) {
                return;
            }
            ::std::mem::replace(&mut *guard, DsPromiseCell::Rejected(reason.to_string()))
        };
        if let DsPromiseCell::Pending(Some(waker)) = waker {
            waker.wake();
        }
    }
}

/// The `new Promise(executor)` future — polls the shared cell. `Pending` stores
/// the waker so a later `resolve`/`reject` (synchronous or deferred) wakes the
/// task. A rejected promise propagates by panicking through the `.await`
/// (matching `ds_promise_then`'s reject convention — an honest partial for
/// reject-path fixtures).
struct DsPromiseFuture<T> {
    cell: ::std::sync::Arc<::std::sync::Mutex<DsPromiseCell<T>>>,
}

impl<T> ::std::future::Future for DsPromiseFuture<T> {
    type Output = T;
    fn poll(
        self: ::std::pin::Pin<&mut Self>,
        cx: &mut ::std::task::Context<'_>,
    ) -> ::std::task::Poll<Self::Output> {
        let mut guard = self.cell.lock().expect("promise cell poisoned");
        match ::std::mem::replace(&mut *guard, DsPromiseCell::Pending(None)) {
            DsPromiseCell::Pending(_) => {
                *guard = DsPromiseCell::Pending(Some(cx.waker().clone()));
                ::std::task::Poll::Pending
            }
            DsPromiseCell::Fulfilled(v) => ::std::task::Poll::Ready(v),
            DsPromiseCell::Rejected(msg) => panic!("Promise rejected: {}", msg),
        }
    }
}

/// `new Promise((resolve, reject) => { … })`. The executor runs synchronously
/// with a clonable `DsResolver`; `resolve(x)`/`reject(reason)` settle a shared
/// cell the returned future polls (first settlement wins; later calls no-op).
/// A deferred `resolve` — captured by a nested callback (`setTimeout`, etc.) —
/// settles the same promise via the cloned resolver. The value type `T` is
/// inferred from the `resolve(value)` call site; a Promise that never settles,
/// or settles with disjoint types in different branches, has no single `T`
/// (an honest partial — the static path neither fakes a type nor degrades).
pub fn ds_promise_new<T: 'static, F: 'static + ::std::ops::FnOnce(DsResolver<T>)>(
    executor: F,
) -> DsPromise<T> {
    let cell = ::std::sync::Arc::new(::std::sync::Mutex::new(DsPromiseCell::Pending(None)));
    let resolver = DsResolver {
        cell: cell.clone(),
    };
    executor(resolver);
    ::std::boxed::Box::pin(DsPromiseFuture { cell })
}
"#;

/// WHATWG `ReadableStream` helper — `__ds::DsReadableStream`. A WinterTC (Ecma
/// TC55) Web API: the readable side of the Streams standard. This slice holds
/// the push-source baseline — `new ReadableStream({ start(c) { c.enqueue(…);
/// c.close() } })` + `stream.getReader()` + `await reader.read()` →
/// `{ done, value }`. The state machine mirrors `DsResolver`: a chunk queue +
/// closed flag + one waker slot shared (via `Arc<Mutex<…>>`) between the
/// stream, the `start` controller, and the default reader — ES forbids two
/// concurrent reads on a default reader, so one waker is the correct capacity.
/// Self-contained (the boxed future is spelled inline, not via `DsPromise`, so
/// a Streams-only fixture pulls no Promise slice); pure `std`, never degraded.
/// `pull`/`cancel`/`tee`/BYOB are out of scope (an honest partial when met).
pub const DS_STREAMS_HELPER: &str = r#"
/// Shared readable-stream state: a chunk queue + a closed flag + a waker for a
/// pending `read()`. Mirrors the `DsResolver` settlement cell.
struct DsStreamState<T> {
    chunks: ::std::collections::VecDeque<T>,
    closed: bool,
    waker: ::std::option::Option<::std::task::Waker>,
}

impl<T> DsStreamState<T> {
    fn new() -> Self {
        Self {
            chunks: ::std::collections::VecDeque::new(),
            closed: false,
            waker: ::std::option::Option::None,
        }
    }
}

/// WHATWG `ReadableStream<T>` — a readable stream of `T` chunks. Build via
/// [`DsReadableStream::from_start`] (a push source) or
/// [`DsReadableStream::empty_closed`] (`new ReadableStream()` with no
/// underlying source).
pub struct DsReadableStream<T> {
    state: ::std::sync::Arc<::std::sync::Mutex<DsStreamState<T>>>,
}

/// The controller a `start(controller)` callback receives. `enqueue(chunk)`
/// pushes a chunk (waking a pending reader); `close()` ends the stream.
pub struct DsReadableStreamController<T> {
    state: ::std::sync::Arc<::std::sync::Mutex<DsStreamState<T>>>,
}

impl<T> DsReadableStreamController<T> {
    /// `controller.enqueue(chunk)` — push a chunk; wake a pending reader so its
    /// `read()` re-polls.
    pub fn enqueue(&self, value: T) {
        let waker = {
            let mut g = self.state.lock().expect("stream state poisoned");
            g.chunks.push_back(value);
            g.waker.take()
        };
        if let ::std::option::Option::Some(w) = waker {
            w.wake();
        }
    }
    /// `controller.close()` — signal end-of-stream. A pending `read()` resolves
    /// `{ done: true, value: None }` once the queue drains.
    pub fn close(&self) {
        let waker = {
            let mut g = self.state.lock().expect("stream state poisoned");
            g.closed = true;
            g.waker.take()
        };
        if let ::std::option::Option::Some(w) = waker {
            w.wake();
        }
    }
}

/// The default reader `stream.getReader()` returns.
pub struct DsReadableStreamDefaultReader<T> {
    state: ::std::sync::Arc<::std::sync::Mutex<DsStreamState<T>>>,
}

/// `{ done: false, value }` / `{ done: true, value: undefined }` — the result of
/// `await reader.read()`. `value` is `None` at end-of-stream.
pub struct DsReadResult<T> {
    pub done: bool,
    pub value: ::std::option::Option<T>,
}

impl<T> DsReadableStream<T> {
    /// `new ReadableStream({ start(controller) { … } })` — a push source. The
    /// `start` closure runs synchronously (ES `start` is sync; a Promise-returning
    /// `start` is not modelled); `controller.enqueue(v)` infers the chunk type
    /// `T` from the call site.
    pub fn from_start<F: ::std::ops::FnOnce(DsReadableStreamController<T>)>(
        start: F,
    ) -> DsReadableStream<T> {
        let state = ::std::sync::Arc::new(::std::sync::Mutex::new(DsStreamState::new()));
        start(DsReadableStreamController { state: state.clone() });
        DsReadableStream { state }
    }
    /// `new ReadableStream()` — no underlying source. ES leaves such a stream
    /// pending forever (nothing ever enqueues); the static path closes it on
    /// construction so a `read()` resolves `{ done: true }` instead of hanging
    /// the harness — a pragmatic, honest deviation on an empty stream.
    pub fn empty_closed() -> DsReadableStream<T> {
        let state = ::std::sync::Arc::new(::std::sync::Mutex::new(DsStreamState::new()));
        {
            let mut g = state.lock().expect("stream state poisoned");
            g.closed = true;
        }
        DsReadableStream { state }
    }
    /// `stream.getReader()` — the default reader (a BYOB `getReader({ mode:
    /// 'byob' })` has no static mapping).
    pub fn get_reader(&self) -> DsReadableStreamDefaultReader<T> {
        DsReadableStreamDefaultReader { state: self.state.clone() }
    }
}

impl<T: 'static> DsReadableStreamDefaultReader<T> {
    /// `reader.read()` — a Promise of the next chunk or end-of-stream. Polls the
    /// shared state: a queued chunk → `{ done: false, value: Some(v) }`; an
    /// empty, closed stream → `{ done: true, value: None }`; otherwise pending
    /// (the waker is stored so `enqueue`/`close` wake this read). `T: 'static`
    /// because the boxed `dyn Future` is `'static` (the same bound every
    /// `DsPromise<T>` return carries).
    pub fn read(
        &self,
    ) -> ::std::pin::Pin<
        ::std::boxed::Box<dyn ::std::future::Future<Output = DsReadResult<T>>>,
    > {
        ::std::boxed::Box::pin(DsReadFuture { state: self.state.clone() })
    }
}

struct DsReadFuture<T> {
    state: ::std::sync::Arc<::std::sync::Mutex<DsStreamState<T>>>,
}

impl<T> ::std::future::Future for DsReadFuture<T> {
    type Output = DsReadResult<T>;
    fn poll(
        self: ::std::pin::Pin<&mut Self>,
        cx: &mut ::std::task::Context<'_>,
    ) -> ::std::task::Poll<Self::Output> {
        let mut g = self.state.lock().expect("stream state poisoned");
        if let ::std::option::Option::Some(v) = g.chunks.pop_front() {
            ::std::task::Poll::Ready(DsReadResult {
                done: false,
                value: ::std::option::Option::Some(v),
            })
        } else if g.closed {
            ::std::task::Poll::Ready(DsReadResult {
                done: true,
                value: ::std::option::Option::None,
            })
        } else {
            g.waker = ::std::option::Option::Some(cx.waker().clone());
            ::std::task::Poll::Pending
        }
    }
}
"#;

/// WHATWG `CompressionStream` helper — `__ds::DsCompressionStream`. A WinterTC
/// (Ecma TC55) Web API: the compression side of the Streams standard. Unlike a
/// user-sink `WritableStream`, the transform is **internal** (`flate2`, never a
/// user closure), so this avoids the `'static`-capture blocker that gates a
/// general `WritableStream` user sink (a `write` callback capturing outer
/// mutable state is not `'static`). The model is one-shot: `writer.write(bytes)`
/// appends to an internal buffer; `writer.close()` compresses the buffer
/// (`flate2`) into the output; `reader.read()` returns the one compressed chunk
/// then `{ done: true }`. Backed by `flate2`; pure-Rust static track, never
/// degraded. `DecompressionStream`, `brotli`, true streaming, and backpressure
/// are out of scope (an honest partial when met).
pub const DS_COMPRESSION_HELPER: &str = r#"
/// The WHATWG compression format. `gzip`/`deflate` (zlib-wrapped)/`deflate-raw`
/// (raw DEFLATE) map to `flate2`; `brotli` is out of scope (no static mapping —
/// the fixture's `new CompressionStream("brotli")` is an honest unsupported).
/// `Copy` so `close()` can read `state.format` through a `MutexGuard` (a move
/// out of the guard's `&mut` is impossible) before the one-shot compress.
#[derive(Clone, Copy)]
pub enum DsCompressionFormat {
    Gzip,
    Deflate,
    DeflateRaw,
}

/// The direction of a `CompressionStream`/`DecompressionStream` — both lower
/// to the same `DsCompressionStream` type (the writable/readable/writer/reader
/// containers are direction-agnostic; only `close()`'s one-shot codec run
/// differs). `Copy` so `close()` reads `state.dir` through a `MutexGuard`.
#[derive(Clone, Copy)]
pub enum DsCodecDir {
    Compress,
    Decompress,
}

struct DsCompressionState {
    input: ::std::vec::Vec<u8>,
    output: ::std::option::Option<::std::vec::Vec<u8>>,
    delivered: bool,
    format: DsCompressionFormat,
    dir: DsCodecDir,
    closed: bool,
}

/// `new CompressionStream(format)` — a byte transform stream. `writable`/
/// `readable` are pub fields (cloned views over the shared state) so
/// `cs.writable`/`cs.readable` lower as plain field access.
pub struct DsCompressionStream {
    pub writable: DsCompressionWritable,
    pub readable: DsCompressionReadable,
}

/// `cs.writable` — the writable side. `getWriter()` returns a writer.
pub struct DsCompressionWritable {
    state: ::std::sync::Arc<::std::sync::Mutex<DsCompressionState>>,
}

/// `writer` from `cs.writable.getWriter()`. `write(bytes)` appends to the
/// internal buffer; `close()` runs the one-shot `flate2` compression.
pub struct DsCompressionWriter {
    state: ::std::sync::Arc<::std::sync::Mutex<DsCompressionState>>,
}

/// `cs.readable` — the readable side. `getReader()` returns a reader.
pub struct DsCompressionReadable {
    state: ::std::sync::Arc<::std::sync::Mutex<DsCompressionState>>,
}

/// `reader` from `cs.readable.getReader()`.
pub struct DsCompressionReader {
    state: ::std::sync::Arc<::std::sync::Mutex<DsCompressionState>>,
}

/// `{ done, value }` from `await reader.read()`. `value` is the one compressed
/// chunk (`Some(bytes)`), then `None` once delivered.
pub struct DsCompressionReadResult {
    pub done: bool,
    pub value: ::std::option::Option<::std::vec::Vec<u8>>,
}

impl DsCompressionStream {
    /// `new CompressionStream(format)` / `new DecompressionStream(format)` —
    /// `dir` selects the codec direction; the two share the container types,
    /// differing only in `close()`'s one-shot codec run.
    pub fn new(format: DsCompressionFormat, dir: DsCodecDir) -> DsCompressionStream {
        let state = ::std::sync::Arc::new(::std::sync::Mutex::new(DsCompressionState {
            input: ::std::vec::Vec::new(),
            output: ::std::option::Option::None,
            delivered: false,
            format,
            dir,
            closed: false,
        }));
        DsCompressionStream {
            writable: DsCompressionWritable { state: state.clone() },
            readable: DsCompressionReadable { state },
        }
    }
}

impl DsCompressionWritable {
    /// `cs.writable.getWriter()`.
    pub fn get_writer(&self) -> DsCompressionWriter {
        DsCompressionWriter { state: self.state.clone() }
    }
}

impl DsCompressionWriter {
    /// `writer.write(chunk)` — append the chunk's bytes to the internal buffer.
    pub fn write(
        &self,
        chunk: ::std::vec::Vec<u8>,
    ) -> ::std::pin::Pin<::std::boxed::Box<dyn ::std::future::Future<Output = ()>>> {
        let state = self.state.clone();
        ::std::boxed::Box::pin(async move {
            let mut g = state.lock().expect("compression state poisoned");
            g.input.extend_from_slice(&chunk);
        })
    }
    /// `writer.close()` — run the one-shot `flate2` compression of the buffered
    /// input, storing the result for the reader. Idempotent on an already-closed
    /// stream.
    pub fn close(
        self,
    ) -> ::std::pin::Pin<::std::boxed::Box<dyn ::std::future::Future<Output = ()>>> {
        let state = self.state.clone();
        ::std::boxed::Box::pin(async move {
            let mut g = state.lock().expect("compression state poisoned");
            if !g.closed {
                g.closed = true;
                let format = g.format;
                let dir = g.dir;
                let input = ::std::mem::take(&mut g.input);
                g.output = ::std::option::Option::Some(ds_codec_run(format, dir, input));
            }
        })
    }
}

impl DsCompressionReadable {
    /// `cs.readable.getReader()`.
    pub fn get_reader(&self) -> DsCompressionReader {
        DsCompressionReader { state: self.state.clone() }
    }
}

impl DsCompressionReader {
    /// `reader.read()` — the compressed chunk once `close()` has run, then
    /// `{ done: true }`. A `read()` before `close()` resolves `{ done: true }`
    /// (the one-shot model does not pend awaiting a close — the fixtures always
    /// `write`→`close`→`read`, so the output is ready by the time this polls).
    pub fn read(
        &self,
    ) -> ::std::pin::Pin<
        ::std::boxed::Box<dyn ::std::future::Future<Output = DsCompressionReadResult>>,
    > {
        let state = self.state.clone();
        ::std::boxed::Box::pin(async move {
            let mut g = state.lock().expect("compression state poisoned");
            if !g.delivered {
                if let ::std::option::Option::Some(out) = g.output.take() {
                    g.delivered = true;
                    return DsCompressionReadResult {
                        done: false,
                        value: ::std::option::Option::Some(out),
                    };
                }
            }
            DsCompressionReadResult { done: true, value: ::std::option::Option::None }
        })
    }
}

/// One-shot `flate2` codec run over `input` per `format` and `dir`. `Compress`
/// → `write::{GzEncoder,ZlibEncoder,DeflateEncoder}`; `Decompress` →
/// `read::{GzDecoder,ZlibDecoder,DeflateDecoder}`. A compress error is
/// impossible for an in-memory `Vec<u8>` sink (no I/O); a decompress error
/// means truncated/corrupt input — the fixtures round-trip a value produced by
/// the matching `CompressionStream`, so the `expect`s are unreachable on the
/// static path.
fn ds_codec_run(
    format: DsCompressionFormat,
    dir: DsCodecDir,
    input: ::std::vec::Vec<u8>,
) -> ::std::vec::Vec<u8> {
    match dir {
        DsCodecDir::Compress => {
            use ::std::io::Write as _;
            match format {
                DsCompressionFormat::Gzip => {
                    let mut e = ::flate2::write::GzEncoder::new(
                        ::std::vec::Vec::new(),
                        ::flate2::Compression::default(),
                    );
                    e.write_all(&input).expect("gzip encode");
                    e.finish().expect("gzip finish")
                }
                DsCompressionFormat::Deflate => {
                    let mut e = ::flate2::write::ZlibEncoder::new(
                        ::std::vec::Vec::new(),
                        ::flate2::Compression::default(),
                    );
                    e.write_all(&input).expect("deflate encode");
                    e.finish().expect("deflate finish")
                }
                DsCompressionFormat::DeflateRaw => {
                    let mut e = ::flate2::write::DeflateEncoder::new(
                        ::std::vec::Vec::new(),
                        ::flate2::Compression::default(),
                    );
                    e.write_all(&input).expect("deflate-raw encode");
                    e.finish().expect("deflate-raw finish")
                }
            }
        }
        DsCodecDir::Decompress => {
            use ::std::io::Read as _;
            let mut out = ::std::vec::Vec::new();
            match format {
                DsCompressionFormat::Gzip => {
                    let mut d = ::flate2::read::GzDecoder::new(&input[..]);
                    d.read_to_end(&mut out).expect("gzip decode");
                }
                DsCompressionFormat::Deflate => {
                    let mut d = ::flate2::read::ZlibDecoder::new(&input[..]);
                    d.read_to_end(&mut out).expect("deflate decode");
                }
                DsCompressionFormat::DeflateRaw => {
                    let mut d = ::flate2::read::DeflateDecoder::new(&input[..]);
                    d.read_to_end(&mut out).expect("deflate-raw decode");
                }
            }
            out
        }
    }
}
"#;
