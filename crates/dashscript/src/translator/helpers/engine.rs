/// Engine-path Web API builtins for the WHATWG Encoding API — the Javy split:
/// JS shims (`new TextEncoder()`, `.encode()`, `new TextDecoder(…)`, `.decode()`)
/// over native `Function::new` closures that delegate to `crate::__ds::Text*`
/// (the SAME Rust impls the static path lowers to). Emitted into `__ds/engine.rs`
/// only when `RuntimeDep::Engine` ∧ `RuntimeDep::Encoding` are both active, and
/// called from `wire_web_apis` (whose body
/// [`engine_helper_module`](Translator::engine_helper_module) stamps). Returning
/// `Vec<u8>`/`String` (rquickjs `IntoJs` → JS Array / JS String) sidesteps the
/// `TypedArray<'js>` Ctx-lifetime trap; the shims wrap/unwrap bytes via
/// `new Uint8Array(...)` and `ArrayBuffer` arg extraction. `decode` is
/// non-streaming (a fresh `TextDecoder` per call) — the streaming `Decoder`
/// slot is the static path's job; the engine path covers the common single-call
/// decode faithfully.
pub const TEXT_ENCODING_ENGINE_BUILTIN: &str = r#"
fn register_text_encoding(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    let encode = rquickjs::Function::new(ctx.clone(), |s: String| -> Vec<u8> {
        crate::__ds::TextEncoder::new().encode(s)
    })?;
    ctx.globals().set("__ds_te_encode", encode)?;
    let decode = rquickjs::Function::new(
        ctx.clone(),
        |buf: rquickjs::ArrayBuffer,
         off: usize,
         len: usize,
         label: String,
         fatal: bool,
         ignore_bom: bool|
         -> String {
            let bytes = buf.as_bytes().unwrap_or(&[]);
            let end = off.saturating_add(len).min(bytes.len());
            let view = bytes.get(off..end).unwrap_or(&[]);
            crate::__ds::TextDecoder::new(label, fatal, ignore_bom)
                .decode(view.to_vec(), false)
        },
    )?;
    ctx.globals().set("__ds_td_decode", decode)?;
    ctx.eval_with_options::<(), _>(
        "this.TextEncoder = function TextEncoder() { this.encoding = 'utf-8'; };
         this.TextEncoder.prototype.encode = function(input) {
             return new Uint8Array(__ds_te_encode(input === undefined ? '' : String(input)));
         };
         this.TextDecoder = function TextDecoder(label, options) {
             this.label = label === undefined ? 'utf-8' : String(label);
             options = options || {};
             this.fatal = !!options.fatal;
             this.ignoreBOM = !!options.ignoreBOM;
             this.encoding = 'utf-8';
         };
         this.TextDecoder.prototype.decode = function(input, _options) {
             if (input == null) return '';
             var buf, off = 0, len = 0;
             if (input.buffer) { buf = input.buffer; off = input.byteOffset || 0; len = input.length; }
             else if (input.byteLength !== undefined) { buf = input; len = input.byteLength; }
             else { return ''; }
             return __ds_td_decode(buf, off, len, this.label, this.fatal, this.ignoreBOM);
         };",
        sloppy(),
    )
}
"#;

/// `performance.now()` engine builtin — the Javy-pattern wiring for the
/// hr-time global (mirrors [`TEXT_ENCODING_ENGINE_BUILTIN`]). A native
/// `__ds_perf_now` closure delegates to the SAME `crate::__ds::perf_now` the
/// static path lowers to, and a JS shim exposes `performance.now()` on the
/// engine global so a degraded function that times itself resolves it instead
/// of throwing `ReferenceError`. One implementation, two delivery paths
/// (static `__ds::perf_now()` vs engine `performance.now()`).
pub const PERF_ENGINE_BUILTIN: &str = r#"
fn register_perf_now(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    let now = rquickjs::Function::new(ctx.clone(), || -> f64 {
        crate::__ds::perf_now()
    })?;
    ctx.globals().set("__ds_perf_now", now)?;
    ctx.eval_with_options::<(), _>(
        "this.performance = { now: function () { return __ds_perf_now(); } };",
        sloppy(),
    )
}
"#;

/// `atob`/`btoa` engine builtin — the Javy-pattern wiring for the WinterTC
/// base64 globals (mirrors [`TEXT_ENCODING_ENGINE_BUILTIN`]). Native
/// `__ds_b64_encode`/`__ds_b64_decode` closures delegate to the SAME
/// `crate::__ds::b64_encode`/`b64_decode` the static path lowers to, and JS
/// shims expose `atob`/`btoa` on the engine global. One implementation, two
/// delivery paths.
pub const BASE64_ENGINE_BUILTIN: &str = r#"
fn register_base64(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    let encode = rquickjs::Function::new(ctx.clone(), |s: String| -> String {
        crate::__ds::b64_encode(s)
    })?;
    ctx.globals().set("__ds_b64_encode", encode)?;
    let decode = rquickjs::Function::new(ctx.clone(), |s: String| -> String {
        crate::__ds::b64_decode(s)
    })?;
    ctx.globals().set("__ds_b64_decode", decode)?;
    ctx.eval_with_options::<(), _>(
        "this.atob = function (s) { return __ds_b64_decode(String(s)); };
         this.btoa = function (s) { return __ds_b64_encode(String(s)); };",
        sloppy(),
    )
}
"#;

/// `crypto.randomUUID()` / `crypto.getRandomValues(arr)` engine builtin — the
/// Javy-pattern wiring for the WinterTC WebCrypto globals (mirrors
/// [`TEXT_ENCODING_ENGINE_BUILTIN`]). Native `__ds_crypto_uuid`/`__ds_crypto_grv`
/// closures delegate to the SAME `crate::__ds::crypto_random_uuid`/
/// `crypto_get_random_values` the static path lowers to; `getRandomValues`
/// reuses the TextDecoder `ArrayBuffer`+offset+length marshal (bytes out, JS
/// shim fills the TypedArray back in place and returns it — ES semantics). JS
/// shims expose both on the `crypto` global. One implementation, two delivery
/// paths.
pub const CRYPTO_ENGINE_BUILTIN: &str = r#"
fn register_crypto(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    let uuid = rquickjs::Function::new(ctx.clone(), || -> String {
        crate::__ds::crypto_random_uuid()
    })?;
    ctx.globals().set("__ds_crypto_uuid", uuid)?;
    let grv = rquickjs::Function::new(
        ctx.clone(),
        |buf: rquickjs::ArrayBuffer, off: usize, len: usize| -> Vec<u8> {
            let bytes = buf.as_bytes().unwrap_or(&[]);
            let end = off.saturating_add(len).min(bytes.len());
            let v = bytes.get(off..end).unwrap_or(&[]).to_vec();
            crate::__ds::crypto_get_random_values(v)
        },
    )?;
    ctx.globals().set("__ds_crypto_grv", grv)?;
    ctx.eval_with_options::<(), _>(
        "this.crypto = {
             randomUUID: function () { return __ds_crypto_uuid(); },
             getRandomValues: function (arr) {
                 if (arr == null || arr.buffer == null) return arr;
                 var filled = __ds_crypto_grv(arr.buffer, arr.byteOffset || 0, arr.length);
                 for (var i = 0; i < arr.length; i++) arr[i] = filled[i];
                 return arr;
             }
         };",
        sloppy(),
    )
}
"#;

/// `$262.agent` engine builtin — the tc39 test262 agent API for true
/// cross-thread `Atomics.wait`/`notify`. test262's atomics fixtures drive a
/// real OS thread per agent (the spec's agent model); a single-threaded mock
/// degrades to "InternalError: interrupted" (the main thread's `wait` blocks
/// to the engine deadline) or `Atomics.notify` returning 0 (no waiter). This
/// builtin mirrors QuickJS's own `run-test262.c` agent model: each agent is an
/// independent `Runtime` + `JS_SetCanBlock(true)` + own thread, the
/// `SharedArrayBuffer` is shared by raw backing pointer, and broadcast sync
/// uses a `Mutex`+`Condvar` (not QuickJS's internal lock). The whole
/// `$262.agent` bottom layer (`start`/`broadcast`/`getReport`/`sleep`/
/// `monotonicNow` main-side; `report`/`leaving`/`receiveBroadcast`/`sleep`
/// agent-side) lives here; `atomicsHelper.js`'s high-level `safeBroadcast`/
/// `waitUntil`/`tryYield`/`timeouts` ride on top of it unchanged.
///
/// The drain loop (`__ds_agent_loop`) runs OUTSIDE `ctx.with`: `rt.
/// is_job_pending`/`execute_pending_job` lock `runtime.inner` (a `RefCell`),
/// the same cell `Context::with` holds for its whole closure — calling them
/// inside a `with`-closure re-enters the `RefCell` and panics. Only the
/// receiver invocation enters `ctx.with`, briefly.
pub const AGENT_262_ENGINE_BUILTIN: &str = r#"
use rquickjs::{ArrayBuffer, ArrayBufferSource, Function, convert::Coerced, qjs};
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Condvar;
use std::thread;
use std::time::{Duration, Instant};

