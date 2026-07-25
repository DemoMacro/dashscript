//! oxc AST → idiomatic Rust source, emitted through `syn` + `prettyplease`.
//!
//! Translation is one file per AST category — `declarations`, `functions`,
//! `types`, `expressions`, `bindings` — so each oxc node maps to a `syn` node
//! one-to-one. The `syn` tree is the project's hub: the translator builds it
//! (oxc → syn), `prettyplease` prints it, and the future `bindgen` parses
//! Rust crates into the same `syn` tree (syn → .d.ts) — one AST, two
//! directions. Parsing reuses `oxc_parser`; DashScript never parses itself.

mod analysis;
pub mod bindings;
mod builtins;
mod check;
mod class;
pub mod context;
pub mod declarations;
pub mod dts;
pub mod expressions;
mod flavor;
pub mod functions;
mod globals;
pub mod imports;
pub mod name_table;
pub mod registry;
pub mod semantic;
pub mod types;

use std::collections::BTreeSet;

use oxc_allocator::Allocator;
use oxc_codegen::Codegen;
use oxc_diagnostics::OxcDiagnostic;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::SourceType;

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
    /// via `__ds_engine` (the `rquickjs` crate). A gated compat fallback — the
    /// body is never lowered, so it carries no text marker.
    Engine,
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
}

impl RuntimeDep {
    /// All variants in declaration order — the order helper slices and cargo
    /// deps are emitted, so output stays deterministic.
    const ALL: [RuntimeDep; 7] = [
        RuntimeDep::RyuJs,
        RuntimeDep::SerdeJson,
        RuntimeDep::Engine,
        RuntimeDep::ArrayHelper,
        RuntimeDep::Regress,
        RuntimeDep::Temporal,
        RuntimeDep::Worker,
    ];

    /// The emitted-text marker that signals this dep was pulled in. `None` for
    /// `Engine` — it is set explicitly when the translator detects a reflection
    /// construct (the body is never lowered, so there is no text to scan).
    fn marker(self) -> Option<&'static str> {
        match self {
            RuntimeDep::RyuJs => Some("__ds::number_to_string"),
            RuntimeDep::SerdeJson => Some("serde_json::"),
            RuntimeDep::ArrayHelper => Some("__ds::array_set"),
            RuntimeDep::Regress => Some("__ds::regex"),
            RuntimeDep::Temporal => Some("temporal_rs::"),
            RuntimeDep::Worker => Some("__ds::Worker"),
            RuntimeDep::Engine => None,
        }
    }

    /// The cargo dependencies to append, if this dep needs any crate(s). A slice
    /// because one runtime dep can pull more than one crate (`Worker` needs both
    /// `serde` — the trait bounds `Serialize`/`DeserializeOwned` — and
    /// `serde_json` for the actual marshal). `append_dep` is idempotent, so an
    /// overlap with another dep (or a user-declared `cargo:serde_json`) is a
    /// no-op, not a duplicate. `None` for `ArrayHelper` (pure `std`).
    fn cargo(self) -> Option<&'static [(&'static str, &'static str)]> {
        match self {
            // The crates.io package is `ryu-js` (hyphen); Rust exposes it as
            // `ryu_js` (underscore) in `use`, so the Cargo.toml key uses the
            // package name.
            RuntimeDep::RyuJs => Some(&[("ryu-js", "\"1.0\"")]),
            RuntimeDep::SerdeJson => Some(&[("serde_json", "\"1\"")]),
            // `rquickjs` bundles QuickJS-NG C sources (compiled via `cc`), so
            // it is only emitted for programs that opt into the engine path.
            RuntimeDep::Engine => Some(&[("rquickjs", "\"0.12\"")]),
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
        }
    }

    /// The `__ds` helper source slice this dep contributes, if any.
    fn helper(self) -> Option<&'static str> {
        match self {
            RuntimeDep::RyuJs => Some(RYUJS_HELPERS),
            RuntimeDep::ArrayHelper => Some(ARRAY_HELPER),
            RuntimeDep::Regress => Some(REGRESS_HELPERS),
            RuntimeDep::SerdeJson | RuntimeDep::Engine | RuntimeDep::Temporal => None,
            RuntimeDep::Worker => Some(WORKER_HELPER),
        }
    }
}

/// Runtime dependencies a translated file pulls in. Collected during
/// translation so `ds build` only links what the source actually uses: a file
/// that never formats a number to an ES string pulls in no `ryu_js`. Adding a
/// new runtime dep is a variant on [`RuntimeDep`] — the construction sites
/// ([`RuntimeDeps::empty`] / [`RuntimeDeps::with`] / [`RuntimeDeps::merge`]) and
/// the consumers ([`RuntimeDeps::helper_module`] /
/// [`RuntimeDeps::apply_to_cargo_toml`], …) are table-driven over
/// [`RuntimeDep::ALL`].
#[derive(Debug, Clone, Default)]
pub struct RuntimeDeps {
    deps: BTreeSet<RuntimeDep>,
}

impl RuntimeDeps {
    /// An empty dep set — the common case (a plain `.ts` file links nothing).
    pub fn empty() -> Self {
        Self::default()
    }

    /// Add `dep` (builder-style: returns `self` moved).
    pub fn with(mut self, dep: RuntimeDep) -> Self {
        self.deps.insert(dep);
        self
    }

    /// Add `dep` to this set in place.
    pub fn insert(&mut self, dep: RuntimeDep) {
        self.deps.insert(dep);
    }

    /// Whether `dep` is in the set.
    pub fn has(&self, dep: RuntimeDep) -> bool {
        self.deps.contains(&dep)
    }

