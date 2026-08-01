//! Canonical knowledge of the ES global built-ins DashScript models — sourced
//! once here so the translator's dispatch and the `check.rs` translatability lint
//! agree, instead of each keeping its own string list (which drifts: a name added
//! to one path but not the other is a silent misclassification).
//!
//! DashScript models these globals only as a static-call/new receiver
//! (`Math.floor`, `Array.isArray`, `new Map()`) or a type annotation (`Map<K,V>`)
//! — never as a first-class value. A bare value reference (`Array` as a value,
//! `var f = Object.keys`) is reflection the static TS→Rust mapping cannot
//! express; detecting it is [`is_static_only_global`].

/// Globals usable *only* as a static-call/new receiver or type annotation. A
/// bare reference to one as a value is unsupported — the translator would
/// snake-case the name (`Array`→`array`) and emit a phantom binding (E0425
/// `partial`). `Number`/`String`/`Boolean` are intentionally absent: they carry
/// mapped static members read as values (`Number.MAX_VALUE`→`f64::MAX`) and a
/// prototype-borrow form (`String.prototype.trim.call(x)`), so blanket-flagging
/// the name would regress those. Their conversion-call form (`Number(x)`) is
/// covered by `global_function`. `RegExp` is included: its call/new forms
/// (`RegExp(pat)`, `new RegExp(pat)`) are mapped, but it has no static *value*
/// members — a bare reference is always reflection (`RegExp.prototype.X`,
/// `RegExp.length`) the static mapping cannot express, so it routes to the
/// engine rather than emitting a phantom `reg_exp` binding. `Function` is
/// included for the same reason: the `Function` constructor has no DashScript
/// mapping (dynamic function creation is reflection), so a bare reference
/// (`Object.getOwnPropertyNames(Function)`, `Function.prototype.X`) routes to
/// the engine instead of emitting a phantom `function` binding. `Date` is
/// *not* here: it has no static mapping at all (no constructor, static, or
/// instance form) — see [`ENGINE_VALUE_GLOBALS`], where every reference
/// (bare/`new`/member) degrades to the engine.
pub const STATIC_ONLY_GLOBALS: &[&str] = &[
    "Array", "Object", "Math", "JSON", "Map", "Set", "RegExp", "Function",
];

/// Names that may stand as the receiver of a mapped static-member read —
/// [`STATIC_ONLY_GLOBALS`] plus the wrapper constructors `Number`/`String`/
/// `Boolean`, which carry mapped static members (`Number.MAX_VALUE`,
/// `String.prototype`, `Boolean.prototype`). Used to skip the *receiver* of a
/// member access so reading a static member is not mistaken for a bare value
/// reference; a bare reference to a [`STATIC_ONLY_GLOBALS`] name as a value is
/// still flagged.
pub const GLOBAL_RECEIVERS: &[&str] = &[
    "Array", "Object", "Math", "JSON", "Map", "Set", "Number", "String", "Boolean",
];

/// Globals the static translator has no mapping for at all — not as a value, a
/// `new` receiver, or a static-member root. A bare reference (or a `new`/
/// member access on one) would otherwise snake-case the name and emit a phantom
/// binding (E0425 `partial`); instead the enclosing function degrades to the
/// embedded engine, where the global exists natively. Disjoint from
/// [`STATIC_ONLY_GLOBALS`] (which carry call/new/type mappings) and from the
/// reflection globals `Symbol`/`Proxy`/`WeakRef`/`FinalizationRegistry`
/// (handled by an earlier explicit arm). The wrapper typed arrays
/// `Uint8Array`/`Uint8ClampedArray`/`Int8Array` are absent — `new` on them
/// maps to `Vec<u8>` (`expressions/new`) — as is `ArrayBuffer` (a
/// type-annotation mapping); flagging either would regress a static mapping.
/// `Temporal` IS listed: its static mapping is partial, so a bare `Temporal`
/// value reference degrades to the engine (which carries the
/// @js-temporal/polyfill); the `Temporal.X(…)` call / `new Temporal.X(…)`
/// forms are routed to the engine by `classify`'s `is_temporal_callee`.
/// `Date` IS listed: DashScript models Temporal (not `Date`), so the `Date`
/// constructor, its static/instance methods, and a bare value reference all
/// lack a static mapping — any of them degrades to the engine (QuickJS ships
/// a complete `Date`) rather than emitting a phantom `date` binding (E0433).
pub const ENGINE_VALUE_GLOBALS: &[&str] = &[
    "Date",
    "Promise",
    "Temporal",
    "DataView",
    "Int16Array",
    "Uint16Array",
    "Int32Array",
    "Uint32Array",
    "Float32Array",
    "Float64Array",
    "BigInt64Array",
    "BigUint64Array",
    "SharedArrayBuffer",
    "Atomics",
    "ShadowRealm",
    "AsyncFunction",
    "GeneratorFunction",
    "AsyncGeneratorFunction",
    "DisposableStack",
    "AsyncDisposableStack",
    "$262",
    "TemporalHelpers",
];

/// True if `name` is a global DashScript models only as a static-call/new
/// receiver — a bare value reference to it is unsupported reflection.
#[inline]
pub fn is_static_only_global(name: &str) -> bool {
    STATIC_ONLY_GLOBALS.contains(&name)
}

/// True if `name` may be the receiver of a mapped static-member read (the root
/// a `<Global>.<member>` chain is read from). See [`GLOBAL_RECEIVERS`].
#[inline]
pub fn is_global_receiver(name: &str) -> bool {
    GLOBAL_RECEIVERS.contains(&name)
}

/// True if `name` is a global with no static mapping — a reference degrades the
/// enclosing function to the engine. See [`ENGINE_VALUE_GLOBALS`].
#[inline]
pub fn is_engine_value_global(name: &str) -> bool {
    ENGINE_VALUE_GLOBALS.contains(&name)
}