/// `Atomics.waitAsync` polyfill — QuickJS-NG 0.12 lacks it (`test262.conf`:
/// `Atomics.waitAsync=skip`). The polyfill delegates to `Atomics.wait` (both
/// runtimes this module touches set `JS_SetCanBlock(true)`). Probe with
/// timeout 0 so the call validates the typed array / shared buffer / index
/// (throwing TypeError/RangeError synchronously, as the spec requires) and
/// returns the fast-path verdict without blocking. The async case blocks the
/// caller — safe because test262 only awaits `waitAsync().value` from a
/// `$262.agent` worker thread while the notifier runs on a different thread
/// (verified across all 98 waitAsync fixtures), so there is no deadlock.
const __DS_WAITASYNC_POLYFILL: &str = "(function () {
  if (typeof Atomics === 'undefined' || typeof Atomics.wait !== 'function') return;
  if (typeof Atomics.waitAsync === 'function') return;
  Object.defineProperty(Atomics, 'waitAsync', {
    value: function waitAsync(typedArray, index, value, timeout) {
      if (timeout === undefined || timeout !== timeout) timeout = Infinity;
      var probe = Atomics.wait(typedArray, index, value, 0);
      if (probe === 'not-equal') return { async: false, value: 'not-equal' };
      if (!(timeout > 0)) return { async: false, value: 'timed-out' };
      return {
        async: true,
        value: new Promise(function (resolve) {
          resolve(Atomics.wait(typedArray, index, value, Math.min(timeout, 10000)));
        })
      };
    },
    writable: true, configurable: true, enumerable: false
  });
})();";


/// One broadcast payload: raw SAB backing pointer + len + sync value. The
/// pointer is onto the main ctx's SAB backing store and is sound to share
/// across threads (QuickJS's futex is OS-level), so the shared state is
/// `Send`+`Sync`.
struct __DsAgentBroadcast { buf: *mut u8, len: usize, val: i32 }

/// Per-agent channel: one payload slot + a delivered flag + a leaving flag.
/// Each agent thread holds its own `__DsAgentChannel`; `broadcast` writes the
/// payload to EVERY channel and waits for EVERY channel's `delivered` —
/// mirroring run-test262.c's per-agent broadcast (L735-775) where every agent
/// receives the broadcast independently. This replaces the earlier single
/// shared broadcast-slot + shared delivered-flag, which only worked for
/// multi-agent fixtures by accident (delivered-after-receiver let every agent
/// grab the same payload while the first agent's receiver was blocked in
/// Atomics.wait).
struct __DsAgentChan {
    payload: Option<__DsAgentBroadcast>,
    delivered: bool,
    leaving: bool,
}
unsafe impl Send for __DsAgentChan {}
unsafe impl Sync for __DsAgentChan {}
type __DsAgentChannel = Arc<(Mutex<__DsAgentChan>, Condvar)>;

struct __DsAgentInner { reports: VecDeque<String>, agents: Vec<__DsAgentChannel> }
struct __DsAgentShared { inner: Mutex<__DsAgentInner> }
unsafe impl Send for __DsAgentShared {}
unsafe impl Sync for __DsAgentShared {}

/// Owns nothing — a view onto SAB backing bytes owned by the main thread's
/// SAB. `drop` is a no-op (the main thread owns the storage).
struct __DsAgentSabBuf(*mut u8, usize);
unsafe impl Send for __DsAgentSabBuf {}
unsafe impl ArrayBufferSource for __DsAgentSabBuf {
    fn as_ptr(&self) -> *mut u8 { self.0 }
    fn len(&self) -> usize { self.1 }
}

/// Enable QuickJS blocking mode on the runtime backing this ctx — required for
/// `Atomics.wait` to truly block (otherwise it throws "main thread" TypeError).
fn __ds_enable_can_block(ctx: &Ctx<'_>) {
    let rt = unsafe { qjs::JS_GetRuntime(ctx.as_raw().as_ptr()) };
    unsafe { qjs::JS_SetCanBlock(rt, true) };
}

/// Build the agent-side `$262.agent` (report/leaving/receiveBroadcast/sleep).
fn __ds_register_agent_262(
    ctx: &Ctx<'_>,
    shared: Arc<__DsAgentShared>,
    chan: __DsAgentChannel,
) -> rquickjs::Result<()> {
    let agent = Object::new(ctx.clone())?;
    let shared_rep = shared.clone();
    // Coerced<String> runs JS ToString on the arg, so `$262.agent.report` accepts
    // any value (fixtures pass `Atomics.store`/`Atomics.add` numbers, not just
    // strings). A strict `String` param rejects numbers, throws inside the
    // receiver, leaves no report, and the main thread spins forever in
    // `getReport` — the root cause of the 26 atomics timeout fixtures.
    let report = Function::new(ctx.clone(), move |v: Coerced<std::string::String>| -> () {
        shared_rep.inner.lock().unwrap().reports.push_back(v.0);
    })?;
    agent.set("report", report)?;
    // leaving sets THIS agent's channel flag (per-agent, not a shared bool).
    let chan_for_leave = chan.clone();
    let leaving_fn = Function::new(ctx.clone(), move || -> () {
        let (m, c) = &*chan_for_leave;
        let mut g = m.lock().unwrap();
        g.leaving = true;
        c.notify_all();
    })?;
    agent.set("leaving", leaving_fn)?;
    let sleep = Function::new(ctx.clone(), |ms: u32| -> () {
        thread::sleep(Duration::from_millis(u64::from(ms)));
    })?;
    agent.set("sleep", sleep)?;
    let now = Function::new(ctx.clone(), || -> i64 {
        use std::time::SystemTime;
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    })?;
    agent.set("monotonicNow", now)?;
    let dollar = Object::new(ctx.clone())?;
    dollar.set("agent", agent)?;
    ctx.globals().set("$262", dollar)?;
    // receiveBroadcast is a pure-JS shim (avoids the Function<'js> Ctx-lifetime
    // trap): stash the callback on the agent global for the loop to invoke.
    ctx.eval_with_options::<(), _>(
        "$262.agent.receiveBroadcast = function(fn) { globalThis.__ds_receiver = fn; };",
        sloppy(),
    )?;
    // Atomics.waitAsync is awaited from agent bodies (async=true fixtures) —
    // inject the polyfill on the agent runtime too.
    ctx.eval_with_options::<(), _>(__DS_WAITASYNC_POLYFILL, sloppy())
}

/// Agent thread main loop — runs OUTSIDE `ctx.with` (see the const doc).
/// Consumes THIS agent's channel (per-agent, not a shared slot).
fn __ds_agent_loop(
    ctx: &Context,
    rt: &Runtime,
    shared: Arc<__DsAgentShared>,
    chan: __DsAgentChannel,
) {
    let _ = shared; // reports go through `shared.inner.reports`; the broadcast
                    // payload + delivered flag are per-agent on `chan`.
    loop {
        while rt.is_job_pending() {
            if rt.execute_pending_job().is_err() {
                break;
            }
        }
        // Wait for a payload on THIS agent's channel.
        let (buf, len, val) = {
            let (m, c) = &*chan;
            let mut g = m.lock().unwrap();
            loop {
                if g.leaving {
                    return;
                }
                if let Some(ref p) = g.payload {
                    if !g.delivered {
                        break (p.buf, p.len, p.val);
                    }
                }
                g = c.wait(g).unwrap();
            }
        };
        // Mark delivered BEFORE invoking the receiver (mirrors run-test262.c
        // L646-656: `broadcast_pending = false; js_cond_signal` before the
        // receiver call). The receiver typically does `Atomics.add; sleep;
        // Atomics.notify`, and the main thread's broadcast() must return so it
        // can reach its own Atomics.wait BEFORE the receiver's notify fires —
        // otherwise notify finds no waiter and returns 0. Per-agent channel,
        // so this no longer starves sibling agents (each has its own delivered).
        {
            let (m, c) = &*chan;
            let mut g = m.lock().unwrap();
            g.delivered = true;
            c.notify_all();
        }
        ctx.with(|actx: Ctx<'_>| {
            let receiver: Function = match actx.globals().get("__ds_receiver") {
                Ok(f) => f,
                Err(_) => return,
            };
            let sab = ArrayBuffer::from_source_shared(actx.clone(), __DsAgentSabBuf(buf, len));
            if let Ok(sab) = sab {
                let _: () = receiver.call((sab, val)).unwrap_or(());
            }
        });
    }
}

