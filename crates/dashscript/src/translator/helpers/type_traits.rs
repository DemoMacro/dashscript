/// The DashScript runtime helper module, written to `src/__ds.rs` and declared
/// `mod __ds;` at each crate root when a translated file references it. The
/// single source for the `__ds` helpers — consumed by both `ds build` (bin) and
/// the conformance harness (lib test) — so the helper text lives in the library
/// rather than either consumer. [`RuntimeDeps::helper_module`] concatenates
/// whichever slices a translation flagged.
pub const ERROR_HELPER: &str = r##"/// An ECMAScript error object lowered through Rust `panic!`/`catch_unwind`.
/// `throw new RangeError("msg")` panics a `DsError`; `catch (e)` downcasts it
/// back. Carries the error class `name` ("RangeError"/"TypeError"/…) and
/// `message`, so `e.constructor.name`/`e.name`/`e.message`/`e.toString()`
/// work without string-matching panic messages.
#[derive(Clone)]
pub struct DsError {
    pub name: &'static str,
    pub message: String,
}

impl DsError {
    #[inline]
    pub fn new(name: &'static str, message: impl Into<String>) -> Self {
        DsError { name, message: message.into() }
    }

    /// Recover a `DsError` from a `catch_unwind` panic payload, accepting a
    /// `DsError`, a `String`, or a `&'static str` (a bare `panic!("msg")`).
    #[inline]
    pub fn from_panic(payload: &Box<dyn std::any::Any + Send>) -> Option<Self> {
        if let Some(e) = payload.downcast_ref::<DsError>() {
            return Some(e.clone());
        }
        if let Some(s) = payload.downcast_ref::<String>() {
            return Some(DsError::new("Error", s.clone()));
        }
        if let Some(s) = payload.downcast_ref::<&'static str>() {
            return Some(DsError::new("Error", (*s).to_string()));
        }
        None
    }
}

impl std::fmt::Display for DsError {
    #[inline]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.message.is_empty() {
            write!(f, "{}", self.name)
        } else {
            write!(f, "{}: {}", self.name, self.message)
        }
    }
}

/// Run `f` under `catch_unwind`, suppressing the panic hook while the body
/// runs. A `.ts` `throw` inside `try` is control flow, not a diagnostic — but
/// Rust's panic hook fires before `catch_unwind` catches, so the default hook
/// would print `Box<dyn Any>` for every caught throw. The hook is taken before
/// and restored after, so an uncaught panic still prints normally.
#[inline]
pub fn catch_quiet<F, R>(f: F) -> std::thread::Result<R>
where
    F: FnOnce() -> R + std::panic::UnwindSafe,
{
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let r = std::panic::catch_unwind(f);
    std::panic::set_hook(prev);
    r
}
"##;

/// ES truthiness for a value used in condition position. The translator emits
/// `__ds::truthy(&expr)` for a non-boolean condition (member access like
/// `opts.indent`, a numeric cast, a call) it cannot lower without a type
/// checker; the Rust compiler picks the matching impl by inferred type. ES
/// falsiness: `0`, `NaN`, `""`, `null`/`undefined` (`None`); everything else is
/// truthy — including empty arrays/objects (an ES quirk vs Python). Pure `std`.
pub const TRUTHY_HELPER: &str = "\
pub trait DsTruthy {
    fn ds_truthy(&self) -> bool;
}

/// Free-function form the translator emits (`__ds::truthy(&expr)`). The trait
/// bound lives inside `__ds`, so call sites need no `use` of the trait — the
/// compiler resolves the impl from the inferred type of the reference.
#[inline]
pub fn truthy<T: DsTruthy>(x: &T) -> bool {
    x.ds_truthy()
}

impl DsTruthy for f64 {
    #[inline]
    fn ds_truthy(&self) -> bool {
        *self != 0.0 && !self.is_nan()
    }
}

macro_rules! __ds_impl_truthy_int {
    ($($t:ty),+ $(,)?) => {
        $(
            impl DsTruthy for $t {
                #[inline]
                fn ds_truthy(&self) -> bool {
                    *self != 0
                }
            }
        )+
    };
}

__ds_impl_truthy_int!(i64, i32, i16, i8, isize, u64, u32, u16, u8, usize);

impl DsTruthy for String {
    #[inline]
    fn ds_truthy(&self) -> bool {
        !self.is_empty()
    }
}

impl DsTruthy for str {
    #[inline]
    fn ds_truthy(&self) -> bool {
        !self.is_empty()
    }
}

impl<T> DsTruthy for Vec<T> {
    #[inline]
    fn ds_truthy(&self) -> bool {
        true
    }
}

impl<K, V> DsTruthy for std::collections::HashMap<K, V> {
    #[inline]
    fn ds_truthy(&self) -> bool {
        true
    }
}

impl<T> DsTruthy for std::collections::HashSet<T> {
    #[inline]
    fn ds_truthy(&self) -> bool {
        true
    }
}

impl<T> DsTruthy for Option<T> {
    #[inline]
    fn ds_truthy(&self) -> bool {
        self.is_some()
    }
}

impl DsTruthy for bool {
    #[inline]
    fn ds_truthy(&self) -> bool {
        *self
    }
}
";

