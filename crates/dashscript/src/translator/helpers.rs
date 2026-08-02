//! Emitted `__ds`/`__ds_engine` runtime helper sources — the `const &str`
//! slices concatenated into the generated `src/__ds.rs` (and the engine
//! module). Each slice maps to a [`super::RuntimeDep`]; [`super::RuntimeDeps`]
//! concatenates whichever a translation flagged (see `helper_module` /
//! `engine_helper_module`).

/// The DashScript runtime helper module, written to `src/__ds.rs` and declared
/// `mod __ds;` at each crate root when a translated file references it. The
/// single source for the `__ds` helpers — consumed by both `ds build` (bin) and
/// the conformance harness (lib test) — so the helper text lives in the library
/// rather than either consumer. [`RuntimeDeps::helper_module`] concatenates
/// whichever slices a translation flagged.
pub(super) const ERROR_HELPER: &str = r##"/// An ECMAScript error object lowered through Rust `panic!`/`catch_unwind`.
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

pub(super) const RYUJS_HELPERS: &str = "\
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
pub(super) const ARRAY_HELPER: &str = "\
/// ES indexed assignment `arr[i] = v` for a `Vec<T>`. ES `Array` auto-grows:
/// `i < len` replaces, `i == len` appends, `i > len` grows with `T::default()`
/// filling the gap (a JS array would use holes, but `T` has no undefined). A
/// negative, non-integer, or `>= 2^32-1` index is a property set in JS, not an
/// element — dropped here (length stays as it was). A bare Rust `vec[i] = v`
/// would panic instead of growing. ES arrays are sparse (holes are free); a
/// dense `Vec` must fill the gap, so a huge gap (e.g. `x[4294967294] = v` on
/// an empty array) is capped to avoid allocating ~32GB and hanging — beyond
/// the cap the store is dropped, honestly failing any later assert.
#[inline]
pub fn array_set<T: Default + Clone>(arr: &mut Vec<T>, i: f64, v: T) {
    if !i.is_finite() || i < 0.0 || i.fract() != 0.0 {
        return;
    }
    let idx = i as usize;
    // An ES array index must be < 2^32-1; at or above it the store is a
    // property set (length unchanged). Vec has no such property, so drop it.
    if idx >= u32::MAX as usize {
        return;
    }
    if idx < arr.len() {
        arr[idx] = v;
    } else if idx == arr.len() {
        arr.push(v);
    } else {
        // Sparse-gap cap: ES holes cost nothing, but a dense Vec must fill the
        // gap with `T::default()`. Drop the store past the cap rather than OOM.
        const SPARSE_GAP_CAP: usize = 1 << 20;
        if idx - arr.len() > SPARSE_GAP_CAP {
            return;
        }
        arr.resize(idx + 1, T::default());
        arr[idx] = v;
    }
}
";

/// WHATWG Encoding API helpers — `__ds::TextEncoder`/`__ds::TextDecoder`. The
/// Encoding API is UTF-8 only, so no encoding table: `encode` is
/// `String::into_bytes` (zero-copy), `decode` is `String::from_utf8_lossy`
/// (invalid bytes become U+FFFD, matching the ES `fatal: false` default). Both
/// structs are stateless, so a single shared instance is sound.
pub(super) const ENCODING_HELPER: &str = "\
pub struct TextEncoder;
impl TextEncoder {
    #[inline]
    pub fn new() -> Self {
        TextEncoder
    }
    #[inline]
    pub fn encode(&self, s: String) -> Vec<u8> {
        s.into_bytes()
    }
}
#[allow(dead_code)]
pub struct TextDecoder;
#[allow(dead_code)]
impl TextDecoder {
    #[inline]
    pub fn new() -> Self {
        TextDecoder
    }
    #[inline]
    pub fn decode(&self, bytes: Vec<u8>) -> String {
        String::from_utf8_lossy(&bytes).into_owned()
    }
}
";