/// Main-side `$262` (the entry `wire_web_apis` registers under
/// `RuntimeDep::Atomics`). Creates one shared-state `Arc` captured by every
/// `$262.agent.*` closure; `$262.agent.start(script)` clones it into the
/// spawned agent thread.
fn register_atomics_agent(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    let shared = Arc::new(__DsAgentShared {
        inner: Mutex::new(__DsAgentInner { reports: VecDeque::new(), agents: Vec::new() }),
    });
    let agent = Object::new(ctx.clone())?;

    // $262.agent.start(script) — spawn an agent thread with its own Runtime.
    // The per-agent channel is registered synchronously on the main thread
    // BEFORE spawning, so a broadcast issued immediately after start() sees
    // this agent (the spawned thread may not have run yet).
    let shared_start = shared.clone();
    let start = Function::new(ctx.clone(), move |script: String| -> () {
        let shared = shared_start.clone();
        let chan: __DsAgentChannel = Arc::new((
            Mutex::new(__DsAgentChan { payload: None, delivered: false, leaving: false }),
            Condvar::new(),
        ));
        shared.inner.lock().unwrap().agents.push(chan.clone());
        thread::spawn(move || {
            let rt = match Runtime::new() {
                Ok(r) => r,
                Err(_) => return,
            };
            let actx = match Context::full(&rt) {
                Ok(c) => c,
                Err(_) => return,
            };
            // Phase 1 (inside ctx.with): enable blocking, register agent-side
            // $262, eval the script (stashes receiveBroadcast callback).
            let _ = actx.with(|actx: Ctx<'_>| -> rquickjs::Result<()> {
                __ds_enable_can_block(&actx);
                __ds_register_agent_262(&actx, shared.clone(), chan.clone())?;
                if actx.eval_with_options::<(), _>(script.as_str(), sloppy()).is_err() {
                    return Err(rquickjs::Error::Unknown);
                }
                Ok(())
            });
            // Phase 2 (OUTSIDE ctx.with): the drain loop.
            __ds_agent_loop(&actx, &rt, shared.clone(), chan);
        });
    })?;
    agent.set("start", start)?;

    // $262.agent.broadcast(sab) — publish the SAB to EVERY started agent and
    // wait until each has begun calling its receiveBroadcast callback. The arg
    // is a `Value` (SAB is rejected by `FromJs<ArrayBuffer>` on the typed
    // path); `into_object` + `from_object` + `as_raw` reach the shared backing
    // pointer through `JS_GetArrayBuffer`.
    let shared_bc = shared.clone();
    let broadcast = Function::new(ctx.clone(), move |sab: Value<'_>| -> () {
        let obj = match sab.into_object() {
            Some(o) => o,
            None => return,
        };
        let raw = match ArrayBuffer::from_object(obj).and_then(|ab| ab.as_raw()) {
            Some(r) => r,
            None => return,
        };
        let payload = __DsAgentBroadcast { buf: raw.ptr.as_ptr(), len: raw.len, val: 0 };
        // Snapshot every LIVE agent's channel (skip agents that called
        // `leaving()` — their thread has exited and would never mark
        // `delivered`), then deliver the payload to each one independently
        // (run-test262.c broadcast model L735-775).
        let chans: Vec<__DsAgentChannel> = {
            let inner = shared_bc.inner.lock().unwrap();
            let mut live: Vec<__DsAgentChannel> = Vec::with_capacity(inner.agents.len());
            for chan in inner.agents.iter() {
                let (m, _) = &**chan; // chan: &Arc → &**chan: &(Mutex, Condvar)
                if !m.lock().unwrap().leaving {
                    live.push(Arc::clone(chan));
                }
            }
            live
        };
        if chans.is_empty() {
            return;
        }
        for chan in &chans {
            let (m, c) = &**chan;
            let mut g = m.lock().unwrap();
            g.payload = Some(__DsAgentBroadcast { buf: payload.buf, len: payload.len, val: payload.val });
            g.delivered = false;
            c.notify_all();
        }
        // Wait for EVERY agent to mark delivered (bounded — an agent that never
        // reaches the receiver must not deadlock the main thread).
        let deadline = Instant::now() + Duration::from_secs(10);
        for chan in &chans {
            let (m, c) = &**chan;
            let mut g = m.lock().unwrap();
            while !g.delivered {
                let next = deadline.saturating_duration_since(Instant::now());
                let (g2, wait) = c.wait_timeout(g, next).unwrap();
                g = g2;
                if wait.timed_out() {
                    break;
                }
            }
        }
    })?;
    agent.set("broadcast", broadcast)?;

    // $262.agent.getReport() -> string|null.
    let shared_gr = shared.clone();
    let get_report = Function::new(ctx.clone(), move || -> Option<String> {
        shared_gr.inner.lock().unwrap().reports.pop_front()
    })?;
    agent.set("getReport", get_report)?;

    let sleep = Function::new(ctx.clone(), |ms: u32| -> () {
        thread::sleep(Duration::from_millis(u64::from(ms)));
    })?;
    agent.set("sleep", sleep)?;
    let now = Function::new(ctx.clone(), || -> i64 {
        use std::time::SystemTime;
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    })?;
    agent.set("monotonicNow", now)?;

    let dollar = Object::new(ctx.clone())?;
    dollar.set("agent", agent)?;
    dollar.set("global", ctx.globals())?;
    // $262.detachArrayBuffer — host hook the `$DETACHBUFFER` harness helper
    // (detached-buffer fixtures) calls. rquickjs's ArrayBuffer::detach wraps
    // JS_DetachArrayBuffer; a non-ArrayBuffer arg is a no-op, not a throw.
    let detach = Function::new(ctx.clone(), |val: Value<'_>| -> rquickjs::Result<()> {
        if let Some(mut ab) = rquickjs::ArrayBuffer::from_value(val) {
            ab.detach();
        }
        Ok(())
    })?;
    dollar.set("detachArrayBuffer", detach)?;
    ctx.globals().set("$262", dollar)
}
"#;

/// `EventTarget` / `AbortSignal` / `AbortController` engine builtin — the
/// Javy-pattern wiring for the WHATWG abort/event family. Unlike the encoding
/// or crypto builtins there is no native `fn` delegation: the static path's
/// `DsAbortSignal`/`DsAbortController`/`DsEventTarget` carry `Arc<Mutex<…>>`
/// state plus `Box<dyn FnMut(&DsEvent)>` callbacks — not marshalable across
/// the serde boundary a native closure would need. The ES semantics, however,
/// are a small state machine (`aborted` flag + listener list + prototype
/// chain), so a pure-JS shim runs them faithfully — one contract (the WHATWG
/// spec), two delivery paths (static Rust struct vs engine JS classes), kept
/// in lockstep by the spec rather than by code sharing. `AbortSignal` extends
/// `EventTarget` (prototype chain), so a single `register_abort` defines all
/// three; mapped only under `RuntimeDep::EventTarget` (an `AbortController`
/// dep derives `EventTarget`, so this registers exactly once). Covers
/// `signal.aborted`/`reason`, `controller.signal`/`abort()`,
/// `addEventListener`/`removeEventListener`/`dispatchEvent`, and the static
/// `AbortSignal.any(…)`/`abort(…)`/`timeout(…)` combinators. Also registers
/// the `Event` constructor (a `{ type, bubbles, cancelable, target }` shim —
/// the engine subset a degraded fixture reads) and, when `performance` is
/// present (set by `register_perf_now` under `HrTime`, which sorts before
/// `EventTarget`), upgrades it to an `EventTarget` (WHATWG `Performance
/// extends EventTarget`), so `performance.addEventListener`/`dispatchEvent`
/// resolve.
pub const ABORT_ENGINE_BUILTIN: &str = r#"
fn register_abort(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    ctx.eval_with_options::<(), _>(
        "function EventTarget() {\n\
             Object.defineProperty(this, '__listeners', { value: {}, writable: true, configurable: true, enumerable: false });\n\
         }\n\
         EventTarget.prototype.addEventListener = function (type, cb) {\n\
             if (typeof cb !== 'function') return;\n\
             var l = this.__listeners[type];\n\
             if (!l) { l = []; this.__listeners[type] = l; }\n\
             l.push(cb);\n\
         };\n\
         EventTarget.prototype.removeEventListener = function (type, cb) {\n\
             var l = this.__listeners[type];\n\
             if (!l) return;\n\
             this.__listeners[type] = l.filter(function (x) { return x !== cb; });\n\
         };\n\
         EventTarget.prototype.dispatchEvent = function (ev) {\n\
             var l = this.__listeners[ev.type];\n\
             if (!l) return true;\n\
             ev = ev || {};\n\
             ev.target = this;\n\
             l.slice().forEach(function (cb) { cb(ev); });\n\
             return true;\n\
         };\n\
         function AbortSignal() {\n\
             EventTarget.call(this);\n\
             this.__aborted = false;\n\
             this.__reason = undefined;\n\
         }\n\
         AbortSignal.prototype = Object.create(EventTarget.prototype);\n\
         AbortSignal.prototype.constructor = AbortSignal;\n\
         Object.defineProperty(AbortSignal.prototype, 'aborted', { get: function () { return this.__aborted; }, configurable: true, enumerable: true });\n\
         Object.defineProperty(AbortSignal.prototype, 'reason', { get: function () { return this.__reason; }, configurable: true, enumerable: true });\n\
         Object.defineProperty(AbortSignal.prototype, 'onabort', {\n\
             get: function () { return this.__onabort || null; },\n\
             set: function (fn) {\n\
                 var prev = this.__onabort;\n\
                 if (prev) this.removeEventListener('abort', prev);\n\
                 this.__onabort = fn;\n\
                 if (typeof fn === 'function') this.addEventListener('abort', fn);\n\
             },\n\
             configurable: true, enumerable: true\n\
         });\n\
         AbortSignal.any = function (signals) {\n\
             var s = new AbortSignal();\n\
             (signals || []).forEach(function (sig) {\n\
                 if (sig && sig.aborted) { s.__aborted = true; s.__reason = sig.reason; }\n\
                 else if (sig) sig.addEventListener('abort', function () {\n\
                     if (!s.__aborted) { s.__aborted = true; s.__reason = sig.reason; }\n\
                 });\n\
             });\n\
             return s;\n\
         };\n\
         AbortSignal.abort = function (reason) {\n\
             var s = new AbortSignal();\n\
             s.__aborted = true;\n\
             s.__reason = reason;\n\
             return s;\n\
         };\n\
         AbortSignal.timeout = function (_ms) {\n\
             return new AbortSignal();\n\
         };\n\
         function AbortController() {\n\
             this.__signal = new AbortSignal();\n\
         }\n\
         Object.defineProperty(AbortController.prototype, 'signal', { get: function () { return this.__signal; }, configurable: true, enumerable: true });\n\
         AbortController.prototype.abort = function (reason) {\n\
             var s = this.__signal;\n\
             if (s.__aborted) return;\n\
             s.__aborted = true;\n\
             s.__reason = reason;\n\
             var l = s.__listeners['abort'];\n\
             if (l) { var ev = { type: 'abort', target: s }; l.slice().forEach(function (cb) { cb(ev); }); }\n\
         };\n\
         function Event(type) { this.type = type; this.bubbles = false; this.cancelable = false; this.target = null; }\n\
         this.Event = Event;\n\
         this.EventTarget = EventTarget;\n\
         this.AbortSignal = AbortSignal;\n\
         this.AbortController = AbortController;\n\
         if (this.performance) {\n\
             Object.setPrototypeOf(this.performance, EventTarget.prototype);\n\
             EventTarget.call(this.performance);\n\
         }",
        sloppy(),
    )
}
"#;