    /// Readable accessors — `deps.needs_engine()` over `deps.has(RuntimeDep::Engine)`.
    pub fn needs_ryu_js(&self) -> bool {
        self.has(RuntimeDep::RyuJs)
    }
    pub fn needs_serde_json(&self) -> bool {
        self.has(RuntimeDep::SerdeJson)
    }
    pub fn needs_engine(&self) -> bool {
        self.has(RuntimeDep::Engine)
    }
    pub fn needs_array_helper(&self) -> bool {
        self.has(RuntimeDep::ArrayHelper)
    }
    pub fn needs_regress(&self) -> bool {
        self.has(RuntimeDep::Regress)
    }
    pub fn needs_temporal(&self) -> bool {
        self.has(RuntimeDep::Temporal)
    }
    /// A `new Worker(handler)` spawns a worker thread (Direction D).
    pub fn needs_worker(&self) -> bool {
        self.has(RuntimeDep::Worker)
    }

    /// Union another dep set into this one — a project links a runtime dep if
    /// any of its translated files does.
    pub fn merge(&mut self, other: &RuntimeDeps) {
        self.deps.extend(&other.deps);
    }

    /// The `__ds` helper module source — assembled from whichever helper slices
    /// this dep set flagged (`number_to_string` for `RyuJs`, `array_set` for
    /// `ArrayHelper`), in [`RuntimeDep::ALL`] order. `None` when neither is
    /// needed, so the caller writes nothing and the default build pulls no
    /// `ryu_js`.
    pub fn helper_module(&self) -> Option<String> {
        let mut src = String::from(
            "//! DashScript runtime helpers: ES-compat shims a bare Rust lowering\n//! would get wrong (Number::toString, Array auto-grow).\n\n",
        );
        let mut any = false;
        for d in RuntimeDep::ALL {
            if self.has(d) {
                if let Some(slice) = d.helper() {
                    src.push_str(slice);
                    any = true;
                }
            }
        }
        any.then_some(src)
    }

    /// The `__ds_engine` compat module source — runs a `.ts` source under an
    /// embedded QuickJS engine — when this dep set flags `Engine`. `None`
    /// otherwise, so the caller writes nothing and pulls no engine dependency.
    pub fn engine_helper_module(&self) -> Option<&'static str> {
        self.needs_engine().then_some(ENGINE_HELPER_MODULE)
    }

    /// Append each flagged cargo dep to a generated `Cargo.toml`, creating the
    /// `[dependencies]` section if absent. A no-op for a dep already declared
    /// (e.g. the project declared `cargo:ryu_js`) — so a consumer can call this
    /// unconditionally and let the dep set gate it. A string-level post-process
    /// keeps the dep out of the user's `package.json` — it is a DashScript-
    /// internal runtime need, not a declared project dependency.
    pub fn apply_to_cargo_toml(&self, cargo_toml: &mut String) {
        for d in RuntimeDep::ALL {
            if self.has(d) {
                if let Some(deps) = d.cargo() {
                    for &(pkg, req) in deps {
                        append_dep(cargo_toml, pkg, req);
                    }
                }
            }
        }
    }
}

/// Append `<pkg> = <req>` to a generated `Cargo.toml`'s `[dependencies]`,
/// creating the section if absent. A no-op when the dep is already declared —
/// the caller gates per dep (via [`RuntimeDeps::has`]) and lets this handle the
/// string edit. A string-level post-process keeps these deps out of the user's
/// `package.json` — they are DashScript-internal runtime needs.
fn append_dep(cargo_toml: &mut String, pkg: &str, req: &str) {
    let needle = format!("{pkg} =");
    if cargo_toml.contains(&needle) {
        return;
    }
    let line = format!("{pkg} = {req}\n");
    if let Some(pos) = cargo_toml.find("[dependencies]\n") {
        cargo_toml.insert_str(pos + "[dependencies]\n".len(), &line);
    } else {
        cargo_toml.push_str(&format!("\n[dependencies]\n{line}"));
    }
}

/// The DashScript runtime helper module, written to `src/__ds.rs` and declared
/// `mod __ds;` at each crate root when a translated file references it. The
/// single source for the `__ds` helpers — consumed by both `ds build` (bin) and
/// the conformance harness (lib test) — so the helper text lives in the library
/// rather than either consumer. [`RuntimeDeps::helper_module`] concatenates
/// whichever slices a translation flagged.
const RYUJS_HELPERS: &str = "\
use ryu_js::Buffer;

/// Format an `f64` as ECMAScript `Number::toString` would. `ryu_js` covers NaN
/// and ±Infinity; signed zero is normalized to `\"0\"` (ES prints both `+0`
/// and `-0` that way).
#[inline]
pub fn number_to_string(x: f64) -> String {
    if x == 0.0 {
        return \"0\".to_string();
    }
    Buffer::new().format(x).to_string()
}
";

/// ES `Array` indexed-assignment auto-grow for a `Vec<T>` — the slice a
/// `xs[i] = v` store flags via `needs_array_helper`. Pure `std` (no external
/// crate), so emitting this slice adds no `Cargo.toml` dependency.
const ARRAY_HELPER: &str = "\
/// ES indexed assignment `arr[i] = v` for a `Vec<T>`. ES `Array` auto-grows:
/// `i < len` replaces, `i == len` appends, `i > len` grows with `T::default()`
/// filling the gap (a JS array would use holes, but `T` has no undefined). A
/// negative or non-integer index is a property set in JS, not an element —
/// ignored here. A bare Rust `vec[i] = v` would panic instead of growing.
#[inline]
pub fn array_set<T: Default + Clone>(arr: &mut Vec<T>, i: f64, v: T) {
    if !i.is_finite() || i < 0.0 || i.fract() != 0.0 {
        return;
    }
    let idx = i as usize;
    if idx < arr.len() {
        arr[idx] = v;
    } else if idx == arr.len() {
        arr.push(v);
    } else {
        arr.resize(idx + 1, T::default());
        arr[idx] = v;
    }
}
";