/// WHATWG URL API helper — `__ds::DsUrlSearchParams`. An ordered name/value
/// list (ES `URLSearchParams` preserves insertion order), backed by
/// `Vec<(String, String)>`. Parsing and serialization route through
/// `form_urlencoded` (the WHATWG `application/x-www-form-urlencoded` reference
/// parser — the same one servo/url uses), so `+`→space and `%xx`
/// percent-decoding/encoding match the spec. `toString` is `Display`, so
/// template-literal interpolation of a `URLSearchParams` works without a
/// separate `DsDisplay` impl.
pub(super) const URL_HELPER: &str = "\
pub struct DsUrlSearchParams {
    pairs: Vec<(String, String)>,
}
impl DsUrlSearchParams {
    /// `new URLSearchParams(s)` — parse `s` as
    /// `application/x-www-form-urlencoded`. `form_urlencoded::parse` splits on
    /// `&`/`=` and percent-decodes each side (`+`→space), matching the spec.
    /// Generic over `AsRef<str>` so the constructor emit passes either a
    /// `String` or a `&str` literal unchanged.
    pub fn from_query<S: AsRef<str>>(init: S) -> Self {
        let pairs = form_urlencoded::parse(init.as_ref().as_bytes())
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        Self { pairs }
    }
    /// `new URLSearchParams()` / `new URLSearchParams(undefined)` — empty.
    pub fn new() -> Self {
        Self { pairs: Vec::new() }
    }
    /// `params.get(name)` — the first value for `name`, or `None` (ES `null`).
    /// Generic over `AsRef<str>` so a `String` or `&str` argument (both TS
    /// `string`) is accepted without a call-site borrow.
    pub fn get<S: AsRef<str>>(&self, name: S) -> Option<String> {
        let name = name.as_ref();
        self.pairs.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone())
    }
    /// `params.has(name)` — whether any pair's name is `name`.
    pub fn has<S: AsRef<str>>(&self, name: S) -> bool {
        let name = name.as_ref();
        self.pairs.iter().any(|(k, _)| k == name)
    }
    /// `params.has(name, value)` (ES2024) — whether a `(name, value)` pair
    /// exists. The single-arg `has(name)` is the common form; the two-arg
    /// form matches both name and value.
    pub fn has_value<N: AsRef<str>, V: AsRef<str>>(&self, name: N, value: V) -> bool {
        let name = name.as_ref();
        let value = value.as_ref();
        self.pairs.iter().any(|(k, v)| k == name && v == value)
    }
    /// `params.set(name, value)` — WHATWG set: update the first matching pair's
    /// value in place, drop any later matches, or append if none. Not
    /// delete-all-then-append — that would move the pair to the end; the spec
    /// keeps the first match position: `set('a','B')` on `'a=b&c=d'` yields
    /// `a=B&c=d`.
    pub fn set<N: AsRef<str>, V: AsRef<str>>(&mut self, name: N, value: V) {
        let name = name.as_ref();
        let value = value.as_ref().to_string();
        let mut found = false;
        // Keep the first match (to update in place), drop later matches.
        self.pairs.retain(|(k, _)| {
            if k == name {
                if found {
                    false
                } else {
                    found = true;
                    true
                }
            } else {
                true
            }
        });
        if found {
            for pair in &mut self.pairs {
                if pair.0 == name {
                    pair.1 = value;
                    break;
                }
            }
        } else {
            self.pairs.push((name.to_string(), value));
        }
    }
    /// `params.append(name, value)` — append a pair (duplicates kept).
    pub fn append<N: AsRef<str>, V: AsRef<str>>(&mut self, name: N, value: V) {
        self.pairs
            .push((name.as_ref().to_string(), value.as_ref().to_string()));
    }
    /// `params.delete(name)` — remove every pair named `name`.
    pub fn delete<S: AsRef<str>>(&mut self, name: S) {
        let name = name.as_ref();
        self.pairs.retain(|(k, _)| k != name);
    }
    /// `params.delete(name, value)` (ES2024) — remove only pairs matching both
    /// `name` and `value`; the single-arg `delete(name)` removes every pair
    /// with that name.
    pub fn delete_value<N: AsRef<str>, V: AsRef<str>>(&mut self, name: N, value: V) {
        let name = name.as_ref();
        let value = value.as_ref();
        self.pairs.retain(|(k, v)| !(k == name && v == value));
    }
    /// `params.getAll(name)` — every value for `name`, in insertion order.
    pub fn get_all<S: AsRef<str>>(&self, name: S) -> Vec<String> {
        let name = name.as_ref();
        self.pairs
            .iter()
            .filter(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
            .collect()
    }
    /// `params.sort()` — sort by name. Rust's `sort_by` is stable, matching
    /// ES (equal names keep their relative order).
    pub fn sort(&mut self) {
        self.pairs.sort_by(|a, b| a.0.cmp(&b.0));
    }
    /// `params.size` — the number of name/value pairs.
    #[inline]
    pub fn len(&self) -> usize {
        self.pairs.len()
    }
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.pairs.is_empty()
    }
}
impl ::core::fmt::Display for DsUrlSearchParams {
    /// `params.toString()` — serialize back to
    /// `application/x-www-form-urlencoded`. `form_urlencoded::Serializer`
    /// percent-encodes per the WHATWG byte set.
    fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
        let mut s = form_urlencoded::Serializer::new(String::new());
        for (k, v) in &self.pairs {
            s.append_pair(k, v);
        }
        write!(f, \"{}\", s.finish())
    }
}
";