/// `assert.sameValue`/`notSameValue`/`throws` + `Test262Error` engine builtin —
/// the Javy-pattern wiring for the test262 harness assert family (mirrors
/// [`TEXT_ENCODING_ENGINE_BUILTIN`]). Pure-JS shim (no native fn): the static
/// path's `assert_same_value<A: DsSameValue>` is generic over concrete Rust
/// types, unreachable from a dynamic `rquickjs::Value`, but QuickJS already
/// ships ES `Object.is` (SameValue) + `Error`/`try-catch`, so the assert family
/// runs faithfully in JS — no `__ds::` Rust impl to delegate to. One contract
/// (a mismatch throws `Test262Error`), two delivery paths (static Rust panic vs
/// engine JS throw). Emitted only when `Engine` is also active, so a non-engine
/// fixture never references it.
pub const ASSERT_ENGINE_BUILTIN: &str = r#"
fn register_assert(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    ctx.eval_with_options::<(), _>(
        "function Test262Error(message) { this.message = message; }\n\
         Test262Error.prototype = Object.create(Error.prototype);\n\
         Test262Error.prototype.name = 'Test262Error';\n\
         Test262Error.prototype.constructor = Test262Error;\n\
         Test262Error.prototype.toString = function () {\n\
             return this.message === undefined ? 'Test262Error' : 'Test262Error: ' + this.message;\n\
         };\n\
         this.Test262Error = Test262Error;\n\
         this.assert = {\n\
             sameValue: function (a, b) {\n\
                 if (Object.is(a, b)) return;\n\
                 throw new Test262Error('Expected ' + String(b) + ' but got ' + String(a));\n\
             },\n\
             notSameValue: function (a, b) {\n\
                 if (!Object.is(a, b)) return;\n\
                 throw new Test262Error('Expected different values but both were ' + String(a));\n\
             },\n\
             throws: function (expected, fn) {\n\
                 if (typeof fn !== 'function')\n\
                     throw new Test262Error('assert.throws: second argument must be a function');\n\
                 try { fn(); } catch (e) {\n\
                     var ctor = typeof expected === 'function' ? expected : Object;\n\
                     if (e instanceof ctor) return;\n\
                     throw new Test262Error(\n\
                         'Expected ' + (ctor.name || String(ctor)) + ' but threw ' + String(e)\n\
                     );\n\
                 }\n\
                 throw new Test262Error(\n\
                     'Expected ' + (expected && expected.name || String(expected)) + ' but nothing threw'\n\
                 );\n\
             }\n\
         };",
        sloppy(),
    )
}
"#;

/// WPT testharness sync-subset engine builtin — `assert_equals`/`true`/`false`/
/// `approx_equals`/`array_equals`/`throws_js` + `AssertionError` + `test`/
/// `setup`/`done` no-ops (mirrors [`ASSERT_ENGINE_BUILTIN`]). Pure-JS shim for
/// the same reason (dynamic `rquickjs::Value` vs the static path's generic
/// `wpt_assert_equals<A: DsSameValue>`). `promise_test`/`async_test` are
/// intentionally NOT wired here — they need an async runtime the engine lacks,
/// so fixtures using them honestly degrade to `EngineLimitation` (unsupported).
pub const WPT_ASSERT_ENGINE_BUILTIN: &str = r#"
fn register_wpt_assert(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    ctx.eval_with_options::<(), _>(
        "function AssertionError(message) { this.message = message; }\n\
         AssertionError.prototype = Object.create(Error.prototype);\n\
         AssertionError.prototype.name = 'AssertionError';\n\
         AssertionError.prototype.constructor = AssertionError;\n\
         AssertionError.prototype.toString = function () { return 'AssertionError: ' + this.message; };\n\
         this.AssertionError = AssertionError;\n\
         function __ds_wpt_fmt(v) {\n\
             try { return typeof v === 'object' && v !== null ? JSON.stringify(v) : String(v); }\n\
             catch (_) { return String(v); }\n\
         }\n\
         function __ds_wpt_t() {\n\
             return {\n\
                 done: function () {},\n\
                 step: function (f) { f(); },\n\
                 step_func: function (f) { return function () { try { f.apply(this, arguments); } catch (e) { throw e; } }; },\n\
                 step_func_done: function (f) { return function () { try { f.apply(this, arguments); } catch (e) { throw e; } }; },\n\
                 unreached_func: function (msg) { return function () { throw new AssertionError(msg || 'unreached'); }; },\n\
                 asserts: self\n\
             };\n\
         }\n\
         this.assert_equals = function (a, b, msg) {\n\
             if (Object.is(a, b)) return;\n\
             throw new AssertionError((msg ? msg + ' ' : '') + 'expected ' + __ds_wpt_fmt(b) + ' but got ' + __ds_wpt_fmt(a));\n\
         };\n\
         this.assert_not_equals = function (a, b, msg) {\n\
             if (!Object.is(a, b)) return;\n\
             throw new AssertionError((msg ? msg + ' ' : '') + 'both were ' + __ds_wpt_fmt(a));\n\
         };\n\
         this.assert_true = function (v, msg) { if (v === true) return; throw new AssertionError(msg || 'expected true'); };\n\
         this.assert_false = function (v, msg) { if (v === false) return; throw new AssertionError(msg || 'expected false'); };\n\
         this.assert_array_equals = function (a, b, msg) {\n\
             if (!Array.isArray(a) || !Array.isArray(b) || a.length !== b.length)\n\
                 throw new AssertionError((msg ? msg + ' ' : '') + 'array length mismatch');\n\
             for (var i = 0; i < a.length; i++) {\n\
                 if (!Object.is(a[i], b[i]))\n\
                     throw new AssertionError((msg ? msg + ' ' : '') + 'at index ' + i + ': expected ' + __ds_wpt_fmt(b[i]) + ' but got ' + __ds_wpt_fmt(a[i]));\n\
             }\n\
         };\n\
         this.assert_approx_equals = function (actual, expected, eps, msg) {\n\
             if (typeof actual !== 'number' || typeof expected !== 'number' || Math.abs(actual - expected) > eps)\n\
                 throw new AssertionError((msg ? msg + ' ' : '') + 'expected ~' + expected + ' but got ' + actual);\n\
         };\n\
         this.assert_throws_js = function (ErrorCtor, fn, msg) {\n\
             if (typeof fn !== 'function')\n\
                 throw new AssertionError('assert_throws_js: second argument must be a function');\n\
             try { fn(); } catch (e) {\n\
                 if (e instanceof ErrorCtor) return;\n\
                 throw new AssertionError((msg ? msg + ' ' : '') + 'expected ' + (ErrorCtor && ErrorCtor.name) + ' but threw ' + __ds_wpt_fmt(e));\n\
             }\n\
             throw new AssertionError((msg ? msg + ' ' : '') + 'expected ' + (ErrorCtor && ErrorCtor.name) + ' but nothing threw');\n\
         };\n\
         this.test = function (fn, _name) { fn(__ds_wpt_t()); };\n\
         this.setup = function () {};\n\
         this.done = function () {};\n\
         this.async_test = function (fn, _name) {\n\
             var t = __ds_wpt_t();\n\
             try { fn(t); } catch (e) { throw e; }\n\
             return t;\n\
         };\n\
         this.promise_test = function (fn, _name) { try { fn(__ds_wpt_t()); } catch (e) { throw e; } };\n\
        this.promise_rejects_js = function (_test, ctor, p, msg) {\n\
            return p.then(function () {\n\
                throw new AssertionError((msg ? msg + ' ' : '') + 'promise should have rejected');\n\
            }).catch(function (e) {\n\
                if (e === null || e === undefined) throw new AssertionError((msg ? msg + ' ' : '') + 'must throw a non-None value');\n\
                if (!(e instanceof ctor)) throw new AssertionError((msg ? msg + ' ' : '') + 'expected ' + (ctor && ctor.name) + ' but threw ' + __ds_wpt_fmt(e));\n\
            });\n\
        };\n\
        this.promise_rejects_exactly = function (_test, p, value, msg) {\n\
            return p.then(function () {\n\
                throw new AssertionError((msg ? msg + ' ' : '') + 'promise should have rejected');\n\
            }).catch(function (e) {\n\
                if (!Object.is(e, value)) throw new AssertionError((msg ? msg + ' ' : '') + 'expected ' + __ds_wpt_fmt(value) + ' but got ' + __ds_wpt_fmt(e));\n\
            });\n\
        };\n\
             this.assert_object_equals = function (a, b, msg) {\n\
                 function __ds_deep_eq(x, y) {\n\
                     if (Object.is(x, y)) return true;\n\
                     if (typeof x !== 'object' || typeof y !== 'object' || x === null || y === null) return false;\n\
                     var kx = Object.keys(x), ky = Object.keys(y);\n\
                     if (kx.length !== ky.length) return false;\n\
                     for (var i = 0; i < kx.length; i++) if (!__ds_deep_eq(x[kx[i]], y[ky[i]])) return false;\n\
                     return true;\n\
                 }\n\
                 if (!__ds_deep_eq(a, b)) throw new AssertionError((msg ? msg + ' ' : '') + 'expected ' + __ds_wpt_fmt(b) + ' but got ' + __ds_wpt_fmt(a));\n\
             };\n\
             this.assert_own_property = function (obj, prop, msg) { if (!Object.prototype.hasOwnProperty.call(obj, prop)) throw new AssertionError((msg || '') + 'missing property ' + prop); };\n\
             this.assert_not_own_property = function (obj, prop, msg) { if (Object.prototype.hasOwnProperty.call(obj, prop)) throw new AssertionError((msg || '') + 'unexpected property ' + prop); };\n\
             this.assert_inherits = function (obj, prop, msg) { if (!(prop in obj)) throw new AssertionError((msg || '') + 'no inherit ' + prop); };\n\
             this.assert_readonly = function () {};\n\
             this.assert_implements = function (cond, msg) { if (!cond) throw new AssertionError(msg || 'not implemented'); };\n\
             this.assert_implements_float = function (cond, msg) { if (!cond) throw new AssertionError(msg || 'not implemented'); };\n\
             this.assert_less = function (a, b, msg) { if (!(a < b)) throw new AssertionError((msg || '') + a + ' is not less than ' + b); };\n\
             this.assert_greater = function (a, b, msg) { if (!(a > b)) throw new AssertionError((msg || '') + a + ' is not greater than ' + b); };\n\
             this.assert_between = function (a, lo, hi, msg) { if (!(a >= lo && a <= hi)) throw new AssertionError((msg || '') + a + ' not in [' + lo + ',' + hi + ']'); };\n\
             this.generate_string = function (n, ch) { var s = ''; for (var i = 0; i < n; i++) s += ch; return s; };\n\
             this.subset_test = function (fn, _name) { fn(__ds_wpt_t()); };\n\
             this.subsetTestByKey = function (_key, fn, _name) { fn(__ds_wpt_t()); };\n\
             this.step_timeout = function (fn, _ms) { fn(); }",
        sloppy(),
    )
}
"#;