/// ES Web Worker–style isolate (Direction D, D1). `new Worker(handler)` spawns a
/// thread that runs `handler(msg)` for each message the main thread sends via
/// `post_message`. Messages cross the thread boundary as JSON (serde), so any
/// `Serialize`/`DeserializeOwned` value works; the handler runs on the worker
/// thread, so its stdout (`console.log`) shares the process's. Pure Rust stack
/// — no JS engine in the worker (decision point 10 MVP). `Drop` closes the
/// channel (ending the worker's receive loop) then joins the thread, so a
/// posted message is guaranteed processed before the process exits.
///
/// D2 bidirectional: a handler with a second `reply` parameter
/// (`new Worker((msg, reply) => { reply.send(v); })`) spawns via
/// `new_with_reply`, which gives the worker a `Reply` sink on a second
/// (worker→main) channel; main reads it with `recv` (the arch's D2 mapping of
/// `worker.on('message')` to a blocking `mpsc::Receiver::recv`). A one-arg
/// handler stays on the D1 one-way `new`. File-based `new Worker('./w.ts')`
/// (worker-entry translation + build-time dep scan) is a later batch that
/// reuses this runtime.
const WORKER_HELPER: &str = "\
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread::{spawn, JoinHandle};

/// A reply sink the worker calls to send a message back to main (D2). The
/// handler's second parameter binds to this; `send` serializes and posts on the
/// worker→main channel, which main reads via [`Worker::recv`]. Named `send` so
/// DashScript `reply.send(v)` maps through the generic member-expr path.
pub struct Reply {
    tx: Sender<serde_json::Value>,
}

impl Reply {
    /// Post a reply to main (serialized to JSON). A serialize failure or a
    /// closed channel is silently dropped (the worker is an isolate, MVP).
    #[inline]
    pub fn send<A: serde::Serialize>(&self, msg: A) {
        if let Ok(val) = serde_json::to_value(&msg) {
            let _ = self.tx.send(val);
        }
    }
}

/// A Web Worker–style isolate: a spawned thread running a message handler, fed
/// by a main→worker mpsc channel (D1), and optionally a worker→main reply
/// channel (D2). The sender, reply receiver, and handle are `Option`s so `Drop`
/// can take them — close the channel, then join.
pub struct Worker {
    tx: Option<Sender<serde_json::Value>>,
    reply_rx: Option<Receiver<serde_json::Value>>,
    handle: Option<JoinHandle<()>>,
}

impl Worker {
    /// D1 one-way: spawn a worker that invokes `handler(msg)` for each message
    /// received. `A` is the message type (deserialized from JSON).
    pub fn new<A, F>(handler: F) -> Self
    where
        A: serde::de::DeserializeOwned,
        F: Fn(A) + Send + 'static,
    {
        let (tx, rx) = channel::<serde_json::Value>();
        let handle = spawn(move || {
            for msg in rx.iter() {
                if let Ok(a) = serde_json::from_value::<A>(msg) {
                    handler(a);
                }
            }
        });
        Worker { tx: Some(tx), reply_rx: None, handle: Some(handle) }
    }

    /// D2 bidirectional: spawn a worker that invokes `handler(msg, reply)`. The
    /// handler calls `reply.send(v)` to post a reply main reads via [`recv`].
    /// `Reply` is cloned per message (its `Sender` is `Clone`).
    pub fn new_with_reply<A, F>(handler: F) -> Self
    where
        A: serde::de::DeserializeOwned,
        F: Fn(A, Reply) + Send + 'static,
    {
        let (tx, rx) = channel::<serde_json::Value>();
        let (reply_tx, reply_rx) = channel::<serde_json::Value>();
        let handle = spawn(move || {
            for msg in rx.iter() {
                if let Ok(a) = serde_json::from_value::<A>(msg) {
                    handler(a, Reply { tx: reply_tx.clone() });
                }
            }
        });
        Worker { tx: Some(tx), reply_rx: Some(reply_rx), handle: Some(handle) }
    }

    /// Send a message to the worker (main → worker). ES `postMessage`. A
    /// serialize failure or a closed channel is silently dropped — the worker
    /// is an isolate, so the sender does not learn of delivery (MVP).
    #[inline]
    pub fn post_message<A: serde::Serialize>(&self, msg: A) {
        if let (Some(tx), Ok(val)) = (&self.tx, serde_json::to_value(&msg)) {
            let _ = tx.send(val);
        }
    }