/// ES truthiness for a value used in condition position. The translator emits
/// `__ds::truthy(&expr)` for a non-boolean condition (member access like
/// `opts.indent`, a numeric cast, a call) it cannot lower without a type
/// checker; the Rust compiler picks the matching impl by inferred type. ES
/// falsiness: `0`, `NaN`, `""`, `null`/`undefined` (`None`); everything else is
/// truthy — including empty arrays/objects (an ES quirk vs Python). Pure `std`.
pub(super) const TRUTHY_HELPER: &str = "\
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

pub(super) const ASSERT_HELPER: &str = r#"
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
    Unit,
    /// `undefined` — an `Option<T>`'s `None`, or `serde_json::Value::Null`.
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
            (DsCmp::Unit, DsCmp::Unit) => true,
            // `undefined` SameValue `undefined` (an Option's None, or Null).
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

impl DsSameValue for () {
    #[inline]
    fn ds_cmp(&self) -> DsCmp<'_> {
        DsCmp::Unit
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
// (`async_test`/`promise_test`) stay `unsupported` — the WinterTC path is
// static-only (degrade-don't-reject does not apply to Web APIs).

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
"#;

/// The `serde_json::Value` `DsSameValue` impl — emitted only when both `Assert`
/// and `SerdeJson` are flagged (see `RuntimeDeps::helper_module`). A scalar JSON
/// value projects to its `DsCmp`; array/object operands have no static reference
/// identity (ES SameValue on objects is reference-equality, which a value-typed
/// `serde_json::Value` cannot express), so they panic — test262 covers them via
/// `compareArray`/`deepEqual` on the engine path, never reaching this arm.
pub(super) const ASSERT_VALUE_HELPER: &str = r#"
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

pub(super) const COLLECTION_KEY_HELPER: &str = r#"
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
pub(super) const F64_MAXMIN_HELPER: &str = r#"
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

pub(super) const DISPLAY_HELPER: &str = r#"
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

pub(super) const INSPECT_HELPER: &str = r#"
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
pub(super) const WORKER_HELPER: &str = "\
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
        // handler panic surfaces via `join`'s `Err`: we resume the unwind so a
        // worker that throws uncaught is not silently swallowed — matching a
        // synchronous handler call that throws. The `thread::panicking()` guard
        // avoids a double-panic abort when this Worker is dropped while the main
        // thread is already unwinding; there the in-flight panic propagates and
        // the worker's is lost (the only safe choice). `reply_rx` drops with the
        // Worker — pending un-received replies are lost (main didn't recv them).
        drop(self.tx.take());
        if let Some(handle) = self.handle.take() {
            if let Err(payload) = handle.join() {
                if !std::thread::panicking() {
                    std::panic::resume_unwind(payload);
                }
            }
        }
    }
}
";

/// ES RegExp helpers — `__ds::regex` compiles a `/pat/flags` literal to a
/// `regress::Regex`. `regress` implements ES regex semantics (backreferences,
/// lookaround, unicode case folding) the `regex` crate cannot express. Only
/// emitted when a translated file uses a regex literal, so a plain `ds build`
/// pulls no `regress` dependency.
pub(super) const REGRESS_HELPERS: &str = r##"
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
/// (ASCII == UTF-16 code-unit index); `input` is the haystack. `groups` carries
/// the named captures in source order (`None` when the pattern had no named
/// groups, so ES `m.groups` is `undefined`); each entry's `Option<String>` is
/// `None` when that group did not participate (ES `undefined`).
pub struct DsMatch {
    pub captures: Vec<Option<String>>,
    pub index: usize,
    pub input: String,
    pub groups: Option<Vec<(String, Option<String>)>>,
}

