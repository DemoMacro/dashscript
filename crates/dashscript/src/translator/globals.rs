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
/// (handled by an earlier explicit arm). The integer/float typed arrays
/// (`Int8Array` … `Float64Array`) and `ArrayBuffer` are absent — `new` on them
/// maps to `Vec<elem>` (`expressions/new`, `typed_array_elem_type`) and a type
/// annotation to `Vec<elem>` (`types`); flagging either would regress a static
/// mapping. Only the BigInt typed arrays stay (DashScript has no BigInt
/// literal, so their element type is unmappable).
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
    // BigInt typed arrays — DashScript has no BigInt literal, so the element
    // type is unmappable (`typed_array_elem_type` returns `None`); any
    // reference degrades to the engine. The integer/float typed arrays
    // (`Int8Array` … `Float64Array`) ARE mapped — `new` → `Vec<elem>`
    // (`expressions/new`) and a type annotation → `Vec<elem>` (`types`) — so
    // they stay off this list, the way the `u8` trio (`Uint8Array`/…) already
    // did. A bare value reference to one (`const a = Int32Array`) has no static
    // value lowering, so it falls through to the generic identifier emit and is
    // caught by the cargo-check-fail engine fallback (degrade, don't reject).
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

/// WPT (web-platform-tests) testharness global functions DashScript lowers
/// statically to `__ds::wpt_*` Rust helpers — the web-platform analogue of
/// test262's `assert.sameValue`, run on the **static path** (translate → cargo
/// → run). WinterTC conformance is pure-Rust: these never degrade to the
/// engine. A bare call to one of these names classifies `Mapped`; see
/// [`TESTHARNESS_REJECTED_GLOBALS`] for the async/composite forms with no
/// static lowering. Distinct from [`HARNESS_HELPER_GLOBALS`] (test262's
/// `$INCLUDE` helpers, which DO degrade — different harness, different layer).
pub const TESTHARNESS_MAPPED_GLOBALS: &[&str] = &[
    "test",
    "promise_test",
    "setup",
    "done",
    "assert_equals",
    "assert_not_equals",
    "assert_array_equals",
    "assert_approx_equals",
    "assert_true",
    "assert_false",
    "assert_throws_dom",
    "assert_throws_js",
    "assert_unreached",
];

/// True if `name` is a WPT testharness function with a static `__ds::wpt_*`
/// lowering. See [`TESTHARNESS_MAPPED_GLOBALS`].
#[inline]
pub fn is_testharness_mapped(name: &str) -> bool {
    TESTHARNESS_MAPPED_GLOBALS.contains(&name)
}

/// WPT testharness functions with NO static lowering — `async_test` (which
/// needs `t.step_func` manual step management the static path does not model)
/// and the composite asserts (`assert_object_equals`/…, whose operands are
/// not plain `DsSameValue` scalars or arrays of them). (`assert_approx_equals`
/// is mapped — numeric operands cast to `f64`; see [`TESTHARNESS_MAPPED_GLOBALS`].)
/// `promise_test` is mapped — it lowers to `wpt_promise_test(name, async move {
/// … }).await` under `#[tokio::main]` (see `testharness_function`). Unlike
/// test262's degrade-don't-reject, WinterTC is static-only — a fixture using
/// one of these is honestly `unsupported`, not engine-degraded. Growing
/// [`TESTHARNESS_MAPPED_GLOBALS`] (add a `__ds::wpt_*` helper + a
/// `testharness_function` arm, then move the name here→there) is how WinterTC
/// coverage expands.
pub const TESTHARNESS_REJECTED_GLOBALS: &[&str] = &[
    "async_test",
    "assert_object_equals",
    "assert_less",
    "assert_greater",
    "assert_between",
    "assert_own_property",
    "assert_not_own_property",
    "assert_inherits",
    "assert_readonly",
    "assert_implements",
    "assert_implements_float",
    "generate_string",
];