    /// Block for one reply from the worker (worker → main). The arch's D2
    /// mapping of `worker.on('message')` to a blocking `mpsc::Receiver::recv`
    /// (DashScript's `fn main` is synchronous — no event loop). Panics if this
    /// is a one-way (D1) worker with no reply channel, or the worker closed
    /// without replying. `B` is inferred from the call's context — annotate the
    /// binding (`const r: number = w.recv()`) when Rust cannot infer it.
    pub fn recv<B: serde::de::DeserializeOwned>(&self) -> B {
        let rx = self
            .reply_rx
            .as_ref()
            .expect(\"Worker::recv on a one-way (D1) worker — use new_with_reply\");
        let msg = rx.recv().expect(\"worker closed without replying\");
        serde_json::from_value::<B>(msg).expect(\"reply failed to deserialize\")
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        // Close the sender first so the worker's `rx.iter()` ends, then join so
        // a posted message is guaranteed processed before the process exits. A
        // handler panic surfaces via `join`'s `Err` (re-panic, matching a worker
        // that throws uncaught). `reply_rx` drops with the Worker — pending
        // un-received replies are lost (main didn't recv them).
        drop(self.tx.take());
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}
";

/// ES RegExp helpers — `__ds::regex` compiles a `/pat/flags` literal to a
/// `regress::Regex`. `regress` implements ES regex semantics (backreferences,
/// lookaround, unicode case folding) the `regex` crate cannot express. Only
/// emitted when a translated file uses a regex literal, so a plain `ds build`
/// pulls no `regress` dependency.
const REGRESS_HELPERS: &str = r##"
use regress::{Match, Regex};

/// Compile an ES RegExp literal `/pattern/flags` to a `regress::Regex`. oxc
/// parses `/pat/` upfront, so an invalid literal never reaches runtime — the
/// fallback compiles an empty pattern rather than panic.
#[inline]
pub fn regex(pattern: &str, flags: &str) -> Regex {
    Regex::with_flags(pattern, flags).unwrap_or_else(|_| Regex::new("").unwrap())
}

/// An ES `String.prototype.match` / `RegExp.prototype.exec` result.
/// `captures[0]` is the whole match; `[1..]` are the capture groups (`None`
/// when a group did not participate). `index` is the match-start byte offset
/// (ASCII == UTF-16 code-unit index); `input` is the haystack.
pub struct DsMatch {
    pub captures: Vec<Option<String>>,
    pub index: usize,
    pub input: String,
}

/// Build a `DsMatch` from one regress `Match` — shared by `regex_match`
/// (re-compiles from a source pattern) and the variable `.exec` lowering (uses
/// an already-compiled `Regex`). regress' `groups()` yields group 0 (the whole
/// match) followed by the capture groups — exactly the ES `m[0]`/`m[1]`/…
/// layout, so no manual whole-match prefix (that would shift every group).
#[inline]
pub fn ds_match_from(text: &str, m: &Match) -> DsMatch {
    let captures: Vec<Option<String>> =
        m.groups().map(|g| g.map(|r| text[r].to_string())).collect();
    DsMatch {
        captures,
        index: m.range().start,
        input: text.to_string(),
    }
}

/// Render a string the way Node inspects it inside a match array: single-
/// quoted, with backslash, quote, and control chars escaped.
fn ds_inspect_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out
}

/// Render a `DsMatch` the way Node's `console.log` prints a `RegExp.prototype
/// .exec` / `String.prototype.match` result: the match array `[ '<match>',
/// '<group1>', …, index: N, input: '<input>', groups: undefined ]`. A capture
/// that did not participate is `undefined`; `groups` is `undefined` when the
/// pattern has no named groups (named groups need the group names, which
/// `DsMatch` does not carry — those fixtures stay on the engine path).
impl std::fmt::Display for DsMatch {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "[ ")?;
        for (i, c) in self.captures.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            match c {
                None => write!(f, "undefined")?,
                Some(s) => write!(f, "'{}'", ds_inspect_str(s))?,
            }
        }
        write!(
            f,
            ", index: {}, input: '{}', groups: undefined ]",
            self.index,
            ds_inspect_str(&self.input)
        )
    }
}

/// `console.log(/pat/.exec(s))` renders `Option<DsMatch>` as Node prints a
/// regex result: `null` (no match) or the match array via `DsMatch`'s Display.
/// `Option` is std's, so `Display` on `Option<DsMatch>` is blocked by the
/// orphan rule; the translator wraps an exec/match console.log argument here.
#[inline]
pub fn fmt_option_match(opt: Option<DsMatch>) -> String {
    match opt {
        None => "null".to_string(),
        Some(m) => format!("{}", m),
    }
}

/// `s.match(/pat/)` (non-global) — the first match as a `DsMatch`, or `None`.
#[inline]
pub fn regex_match(pattern: &str, flags: &str, text: &str) -> Option<DsMatch> {
    let re = Regex::with_flags(pattern, flags).ok()?;
    let m = re.find(text)?;
    Some(ds_match_from(text, &m))
}

/// `s.search(/pat/)` — the byte index of the first match, or `-1` (ES returns
/// -1 when no match). ASCII offsets match UTF-16 code-unit indices.
#[inline]
pub fn regex_search(pattern: &str, flags: &str, text: &str) -> f64 {
    match Regex::with_flags(pattern, flags) {
        Ok(re) => re.find(text).map(|m| m.range().start as f64).unwrap_or(-1_f64),
        Err(_) => -1_f64,
    }
}

/// `s.replace(/pat/, repl)` (non-global) — the first match replaced by `repl`,
/// with `$` patterns expanded (`$&` whole match, `$1`/`$2`… groups, `` $` ``
/// before, `$'` after, `$$` literal `$`). Returns the input unchanged on no
/// match. The global flag (`g`) replaces every match instead of just the first;
/// a zero-width match advances one char so the loop terminates.
#[inline]
pub fn regex_replace(pattern: &str, flags: &str, text: &str, repl: &str) -> String {
    let Ok(re) = Regex::with_flags(pattern, flags) else {
        return text.to_string();
    };
    if flags.contains('g') {
        let mut out = String::with_capacity(text.len() + repl.len());
        let mut last = 0usize;
        for m in re.find_iter(text) {
            let r = m.range();
            out.push_str(&text[last..r.start]);
            expand_replacement(repl, text, &m, &mut out);
            last = r.end;
            if r.start == r.end {
                // zero-width: copy one char so the next match advances
                if let Some((_, ch)) = text[r.end..].char_indices().next() {
                    out.push(ch);
                    last += ch.len_utf8();
                }
            }
        }
        out.push_str(&text[last..]);
        return out;
    }
    let Some(m) = re.find(text) else {
        return text.to_string();
    };
    let r = m.range();
    let mut out = String::with_capacity(text.len() + repl.len());
    out.push_str(&text[..r.start]);
    expand_replacement(repl, text, &m, &mut out);
    out.push_str(&text[r.end..]);
    out
}