impl DsMatch {
    /// `m.groups.name` — the named capture's value, or `None` if the group did
    /// not participate (ES `undefined`). Duplicate named groups: regress'
    /// `named_groups` already collapses duplicates to one entry preferring the
    /// matched branch, matching ES `groups.x` semantics.
    #[inline]
    pub fn group_named(&self, name: &str) -> Option<String> {
        self.groups
            .as_ref()?
            .iter()
            .find(|(n, _)| n == name)
            .and_then(|(_, v)| v.clone())
    }
}

/// Build a `DsMatch` from one regress `Match` — shared by `regex_match`
/// (re-compiles from a source pattern) and the variable `.exec` lowering (uses
/// an already-compiled `Regex`). regress' `groups()` yields group 0 (the whole
/// match) followed by the capture groups — exactly the ES `m[0]`/`m[1]`/…
/// layout, so no manual whole-match prefix (that would shift every group).
/// `named_groups()` yields each named group once (collapsing duplicates) with
/// its matched range, so ES `groups.x` reflects whichever branch matched.
#[inline]
pub fn ds_match_from(text: &str, m: &Match) -> DsMatch {
    let captures: Vec<Option<String>> =
        m.groups().map(|g| g.map(|r| text[r].to_string())).collect();
    let named: Vec<(String, Option<String>)> = m
        .named_groups()
        .map(|(name, range)| (name.to_string(), range.map(|r| text[r].to_string())))
        .collect();
    DsMatch {
        captures,
        index: m.range().start,
        input: text.to_string(),
        groups: if named.is_empty() { None } else { Some(named) },
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
            ", index: {}, input: '{}'",
            self.index,
            ds_inspect_str(&self.input)
        )?;
        match &self.groups {
            // `groups: undefined` — the pattern had no named groups.
            None => write!(f, ", groups: undefined ]"),
            // Node inspects the groups object as `{ name: 'val', name2: undefined }`.
            Some(ng) => {
                let entries: Vec<String> = ng
                    .iter()
                    .map(|(n, v)| match v {
                        Some(s) => format!("{}: '{}'", n, ds_inspect_str(s)),
                        None => format!("{}: undefined", n),
                    })
                    .collect();
                write!(f, ", groups: {{ {} }} ]", entries.join(", "))
            }
        }
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

/// ES `GetSubstitution` for a literal (string) search — the `$` patterns in a
/// `replace`/`replaceAll` replacement string. `$$`→`$`, `$&`→the matched text,
/// `` $` ``→the text before the match, `$'`→the text after; `$n`/`$<…>` are
/// literal (a string search carries no captures). Pushes the expanded
/// replacement for one match at byte range `[start, end)` onto `out`. The byte
/// offsets come from `str::find`, which always lands on a UTF-8 boundary, so the
/// `text[..start]`/`text[end..]` slices are valid for non-BMP haystacks too.
pub(super) const STRING_REPLACE_HELPER: &str = r##"
fn expand_literal_replacement(repl: &str, text: &str, start: usize, end: usize, out: &mut String) {
    let mut chars = repl.chars();
    while let Some(c) = chars.next() {
        if c != '$' {
            out.push(c);
            continue;
        }
        match chars.clone().next() {
            Some('$') => { out.push('$'); chars.next(); }
            Some('&') => { out.push_str(&text[start..end]); chars.next(); }
            Some('`') => { out.push_str(&text[..start]); chars.next(); }
            Some('\'') => { out.push_str(&text[end..]); chars.next(); }
            // Lone `$` (end of replacement) or `$X` with no capture to splice —
            // emit a literal `$`; the next char is reprocessed next iteration.
            _ => out.push('$'),
        }
    }
}

/// `s.replaceAll(search, repl)` — every occurrence replaced, with `repl`'s `$`
/// patterns expanded per ES `GetSubstitution` at each match (`` $` ``/`$'`
/// reflect that match's position). Rust's `str::replace` treats `repl` literally,
/// so a `$&` would survive verbatim. Empty `search` is rare; its ES semantics
/// (a match at every code-unit boundary) fall back to Rust's literal `replace`.
#[inline]
pub fn ds_replace_all(haystack: &str, needle: &str, repl: &str) -> String {
    if needle.is_empty() {
        return haystack.replace(needle, repl);
    }
    let mut out = String::with_capacity(haystack.len() + repl.len());
    let mut last = 0usize;
    let mut search = 0usize;
    while let Some(rel) = haystack[search..].find(needle) {
        let start = search + rel;
        let end = start + needle.len();
        out.push_str(&haystack[last..start]);
        expand_literal_replacement(repl, haystack, start, end, &mut out);
        last = end;
        search = end;
    }
    out.push_str(&haystack[last..]);
    out
}

/// `s.replace(search, repl)` — the first occurrence only (ES replaces the first
/// match; Rust's `replacen(.., 1)` treats `repl` literally, leaving `$&` intact).
/// The haystack is returned unchanged on no match.
#[inline]
pub fn ds_replace(haystack: &str, needle: &str, repl: &str) -> String {
    if needle.is_empty() {
        return haystack.replacen(needle, repl, 1);
    }
    match haystack.find(needle) {
        None => haystack.to_string(),
        Some(start) => {
            let end = start + needle.len();
            let mut out = String::with_capacity(haystack.len() + repl.len());
            out.push_str(&haystack[..start]);
            expand_literal_replacement(repl, haystack, start, end, &mut out);
            out.push_str(&haystack[end..]);
            out
        }
    }
}
"##;

/// The DashScript compat engine module, written to `src/__ds_engine.rs` and
/// declared `mod __ds_engine;` at the crate root when a translated file uses ES
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
pub(super) const ENGINE_HELPER_MODULE: &str = r##"//! DashScript compat engine: run a `.ts` source, or a single function's
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

/// Build-time-resolved `.js` module table: ESM specifier → inlined source. The
/// translator reads each degraded `.js` module's source at build time and
/// emits a `register_js_module(specifier, source)` call, so the `Loader`'s
/// `source_of` is a table lookup — the emitted crate is self-contained (no
/// runtime `.js` files), and node_modules resolution already happened at build
/// time, so the engine never walks the filesystem to resolve an `import`.
static JS_MODULES: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

/// Register a degraded `.js` module's source so the engine's `Loader` can find
/// it. The translator emits one call per `.js` module that degrades to the
/// engine, inlining the source at build time. Idempotent — a stub `fn`
/// re-registers on every call, so a module imported through several stubs
/// registers once.
pub fn register_js_module(specifier: &str, source: &str) {
    let mut v = JS_MODULES.lock().expect("JS_MODULES lock");
    if !v.iter().any(|(s, _)| s == specifier) {
        v.push((specifier.to_string(), source.to_string()));
    }
}

/// Read a module's source: the runtime `JS_MODULES` table first (a stub's
/// `register_js_module` call), then the build-time `__DS_MODULE_SOURCES`
/// table — so a module with no `export function` (no stub emitted, never
/// registered at runtime) still resolves.
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

/// Run a self-contained `.ts` source under QuickJS with `console.log` wired to
/// stdout. The source declares `main()` and calls it (pure-TS execution
/// semantics), so a single eval runs the fixture.
pub fn run(source: &str) {
    let result = RUNTIME.with(|runtime| -> rquickjs::Result<()> {
        let ctx = Context::full(runtime).expect("rquickjs Context");
        ctx.with(|ctx: Ctx<'_>| {
            wire_console(&ctx)?;
            ctx.eval_with_options::<(), _>(source, sloppy())?;
            Ok(())
        })
    });
    result.expect("rquickjs eval");
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
            ctx.eval_with_options::<(), _>(body_js, sloppy())?;
            let js_args = Array::new(ctx.clone())?;
            for (i, a) in args.iter().enumerate() {
                js_args.set(i, json_to_js(&ctx, a)?)?;
            }
            ctx.globals().set("__ds_call_args", js_args)?;
            let expr = format!("{fn_name}(...__ds_call_args)");
            let ret: Value = ctx.eval_with_options::<Value, _>(expr, sloppy())?;
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
            ensure_module_installed(&ctx, module_key)?;
            let js_args = Array::new(ctx.clone())?;
            for (i, a) in args.iter().enumerate() {
                js_args.set(i, json_to_js(&ctx, a)?)?;
            }
            ctx.globals().set("__ds_call_args", js_args)?;
            let expr = format!("__ds_modules['{module_key}'].{fn_name}(...__ds_call_args)");
            let ret: Value = ctx.eval_with_options::<Value, _>(expr, sloppy())?;
            let _ = ctx.globals().remove("__ds_call_args");
            js_to_json(&ctx, ret)
        })
    });
    result.unwrap_or_else(|e| panic!("rquickjs call_module_fn({module_key}.{fn_name}): {e:?}"))
}
"##;