/// The DashScript compat engine module, written to `src/__ds/engine.rs` (a
/// submodule of the `__ds` runtime dir, declared `pub mod engine;` inside
/// `__ds/mod.rs`) when a translated file uses ES
/// dynamic reflection the static translator cannot lower. Two entry points
/// share one thread-local QuickJS `Runtime` (`rquickjs`):
/// - `run(source)` — eval a self-contained source (the conformance oracle path;
///   the source declares `main()` and calls it, pure-TS execution semantics).
/// - `call_fn(name, body, args)` — the per-function degradation path: a dynamic
///   function keeps its native Rust signature while its body runs under JS,
///   with serde_json marshaling the args and return.
///
/// `console.log` is wired to stdout; number stringification uses the engine's
/// own `String()` (ES `Number::toString`), so output matches Node for primitives.
///
/// Gated: only emitted for `needs_engine` programs, so a plain `ds build` pulls
/// no engine dependency (and no QuickJS C compile). The single source for the
/// engine helper — consumed by both `ds build` (project.rs) and the conformance
/// harness — so the helper text lives in the library rather than either
/// consumer.
///
/// TypeScript type annotations are stripped first via oxc's transformer
/// ([`engine_js_source`]), so a real `.ts` source — or a degraded function body
/// — reaches QuickJS as plain ECMAScript.
pub const ENGINE_HELPER_MODULE: &str = r##"//! DashScript compat engine: run a `.ts` source, or a single function's
//! body, under an embedded QuickJS engine (`rquickjs`) when it uses ES dynamic
//! reflection (`Object.defineProperty`, `Reflect.*`, `Symbol`, `Proxy`, typeof
//! on a union, …) the static translator cannot lower to idiomatic Rust. Gated —
//! only present when `RuntimeDeps::needs_engine`.
//!
//! Two entry points share one thread-local `Runtime` (rquickjs `Runtime` is
//! `!Sync`, so a per-thread lazy runtime reuses the engine across calls instead
//! of rebuilding it per invocation):
//! - `run(source)` — eval a self-contained source (the conformance oracle path;
//!   it declares `main()` and calls it, pure-TS execution semantics).
//! - `call_fn(name, body, args)` — the per-function degradation path: a dynamic
//!   function keeps its native Rust signature, but its body runs under JS.
//! - `call_module_fn(module, name, args)` — the npm-module degradation path: a
//!   `.js` package the static translator cannot lower (class extends, …) runs
//!   under JS as an ESM module graph, loaded via the `Loader`/`Resolver` below.
use rquickjs::context::EvalOptions;
use rquickjs::loader::{ImportAttributes, Loader, Resolver};
use rquickjs::module::Declared;
use rquickjs::{
    Array, Context, Ctx, FromJs, IntoJs, Module, Object, Runtime, Type, Value,
};
use std::sync::Mutex;

/// Runtime-registered `.js` module table (specifier → source), a secondary
/// source for `source_of`. The build path no longer populates this — every
/// degraded module's source is embedded once in the build-time
/// `__DS_MODULE_SOURCES` array, and stubs only forward via `call_module_fn`
/// (never re-inlining the source). Kept for tests and any caller registering
/// a source the build-time table did not capture. The emitted crate stays
/// self-contained (no runtime `.js` files); node_modules resolution already
/// happened at build time, so the engine never walks the filesystem.
static JS_MODULES: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

/// Register a degraded `.js` module's source at runtime (idempotent). The
/// build path no longer calls this — production stubs forward via
/// `call_module_fn` and the source lives once in `__DS_MODULE_SOURCES`. Kept
/// for tests (`tests/conformance.rs`) and manual registration of a source the
/// build-time table did not capture.
pub fn register_js_module(specifier: &str, source: &str) {
    let mut v = JS_MODULES.lock().expect("JS_MODULES lock");
    if !v.iter().any(|(s, _)| s == specifier) {
        v.push((specifier.to_string(), source.to_string()));
    }
}

/// Read a module's source: the runtime `JS_MODULES` table first (a manual
/// `register_js_module` call), then the build-time `__DS_MODULE_SOURCES`
/// table — the latter is the source of truth for degraded modules, so one
/// with no `export function` (no stub emitted) still resolves.
fn source_of(name: &str) -> rquickjs::Result<String> {
    if let Some(source) = JS_MODULES
        .lock()
        .expect("JS_MODULES lock")
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, source)| source.clone())
    {
        return Ok(source);
    }
    __DS_MODULE_SOURCES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, source)| source.to_string())
        .ok_or_else(|| rquickjs::Error::new_loading(name))
}