/// Expand `$&`/`$1`/`` $` ``/`$'`/`$$` patterns in a replacement string against
/// one match. `$nn` is used when it indexes a capture group, else `$n`; an
/// out-of-range or non-participating group expands to empty.
fn expand_replacement(repl: &str, text: &str, m: &Match, out: &mut String) {
    let r = m.range();
    let chars: Vec<char> = repl.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c != '$' {
            out.push(c);
            i += 1;
            continue;
        }
        if i + 1 >= chars.len() {
            out.push('$');
            i += 1;
            continue;
        }
        let nc = chars[i + 1];
        match nc {
            '$' => {
                out.push('$');
                i += 2;
            }
            '&' => {
                out.push_str(&text[r.start..r.end]);
                i += 2;
            }
            '`' => {
                out.push_str(&text[..r.start]);
                i += 2;
            }
            '\'' => {
                out.push_str(&text[r.end..]);
                i += 2;
            }
            d @ '0'..='9' => {
                let one = (d as u8 - b'0') as usize;
                let (n, two) = if i + 2 < chars.len() && chars[i + 2].is_ascii_digit() {
                    let cand = one * 10 + (chars[i + 2] as u8 - b'0') as usize;
                    if m.group(cand).is_some() {
                        (cand, true)
                    } else {
                        (one, false)
                    }
                } else {
                    (one, false)
                };
                if let Some(gr) = m.group(n) {
                    out.push_str(&text[gr]);
                }
                i += 2 + usize::from(two);
            }
            _ => {
                out.push('$');
                out.push(nc);
                i += 2;
            }
        }
    }
}

/// `s.split(/pat/, limit?)` — split `text` on regex matches into owned
/// segments. `limit` caps the result count (`None` → unbounded; `Some(0)` →
/// empty). A zero-width match at the cursor is skipped so the split advances
/// (mirroring ES split's non-empty progression). Capture groups are not
/// interleaved into the output (a later phase).
#[inline]
pub fn regex_split(pattern: &str, flags: &str, text: &str, limit: Option<usize>) -> Vec<String> {
    let Ok(re) = Regex::with_flags(pattern, flags) else {
        return vec![text.to_string()];
    };
    let cap = limit.unwrap_or(usize::MAX);
    if cap == 0 {
        return Vec::new();
    }
    let mut parts: Vec<String> = Vec::new();
    let mut last = 0usize;
    for m in re.find_iter(text) {
        if parts.len() + 1 >= cap {
            break;
        }
        let r = m.range();
        if r.start == r.end && r.start == last {
            continue;
        }
        parts.push(text[last..r.start].to_string());
        last = r.end;
    }
    parts.push(text[last..].to_string());
    parts
}
"##;

/// The DashScript compat engine module, written to `src/__ds_engine.rs` and
/// declared `mod __ds_engine;` at the crate root when a translated file uses ES
/// dynamic reflection the static translator cannot lower. It runs the whole
/// `.ts` source under an embedded QuickJS engine (`rquickjs`), with a
/// `console.log` wired to stdout. Number stringification uses the engine's own
/// `String()` (ES `Number::toString`), so output matches Node for primitives.
///
/// Gated: only emitted for `needs_engine` programs, so a plain `ds build` pulls
/// no engine dependency (and no QuickJS C compile). The single source for the
/// engine helper — consumed by both `ds build` (project.rs) and the conformance
/// harness — so the helper text lives in the library rather than either
/// consumer.
///
/// Note: the engine evaluates the source as plain ECMAScript, so a `.ts` source
/// with TypeScript type annotations is not yet handled on this path (today it
/// serves the conformance oracle, whose test262 fixtures are annotation-free
/// JS). Stripping annotations for real `.ts` sources is a follow-up.
const ENGINE_HELPER_MODULE: &str = r##"//! DashScript compat engine: run a `.ts` source under an embedded QuickJS
//! engine (`rquickjs`) when it uses ES dynamic reflection
//! (`Object.defineProperty`, `Reflect.*`, `Symbol`, `Proxy`, …) the static
//! translator cannot lower to idiomatic Rust. Gated — only present when
//! `RuntimeDeps::needs_engine`.
use rquickjs::{Context, Ctx, Runtime};

/// Run a `.ts` source under QuickJS with `console.log` wired to stdout. The
/// source is self-contained — it declares `main()` and calls it (pure-TS
/// execution semantics: a declaration alone does not run), so a single eval
/// runs the fixture. `console.log` joins its arguments with spaces, stringified
/// by the engine's own `String()` coercion — ES `Number::toString` for numbers
/// — so the output matches Node for primitives.
pub fn run(source: &str) {
    use rquickjs::context::EvalOptions;
    let runtime = Runtime::new().expect("rquickjs Runtime");
    let ctx = Context::full(&runtime).expect("rquickjs Context");
    // Sloppy-mode eval (strict=false): test262 fixtures use `this` at the top
    // of `main` for property-attribute setup (`this.configurable = true`), the
    // sloppy-mode `this`=global. Node runs the oracle the same way (a plain
    // script, not a strict module); strict eval would make `this`=undefined
    // and throw before the first console.log.
    let sloppy = || {
        let mut o = EvalOptions::default();
        o.strict = false;
        o
    };
    let result = ctx.with(|ctx: Ctx<'_>| -> rquickjs::Result<()> {
        // A native line-print primitive; `console.log` (defined in JS below)
        // joins its arguments with spaces and hands each finished line here.
        let print_line = rquickjs::Function::new(ctx.clone(), |s: String| {
            println!("{s}");
        })?;
        ctx.globals().set("__ds_print_line", print_line)?;
        // Define `console.log` in JS so argument stringification uses the
        // engine's own `String()` coercion (ES NumberToString for numbers),
        // matching Node's `console.log` output for primitives. A plain number
        // arg prints `1e+21` (not Rust's `f64` Display spelling).
        ctx.eval_with_options::<(), _>(
            r#"this.console = { log: function () {
                for (var i = 0, out = []; i < arguments.length; i++) {
                    out.push(String(arguments[i]));
                }
                __ds_print_line(out.join(" "));
            } };"#,
            sloppy(),
        )?;
        // Eval the source — it is self-contained (declares `main` and calls
        // it, pure-TS execution semantics), so a single eval runs the fixture.
        ctx.eval_with_options::<(), _>(source, sloppy())?;
        Ok(())
    });
    result.expect("rquickjs eval");
}
"##;