pub const ASSERT_HELPER: &str = r#"
/// test262 SameValue (Object.is): `===` plus distinct +0/-0 and NaN===NaN.
/// A scalar-only trait — composite operands route to the engine, where ES
/// reference-SameValue runs natively. Each scalar projects to a [`DsCmp`]
/// kind rather than comparing `&Self`, so a `&str` operand and a `String`
/// operand (both TS `string`, but lowered to different Rust forms) compare by
/// content — the translator emits `assert.sameValue(methodCall(), "lit")`,
/// where the method returns `&str` and the literal is `String`.
pub trait DsSameValue: std::fmt::Debug {
    fn ds_cmp(&self) -> DsCmp<'_>;
}

/// The comparable projection of a scalar — strings borrow, so a `&str` operand
/// and a `String` operand both yield [`DsCmp::Str`] and compare by content.
pub enum DsCmp<'a> {
    Num(f64),
    Bool(bool),
    Str(&'a str),
    /// `undefined` — an `Option<T>`'s `None`, a `serde_json::Value::Null`, or the
    /// `()` a void-returning function yields. ES `void fn()` is `undefined`, so
    /// a void return and an `undefined` literal are the same value and must
    /// compare SameValue-equal (`assert_equals(fn(), undefined)` holds).
    Undefined,
}

impl DsCmp<'_> {
    fn same(&self, other: &Self) -> bool {
        match (self, other) {
            // Object.is: `===` but +0 !== -0 (Rust `==` treats them equal) and
            // NaN === NaN (Rust `==` says false).
            (DsCmp::Num(a), DsCmp::Num(b)) => {
                (*a == *b
                    && (*a != 0.0 || a.is_sign_negative() == b.is_sign_negative()))
                    || (a.is_nan() && b.is_nan())
            }
            (DsCmp::Bool(a), DsCmp::Bool(b)) => a == b,
            (DsCmp::Str(a), DsCmp::Str(b)) => *a == *b,
            // `undefined` SameValue `undefined` (an Option's None, a Null, or the
            // `()` a void-returning function yields — all project to Undefined).
            (DsCmp::Undefined, DsCmp::Undefined) => true,
            _ => false,
        }
    }
}

/// test262 `assert.sameValue(a, b)` — panics a `Test262Error` on mismatch.
/// The `Test262Error:` prefix lets the conformance harness distinguish an
/// assert failure (partial) from a build error (unsupported). Two type params
/// so a `&str` operand and a `String` operand (both TS `string`) compare.
#[inline]
pub fn assert_same_value<A: DsSameValue, B: DsSameValue>(a: &A, b: &B) {
    if !a.ds_cmp().same(&b.ds_cmp()) {
        panic!(
            "Test262Error: Expected SameValue(«{:?}», «{:?}») to be true",
            a, b
        );
    }
}

/// test262 `assert.notSameValue(a, b)` — panics if the values are SameValue.
#[inline]
pub fn assert_not_same_value<A: DsSameValue, B: DsSameValue>(a: &A, b: &B) {
    if a.ds_cmp().same(&b.ds_cmp()) {
        panic!(
            "Test262Error: Expected SameValue(«{:?}», «{:?}») to be false",
            a, b
        );
    }
}

/// test262 `assert.throws(Ctor, fn)` — catch_unwinds `fn` and checks the thrown
/// error's class (`DsError.name`) equals `Ctor`. Passes silently on a match; a
/// mismatch or a no-throw return panics a `Test262Error`. `R` is the closure's
/// return type (discarded — `assert.throws` only inspects the thrown class), so
/// `() => Temporal.Duration.from("garbage")` (returning a `Duration`) satisfies
/// `FnOnce() -> R`. `AssertUnwindSafe` wraps the closure the way `try` does — a
/// capturing closure is not `UnwindSafe` on its own. `catch_quiet` + `DsError`
/// live in the Error slice; a fixture using `assert_throws` pulls it via the
/// dep scan (`__ds::assert_throws` ⇒ `RuntimeDep::Error`).
#[inline]
pub fn assert_throws<R>(expected: &str, f: impl FnOnce() -> R) {
    match catch_quiet(::std::panic::AssertUnwindSafe(f)) {
        Err(payload) => {
            let got = DsError::from_panic(&payload)
                .map(|e| e.name.to_string())
                .unwrap_or_else(|| "Error".to_string());
            if got == expected {
                return;
            }
            panic!(
                "Test262Error: Expected a {expected} to be thrown but got a {got}"
            );
        }
        Ok(_) => {
            panic!(
                "Test262Error: Expected a {expected} to be thrown but no exception was thrown"
            )
        }
    }
}

impl DsSameValue for f64 {
    #[inline]
    fn ds_cmp(&self) -> DsCmp<'_> {
        DsCmp::Num(*self)
    }
}

/// DashScript flavors a `number` as `i64`/`u64`/`u8` for bitwise/integer/byte
/// contexts, but ES SameValue is f64 semantics (every ES Number is an f64), so
/// an integer-flavored operand projects to [`DsCmp::Num`] via an f64 cast —
/// `assert.sameValue(i64Value, f64Value)` then compares numerically.
impl DsSameValue for i64 {
    #[inline]
    fn ds_cmp(&self) -> DsCmp<'_> {
        DsCmp::Num(*self as f64)
    }
}

impl DsSameValue for u64 {
    #[inline]
    fn ds_cmp(&self) -> DsCmp<'_> {
        DsCmp::Num(*self as f64)
    }
}