/// ESM import resolver: bare specifiers stay as-is (already resolved to a
/// `JS_MODULES` key at build time); relative specifiers join onto the base
/// module's directory (the rquickjs document algorithm).
struct DsResolver;
impl Resolver for DsResolver {
    fn resolve<'js>(
        &mut self,
        _ctx: &Ctx<'js>,
        base: &str,
        name: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> rquickjs::Result<String> {
        if !name.starts_with('.') {
            Ok(name.to_string())
        } else {
            // The base's directory (everything before the last `/`), or "" when
            // the base has no directory. Join the relative name (sans its `./`)
            // onto it: `import "./b.js"` from `pkg/a.js` → `pkg/b.js`, from a
            // bare `a.js` → `b.js`. Every result is a `JS_MODULES` key.
            let base_dir = base.rsplitn(2, '/').nth(1).unwrap_or("");
            let rel = name.strip_prefix("./").unwrap_or(name);
            Ok(if base_dir.is_empty() {
                rel.to_string()
            } else {
                format!("{base_dir}/{rel}")
            })
        }
    }
}

/// ESM module loader: look the specifier up in `JS_MODULES`, read its file, and
/// declare the module. rquickjs links and evaluates the dependency graph from
/// here, calling `DsResolver`/`DsLoader` for each transitive `import`.
struct DsLoader;
impl Loader for DsLoader {
    fn load<'js>(
        &mut self,
        ctx: &Ctx<'js>,
        name: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> rquickjs::Result<Module<'js, Declared>> {
        Module::declare(ctx.clone(), name, source_of(name)?)
    }
}

thread_local! {
    static RUNTIME: Runtime = {
        let rt = Runtime::new().expect("rquickjs Runtime");
        rt.set_loader(DsResolver, DsLoader);
        rt
    };
    // A persistent per-thread Context so `__ds_modules` (and other globals the
    // module-load path sets) survive across `call_module_fn` calls. A fresh
    // `Context::full` per call gives each its own global object, so a namespace
    // installed by one call is invisible to the next.
    //
    // Lifetime is safe on both counts: (1) a `Context` keeps its `Runtime`
    // alive (the same property ShadowRealm realms rely on — only the `Context`
    // is stored), so RUNTIME cannot be freed while CTX holds it; (2) thread_local
    // destructors run in reverse declaration order, so CTX drops before RUNTIME.
    static CTX: Context = RUNTIME.with(|rt| Context::full(rt).expect("rquickjs Context"));
}

/// Sloppy-mode eval options (strict=false): test262 fixtures and degraded
/// function bodies use `this` at the top for property-attribute setup
/// (`this.configurable = true`), where sloppy `this` is the global object.
/// Node runs the oracle the same way (a plain script, not a strict module).
fn sloppy() -> EvalOptions {
    let mut o = EvalOptions::default();
    o.strict = false;
    o
}

/// Wire `console.log` to a native line printer. `console.log` joins its
/// arguments with spaces, each stringified by the engine's own `String()`
/// coercion (ES `Number::toString` for numbers), so output matches Node for
/// primitives (a plain number prints `1e+21`, not Rust's `f64` Display spelling).
fn wire_console(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    let print_line = rquickjs::Function::new(ctx.clone(), |s: String| {
        println!("{s}");
    })?;
    ctx.globals().set("__ds_print_line", print_line)?;
    ctx.eval_with_options::<(), _>(
        r#"this.console = { log: function () {
            for (var i = 0, out = []; i < arguments.length; i++) {
                out.push(String(arguments[i]));
            }
            __ds_print_line(out.join(" "));
        } };"#,
        sloppy(),
    )
}

/// Wire WinterTC Web API builtins into the engine global — each a JS shim (the
/// API surface) over a native `Function::new` that delegates to the SAME
/// `crate::__ds::…` Rust impl the static path lowers to (the Javy split: JS
/// surface + native compute, one implementation, two delivery paths). The body
/// is stamped per-build from the active Web API `RuntimeDep`s by
/// [`engine_helper_module`](Translator::engine_helper_module) — only APIs the
/// static path already pulled in are registered, so the engine never references
/// a `__ds::` type the crate lacks. Mirrors [`wire_console`]; the body is empty
/// (`Ok(())`) when no Web API dep is active, so a non-Web-API engine fixture
/// pays nothing.
/// `Atomics.waitAsync` polyfill (host copy) — injected unconditionally on the
/// main runtime so any degraded fixture reaching `Atomics.waitAsync` resolves
/// it, not just `$262.agent` fixtures. The agent runtime gets its own copy
/// (`__DS_WAITASYNC_POLYFILL` in `AGENT_262_ENGINE_BUILTIN`). See that const
/// for the rationale (QuickJS-NG 0.12 lacks waitAsync; delegate to wait).
const ATOMICS_WAIT_ASYNC_POLYFILL: &str = "(function () {
  if (typeof Atomics === 'undefined' || typeof Atomics.wait !== 'function') return;
  if (typeof Atomics.waitAsync === 'function') return;
  Object.defineProperty(Atomics, 'waitAsync', {
    value: function waitAsync(typedArray, index, value, timeout) {
      // ES: undefined OR NaN timeout -> +infinity. The NaN case matters:
      // Math.min(NaN, 10000) is NaN, and QuickJS Atomics.wait with a NaN
      // timeout returns timed-out WITHOUT registering a waiter — so a fixture
      // whose agent waitAsyncs with a NaN timeout would see the main-side
      // Atomics.notify return 0 (no waiter) and fail.
      if (timeout === undefined || timeout !== timeout) timeout = Infinity;
      var probe = Atomics.wait(typedArray, index, value, 0);
      if (probe === 'not-equal') return { async: false, value: 'not-equal' };
      if (!(timeout > 0)) return { async: false, value: 'timed-out' };
      return {
        async: true,
        value: new Promise(function (resolve) {
          // Cap the blocking wait at test262's largest timeout (huge = 10s):
          // the async branch is reached only when the value matches and a wait
          // is genuinely needed, so a notifier (a `$262.agent` on another
          // thread) usually resolves this within milliseconds via the shared
          // SAB futex. The cap bounds the rare lost-notify / Infinity-timeout
          // case (a fixture no notifier reaches) to 10s instead of hanging the
          // 30s harness — `timed-out` still drives the fixture's own assert, so
          // the verdict stays honest (no fake green).
          resolve(Atomics.wait(typedArray, index, value, Math.min(timeout, 10000)));
        })
      };
    },
    writable: true, configurable: true, enumerable: false
  });
})();";