/// Strip TS type annotations from a program's top-level function declarations
/// (a `.ts` source annotates `main`'s return; test262 fixtures wrap the file in
/// `function main(): void`) and regenerate the source via oxc codegen, so the
/// embedded QuickJS engine evaluates plain ECMAScript rather than TypeScript.
/// Shared by the engine lowering ([`Translator::translate_with_deps`]) and
/// [`Translator::engine_source`] (the conformance harness's direct-eval path),
/// so both run the exact same bytes.
fn engine_js_source(program: &mut oxc_ast::ast::Program<'_>) -> String {
    for stmt in &mut program.body {
        if let oxc_ast::ast::Statement::FunctionDeclaration(f) = stmt {
            f.return_type = None;
        }
    }
    Codegen::new().build(&*program).code
}

/// A `.ts` file's role in its project — the file-role distinction the
/// architecture's implicit-`main` design hinges on (decision point 8). `ds
/// build` sets it from the package manifest for each file it translates.
///
/// `BinEntry` (the default) lowers top-level executable statements into an
/// implicit `fn main`, the way Node runs an entry script — so a lone file (no
/// `package.json`) and a conformance fixture are always `BinEntry`. `Module`
/// lowers declarations only and rejects top-level executable statements: a
/// module declares an API, it does not run, so a `console.log` at the top of a
/// module file has no entry to land in. The `translate`/`check` entry points
/// default to `BinEntry`; `_as` variants take a role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FileRole {
    /// A bin/lib entry — top-level executable statements collect into an
    /// implicit `fn main` the translator always emits.
    #[default]
    BinEntry,
    /// A module imported by an entry — declarations only; top-level executable
    /// statements are rejected (a module declares, it does not execute).
    Module,
}

/// Translates a TypeScript-flavored `.ts` program into Rust source.
#[derive(Default)]
pub struct Translator;

impl Translator {
    /// Create a translator with default options.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Parse `.ts` source with oxc and translate the AST to Rust source.
    ///
    /// Convenience wrapper around [`Self::translate_with_deps`] that drops the
    /// runtime-dependency report — for callers (tests, LSP) that only want the
    /// Rust text. `ds build` uses [`Self::translate_with_deps`] so the project
    /// links only what the source uses.
    ///
    /// # Errors
    /// Returns an error string if oxc reports parse diagnostics.
    pub fn translate(&self, source: &str) -> Result<String, String> {
        Ok(self.translate_with_deps(source)?.0)
    }

    /// Parse `.ts` source, translate the AST to Rust source, and report the
    /// runtime dependencies the generated code needs. Lowers as
    /// [`FileRole::BinEntry`] — the default for a lone file (always run) and a
    /// conformance fixture.
    ///
    /// The Rust text matches [`Self::translate`]; the second return value is the
    /// set of extra crates / helper modules the translated code references, so
    /// the project emitter can add them to `Cargo.toml` and write the helper
    /// module only when needed.
    ///
    /// # Errors
    /// Returns an error string if oxc reports parse diagnostics.
    pub fn translate_with_deps(&self, source: &str) -> Result<(String, RuntimeDeps), String> {
        self.translate_with_deps_as(source, FileRole::BinEntry)
    }

    /// Parse `.ts` source, translate the AST to Rust source, report the runtime
    /// dependencies, and lower according to `role`. [`FileRole::BinEntry`] emits
    /// an implicit `fn main` collecting top-level executable statements;
    /// [`FileRole::Module`] emits declarations only and rejects top-level
    /// executable statements (a module declares an API, it does not run). `ds
    /// build` passes `Module` for a file that is not a package entry.
    ///
    /// # Errors
    /// Returns an error string if oxc reports parse diagnostics, or if `role`
    /// is [`FileRole::Module`] and the file has top-level executable statements.
    pub fn translate_with_deps_as(
        &self,
        source: &str,
        role: FileRole,
    ) -> Result<(String, RuntimeDeps), String> {
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, source, SourceType::ts()).parse();

        if !ret.diagnostics.is_empty() {
            return Err(format!(
                "dashscript: oxc reported {} parse diagnostic(s)",
                ret.diagnostics.len()
            ));
        }

        // Move the program into the arena so the arena, the program, and the
        // semantic analysis all share one lifetime `'a` (the same trick
        // `semantic::analyze_symbols` uses). `with_build_nodes(true)` fills the
        // `symbol_id` / `reference_id` cells on each `BindingIdentifier` /
        // `IdentifierReference` so the translator can resolve any identifier to
        // its `SymbolId` — the identity `NameTable` keys on, replacing the lossy
        // `snake(name)` string fold.
        let program = allocator.alloc(ret.program);
        let sret = SemanticBuilder::new().with_build_nodes(true).build(program);
        let mut names = name_table::build(sret.semantic.scoping());