impl DsSameValue for u8 {
    #[inline]
    fn ds_cmp(&self) -> DsCmp<'_> {
        DsCmp::Num(*self as f64)
    }
}

impl DsSameValue for bool {
    #[inline]
    fn ds_cmp(&self) -> DsCmp<'_> {
        DsCmp::Bool(*self)
    }
}

impl DsSameValue for String {
    #[inline]
    fn ds_cmp(&self) -> DsCmp<'_> {
        DsCmp::Str(self.as_str())
    }
}

impl DsSameValue for str {
    #[inline]
    fn ds_cmp(&self) -> DsCmp<'_> {
        DsCmp::Str(self)
    }
}

/// A `&str` operand — `&("x".trim())` lowers to `&&str`. This projects to the
/// same `Str` kind as an owned `String`, so cross-form string asserts compare.
impl DsSameValue for &str {
    #[inline]
    fn ds_cmp(&self) -> DsCmp<'_> {
        DsCmp::Str(*self)
    }
}

/// The `()` a void-returning function yields — ES `void fn()` is `undefined`,
/// so it projects to [`DsCmp::Undefined`] and `assert_equals(fn(), undefined)`
/// holds. An `undefined` literal lowers to `Option::<()>::None`, which also
/// projects to `Undefined`, so the two forms of ES `undefined` compare equal.
impl DsSameValue for () {
    #[inline]
    fn ds_cmp(&self) -> DsCmp<'_> {
        DsCmp::Undefined
    }
}

/// An `Option<T>` — a `Map.get(k)` (ES `V | undefined`). `None` projects to
/// [`DsCmp::Undefined`] so `assert.sameValue(m.get(k), undefined)` holds when
/// the key is absent; `Some(v)` delegates to `T`, so a present value compares
/// against a bare `T` operand (`assert.sameValue(m.get(k), v)`).
impl<T: DsSameValue> DsSameValue for Option<T> {
    #[inline]
    fn ds_cmp(&self) -> DsCmp<'_> {
        match self {
            Some(v) => v.ds_cmp(),
            None => DsCmp::Undefined,
        }
    }
}

// WPT (web-platform-tests) testharness asserts — the web-platform analogue of
// test262's `assert.sameValue`. A WinterTC conformance fixture runs on the
// static path (translate → cargo → run), so these lower to Rust helpers in the
// same `__ds` module and panic an `AssertionError` on failure (the WPT
// testharness convention). The `AssertionError:` prefix lets the conformance
// harness distinguish a WPT assert failure (`partial`) from a build error
// (`unsupported`), the way `Test262Error:` does for test262. Composite WPT
// asserts (`assert_array_equals`/`assert_object_equals`/…) and async forms
// (`async_test`/`promise_test`) have no static lowering — the fixture falls
// back to the engine (WinterTC is static-first + per-function degrade, same
// as test262).

/// WPT `assert_equals(a, b)` — panics an `AssertionError` on mismatch. Same
/// SameValue (Object.is) semantics as `assert_same_value`; two type params so
/// a `&str` operand and a `String` operand (both TS `string`) compare. WPT's
/// `assert_true(x)`/`assert_false(x)` route here too against `&true`/`&false`
/// (WPT requires `actual === true`/`=== false` strictly — SameValue against the
/// boolean is exactly that, and accepts any `DsSameValue` operand type).
#[inline]
pub fn wpt_assert_equals<A: DsSameValue, B: DsSameValue>(a: &A, b: &B) {
    if !a.ds_cmp().same(&b.ds_cmp()) {
        panic!(
            "AssertionError: Expected SameValue(«{:?}», «{:?}») to be true",
            a, b
        );
    }
}

/// WPT `assert_not_equals(a, b)` — panics if the values are SameValue.
#[inline]
pub fn wpt_assert_not_equals<A: DsSameValue, B: DsSameValue>(a: &A, b: &B) {
    if a.ds_cmp().same(&b.ds_cmp()) {
        panic!(
            "AssertionError: Expected SameValue(«{:?}», «{:?}») to be false",
            a, b
        );
    }
}

/// WPT `assert_throws_dom(name, fn)` / `assert_throws_js(ctor, fn)` —
/// catch_unwinds `fn` and checks the thrown error's class (`DsError.name`)
/// equals `expected` (a DOMException name like `"NetworkError"` or a JS
/// constructor name like `"TypeError"`). Passes silently on a match; a
/// mismatch or a no-throw return panics an `AssertionError`. `R` is the
/// closure's return type (discarded). Uses `catch_quiet`/`DsError` from the
/// Error slice, so a fixture using `wpt_assert_throws` pulls `RuntimeDep::Error`
/// (the WptAssert→Error联动 in `translator/mod.rs`).
#[inline]
pub fn wpt_assert_throws<R>(expected: &str, f: impl FnOnce() -> R) {
    match catch_quiet(::std::panic::AssertUnwindSafe(f)) {
        Err(payload) => {
            let got = DsError::from_panic(&payload)
                .map(|e| e.name.to_string())
                .unwrap_or_else(|| "Error".to_string());
            if got == expected {
                return;
            }
            panic!(
                "AssertionError: Expected a {expected} to be thrown but got a {got}"
            );
        }
        Ok(_) => {
            panic!(
                "AssertionError: Expected a {expected} to be thrown but no exception was thrown"
            )
        }
    }
}