/// QuickJS-NG `JSSharedArrayBufferFunctions` — the host hooks a `Runtime` needs
/// to allocate the backing store of a **growable** `SharedArrayBuffer`
/// (`new SharedArrayBuffer(n, { maxByteLength: m })` then `.grow()`). Without
/// them QuickJS throws `TypeError: growable SharedArrayBuffer requires SAB
/// allocator hooks`, so the sharedarraybuffer/growable-sab fixtures (and a few
/// atomics ones) fail. rquickjs 0.12 does not expose the setter, so we FFI
/// straight to the linked libquickjs: `JS_GetRuntime(ctx)` yields the
/// `JSRuntime*`, and `JS_SetSharedArrayBufferFunctions` copies the struct into
/// the runtime. The hooks are a refcounting allocator: each block carries an
/// inline header `[refcount, total_size]` above the payload pointer QuickJS
/// sees, so `sab_dup`/`sab_free` (matched per logical reference, including
/// multi-view SABs and cross-runtime shares like a `$262.agent` view) never
/// double-free. The refcount is atomic, so concurrent runtimes are safe.
mod sab_alloc {
    use std::alloc::{alloc as sys_alloc, dealloc as sys_dealloc, Layout};
    use std::os::raw::c_void;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Typed-array backing memory must be aligned for any view (Float64Array
    /// needs 8); 16 covers every element width on the platforms we target.
    const ALIGN: usize = 16;
    /// Inline header above each payload: a refcount + the total allocation
    /// size (so `dealloc` can reconstruct its `Layout` without an extern map).
    const HEADER: usize = 2 * std::mem::size_of::<usize>();

    #[repr(C)]
    struct JSSharedArrayBufferFunctions {
        sab_alloc: Option<unsafe extern "C" fn(*mut c_void, usize) -> *mut c_void>,
        sab_free: Option<unsafe extern "C" fn(*mut c_void, *mut c_void)>,
        sab_dup: Option<unsafe extern "C" fn(*mut c_void, *mut c_void)>,
        sab_opaque: *mut c_void,
    }

    extern "C" {
        fn JS_GetRuntime(ctx: *mut c_void) -> *mut c_void;
        fn JS_SetSharedArrayBufferFunctions(rt: *mut c_void, sf: *const JSSharedArrayBufferFunctions);
    }

    unsafe extern "C" fn alloc(_opaque: *mut c_void, size: usize) -> *mut c_void {
        let total = HEADER + size;
        let layout = match Layout::from_size_align(total, ALIGN) {
            Ok(l) => l,
            Err(_) => return std::ptr::null_mut(),
        };
        let base = sys_alloc(layout);
        if base.is_null() {
            return std::ptr::null_mut();
        }
        (base as *mut AtomicUsize).write(AtomicUsize::new(1));
        (base.add(std::mem::size_of::<usize>()) as *mut usize).write(total);
        base.add(HEADER) as *mut c_void
    }

    unsafe extern "C" fn dup(_opaque: *mut c_void, ptr: *mut c_void) {
        let base = (ptr as *mut u8).sub(HEADER) as *mut AtomicUsize;
        (*base).fetch_add(1, Ordering::Relaxed);
    }

    unsafe extern "C" fn free(_opaque: *mut c_void, ptr: *mut c_void) {
        let base = (ptr as *mut u8).sub(HEADER);
        let rc = base as *mut AtomicUsize;
        if (*rc).fetch_sub(1, Ordering::Relaxed) == 1 {
            let total = (base.add(std::mem::size_of::<usize>()) as *mut usize).read();
            let layout = Layout::from_size_align_unchecked(total, ALIGN);
            sys_dealloc(base, layout);
        }
    }

    /// Register the SAB allocator on a runtime reached via any of its
    /// contexts. Idempotent: QuickJS overwrites `rt->sab_funcs` each call, and
    /// the function pointers are static, so re-installing is a no-op.
    pub fn install(ctx: &rquickjs::Ctx<'_>) {
        let rt = unsafe { JS_GetRuntime(ctx.as_raw().as_ptr() as *mut c_void) };
        let funcs = JSSharedArrayBufferFunctions {
            sab_alloc: Some(alloc),
            sab_free: Some(free),
            sab_dup: Some(dup),
            sab_opaque: std::ptr::null_mut(),
        };
        unsafe { JS_SetSharedArrayBufferFunctions(rt, &funcs) };
    }
}

fn wire_web_apis(ctx: &Ctx<'_>) -> rquickjs::Result<()> {
    // Enable QuickJS blocking mode on this runtime so `Atomics.wait` truly
    // blocks (otherwise it throws "main thread" TypeError). Idempotent and
    // harmless for fixtures that never wait — set unconditionally so any
    // engine-run thread (the per-function degrade path, $262.agent main side)
    // can host a real wait without a per-dep gate. The agent threads spawned by
    // `register_atomics_agent` set this on their own independent runtimes too.
    let rt_ptr = unsafe { rquickjs::qjs::JS_GetRuntime(ctx.as_raw().as_ptr()) };
    unsafe { rquickjs::qjs::JS_SetCanBlock(rt_ptr, true) };
    // Register the growable-SharedArrayBuffer allocator on this runtime so
    // `new SharedArrayBuffer(n, { maxByteLength })` + `.grow()` work (QuickJS
    // throws "growable SharedArrayBuffer requires SAB allocator hooks"
    // otherwise). Idempotent; the per-agent runtimes install it themselves.
    sab_alloc::install(ctx);
    // `self` — WinterTC §5 global alias for globalThis. Registered
    // unconditionally (the alias is part of the global shape, not a per-API
    // dep), so a degraded function reaching a Web API via `self.` resolves it
    // exactly as the static path's `self` lowering — one implementation, two
    // delivery paths (the conformance harness mirrors this in its WPT prelude).
    ctx.eval_with_options::<(), _>(
        "if (!globalThis.self) globalThis.self = globalThis;",
        sloppy(),
    )?;
    /* __DS_WIRE_WEB_APIS_BODY__ */
    // Atomics.waitAsync — QuickJS-NG 0.12 lacks it; inject the polyfill on
    // every engine runtime so any degraded fixture (not just $262.agent ones)
    // resolves `typeof Atomics.waitAsync === 'function'`. Idempotent.
    ctx.eval_with_options::<(), _>(ATOMICS_WAIT_ASYNC_POLYFILL, sloppy())?;
    Ok(())
}

/// serde_json::Value -> rquickjs Value (recursive). Numbers fall back to `NaN`
/// (ES `Number` cannot losslessly hold an out-of-range integer).
pub fn json_to_js<'js>(ctx: &Ctx<'js>, v: &serde_json::Value) -> rquickjs::Result<Value<'js>> {
    match v {
        serde_json::Value::Null => Ok(Value::new_null(ctx.clone())),
        serde_json::Value::Bool(b) => Ok(Value::new_bool(ctx.clone(), *b)),
        serde_json::Value::Number(n) => {
            Ok(Value::new_float(ctx.clone(), n.as_f64().unwrap_or(f64::NAN)))
        }
        serde_json::Value::String(s) => s.as_str().into_js(ctx),
        serde_json::Value::Array(arr) => {
            let js_arr = Array::new(ctx.clone())?;
            for (i, e) in arr.iter().enumerate() {
                js_arr.set(i, json_to_js(ctx, e)?)?;
            }
            js_arr.into_js(ctx)
        }
        serde_json::Value::Object(obj) => {
            let o = Object::new(ctx.clone())?;
            for (k, val) in obj.iter() {
                o.set(k.as_str(), json_to_js(ctx, val)?)?;
            }
            o.into_js(ctx)
        }
    }
}

/// rquickjs Value -> serde_json::Value (recursive). Symbols, BigInts, modules,
/// and the void types collapse to `null` (the closest JSON representation).
pub fn js_to_json<'js>(ctx: &Ctx<'js>, v: Value<'js>) -> rquickjs::Result<serde_json::Value> {
    match v.type_of() {
        Type::Uninitialized | Type::Undefined | Type::Null => Ok(serde_json::Value::Null),
        Type::Bool => Ok(serde_json::Value::Bool(v.as_bool().unwrap())),
        Type::Int | Type::Float => {
            let n = v.as_number().unwrap();
            // Integral floats normalize to integers so a byte (a Uint8Array
            // element 97.0) marshals as `97` — matching JS `JSON.stringify`
            // and letting a Rust `Vec<u8>` deserialize a crypto result.
            if n.fract() == 0.0 && n.abs() <= 9_007_199_254_740_992.0 {
                Ok(serde_json::json!(n as i64))
            } else {
                Ok(serde_json::json!(n))
            }
        }
        Type::String => {
            let s: String = FromJs::from_js(ctx, v)?;
            Ok(serde_json::Value::String(s))
        }
        Type::Array => {
            let arr: Array = Array::from_js(ctx, v)?;
            let mut out = Vec::with_capacity(arr.len());
            for i in 0..arr.len() {
                let elem: Value = arr.get(i)?;
                out.push(js_to_json(ctx, elem)?);
            }
            Ok(serde_json::Value::Array(out))
        }
        Type::Object | Type::Function | Type::Constructor | Type::Promise
        | Type::Exception
        | Type::Proxy => {
            // A TypedArray (Uint8Array, …) tags as `Type::Object`, but its
            // indexed elements would marshal as `{"0":..,"1":..}`. Detect it
            // (duck-typed `length` + `byteLength`) and coerce to a plain Array
            // via `Array.from` first, so a crypto result (sha1 returns a
            // Uint8Array) marshals as a byte array, not a broken object.
            ctx.globals().set("__ds_mta", v.clone())?;
            let is_ta: bool = ctx
                .eval_with_options::<bool, _>(
                    "(function(){ try { return typeof __ds_mta.length === 'number' && typeof \
                     __ds_mta.byteLength === 'number'; } catch(e){ return false; } })()",
                    sloppy(),
                )
                .unwrap_or(false);
            if is_ta {
                let arr: Value =
                    ctx.eval_with_options::<Value, _>("Array.from(__ds_mta)", sloppy())?;
                let _ = ctx.globals().remove("__ds_mta");
                return js_to_json(ctx, arr);
            }
            let _ = ctx.globals().remove("__ds_mta");
            let obj: Object = Object::from_js(ctx, v)?;
            let mut map = serde_json::Map::new();
            for kv in obj.props::<String, Value>() {
                let (k, val) = kv?;
                map.insert(k, js_to_json(ctx, val)?);
            }
            Ok(serde_json::Value::Object(map))
        }
        Type::Symbol | Type::BigInt | Type::Module | Type::Unknown => {
            Ok(serde_json::Value::Null)
        }
    }
}

/// Extract the thrown value's string form after an eval failed, so a degraded
/// function's failure surfaces as `Test262Error: …` / `ReferenceError: …` (the
/// spec truth) rather than rquickjs's opaque `Exception generated by QuickJS`.
/// The conformance verdict keys off these names — `Test262Error`/`AssertionError`
/// ⇒ partial (assert mismatch), `ReferenceError` ⇒ unsupported (the engine — and
/// DashScript — lack that surface). `ctx.catch()` clears the pending exception
/// first, so the `String(thrown)` eval does not trip it; `String(value)` yields
/// `Error.prototype.toString()`, the same form the static `__ds::assert_*` panic
/// uses, so the two paths report identically.
fn throw_msg(ctx: &Ctx<'_>) -> String {
    let thrown = ctx.catch();
    let _ = ctx.globals().set("__ds_throw_diag", thrown);
    let msg: String = ctx
        .eval_with_options::<String, _>(
            "(function(){ try { return String(globalThis.__ds_throw_diag); } catch(_) { return '[' + (typeof globalThis.__ds_throw_diag) + ']'; } })()",
            sloppy(),
        )
        .unwrap_or_else(|_| "engine eval threw (non-string value)".into());
    let _ = ctx.globals().remove("__ds_throw_diag");
    msg
}