/// True if `name` is a WPT testharness function with no static lowering (and no
/// engine fallback — WinterTC is static-only). See [`TESTHARNESS_REJECTED_GLOBALS`].
#[inline]
pub fn is_testharness_rejected(name: &str) -> bool {
    TESTHARNESS_REJECTED_GLOBALS.contains(&name)
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

/// ES native Error constructors plus the test262 harness's `Test262Error`. `new
/// <X>(message)` lowers to a `DsError` value — the same model `throw new <X>(…)`
/// uses (`functions/try_throw` → `panic_any(DsError)`). `throw new <X>(<string
/// literal>)` is intercepted before `new_expr` by `thrown_error`; but `throw new
/// <X>(<dynamic message>)` (whose message is not a literal, so `thrown_error`
/// returns `None` and the throw falls back to `throw expr`) and any `new <X>(…)`
/// used as a value (`var e = new TypeError("x")`) reach `new_expr`, which emits
/// `DsError::new("<X>", msg.to_string())` — a value carrying `name`/`message`
/// fields and a `Display` impl, so `e.message`/`e.name`/`e.toString()` work.
/// `AggregateError`/`SuppressedError` are intentionally absent: their constructor
/// signatures are not `(message)` (`AggregateError` takes an errors iterable
/// first, `SuppressedError` takes `(error, suppressed, message)`), so a
/// first-arg-as-message lowering would be unsound.
pub const ERROR_CTOR_NAMES: &[&str] = &[
    "Error",
    "RangeError",
    "TypeError",
    "SyntaxError",
    "ReferenceError",
    "EvalError",
    "URIError",
    "Test262Error",
];

/// The ES error-class name a `new <X>(…)` constructor lowers to (the class name
/// itself), or `None` if `name` is not an ES native Error constructor or
/// `Test262Error`. See [`ERROR_CTOR_NAMES`].
#[inline]
pub fn error_ctor_name(name: &str) -> Option<&'static str> {
    ERROR_CTOR_NAMES.iter().copied().find(|n| *n == name)
}

/// Constructors whose `new <X>(…)` lowers to a *known concrete Rust type*,
/// paired with that type's last path segment. `x instanceof X` on a typed local
/// then folds to a compile-time `true`/`false`: DashScript has no inheritance
/// (a value's Rust type IS its only type — `extends` is unsupported), so an
/// `instanceof` test is an exact-type check, never a prototype-chain walk. The
/// receiver's type comes from its `new` initializer (`check` records it as a
/// `LocalKind::Ctor`; `translate` records the Rust path via
/// `register_declarator`), and the two stay in sync because both derive from
/// the same `new <X>(…)` shape — a drift metatest pins that.
///
/// `Array`/`Object` are intentionally absent: `instanceof Array` matches any
/// `Vec` and `instanceof Object` any reference type, so they take a special
/// arm in `instanceof_expr` rather than an exact-segment compare. The typed
/// arrays (`Uint8Array`/…) lower to `Vec<elem>` — already covered by the
/// `Array` arm. A ctor whose mapping is added in `expressions/new` without an
/// entry here leaves its `instanceof` on the engine (a safe, lazy default —
/// the function degrades rather than emit a wrong type check).
pub const MAPPED_CTOR_RUST_TYPE: &[(&str, &str)] = &[
    ("URL", "DsUrl"),
    ("URLSearchParams", "DsUrlSearchParams"),
    ("URLPattern", "DsURLPattern"),
    ("Headers", "DsHeaders"),
    ("Response", "DsResponse"),
    ("EventTarget", "DsEventTarget"),
    ("Event", "DsEvent"),
    ("AbortController", "DsAbortController"),
    ("AbortSignal", "DsAbortSignal"),
    ("Blob", "DsBlob"),
    ("File", "DsFile"),
    ("FormData", "DsFormData"),
    // Collections — `new Map()`/`new Set()` (and the weak aliases, which lower
    // to the same strong backing) all map to HashMap/HashSet.
    ("Map", "HashMap"),
    ("WeakMap", "HashMap"),
    ("Set", "HashSet"),
    ("WeakSet", "HashSet"),
    // Errors — every ES native Error ctor, `Test262Error`, and `DOMException`
    // lower to the same `DsError` value, so they share one target segment.
    ("Error", "DsError"),
    ("RangeError", "DsError"),
    ("TypeError", "DsError"),
    ("SyntaxError", "DsError"),
    ("ReferenceError", "DsError"),
    ("EvalError", "DsError"),
    ("URIError", "DsError"),
    ("Test262Error", "DsError"),
    ("DOMException", "DsError"),
];

/// The last path segment of the Rust type a `new <name>(…)` constructor lowers
/// to, or `None` if `name` is not a mapped ctor. See [`MAPPED_CTOR_RUST_TYPE`].
#[inline]
pub fn mapped_ctor_rust_type(name: &str) -> Option<&'static str> {
    MAPPED_CTOR_RUST_TYPE
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, t)| *t)
}