/// WPT `assert_unreached([msg])` — always panics an `AssertionError`. The
/// optional message is dropped at the call site (the verdict keys off the
/// `AssertionError:` prefix only).
#[inline]
pub fn wpt_assert_unreached() {
    panic!("AssertionError: unreachable");
}

/// WPT `promise_test(async fn, name)` — awaits the callback's future. The
/// callback lowers to `async move { body }`, so a panic inside it (an assert
/// failure) propagates through the `.await` as usual — fail-fast, the verdict
/// keys off the `AssertionError:` prefix (the `name` is dropped, same as
/// `test()`). Runs on the static path under `#[tokio::main]`: a top-level
/// `promise_test(async () => { … }, "n")` emits
/// `__ds::wpt_promise_test("n", async move { … }).await`, and that `.await`
/// makes the entry's `main` async (see `translator/mod.rs`). The future needs
/// no `Send` bound — the entry uses a single-thread runtime.
pub async fn wpt_promise_test<F>(_name: &str, fut: F)
where
    F: std::future::Future<Output = ()>,
{
    fut.await;
}

/// WPT `assert_array_equals(actual, expected[, msg])` — panics an
/// `AssertionError` if the arrays differ in length or any element pair is not
/// SameValue. Operands coerce from `&Vec<T>`/`&[T]` (`Vec: Deref<Target =
/// [T]>`); element comparison goes through `DsSameValue` (SameValue, not
/// `==`), mirroring test262's `compareArray` semantics WPT matches. Different
/// element types across the two operands fail inference (E0308) — the static
/// path's honest partial.
#[inline]
pub fn wpt_assert_array_equals<T: DsSameValue, U: DsSameValue>(
    actual: &[T],
    expected: &[U],
) {
    if actual.len() != expected.len() {
        panic!(
            "AssertionError: array length {} !== {}",
            actual.len(),
            expected.len()
        );
    }
    for (i, (a, b)) in actual.iter().zip(expected.iter()).enumerate() {
        if !a.ds_cmp().same(&b.ds_cmp()) {
            panic!(
                "AssertionError: array[{}] Expected SameValue(«{:?}», «{:?}») to be true",
                i, a, b
            );
        }
    }
}

/// WPT `assert_approx_equals(actual, expected, epsilon[, msg])` — panics an
/// `AssertionError` when `|actual - expected| > epsilon` (WPT testharness.js:
/// pass iff the difference is within `epsilon`, inclusive). Each operand is a
/// `number`; the call site casts to `f64` so an `i64`-flavor local type-checks.
/// A non-numeric operand fails inference (E0308) — the static path's honest
/// partial. The optional `msg` (arg 3) is dropped at the call site (the verdict
/// keys off the `AssertionError:` prefix).
#[inline]
pub fn wpt_assert_approx_equals(actual: f64, expected: f64, epsilon: f64) {
    if (actual - expected).abs() > epsilon {
        panic!(
            "AssertionError: assert_approx_equals: |{} - {}| > {}",
            actual, expected, epsilon
        );
    }
}
"#;

/// The WPT timer scheduling helpers — `__ds::wpt_set_timeout`/
/// `wpt_set_interval`/`wpt_clear_timer`/`wpt_done`/`wpt_run_timers`. ES
/// `setTimeout`/`setInterval` queue a callback on the event loop's task queue;
/// the static path models that queue as a `thread_local` drain run at the
/// entry's end — the moment ES itself drains (main returned, call stack empty).
/// `done()` sets a flag the drain checks after every fire (HTML "stop"
/// semantics), so a `setTimeout(assert_unreached, …)` queued before `done`
/// never runs; `assert_unreached` as a callback panics if it ever fires. WPT's
/// timer fixtures clamp every delay to 0 (negative, `2^32`-overflow, or
/// missing), so the drain is a deterministic CPU loop with no real wait; the
/// `Instant` comparison is kept so a future delay>0 fixture sleeps correctly
/// without a redesign. Pure `std`.
pub const TIMERS_HELPER: &str = r#"
/// One scheduled timer. `interval_ms = Some(_)` is a recurring `setInterval`;
/// `None` is a one-shot `setTimeout` (removed after firing). `seq` is the
/// registration order — the FIFO tiebreak between same-deadline timers (HTML
/// fires same-deadline timers in registration order).
struct WptTimer {
    id: u64,
    when_ms: u64,
    seq: u64,
    interval_ms: ::std::option::Option<u64>,
    cb: Box<dyn FnMut()>,
}

thread_local! {
    static WPT_TIMERS: std::cell::RefCell<Vec<WptTimer>> = std::cell::RefCell::new(Vec::new());
    static WPT_NEXT_ID: std::cell::Cell<u64> = std::cell::Cell::new(1);
    static WPT_SEQ: std::cell::Cell<u64> = std::cell::Cell::new(0);
    static WPT_DONE: std::cell::Cell<bool> = std::cell::Cell::new(false);
    static WPT_CANCELLED: std::cell::RefCell<std::collections::HashSet<u64>> =
        std::cell::RefCell::new(std::collections::HashSet::new());
    // The microtask queue — a FIFO of `queueMicrotask` callbacks. Drained to
    // empty at every task boundary (after each timer fire, and at the entry's
    // end before the timer queue runs) per HTML's microtask checkpoint: a
    // callback queued by a firing timer runs before the next timer fires.
    static WPT_MICROTASKS: std::cell::RefCell<std::collections::VecDeque<Box<dyn FnMut()>>> =
        std::cell::RefCell::new(std::collections::VecDeque::new());
}

