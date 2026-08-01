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
    // Reflection/metaprogramming globals — no static mapping exists, but
    // QuickJS ships them natively, so any reference degrades to the engine
    // rather than the hard `reject` these used to be (degrade, don't reject).
    "Symbol",
    "Proxy",
    "WeakRef",
    "FinalizationRegistry",
    "Reflect",
    // Weak collections — `new WeakMap()` as a class field initializer still
    // maps to a strong `HashMap` via the class-enhancement path; a bare
    // reference or a member call degrades.
    "WeakMap",
    "WeakSet",
    // Other unmapped constructors/namespaces — a bare reference or member
    // call would otherwise snake-case the name (E0425). `Error` is
    // intentionally absent: `new Error("…")` has a static mapping (`throw
    // new Error` → `DsError`), and listing it here would make the `new`
    // callee trip the bare-value rule (`is_global_object_callee` does not
    // skip engine-value globals as `new` receivers), forcing every `throw`
    // site onto the engine. A bare `Error` value reference (reflection like
    // `Object.getOwnPropertyNames(Error)`) stays `Mapped` and lowers
    // generically — an honest partial, rare in practice.
    "BigInt",
    "Generator",
    "Intl",
];

/// test262 harness helper functions — defined only in the harness `$INCLUDE`
/// files (propertyHelper.js, isConstructor.js, testTypedArray.js, compareArray.js,
/// …) the engine path injects before a fixture. They have no static mapping (and
/// no standard-ES analogue), so a bare call `isConstructor(x)` or a value read
/// `arr.map(compareArray)` would otherwise snake-case the name into a phantom
/// binding (E0425) — an honest `partial` that never reaches the harness. Listing
/// them here routes any fixture that references one onto the engine path, where
/// the `$INCLUDE` defines them and the assert family runs with reference
/// semantics. `assert` (already special-cased in `classify_call`) and
/// `Test262Error` (a constructor with its own `throw new Test262Error` mapping)
/// are intentionally absent. A fixture that locally re-declares one of these
/// names shadows the global — rare in test262, and the engine still runs the
/// fixture correctly when it does degrade.
pub const HARNESS_HELPER_GLOBALS: &[&str] = &[
    "CollectValuesAndResize",
    "CreateRabForTest",
    "CreateResizableArrayBuffer",
    "MayNeedBigInt",
    "TestIterationAndResize",
    "ToNumbers",
    "allowProxyTraps",
    "assertIsPackedArray",
    "assertIteratorResult",
    "assertZipped",
    "assertZippedKeyed",
    "asyncTest",
    "buildString",
    "checkSequence",
    "checkSettledPromises",
    "compareArray",
    "compareIterator",
    "ctorArgFactoryMatchesSome",
    "escapeKey",
    "floatTypedArrayConstructorPrecision",
    "forEachSequenceCombination",
    "forEachSequenceCombinationKeyed",
    "formatIdentityFreeValue",
    "formatPropertyName",
    "formatSimpleValue",
    "getWellKnownIntrinsicObject",
    "isConfigurable",
    "isConstructor",
    "isEnumerable",
    "isFloatTypedArrayConstructor",
    "isNegativeZero",
    "isPrimitive",
    "isSameValue",
    "isWritable",
    "makeArray",
    "makeArrayBuffer",
    "makeArrayLike",
    "makeNativeError",
    "makePassthrough",
    "matchValidator",
    "printCodePoint",
    "printStringCodePoints",
    "stringFromTemplate",
    "subClass",
    "testAtomics",
    "testPropertyEscapes",
    "testPropertyOfStrings",
    "testTypedArrayConversions",
    "testWithAllTypedArrayConstructors",
    "testWithAtomicsFriendlyTypedArrayConstructors",
    "testWithAtomicsInBoundsIndices",
    "testWithAtomicsNonViewValues",
    "testWithAtomicsOutOfBoundsIndices",
    "testWithBigIntTypedArrayConstructors",
    "testWithNonAtomicsFriendlyTypedArrayConstructors",
    "testWithTypedArrayConstructors",
    "verifyAccessorProperty",
    "verifyCallableProperty",
    "verifyConfigurable",
    "verifyEnumerable",
    "verifyEqualTo",
    "verifyNotConfigurable",
    "verifyNotEnumerable",
    "verifyNotWritable",
    "verifyPrimordialAccessorProperty",
    "verifyPrimordialCallableProperty",
    "verifyProperty",
    "verifyWritable",
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

/// True if `name` is a test262 harness helper function (defined only in a
/// `$INCLUDE` the engine injects). A call or value read degrades the enclosing
/// function to the engine. See [`HARNESS_HELPER_GLOBALS`].
#[inline]
pub fn is_harness_helper(name: &str) -> bool {
    HARNESS_HELPER_GLOBALS.contains(&name)
}

/// ES builtin/wrapper constructors whose `new <X>(…)` form has no static
/// lowering in `expressions/new`. `new_expr` special-cases `Map`/`WeakMap`/
/// `Set`/`WeakSet`, the `u8` typed arrays, `RegExp`, `TextEncoder`/
/// `TextDecoder`, `Worker`, and `Temporal.<Type>`; every other `new` callee
/// falls through to the generic `Foo::new(…)` emit. For the names here that
/// emit produces a phantom type (`new Array(0)` → `Array::new(0)` → E0433
/// `cannot find type Array`) — there is no `Array`/`ArrayBuffer`/`Object`/
/// `Function`/`Number`/`String`/`Boolean` Rust item. None of these `new` forms
/// ever passes statically, so degrading them to the engine (where the boxed
/// wrapper / sparse-array constructor runs natively) cannot regress a static
/// pass. Disjoint from [`ENGINE_VALUE_GLOBALS`]: those (`Promise`/`DataView`/
/// the non-`u8` typed arrays) reach `new` inside reflection fixtures that
/// already degrade via the `.constructor` rule, and a `new`-site degrade there
/// carries a per-function emit-interaction risk (see the `NewExpression` arm in
/// `classify`); the wrapper/static-only constructors here have no such
/// exposure, so they degrade cleanly.
pub const UNMAPPED_NEW_GLOBALS: &[&str] = &[
    "Array",
    "ArrayBuffer",
    "Object",
    "Function",
    "Number",
    "String",
    "Boolean",
];

/// True if `new <name>(…)` has no static lowering — the call degrades the
/// enclosing function to the engine. See [`UNMAPPED_NEW_GLOBALS`].
#[inline]
pub fn is_unmapped_new_global(name: &str) -> bool {
    UNMAPPED_NEW_GLOBALS.contains(&name)
}