#[cfg(test)]
mod drift {
    //! Classify-data drift guard. The verdict a bare global name gets in
    //! [`super::classify_expr`]'s `Identifier` arm depends on WHICH list a name
    //! is in (engine-value → degrade, static-only → reject, harness-helper →
    //! degrade, unmapped-new on `new` → degrade), checked in that order. A name
    //! in two lists with contradictory semantics is an undocumented drift: the
    //! first arm to match wins, so the verdict silently depends on list order
    //! rather than intent. These tests pin the disjointness the comments claim.
    use super::{
        ENGINE_VALUE_GLOBALS, HARNESS_HELPER_GLOBALS, STATIC_ONLY_GLOBALS,
        TESTHARNESS_MAPPED_GLOBALS, TESTHARNESS_REJECTED_GLOBALS, UNMAPPED_NEW_GLOBALS,
    };

    fn intersect(a: &[&'static str], b: &[&'static str]) -> Vec<&'static str> {
        a.iter().filter(|x| b.contains(x)).copied().collect()
    }

    #[test]
    fn static_only_disjoint_from_engine_value() {
        let overlap = intersect(STATIC_ONLY_GLOBALS, ENGINE_VALUE_GLOBALS);
        assert!(
            overlap.is_empty(),
            "STATIC_ONLY_GLOBALS ∩ ENGINE_VALUE_GLOBALS = {overlap:?} (non-empty): a name in both \
             makes a bare value reference degrade (engine wins the Identifier arm) even though the \
             name carries a static call/new/type mapping — pick one list"
        );
    }

    #[test]
    fn unmapped_new_disjoint_from_engine_value() {
        let overlap = intersect(UNMAPPED_NEW_GLOBALS, ENGINE_VALUE_GLOBALS);
        assert!(
            overlap.is_empty(),
            "UNMAPPED_NEW_GLOBALS ∩ ENGINE_VALUE_GLOBALS = {overlap:?} (non-empty): a name in both \
             gets a `new`-site degrade that the comment in `UNMAPPED_NEW_GLOBALS` says it must not \
             carry (per-function emit-interaction risk) — pick one list"
        );
    }

    #[test]
    fn harness_helpers_distinct_from_globals() {
        for (label, list) in [
            ("STATIC_ONLY_GLOBALS", STATIC_ONLY_GLOBALS),
            ("ENGINE_VALUE_GLOBALS", ENGINE_VALUE_GLOBALS),
            ("UNMAPPED_NEW_GLOBALS", UNMAPPED_NEW_GLOBALS),
        ] {
            let overlap = intersect(HARNESS_HELPER_GLOBALS, list);
            assert!(
                overlap.is_empty(),
                "HARNESS_HELPER_GLOBALS ∩ {label} = {overlap:?} (non-empty): a test262 harness \
                 helper is never an ES global; a dual-listed name's verdict depends on Identifier \
                 arm order, not intent"
            );
        }
    }

    #[test]
    fn testharness_lists_disjoint_and_distinct() {
        // mapped ∩ rejected = ∅: a name in both has an ambiguous verdict (the
        // classify_call arm checks rejected before mapped, but a dual-listed
        // name is still an undocumented drift — pick one list).
        let overlap = intersect(TESTHARNESS_MAPPED_GLOBALS, TESTHARNESS_REJECTED_GLOBALS);
        assert!(
            overlap.is_empty(),
            "TESTHARNESS_MAPPED_GLOBALS ∩ TESTHARNESS_REJECTED_GLOBALS = {overlap:?} (non-empty): \
             a WPT testharness name in both has an ambiguous verdict"
        );
        // A WPT testharness name is never a test262 harness helper, an ES
        // engine-value global, a static-only global, or an unmapped `new` global
        // — a dual-listed name's classify_call verdict would depend on arm
        // order, not intent.
        for (label, list) in [
            ("HARNESS_HELPER_GLOBALS", HARNESS_HELPER_GLOBALS),
            ("ENGINE_VALUE_GLOBALS", ENGINE_VALUE_GLOBALS),
            ("STATIC_ONLY_GLOBALS", STATIC_ONLY_GLOBALS),
            ("UNMAPPED_NEW_GLOBALS", UNMAPPED_NEW_GLOBALS),
        ] {
            for th in [TESTHARNESS_MAPPED_GLOBALS, TESTHARNESS_REJECTED_GLOBALS] {
                let overlap = intersect(th, list);
                assert!(
                    overlap.is_empty(),
                    "testharness ∩ {label} = {overlap:?} (non-empty): a WPT testharness name is \
                     never an ES/test262 global; a dual-listed name's verdict depends on \
                     classify_call arm order, not intent"
                );
            }
        }
    }
}