/// HTML timer delay clamp: WebIDL `long` (signed 32-bit) conversion followed by
/// the "if timeout < 0, set to 0" rule. So `-100 → 0`, `Math.pow(2,32) → 0`
/// (the low 32 bits are 0), `100 → 100`. A non-finite delay (a missing 2nd arg
/// is `undefined` → `NaN`) → 0.
#[inline]
fn wpt_clamp_delay(delay: f64) -> u64 {
    if !delay.is_finite() {
        return 0;
    }
    // `as i64` then `as i32` truncates to the low 32 bits (i32 is the WebIDL
    // `long` type), so 2^32 wraps to 0 the way the browser's timer init does.
    let int32 = (delay.trunc() as i64) as i32;
    if int32 < 0 { 0 } else { int32 as u64 }
}

fn wpt_next_seq() -> u64 {
    WPT_SEQ.with(|s| {
        let n = s.get();
        s.set(n + 1);
        n
    })
}

/// `setTimeout(cb, delay)` — schedule `cb` to fire once after `delay` ms
/// (clamped). The callback is `FnMut` (a named listener may mutate captured
/// state, e.g. a counter); a one-shot fires it once then is dropped.
#[inline]
pub fn wpt_set_timeout(cb: Box<dyn FnMut()>, delay: f64) {
    let id = WPT_NEXT_ID.with(|n| {
        let i = n.get();
        n.set(i + 1);
        i
    });
    WPT_TIMERS.with(|t| {
        t.borrow_mut().push(WptTimer {
            id,
            when_ms: wpt_clamp_delay(delay),
            seq: wpt_next_seq(),
            interval_ms: ::std::option::Option::None,
            cb,
        });
    });
}

/// `setInterval(cb, delay)` — schedule `cb` to fire repeatedly every `delay` ms
/// (clamped). Returns the timer id (ES `setInterval` returns a handle for
/// `clearInterval`).
#[inline]
pub fn wpt_set_interval(cb: Box<dyn FnMut()>, delay: f64) -> u64 {
    let id = WPT_NEXT_ID.with(|n| {
        let i = n.get();
        n.set(i + 1);
        i
    });
    let clamped = wpt_clamp_delay(delay);
    WPT_TIMERS.with(|t| {
        t.borrow_mut().push(WptTimer {
            id,
            when_ms: clamped,
            seq: wpt_next_seq(),
            interval_ms: ::std::option::Option::Some(clamped),
            cb,
        });
    });
    id
}

/// `clearTimeout(id)` / `clearInterval(id)` — cancel a pending timer. ES keeps
/// both handle kinds in one id space, so one clear covers both.
#[inline]
pub fn wpt_clear_timer(id: u64) {
    WPT_CANCELLED.with(|c| {
        c.borrow_mut().insert(id);
    });
}

/// `done()` — the WPT single-test "stop" signal. The drain checks this after
/// every fire and stops, so a callback queued after `done` never fires.
#[inline]
pub fn wpt_done() {
    WPT_DONE.with(|d| d.set(true));
}

/// `queueMicrotask(cb)` — push `cb` onto the FIFO microtask queue. The callback
/// is `FnMut` (a named listener may mutate captured state); like `setTimeout`'s
/// callback it is a `Box<dyn FnMut()>` whose captured state the translator
/// clones/moves in at the call site, so it is `'static` (the queue is
/// `thread_local`, single-threaded — no `Send` bound, unlike an `async`
/// WritableStream sink that crosses an `await` on a multi-thread executor).
#[inline]
pub fn wpt_queue_microtask(cb: Box<dyn FnMut()>) {
    WPT_MICROTASKS.with(|q| q.borrow_mut().push_back(cb));
}

/// Drain the microtask queue to empty — HTML's microtask checkpoint. Fires
/// every queued callback in FIFO order; a callback that itself queues another
/// microtask is caught by the loop (the `pop_front` re-reads the queue each
/// iteration, so a `push_back` during a fire is seen on the next pass). Called
/// at every task boundary: after each timer fire (inside `wpt_run_timers`) and
/// once at the entry's end before the timer queue runs. `done()` does NOT stop
/// the microtask drain — HTML runs pending microtasks even after the "stop"
/// signal, only the macrotask (timer) queue respects `done()`.
pub fn wpt_drain_microtasks() {
    loop {
        let mut cb = WPT_MICROTASKS.with(|q| q.borrow_mut().pop_front());
        match cb.as_mut() {
            ::std::option::Option::Some(f) => f(),
            ::std::option::Option::None => break,
        }
    }
}