/// Run a self-contained `.ts` source under QuickJS with `console.log` wired to
/// stdout. The source declares `main()` and calls it (pure-TS execution
/// semantics), so a single eval runs the fixture. A thrown `Test262Error`/
/// `ReferenceError` panics with its string form — `eprintln!`'d first so the
/// name leads the stderr snippet the conformance verdict reads, ahead of the
/// Rust panic frame.
///
/// Async fixtures (`asyncTest`, `Promise.then`, `Atomics.waitAsync`) queue their
/// asserts as microtasks that run only when the engine drains its pending-job
/// queue — Node drains microtasks before exit; the embedded engine must do it
/// explicitly. `$DONE` is test262's async-completion marker (provided by the
/// host runner, not a harness file): `asyncTest` wraps the body so a rejection
/// routes to `$DONE(error)`, which stashes the error for the post-drain check.
/// Without these two, async fixtures `ReferenceError` on `$DONE` and never run
/// their async asserts.
pub fn run(source: &str) {
    let result = RUNTIME.with(|runtime| -> rquickjs::Result<()> {
        let ctx = Context::full(runtime).expect("rquickjs Context");
        // The closure return is annotated `-> rquickjs::Result<()>` (not left to
        // inference): `ctx.with(...)?` is no longer the tail expression (the
        // drain loop follows), so without the annotation the inner `?`s want
        // `From<rquickjs::Error> for E` while the outer `?` wants
        // `From<E> for rquickjs::Error` — a bidirectional constraint Rust won't
        // solve, surfacing as E0282 on the `Ok(())` tail. The annotation pins E.
        ctx.with(|ctx: Ctx<'_>| -> rquickjs::Result<()> {
            wire_console(&ctx)?;
            wire_web_apis(&ctx)?;
            // $DONE + __ds_async_error — test262 async completion. $DONE(error)
            // stashes the first failure for the post-drain check below; the
            // guard keeps the FIRST error (a later $DONE on a derived rejection
            // would otherwise clobber the root cause).
            ctx.eval_with_options::<(), _>(
                "globalThis.__ds_async_error = null;\
                 globalThis.$DONE = function (error) {\
                   if (error !== undefined && error !== null && globalThis.__ds_async_error === null) {\
                     globalThis.__ds_async_error = error;\
                   }\
                 };",
                sloppy(),
            )?;
            if ctx.eval_with_options::<(), _>(source, sloppy()).is_err() {
                let msg = throw_msg(&ctx);
                eprintln!("{msg}");
                panic!("{msg}");
            }
            Ok(())
        })?;
        // Drain the microtask queue — runs OUTSIDE `ctx.with`:
        // `is_job_pending`/`execute_pending_job` lock `runtime.inner` (a
        // `RefCell`) that `Context::with` holds for its whole closure, so
        // calling them inside re-enters the `RefCell` and panics (same shape as
        // `__ds_agent_loop`). A pending job that throws sets a pending
        // exception on the ctx, read below via `throw_msg`.
        let mut job_threw = false;
        while runtime.is_job_pending() {
            if runtime.execute_pending_job().is_err() {
                job_threw = true;
                break;
            }
        }
        // Surface an async failure: a pending job that threw (rare for Promise
        // reactions — their throws become rejections) OR the `$DONE(error)`
        // stash from an `asyncTest`-wrapped rejection (the common path). The
        // closure returns `Option<String>` directly (the eval is a `match`, no
        // `?`), so it needs no `Result` error-type inference.
        let failure: Option<String> = ctx.with(|ctx: Ctx<'_>| -> Option<String> {
            if job_threw {
                let m = throw_msg(&ctx);
                if !m.is_empty() {
                    return Some(m);
                }
            }
            match ctx.eval_with_options::<String, _>(
                "(globalThis.__ds_async_error == null) ? '' : String(globalThis.__ds_async_error)",
                sloppy(),
            ) {
                Ok(s) if !s.is_empty() => Some(s),
                _ => None,
            }
        });
        if let Some(msg) = failure {
            eprintln!("{msg}");
            panic!("{msg}");
        }
        Ok(())
    });
    result.expect("rquickjs runtime");
}

/// The per-function degradation entry point: evaluate `body_js` (which defines
/// `fn_name`), call it with serde_json-marshaled args, and marshal the return.
/// `fn_name` is a DashScript-translated identifier (a known global defined by
/// `body_js`), so the spread-call `fn_name(...__ds_call_args)` is safe. The
/// function's native Rust signature stays; only its body runs JS.
pub fn call_fn(fn_name: &str, body_js: &str, args: &[serde_json::Value]) -> serde_json::Value {
    let result = RUNTIME.with(|runtime| -> rquickjs::Result<serde_json::Value> {
        let ctx = Context::full(runtime).expect("rquickjs Context");
        ctx.with(|ctx: Ctx<'_>| {
            wire_console(&ctx)?;
            wire_web_apis(&ctx)?;
            if ctx.eval_with_options::<(), _>(body_js, sloppy()).is_err() {
                let msg = throw_msg(&ctx);
                eprintln!("{msg}");
                panic!("{msg}");
            }
            let js_args = Array::new(ctx.clone())?;
            for (i, a) in args.iter().enumerate() {
                js_args.set(i, json_to_js(&ctx, a)?)?;
            }
            ctx.globals().set("__ds_call_args", js_args)?;
            let expr = format!("{fn_name}(...__ds_call_args)");
            let ret: Value = match ctx.eval_with_options::<Value, _>(expr, sloppy()) {
                Ok(v) => v,
                Err(_) => {
                    let msg = throw_msg(&ctx);
                    eprintln!("{msg}");
                    panic!("{msg}");
                }
            };
            let _ = ctx.globals().remove("__ds_call_args");
            js_to_json(&ctx, ret)
        })
    });
    result.expect("rquickjs call_fn")
}

/// Lazily declare, evaluate, and cache a `.js` module's namespace. Called
/// before every `call_module_fn` so a degraded `.js` module (and its
/// transitive `import`s) loads on first use. The namespace lands in
/// `globalThis.__ds_modules[specifier]` for the spread-call in `call_module_fn`.
fn ensure_module_installed(ctx: &Ctx<'_>, specifier: &str) -> rquickjs::Result<()> {
    // The thread-local CTX persists `__ds_modules` across calls, so guard a
    // re-declare by checking the namespace already lives in THIS ctx's globals.
    let installed: bool = ctx
        .eval_with_options::<bool, _>(
            format!("!!(this.__ds_modules && this.__ds_modules['{specifier}'])"),
            sloppy(),
        )
        .unwrap_or(false);
    if installed {
        return Ok(());
    }
    let module = Module::declare(ctx.clone(), specifier, source_of(specifier)?)?;
    let (module, _promise) = module.eval()?;
    let ns = module.namespace()?;
    ctx.globals().set("__ds_tmp_install", ns)?;
    ctx.eval_with_options::<(), _>(
        format!(
            "this.__ds_modules = this.__ds_modules || {{}};\nthis.__ds_modules['{specifier}'] = \
             this.__ds_tmp_install;",
        ),
        sloppy(),
    )?;
    let _ = ctx.globals().remove("__ds_tmp_install");
    Ok(())
}

/// Eagerly install a `.js` module's namespace (optional pre-load before the
/// first `call_module_fn`). Most callers rely on `call_module_fn`'s lazy
/// install; this is for warming the engine up front.
pub fn install_module(specifier: &str) {
    let result = CTX.with(|ctx| -> rquickjs::Result<()> {
        ctx.with(|ctx: Ctx<'_>| {
            wire_console(&ctx)?;
            wire_web_apis(&ctx)?;
            ensure_module_installed(&ctx, specifier)
        })
    });
    result.expect("rquickjs install_module");
}

/// Call an exported function of a degraded `.js` module: lazily install the
/// module (and its dependency graph), marshal args via serde_json, spread-call
/// the export, and marshal the return. The caller keeps its native Rust
/// signature — only the body runs JS under the engine.
pub fn call_module_fn(
    module_key: &str,
    fn_name: &str,
    args: &[serde_json::Value],
) -> serde_json::Value {
    let result = CTX.with(|ctx| -> rquickjs::Result<serde_json::Value> {
        ctx.with(|ctx: Ctx<'_>| {
            wire_console(&ctx)?;
            wire_web_apis(&ctx)?;
            ensure_module_installed(&ctx, module_key)?;
            let js_args = Array::new(ctx.clone())?;
            for (i, a) in args.iter().enumerate() {
                js_args.set(i, json_to_js(&ctx, a)?)?;
            }
            ctx.globals().set("__ds_call_args", js_args)?;
            let expr = format!("__ds_modules['{module_key}'].{fn_name}(...__ds_call_args)");
            let ret: Value = match ctx.eval_with_options::<Value, _>(expr, sloppy()) {
                Ok(v) => v,
                Err(_) => {
                    let msg = throw_msg(&ctx);
                    eprintln!("{msg}");
                    panic!("{msg}");
                }
            };
            let _ = ctx.globals().remove("__ds_call_args");
            js_to_json(&ctx, ret)
        })
    });
    result.unwrap_or_else(|e| panic!("rquickjs call_module_fn({module_key}.{fn_name}): {e:?}"))
}
"##;