        // Engine-gated compat path: a source using ES dynamic reflection
        // (`Object.defineProperty`, `Reflect.*`, `Symbol`, `Proxy`,
        // `instanceof`, …) the static translator cannot lower is run whole
        // under an embedded QuickJS engine instead of being lowered to Rust.
        // The same `collect_unsupported` walk that flags these as
        // `unsupported` in `ds lint` here flips the file to the engine path —
        // a single source of truth for what the engine covers, so the lint and
        // the lowering cannot drift. Default `ds build` output stays pure Rust;
        // only a program that actually uses such a construct pulls the
        // `rquickjs` engine dep (and its C compile).
        if check::program_uses_engine(program) {
            // The engine evaluates ECMAScript, so strip the TS type annotations
            // the source carries — QuickJS parses JS, not TS. `engine_js_source`
            // does the strip + codegen and is shared with `engine_source`, so
            // the conformance harness can run the exact bytes the engine path
            // embeds without compiling a throwaway cargo project per fixture.
            let js_source = engine_js_source(program);
            let src_lit = syn::LitStr::new(&js_source, proc_macro2::Span::call_site());
            let main_item: syn::Item = syn::parse_quote! {
                fn main() {
                    crate::__ds_engine::run(#src_lit);
                }
            };
            let rust = prettyplease::unparse(&syn::File {
                shebang: None,
                attrs: Vec::new(),
                items: vec![main_item],
            });
            let mut deps = RuntimeDeps::empty();
            deps.insert(RuntimeDep::Engine);
            return Ok((rust, deps));
        }

        // Record the file's namespace-import bindings (`import * as ns`) so a
        // reference to `ns` is recognized as a module-path prefix (`ns.foo` →
        // `ns::foo`) rather than a field access. The engine path returns above,
        // so this only runs for the statically-lowered Rust path.
        names.register_namespaces(&program.body);

        // First pass: collect discriminated-union enum shapes so later
        // expression translation can build variant constructors.
        let registry = registry::build_registry(&program.body, &names);
        // Escape promotion (A3): a top-level `const` number/boolean literal
        // referenced from a top-level `function` cannot stay in `fn main` (a
        // Rust fn item cannot close over a `main` local), so it is hoisted to a
        // crate-level `const` item. Register the numeric ones in the name table
        // BEFORE any body is translated, so a function that appears before the
        // const in source order still sees it as an `f64` value for number→
        // string routing (Rust items are hoisted; ES top-level bindings are
        // order-independent at the module level).
        let promoted = functions::promoted_const_names(&program.body, &names, &registry);
        for s in &program.body {
            if let oxc_ast::ast::Statement::VariableDeclaration(v) = s {
                if let Some((sym, name, kind)) = functions::promotable_const_info(v, &names) {
                    if kind.is_number() && promoted.contains(&name) {
                        names.register_number_const(sym);
                    }
                }
            }
        }
        // Pure-TS execution semantics: a top-level statement that *runs* in
        // source order (a `const`, an expression, control flow, a throw) does
        // not map to a Rust item — it belongs inside the entry point, the way
        // Node runs a script's top-level statements immediately. Declarations
        // (`function` / `class` / `interface` / `type` / `import` / `export`)
        // still lower to Rust items. Split the body: declarations → items;
        // executable statements → one implicit `fn main` body (or an empty
        // `fn main {}` when there are none — a Rust binary needs an entry).
        let mut items: Vec<syn::Item> = Vec::new();
        // Inline scalar-union enums (`__DsUnion…`) discovered by the registry
        // pre-pass are emitted first, before any item that names them. A
        // `FileRole::Module` skips emission: the entry emits the enum at the
        // crate root, and every reference is `crate::`-prefixed (`types` /
        // `binary` / `object` / `unary`) so a module resolves to that one
        // definition instead of its own (a per-module `enum __DsUnion…` would
        // be a distinct nominal type → E0308 at any cross-module call).
        if !matches!(role, FileRole::Module) {
            let mut union_enum_names: Vec<&syn::Ident> = registry.union_enums.keys().collect();
            union_enum_names.sort();
            items.extend(union_enum_names.into_iter().flat_map(|name| {
                let e = &registry.union_enums[name];
                [
                    syn::Item::Enum(e.clone()),
                    syn::Item::Impl(declarations::union_display_impl(e)),
                ]
            }));
        }
        let mut exec_stmts: Vec<&oxc_ast::ast::Statement> = Vec::new();
        for s in &program.body {
            // A promoted const-expr `const` lowers to a crate-level `const`
            // item here (escape promotion, A3) — NOT collected into `fn main`,
            // so a top-level function reading it resolves to the item, not a
            // `main` local it cannot see.
            if let Some(item) = functions::promoted_const_item(s, &promoted, &names) {
                items.push(item);
                continue;
            }
            if functions::is_executable_top_level(s) {
                exec_stmts.push(s);
            } else {
                items.extend(functions::translate_statement(s, &registry, &names));
            }
        }
        // The implicit entry analyzes the top-level executable statements the
        // same way a function body is analyzed (mutations, member mutations, use
        // counts, number flavor). Declaration statements are no-ops in the
        // walk, so passing the full `program.body` slice is equivalent to the
        // executable subset. `return_path` is `None` — a top-level `return
        // expr;` cannot yield a value (binary `main` returns `()`); `check`
        // flags it unsupported.
        match role {
            FileRole::BinEntry => {
                let main_item: syn::Item = {
                    let mut locals = context::Locals::new();
                    let analysis = analysis::analyze(
                        &program.body,
                        &names,
                        &registry.mut_methods,
                        &registry.ref_params,
                    );
                    locals.mutated = analysis.mutated;
                    locals.member_mutated = analysis.member_mutated;
                    locals.use_counts = analysis.use_counts;
                    locals.number_flavors = flavor::infer(&program.body, &names);
                    let mut out: Vec<syn::Stmt> = exec_stmts
                        .into_iter()
                        .flat_map(|s| {
                            functions::translate_stmt(
                                s,
                                &mut locals,
                                &registry,
                                &context::Narrow::default(),
                                None,
                                &names,
                            )
                        })
                        .collect();
                    functions::drop_trailing_return(&mut out);
                    let block: syn::Block = syn::parse_quote!({ #(#out)* });
                    syn::parse_quote! {
                        fn main() #block
                    }
                };
                items.push(main_item);
            }
            FileRole::Module => {
                // Module semantics (arch decision point 8): a module only
                // declares, never executes. Top-level executable statements have
                // no `fn main` to run in (a Node module only exports; it does
                // not run top-level statements unless it is an entry) — reject,
                // rather than silently dropping their side effects.
                if !exec_stmts.is_empty() {
                    return Err(
                        "a module file may only declare (function / class / interface / \
                         type / import / export) — top-level executable statements have no \
                         entry to run in; move them into a function, or make this file a \
                         bin entry"
                            .into(),
                    );
                }
                // declarations-only: a crate-internal module (src/<stem>.rs)
                // with no `fn main`, brought in by the entry via `mod <stem>;`.
            }
        }
        let file = syn::File {
            shebang: None,
            attrs: Vec::new(),
            items,
        };
        // An emit point that routes an `f64` through the ES NumberToString
        // helper writes a `crate::__ds::number_to_string` call into the Rust
        // text; a `JSON.parse`/`JSON.stringify` call inlines `serde_json::`.
        // Either prefix means the generated crate needs the matching crate (and
        // the `__ds` helper module, for ryu_js). Scanning the emitted text
        // (rather than threading a `RefCell<RuntimeDeps>` through every
        // expression) keeps the dep report a pure function of the output — the
        // `__ds::` prefix is a DashScript-reserved namespace a `.ts` source
        // cannot produce any other way, and `serde_json::` likewise only
        // appears via the `JSON` builtin.
        let rust = prettyplease::unparse(&file);
        let mut deps = RuntimeDeps::empty();
        for d in RuntimeDep::ALL {
            if d.marker().is_some_and(|m| rust.contains(m)) {
                deps.insert(d);
            }
        }
        Ok((rust, deps))
    }

    /// Check `.ts` source for translatability without emitting Rust.
    ///
    /// Returns syntax errors from `oxc_parser` plus one diagnostic per
    /// top-level statement the translator cannot map. An empty `Vec` means the
    /// file is translatable to valid Rust (as far as DashScript can tell).
    #[must_use]
    pub fn check(&self, source: &str) -> Vec<OxcDiagnostic> {
        check::check(source)
    }

    /// Role-aware translatability check — see [`Self::check`]. [`FileRole::Module`]
    /// additionally flags top-level executable statements (a module declares,
    /// it does not run).
    #[must_use]
    pub fn check_as(&self, source: &str, role: FileRole) -> Vec<OxcDiagnostic> {
        check::check_as(source, role)
    }

    /// The annotation-stripped ECMAScript the engine compat path would run,
    /// when the source uses ES dynamic reflection (`Object.defineProperty`,
    /// `Reflect.*`, …) the static translator cannot lower. `None` for a plain
    /// source (no engine). The conformance harness uses this to run an engine
    /// fixture directly under an embedded QuickJS engine — the exact bytes
    /// `translate_with_deps` embeds in `__ds_engine::run` — without compiling
    /// a throwaway cargo project per fixture.
    #[must_use]
    pub fn engine_source(&self, source: &str) -> Option<String> {
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, source, SourceType::ts()).parse();
        if !ret.diagnostics.is_empty() {
            return None;
        }
        let program = allocator.alloc(ret.program);
        if check::program_uses_engine(program) {
            Some(engine_js_source(program))
        } else {
            None
        }
    }

    /// The local `.ts` modules this file imports (`import { x } from "./other"`
    /// → `other`), for `ds build` to assemble one Rust module per dependency.
    #[must_use]
    pub fn imports(&self, source: &str) -> Vec<imports::ImportRef> {
        imports::collect_imports(source)
    }

    /// Translate a `.d.ts` declaration source to a Rust module body — each
    /// `interface`/`type` becomes a `pub` struct/alias. A pure `.d.ts` (an
    /// `@types/*` package with no sibling `.js`) carries types only, so a
    /// value import surfaces as a `cargo check` "cannot find function"
    /// honestly. Used by `ds build` when a dependency resolves to a `.d.ts`.
    #[must_use]
    pub fn translate_dts(&self, source: &str) -> String {
        dts::translate_dts(source)
    }

    /// The bare-crate imports in a `.ts` file (`import { X } from "crate"`),
    /// each with its `.ts` byte span. Used by `ds lsp` to resolve
    /// go-to-definition on an import specifier to the crate's `~/.cargo` source.
    #[must_use]
    pub fn crate_imports(&self, source: &str) -> Vec<imports::CrateImport> {
        imports::collect_crate_imports(source)
    }

    /// The locally declarable names in a `.ts` file (`function`, `interface`,
    /// `type`, `export`, `import`), each with its binding byte span. Used by
    /// `ds lsp` for in-file go-to-definition (everything but crate imports).
    #[must_use]
    pub fn declarations(&self, source: &str) -> Vec<imports::LocalSymbol> {
        imports::collect_declarations(source)
    }

    /// Whether the `.ts` source declares a top-level `function main()`.
    ///
    /// Under pure-TS execution semantics, `function main` is an ordinary
    /// declaration (renamed `__ds_main`); the translator always emits an
    /// implicit `fn main`. So this reports only whether a binding named `main`
    /// was declared — it no longer gates the binary entry. AST-level (not a
    /// substring scan), so `main_loop` or a `"fn main"` string literal cannot
    /// trip it.
    #[must_use]
    pub fn has_main(&self, source: &str) -> bool {
        imports::has_main(source)
    }

    /// Symbol-level analysis for one `.ts` file: every declaration's span,
    /// kind, and resolved references (read/write). Powers LSP find-references /
    /// rename with **symbol-level precision** — two same-named bindings in
    /// different scopes are distinct symbols, so renaming one never touches the
    /// other. Returns an owned snapshot that borrows nothing (the parse arena is
    /// released). An empty table means the file failed to parse.
    #[must_use]
    pub fn symbols(&self, source: &str) -> semantic::SymbolTable {
        semantic::analyze_symbols(source)
    }
}

#[cfg(test)]
mod tests;