/// Drain the timer queue — the ES event loop's task queue, run at the entry's
/// end. Fires the earliest-deadline live (non-cancelled) timer by `(deadline,
/// seq)`, re-queues an interval's next fire, and stops when `done()` was called
/// or no live timer remains. The callback is taken out of the slot before the
/// fire so a body that registers another timer (or calls `done`/`clear`) does
/// not re-borrow the queue and panic. WPT timer fixtures clamp every delay to
/// 0, so this is a deterministic CPU loop (no real wait); a future delay>0
/// fixture sleeps via the `Instant` comparison.
pub fn wpt_run_timers() {
    let start = ::std::time::Instant::now();
    loop {
        if WPT_DONE.with(|d| d.get()) {
            return;
        }
        // Pick the earliest live timer by (deadline, seq).
        let target = WPT_TIMERS.with(|t| {
            let v = t.borrow();
            // Clone the cancelled-id set so the `Ref` borrow ends inside its own
            // `with` — a `Ref` cannot escape the `with` call (lifetime errors
            // out). The set is tiny (a handful of `clear*` ids), so the clone
            // is negligible across the drain's few iterations.
            let cancelled: std::collections::HashSet<u64> =
                WPT_CANCELLED.with(|c| c.borrow().clone());
            v.iter()
                .filter(|e| !cancelled.contains(&e.id))
                .min_by_key(|e| (e.when_ms, e.seq))
                .map(|e| (e.id, e.when_ms))
        });
        let Some((id, when_ms)) = target else {
            return; // no live timers — drain done
        };
        // Sleep until the deadline (a no-op when the delay clamped to 0, as in
        // every WPT timer fixture today).
        let now_ms = start.elapsed().as_millis() as u64;
        if when_ms > now_ms {
            ::std::thread::sleep(::std::time::Duration::from_millis(when_ms - now_ms));
        }
        if WPT_DONE.with(|d| d.get()) {
            return;
        }
        // Take the callback out so the fire holds no `WPT_TIMERS` borrow — a
        // callback that registers another timer / calls `done` / `clear` would
        // otherwise re-borrow and panic. A one-shot is removed now; an interval
        // keeps its slot (the callback is written back after the fire).
        let (interval, mut cb) = WPT_TIMERS.with(|t| {
            let mut v = t.borrow_mut();
            let Some(pos) = v.iter().position(|e| e.id == id) else {
                return (::std::option::Option::None, Box::new(|| ()) as Box<dyn FnMut()>);
            };
            let interval = v[pos].interval_ms;
            let cb = std::mem::replace(&mut v[pos].cb, Box::new(|| ()) as Box<dyn FnMut()>);
            if interval.is_none() {
                v.swap_remove(pos);
            }
            (interval, cb)
        });
        cb();
        // HTML microtask checkpoint: drain queued `queueMicrotask` callbacks
        // after each timer fire, before the next timer runs.
        wpt_drain_microtasks();
        if let ::std::option::Option::Some(ms) = interval {
            if !WPT_DONE.with(|d| d.get()) {
                WPT_TIMERS.with(|t| {
                    let mut v = t.borrow_mut();
                    if let Some(pos) = v.iter().position(|e| e.id == id) {
                        v[pos].cb = cb;
                        v[pos].when_ms = v[pos].when_ms.saturating_add(ms);
                        v[pos].seq = wpt_next_seq();
                    }
                });
            }
        }
    }
}
"#;

/// The `serde_json::Value` `DsSameValue` impl — emitted only when both `Assert`
/// and `SerdeJson` are flagged (see `RuntimeDeps::helper_module`). A scalar JSON
/// value projects to its `DsCmp`; array/object operands have no static reference
/// identity (ES SameValue on objects is reference-equality, which a value-typed
/// `serde_json::Value` cannot express), so they panic — test262 covers them via
/// `compareArray`/`deepEqual` on the engine path, never reaching this arm.
pub const ASSERT_VALUE_HELPER: &str = r#"
impl DsSameValue for serde_json::Value {
    #[inline]
    fn ds_cmp(&self) -> DsCmp<'_> {
        match self {
            serde_json::Value::Null => DsCmp::Undefined,
            serde_json::Value::Bool(b) => DsCmp::Bool(*b),
            serde_json::Value::Number(n) => DsCmp::Num(n.as_f64().unwrap_or(f64::NAN)),
            serde_json::Value::String(s) => DsCmp::Str(s.as_str()),
            _ => panic!("DsSameValue: array/object Value operand has no static reference identity"),
        }
    }
}
"#;

pub const COLLECTION_KEY_HELPER: &str = r#"
/// A `number` used as a `Set`/`Map` key. ES `Set`/`Map` compare keys by
/// SameValueZero, but Rust `f64` lacks `Eq`/`Hash` (NaN breaks reflexivity), so
/// `Set<number>`/`Map<number, _>` lower to `HashSet<DsF64Key>`/`HashMap<DsF64Key, V>`.
/// SameValueZero: `+0 === -0` (sign collapsed) and `NaN === NaN` (all NaN bit
/// patterns collapse to one), so values that are SameValueZero-equal hash equal.
#[derive(Clone, Copy)]
pub struct DsF64Key(pub f64);

impl PartialEq for DsF64Key {
    fn eq(&self, other: &Self) -> bool {
        let (a, b) = (self.0, other.0);
        (a.is_nan() && b.is_nan()) || a == b || (a == 0.0 && b == 0.0)
    }
}
impl Eq for DsF64Key {}

impl std::hash::Hash for DsF64Key {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        let bits = if self.0 == 0.0 {
            0u64
        } else if self.0.is_nan() {
            0x7ff8_0000_0000_0000u64
        } else {
            self.0.to_bits()
        };
        bits.hash(state);
    }
}

impl std::fmt::Debug for DsF64Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
"#;

/// ES `Math.max`/`Math.min` — differs from Rust `f64::max`/`min` on two edges:
/// any `NaN` argument yields `NaN` (Rust returns the other operand), and
/// `+0`/`-0` are ordered (`Math.max(-0, +0)` = `+0`, `Math.min(-0, +0)` = `-0`;
/// Rust returns the left operand when they compare equal). Variadic `max`/`min`
/// folds these left to right.
pub const F64_MAXMIN_HELPER: &str = r#"
pub fn ds_f64_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        ::core::f64::NAN
    } else if a != b {
        if a > b { a } else { b }
    } else if a == 0.0 && (a.is_sign_positive() || b.is_sign_positive()) {
        0.0
    } else {
        a
    }
}
pub fn ds_f64_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        ::core::f64::NAN
    } else if a != b {
        if a < b { a } else { b }
    } else if a == 0.0 && (a.is_sign_negative() || b.is_sign_negative()) {
        -0.0f64
    } else {
        a
    }
}
"#;

pub const DISPLAY_HELPER: &str = r#"
pub trait DsDisplay {
    fn ds_display(&self) -> String;
}

/// Free-function form the translator emits (`__ds::display(&expr)`) for a
/// template-literal interpolation or concatenation. ES rendering: `None`
/// (undefined/null) -> "undefined", a boolean -> "true"/"false", an array ->
/// elements joined by ",", an object -> "[object Object]".
#[inline]
pub fn display<T: DsDisplay>(x: &T) -> String {
    x.ds_display()
}

impl DsDisplay for String {
    #[inline]
    fn ds_display(&self) -> String {
        self.clone()
    }
}

impl DsDisplay for str {
    #[inline]
    fn ds_display(&self) -> String {
        self.to_string()
    }
}

/// `&T` displays as `T` — a template-literal interpolation or `+` concat of a
/// borrowed value (a `for…of` loop variable bound by reference, a `&String`
/// field, etc.) reaches `display(&expr)` as a `&T`. Without this a borrowed
/// operand surfaces as `E0277: the trait bound \`&String: DsDisplay\` is not
/// satisfied`; the blanket forwards so any `T: DsDisplay` borrows for free.
impl<T: DsDisplay + ?Sized> DsDisplay for &T {
    #[inline]
    fn ds_display(&self) -> String {
        (**self).ds_display()
    }
}

impl DsDisplay for bool {
    #[inline]
    fn ds_display(&self) -> String {
        if *self { "true".to_string() } else { "false".to_string() }
    }
}

impl DsDisplay for () {
    #[inline]
    fn ds_display(&self) -> String {
        "undefined".to_string()
    }
}

impl<T: DsDisplay> DsDisplay for Option<T> {
    #[inline]
    fn ds_display(&self) -> String {
        match self {
            Some(x) => x.ds_display(),
            None => "undefined".to_string(),
        }
    }
}

impl<T: DsDisplay> DsDisplay for Vec<T> {
    #[inline]
    fn ds_display(&self) -> String {
        let mut s = String::new();
        for (i, x) in self.iter().enumerate() {
            if i > 0 { s.push(','); }
            s.push_str(&x.ds_display());
        }
        s
    }
}

impl<K, V> DsDisplay for std::collections::HashMap<K, V> {
    #[inline]
    fn ds_display(&self) -> String {
        "[object Object]".to_string()
    }
}
"#;

pub const INSPECT_HELPER: &str = r#"
use std::collections::{HashMap, HashSet};

/// Node `console.log`/`util.inspect` rendering — distinct from [`DsDisplay`]
/// (ES `ToString`: an object is "[object Object]", an array joins by ","). A
/// `console.log` argument that is not a primitive routes through `inspect`;
/// nested composites recurse to `depth` (Node's default 2), then collapse to
/// `[Array]`/`[Object]`. A top-level `console.log` STRING prints verbatim (the
/// translator keeps it on `Display`); a NESTED string quotes (`'x'`).
///
/// Static-first, per the scriptc/boa precedent: one trait, monomorphized per
/// concrete type, so a `Vec<String>`/`HashMap<String, V>` lowers to zero-cost
/// Rust (no dynamic value enum). A `console.log` of a bare `Vec`/`HashMap`
/// would otherwise not compile (std collections have no `Display`).
pub trait DsInspect {
    fn ds_inspect(&self, recurse: u32, depth: i32) -> String;
}

/// Render `x` the way Node's `console.log` prints a non-primitive argument, at
/// the Node default depth (2). The translator emits `__ds::inspect(&expr)`.
#[inline]
pub fn inspect<T: DsInspect>(x: &T) -> String {
    x.ds_inspect(0, 2)
}

/// A nested string quotes with single quotes, escaping `'`/`\`/control —
/// Node's inspect quoting (the common case; the full quote ladder — single →
/// double → backtick — is a later refinement).
fn inspect_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        match c {
            '\'' => out.push_str("\\'"),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out.push('\'');
    out
}

/// Render an `f64` as an ES `Number` string (ryu-js: `1.0` → "1", `1e21` →
/// "1e+21"). Shared by the scalar `f64` impl and the `serde_json::Value`
/// `Number` arm.
fn inspect_num(n: f64) -> String {
    if n == 0.0 {
        "0".to_string()
    } else {
        ryu_js::Buffer::new().format(n).to_string()
    }
}

impl DsInspect for f64 {
    #[inline]
    fn ds_inspect(&self, _recurse: u32, _depth: i32) -> String {
        inspect_num(*self)
    }
}

impl DsInspect for bool {
    #[inline]
    fn ds_inspect(&self, _recurse: u32, _depth: i32) -> String {
        if *self { "true".to_string() } else { "false".to_string() }
    }
}

impl DsInspect for String {
    #[inline]
    fn ds_inspect(&self, _recurse: u32, _depth: i32) -> String {
        inspect_quote(self)
    }
}

impl DsInspect for str {
    #[inline]
    fn ds_inspect(&self, _recurse: u32, _depth: i32) -> String {
        inspect_quote(self)
    }
}

impl DsInspect for () {
    #[inline]
    fn ds_inspect(&self, _recurse: u32, _depth: i32) -> String {
        "undefined".to_string()
    }
}

impl<T: DsInspect> DsInspect for Option<T> {
    #[inline]
    fn ds_inspect(&self, recurse: u32, depth: i32) -> String {
        match self {
            Some(x) => x.ds_inspect(recurse, depth),
            None => "null".to_string(),
        }
    }
}

impl<T: DsInspect> DsInspect for Vec<T> {
    #[inline]
    fn ds_inspect(&self, recurse: u32, depth: i32) -> String {
        if self.is_empty() {
            return "[]".to_string();
        }
        if depth >= 0 && recurse > depth as u32 {
            return "[Array]".to_string();
        }
        let mut s = String::from("[ ");
        for (i, x) in self.iter().enumerate() {
            if i > 0 {
                s.push_str(", ");
            }
            s.push_str(&x.ds_inspect(recurse + 1, depth));
        }
        s.push_str(" ]");
        s
    }
}

impl<V: DsInspect> DsInspect for HashMap<String, V> {
    #[inline]
    fn ds_inspect(&self, recurse: u32, depth: i32) -> String {
        if self.is_empty() {
            return "{}".to_string();
        }
        if depth >= 0 && recurse > depth as u32 {
            return "[Object]".to_string();
        }
        let mut s = String::from("{ ");
        for (i, (k, v)) in self.iter().enumerate() {
            if i > 0 {
                s.push_str(", ");
            }
            s.push_str(k);
            s.push_str(": ");
            s.push_str(&v.ds_inspect(recurse + 1, depth));
        }
        s.push_str(" }");
        s
    }
}

impl<T: DsInspect> DsInspect for HashSet<T> {
    #[inline]
    fn ds_inspect(&self, recurse: u32, depth: i32) -> String {
        if self.is_empty() {
            return "Set(0) {}".to_string();
        }
        if depth >= 0 && recurse > depth as u32 {
            return "[Set]".to_string();
        }
        let mut s = String::from("Set { ");
        for (i, x) in self.iter().enumerate() {
            if i > 0 {
                s.push_str(", ");
            }
            s.push_str(&x.ds_inspect(recurse + 1, depth));
        }
        s.push_str(" }");
        s
    }
}

/// A `serde_json::Value` (a `JSON.parse` result) renders the way Node prints
/// the parsed JS value: a top-level string prints verbatim (`abc`, not the
/// JSON `"abc"` a bare `Value: Display` would emit), while a string nested in
/// an array/object quotes (`'abc'`). Without this, `console.log(JSON.parse(
/// '"abc"'))` printed `"abc"` — a `serde_json::Value` has no other
/// `Display`-free path. Key order follows `serde_json`'s default `Map`
/// (sorted), which diverges from Node's insertion order — the same limitation
/// as the `HashMap` impl above.
impl DsInspect for serde_json::Value {
    fn ds_inspect(&self, recurse: u32, depth: i32) -> String {
        // Fully-qualified variants: a `use serde_json::Value::*` glob would
        // shadow the `String` *type* with the `Value::String` *variant*, so
        // `String::from(...)` below would resolve to the variant.
        match self {
            serde_json::Value::Null => "null".to_string(),
            serde_json::Value::Bool(b) => {
                if *b {
                    "true".to_string()
                } else {
                    "false".to_string()
                }
            }
            serde_json::Value::Number(n) => inspect_num(n.as_f64().unwrap_or(f64::NAN)),
            serde_json::Value::String(s) => {
                // Top-level: raw (Node prints the string as-is). Nested: quoted.
                if recurse == 0 {
                    s.clone()
                } else {
                    inspect_quote(s)
                }
            }
            serde_json::Value::Array(arr) => {
                if arr.is_empty() {
                    return "[]".to_string();
                }
                if depth >= 0 && recurse > depth as u32 {
                    return "[Array]".to_string();
                }
                let mut s = String::from("[ ");
                for (i, e) in arr.iter().enumerate() {
                    if i > 0 {
                        s.push_str(", ");
                    }
                    s.push_str(&e.ds_inspect(recurse + 1, depth));
                }
                s.push_str(" ]");
                s
            }
            serde_json::Value::Object(obj) => {
                if obj.is_empty() {
                    return "{}".to_string();
                }
                if depth >= 0 && recurse > depth as u32 {
                    return "[Object]".to_string();
                }
                let mut s = String::from("{ ");
                for (i, (k, v)) in obj.iter().enumerate() {
                    if i > 0 {
                        s.push_str(", ");
                    }
                    // Node prints an object key bare when it is a valid
                    // identifier, quoted otherwise; the common JSON-parse case
                    // is identifier keys, so print bare (consistent with the
                    // `HashMap<String, V>` impl above).
                    s.push_str(k);
                    s.push_str(": ");
                    s.push_str(&v.ds_inspect(recurse + 1, depth));
                }
                s.push_str(" }");
                s
            }
        }
    }
}
"#;
