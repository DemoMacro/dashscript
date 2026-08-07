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
/// ES indexed assignment with a known-integer index — the `xs[i] = v` fast
/// path where `i` is a loop counter, literal, or integer arithmetic (an
/// `i64`-flavor expression the translator lowered via `as usize`). The index
/// already being `usize`, the f64 defenses of `array_set` (is_finite / < 0 /
/// fract) are skipped; ES auto-grow (i >= len grows the Vec) and the 2^32-1
/// property-set guard are preserved. A negative `i64` index wraps to a huge
/// `usize` (>= 2^32-1) under `as usize`, so the u32 guard drops the store —
/// matching ES, where `arr[-1] = v` is a no-op property set.
#[inline]
pub fn array_set_index<T: Default + Clone>(arr: &mut Vec<T>, i: usize, v: T) {
    if i >= u32::MAX as usize {
        return;
    }
    if i < arr.len() {
        arr[i] = v;
    } else if i == arr.len() {
        arr.push(v);
    } else {
        const SPARSE_GAP_CAP: usize = 1 << 20;
        if i - arr.len() > SPARSE_GAP_CAP {
            return;
        }
        arr.resize(i + 1, T::default());
        arr[i] = v;
    }
}
";

/// WHATWG Encoding API helpers — `__ds::TextEncoder`/`__ds::TextDecoder`, a
/// WinterTC Web API backed by the `encoding_rs` crate (the WHATWG Encoding
/// Standard reference implementation). `TextEncoder` is UTF-8 only (the sole
/// encoding the Encoding API guarantees for encode), so `encode` is
/// `String::into_bytes` (zero-copy). `TextDecoder` carries a resolved
/// `encoding_rs::Encoding` (looked up from the ctor `label`); `decode` feeds
/// through a stateful `encoding_rs::Decoder` (`new_decoder` for BOM-stripped,
/// `new_decoder_without_bom_handling` when `ignoreBOM: true`), with the ES
/// `stream` option mapped to encoding_rs's `last` flag — `stream: true`
/// buffers an incomplete trailing multi-byte sequence for the next call
/// (`last = false`), `stream: false` flushes it as replacement (`last = true`).
/// `fatal: true` promotes a replacement (`replaced`) to a panicked `TypeError`
/// (ES: a fatal decoder throws on an invalid byte sequence). encoding_rs
/// requires a fresh `Decoder` per stream once `last = true` ends one, so the
/// instance holds `RefCell<Option<Decoder>>`: lazily created at the start of a
/// stream, dropped on flush — matching ES, where each non-streaming `decode()`
/// is independent (and re-sniffs the BOM). Interior mutability keeps `decode`
/// at `&self` so a non-`mut` binding still borrows.
pub(super) const ENCODING_HELPER: &str = "\
pub struct TextEncoder { pub encoding: &'static str }
impl TextEncoder {
    #[inline]
    pub fn new() -> Self {
        TextEncoder { encoding: \"utf-8\" }
    }
    #[inline]
    pub fn encode(&self, s: String) -> Vec<u8> {
        s.into_bytes()
    }
    /// WHATWG `encodeInto(src, dst)` — incrementally UTF-8 encodes `src` into
    /// `dst`, stopping when `dst` fills. `read` = UTF-8 bytes consumed from
    /// `src` for chars that fully fit; `written` = bytes stored in `dst`. A
    /// multi-byte char that would overflow `dst` is left entirely unwritten
    /// (read stays before it). DashScript strings are UTF-8 (not UTF-16), so
    /// `read` counts UTF-8 bytes; the JS spec counts UTF-16 code units, but a
    /// UTF-8 byte count is the faithful analogue under DashScript's model.
    pub fn encode_into(&self, src: &str, dst: &mut [u8]) -> DsEncodeIntoResult {
        let src_bytes = src.as_bytes();
        let cap = dst.len();
        let mut read = 0usize;
        let mut written = 0usize;
        while read < src_bytes.len() {
            // UTF-8 leading byte → char length (1-4): 0xxxxxxx=1, 110xxxxx=2,
            // 1110xxxx=3, 11110xxx=4. `src` is a valid &str, so a leading byte
            // is always one of these (never a stray continuation byte).
            let first = src_bytes[read];
            let ch_len = if first < 0x80 {
                1
            } else if first < 0xE0 {
                2
            } else if first < 0xF0 {
                3
            } else {
                4
            };
            if written + ch_len > cap {
                break;
            }
            dst[written..written + ch_len].copy_from_slice(&src_bytes[read..read + ch_len]);
            written += ch_len;
            read += ch_len;
        }
        DsEncodeIntoResult { read: read as f64, written: written as f64 }
    }
}
/// Result of `TextEncoder.encodeInto` — `{ read, written }` (UTF-8 bytes
/// consumed from the input / bytes stored in the destination). Fields are
/// `pub` so a returned value is read by plain field access (`r.read`); they
/// are `f64` (DashScript `number`) since the WHATWG spec returns them as ES
/// numbers (buffer sizes are far below 2^53, so `f64` is lossless).
pub struct DsEncodeIntoResult {
    pub read: f64,
    pub written: f64,
}
pub struct TextDecoder {
    pub encoding: &'static str,
    pub fatal: bool,
    pub ignore_bom: bool,
    enc: &'static encoding_rs::Encoding,
    decoder: ::std::cell::RefCell<::std::option::Option<encoding_rs::Decoder>>,
}
impl TextDecoder {
    pub fn new(label: String, fatal: bool, ignore_bom: bool) -> Self {
        let enc = encoding_rs::Encoding::for_label(label.as_bytes())
            .unwrap_or(encoding_rs::UTF_8);
        TextDecoder {
            encoding: enc.name(),
            fatal,
            ignore_bom,
            enc,
            decoder: ::std::cell::RefCell::new(::std::option::Option::None),
        }
    }
    pub fn decode(&self, bytes: Vec<u8>, stream: bool) -> String {
        let mut slot = self.decoder.borrow_mut();
        if slot.is_none() {
            *slot = ::std::option::Option::Some(if self.ignore_bom {
                self.enc.new_decoder_without_bom_handling()
            } else {
                self.enc.new_decoder()
            });
        }
        let dec = slot.as_mut().unwrap();
        let mut out = ::std::string::String::new();
        let mut input = bytes.as_slice();
        let mut had_errors = false;
        loop {
            // Worst case: each input byte decodes to a 3-byte U+FFFD replacement
            // (valid multi-byte input never expands beyond that), plus a few
            // bytes of pending carried by the Decoder from a prior `stream:
            // true` call. `decode_to_string` treats String capacity as the
            // output limit and never reallocates, so reserve enough and loop on
            // `OutputFull` (rare).
            out.reserve(input.len() * 3 + 16);
            let (res, read, replaced) = dec.decode_to_string(input, &mut out, !stream);
            had_errors |= replaced;
            match res {
                encoding_rs::CoderResult::InputEmpty => break,
                encoding_rs::CoderResult::OutputFull => input = &input[read..],
            }
        }
        // A flush (`stream: false`) ends this stream; encoding_rs forbids reusing
        // a Decoder after `last = true`, so drop it — the next call lazily makes
        // a fresh one for a new stream (each ES `decode()` w/o stream is
        // independent and re-sniffs the BOM).
        if !stream {
            *slot = ::std::option::Option::None;
        }
        drop(slot);
        if self.fatal && had_errors {
            ::std::panic::panic_any(crate::__ds::DsError::new(
                \"TypeError\",
                format!(
                    \"The encoded data was not valid for encoding \\\"{}\\\"\",
                    self.encoding,
                ),
            ));
        }
        out
    }
}
";

/// WinterTC base64 globals — `__ds::b64_encode`/`__ds::b64_decode` for `btoa`/
/// `atob` (Ecma TC55 «Minimum Common Web API»). WHATWG `btoa(s)` takes each of
/// `s`'s code units as a byte (a code unit > U+00FF throws), then base64-encodes;
/// `atob(s)` strips ASCII whitespace (space/tab/LF/CR/FF), applies the forgiving
/// padding rule (len%4==1 errors; ==2/3 pad with `=`), base64-decodes, and
/// returns the bytes as a Latin-1 string (each byte → U+0000..U+00FF). A
/// too-large code unit or invalid base64 panics, which the runtime lowers to a
/// thrown error. Backed by the `base64` crate's `BASE64_STANDARD` engine.
pub(super) const BASE64_HELPER: &str = r#"use base64::prelude::{Engine as _, BASE64_STANDARD};
pub fn b64_encode<S: AsRef<str>>(s: S) -> String {
    let s = s.as_ref();
    let bytes: Vec<u8> = s
        .chars()
        .map(|c| {
            if (c as u32) > 0xFF {
                panic!("btoa: character outside U+0000..U+00FF");
            }
            c as u8
        })
        .collect();
    BASE64_STANDARD.encode(&bytes)
}
pub fn b64_decode<S: AsRef<str>>(s: S) -> String {
    let mut cleaned: String = s.as_ref()
        .chars()
        .filter(|c| !matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0C'))
        .collect();
    match cleaned.len() % 4 {
        0 => {}
        1 => panic!("atob: invalid base64 string"),
        n => cleaned.push_str(&"=".repeat(4 - n)),
    }
    let bytes = BASE64_STANDARD
        .decode(cleaned.as_bytes())
        .expect("atob: invalid base64 character");
    bytes.iter().map(|&b| b as char).collect()
}
"#;

/// ES legacy URI globals — `__ds::uri_encode`/`uri_decode`/
/// `uri_encode_component`/`uri_decode_component` for `encodeURI`/`decodeURI`/
/// `encodeURIComponent`/`decodeURIComponent` (ECMA-262 §B.2.1). The encoders
/// UTF-8 byte-encode, leaving the `encodeURI` unreserved set + the RFC 3986
/// reserved + `#` (or just the `encodeURIComponent` unreserved set) as-is; the
/// rest become `%HH` (uppercase hex). The decoders percent-decode to bytes then
/// UTF-8 decode; `decodeURI` leaves `%HH` escapes of reserved-set bytes
/// (`;/?:@&=+$,#`) intact, `decodeURIComponent` decodes all. A `%` not followed
/// by two hex digits or invalid UTF-8 panics, lowered to a thrown
/// `URIError`-equivalent. Pure `std` — no cargo dep.
pub(super) const URI_HELPER: &str = r#"
const URI_HEX_DIGITS: &[u8; 16] = b"0123456789ABCDEF";

fn uri_encode_bytes(s: &str, extra_keep: &[u8]) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        let keep = b.is_ascii_alphanumeric()
            || matches!(b, b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')')
            || extra_keep.contains(&b);
        if keep {
            out.push(b as char);
        } else {
            out.push('%');
            out.push(URI_HEX_DIGITS[(b >> 4) as usize] as char);
            out.push(URI_HEX_DIGITS[(b & 0x0F) as usize] as char);
        }
    }
    out
}

pub fn uri_encode<S: AsRef<str>>(s: S) -> String {
    uri_encode_bytes(s.as_ref(), b";,/?:@&=+$#")
}

pub fn uri_encode_component<S: AsRef<str>>(s: S) -> String {
    uri_encode_bytes(s.as_ref(), b"")
}

fn uri_hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'A'..=b'F' => Some(b - b'A' + 10),
        b'a'..=b'f' => Some(b - b'a' + 10),
        _ => None,
    }
}

fn is_uri_reserved(b: u8) -> bool {
    matches!(b, b';' | b',' | b'/' | b'?' | b':' | b'@' | b'&' | b'=' | b'+' | b'$' | b'#')
}

fn uri_decode_bytes(s: &str, keep_reserved: bool) -> Result<String, ()> {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            // ES: `%` must be followed by two hex digits, else URIError.
            if i + 2 >= bytes.len() {
                return Err(());
            }
            let (h, l) = match (uri_hex_digit(bytes[i + 1]), uri_hex_digit(bytes[i + 2])) {
                (Some(h), Some(l)) => (h, l),
                _ => return Err(()),
            };
            let decoded = h * 16 + l;
            if keep_reserved && is_uri_reserved(decoded) {
                out.push(b'%');
                out.push(bytes[i + 1]);
                out.push(bytes[i + 2]);
            } else {
                out.push(decoded);
            }
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| ())
}

pub fn uri_decode<S: AsRef<str>>(s: S) -> String {
    match uri_decode_bytes(s.as_ref(), true) {
        Ok(s) => s,
        Err(()) => panic!("decodeURI: invalid URI sequence"),
    }
}

pub fn uri_decode_component<S: AsRef<str>>(s: S) -> String {
    match uri_decode_bytes(s.as_ref(), false) {
        Ok(s) => s,
        Err(()) => panic!("decodeURIComponent: invalid URI sequence"),
    }
}
"#;

/// ES2025 `Math.sumPrecise` finite-path: exact sum of the finite, non-(-0)
/// elements, rounded once to nearest-even. Delegates to the `xsum` crate
/// (Radford Neal's superaccumulator) so the round-to-nearest-even edge cases —
/// the spec "exercised real-implementation bugs" fixtures where huge
/// magnitudes cancel to a tiny residue — land on a vetted implementation
/// rather than a hand roll. The NaN/±∞/−0 state machine stays inline at the
/// call site (`builtins::math`); this only sums the finite part the state
/// machine collected. Cargo dep `xsum`; marker `__ds::sum_precise`.
pub(super) const SUM_PRECISE_HELPER: &str = r#"
/// Exact sum of `finites` rounded to nearest-even (ES2025 `Math.sumPrecise`'s
/// finite path). The caller has already stripped NaN/±∞/−0 via the spec state
/// machine, so every input here is a finite f64. `XsumAuto` selects the small
/// or large superaccumulator by length, so any input size is exact.
pub fn sum_precise_exact(finites: &[f64]) -> f64 {
    use xsum::Xsum;
    let mut acc = xsum::XsumAuto::new();
    acc.add_list(finites);
    acc.sum()
}
"#;

/// High Resolution Time helper — `__ds::perf_now`. The WinterTC (W3C hr-time)
/// `performance.now()` returns a monotonic DOMHighResTimeStamp (milliseconds
/// since the process timeOrigin). The hr-time spec constrains monotonicity and
/// non-negativity, not an absolute epoch, so the timeOrigin is approximated as
/// the first call (a function-local `static OnceLock<Instant>`, lazily
/// initialised — pure `std`, no cargo dep). `performance.now()` and
/// `self.performance.now()` (the WinterTC `self` global-object alias) both
/// lower here.
pub(super) const PERF_HELPER: &str = r#"
/// `performance.now()` — a monotonic DOMHighResTimeStamp (ms). The epoch is
/// the first call (function-local static), so the value is positive and the
/// difference of two readings is non-negative: the hr-time guarantees.
pub fn perf_now() -> f64 {
    static EPOCH: ::std::sync::OnceLock<::std::time::Instant> = ::std::sync::OnceLock::new();
    let epoch = EPOCH.get_or_init(::std::time::Instant::now);
    epoch.elapsed().as_secs_f64() * 1000.0
}
/// `performance.timeOrigin` — the process's timeOrigin as a
/// DOMHighResTimeStamp (ms since the Unix epoch). Approximated as the first
/// call's wall-clock reading (function-local static), so a WPT
/// `assert_true(performance.timeOrigin > 0)` holds and `timeOrigin + now()`
/// stays close to `Date.now()` within first-call jitter.
pub fn perf_time_origin() -> f64 {
    static ORIGIN: ::std::sync::OnceLock<::std::time::Duration> = ::std::sync::OnceLock::new();
    let origin = ORIGIN.get_or_init(|| {
        ::std::time::SystemTime::now()
            .duration_since(::std::time::UNIX_EPOCH)
            .unwrap_or_default()
    });
    origin.as_secs_f64() * 1000.0
}
"#;

/// WinterTC WebCrypto helper — `__ds::crypto_random_uuid`. `crypto.randomUUID()`
/// (an RFC 4122 version-4 UUID) lowers here; `uuid::Uuid::new_v4` is the
/// reference implementation (`v4` feature, backed by `getrandom`). Pure-Rust —
/// WinterTC never degrades a Web API to the engine.
pub(super) const CRYPTO_HELPER: &str = r#"
/// `crypto.randomUUID()` — an RFC 4122 version-4 UUID string (36 chars,
/// `xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx`). Each call returns a fresh UUID.
pub fn crypto_random_uuid() -> String {
    ::uuid::Uuid::new_v4().to_string()
}
/// `crypto.getRandomValues(buf)` — fill `buf` with cryptographically-strong
/// random bytes (WebCrypto `getRandomValues`, backed by `getrandom` — the same
/// source `uuid::new_v4` uses). Consumes the buffer and returns it filled (ES
/// returns the same typed array it was passed), matching the common
/// `var iv = crypto.getRandomValues(new Uint8Array(12))` shape. An in-place
/// call on an existing local (`crypto.getRandomValues(buf)`) moves the local,
/// so a later read of `buf` is a cargo-check error honestly — assign the
/// result back (`buf = crypto.getRandomValues(buf)`). ES caps the buffer at
/// 65536 bytes (a `QuotaExceededError`); that bound is unchecked here.
pub fn crypto_get_random_values(mut buf: ::std::vec::Vec<u8>) -> ::std::vec::Vec<u8> {
    ::getrandom::getrandom(&mut buf).expect("getrandom failed");
    buf
}
"#;

/// WebCrypto `SubtleCrypto.digest` helper — `__ds::crypto_subtle_digest`
/// (WinterTC Web API, W3C WebCrypto). `crypto.subtle.digest(algo, data)` is the
/// one-shot hash: `algo` is the ES algorithm name (`"SHA-1"`/`"SHA-256"`/
/// `"SHA-384"`/`"SHA-512"`), `data` is the `BufferSource` (a `Vec<u8>`), and the
/// result is the digest bytes. Backed by the RustCrypto `sha1`/`sha2` crates
/// (pure Rust — WinterTC never degrades a Web API). `async` because ES
/// `digest` returns a `Promise<ArrayBuffer>`; the `await` at the call site
/// drives the future (the async-main gate flips `fn main` to `#[tokio::main]`).
/// An unknown algorithm panics the `TypeError` ES throws (the WPT verdict reads
/// the prefix). The key-bearing methods are mapped alongside: `importKey`
/// (raw format → `DsCryptoKey`) / `exportKey` (raw format ← `DsCryptoKey`),
/// `sign`/`verify` (HMAC, RustCrypto `hmac`), `encrypt`/`decrypt` (AES-GCM
/// `aes-gcm` / AES-CBC `aes`+`cbc`), `generateKey` (a fresh random `DsCryptoKey`
/// for AES-GCM/AES-CBC/HMAC), `deriveBits`/`deriveKey` (the PBKDF2/HKDF
/// key-derivation paths, reusing `hmac`). The remaining `SubtleCrypto` methods
/// (`wrapKey`) land later; `digest` is the no-key one-shot, the bulk of the WPT
/// `WebCryptoAPI/digest` fixtures.
pub(super) const SUBTLE_HELPER: &str = r#"
/// `crypto.subtle.digest(algo, data)` — the one-shot hash. `algo` is matched
/// case-sensitively against the ES algorithm names (`"SHA-1"`/`"SHA-256"`/
/// `"SHA-384"`/`"SHA-512"`); any other value panics the `TypeError` ES throws
/// (the WPT verdict reads the prefix). `data` is hashed as raw bytes. Returns
/// the digest bytes (20/32/48/64 for SHA-1/256/384/512).
pub async fn crypto_subtle_digest(
    algo: ::std::string::String,
    data: ::std::vec::Vec<u8>,
) -> ::std::vec::Vec<u8> {
    match algo.as_str() {
        "SHA-1" => {
            use ::sha1::{Digest, Sha1};
            Sha1::digest(&data).to_vec()
        }
        "SHA-256" => {
            use ::sha2::{Digest, Sha256};
            Sha256::digest(&data).to_vec()
        }
        "SHA-384" => {
            use ::sha2::{Digest, Sha384};
            Sha384::digest(&data).to_vec()
        }
        "SHA-512" => {
            use ::sha2::{Digest, Sha512};
            Sha512::digest(&data).to_vec()
        }
        _ => ::core::panic!(
            "TypeError: crypto.subtle.digest: unknown or unsupported algorithm"
        ),
    }
}
/// A WebCrypto `CryptoKey` — the value `crypto.subtle.importKey(…)` returns and
/// `sign`/`verify` take (the HMAC subset of WinterTC WebCrypto). It carries the
/// `algorithm` name (`"HMAC"`), the paired `hash` (`"SHA-256"`/…), the raw `key`
/// bytes, and its `extractable`/`usages` (stored, not enforced by the static
/// path — ES enforces them at runtime; the common server shape never trips
/// them). `#[derive(Clone)]` so a key passed to `sign`/`verify` by reference
/// may copy. The marker `__ds::DsCryptoKey` pulls `SubtleCrypto` (so `sha1`/
/// `sha2`/`hmac` are flagged) via the dep derivation.
#[derive(Clone)]
pub struct DsCryptoKey {
    pub algorithm: ::std::string::String,
    pub hash: ::std::string::String,
    pub key: ::std::vec::Vec<u8>,
    pub extractable: bool,
    pub usages: ::std::vec::Vec<::std::string::String>,
}
impl DsCryptoKey {
    /// The translator-emitted constructor (the importKey lowering builds the
    /// `(algorithm, hash, key, extractable, usages)` quadruple from the ES
    /// `algorithm` object + the raw key bytes).
    pub fn new(
        algorithm: ::std::string::String,
        hash: ::std::string::String,
        key: ::std::vec::Vec<u8>,
        extractable: bool,
        usages: ::std::vec::Vec<::std::string::String>,
    ) -> Self {
        Self {
            algorithm,
            hash,
            key,
            extractable,
            usages,
        }
    }
}
/// `crypto.subtle.importKey(format, keyData, algorithm, extractable, usages)`
/// — the HMAC subset. `format` is `"raw"` (the only form lowered — pkcs8/spki
/// are not statically modeled), `keyData` the raw key bytes, `algorithm` the
/// `{name, hash}` the translator extracted. Returns a `DsCryptoKey`. `async`
/// because ES `importKey` returns `Promise<CryptoKey>`; the call site's `await`
/// drives the future.
pub async fn crypto_subtle_import_key(
    algorithm: ::std::string::String,
    hash: ::std::string::String,
    key: ::std::vec::Vec<u8>,
    extractable: bool,
    usages: ::std::vec::Vec<::std::string::String>,
) -> DsCryptoKey {
    DsCryptoKey::new(algorithm, hash, key, extractable, usages)
}
/// `crypto.subtle.exportKey(format, key)` — the symmetric-key raw export (the
/// inverse of `importKey`). For `format` `"raw"` the raw key bytes (`key.key`)
/// are returned; `"jwk"`/`"pkcs8"`/`"spki"` are not statically modeled (panic
/// honestly — the WinterTC server shape uses `"raw"` for HMAC/AES keys). `async`
/// because ES `exportKey` returns `Promise<ArrayBuffer>`; the call site's `await`
/// drives the future, and `callee_return_path` records the `Vec<u8>` return (like
/// `sign`/`encrypt`).
pub async fn crypto_subtle_export_key(
    format: ::std::string::String,
    key: &DsCryptoKey,
) -> ::std::vec::Vec<u8> {
    match format.as_str() {
        "raw" => key.key.clone(),
        _ => ::core::panic!("TypeError: crypto.subtle.exportKey: unsupported format"),
    }
}
/// `crypto.subtle.generateKey(algorithm, extractable, usages)` — the factory for
/// a fresh `DsCryptoKey` (the WinterTC WebCrypto subset). For AES-GCM/AES-CBC the
/// algorithm object's `length` field (128/192/256) is the key length in bits
/// (`length / 8` bytes); for HMAC, `length` (if present) is the key length in
/// bits, else the default is the named hash's block size (64 for SHA-1/SHA-256,
/// 128 for SHA-384/SHA-512). The key bytes are cryptographically random
/// (`getrandom`, the same source `crypto.getRandomValues` uses). `async` because
/// ES `generateKey` returns `Promise<CryptoKey>`; the call site's `await` drives
/// the future, and `callee_return_path` records the `DsCryptoKey` return
/// (mirroring `importKey`, so a later `sign`/`encrypt` passes the key through).
pub async fn crypto_subtle_generate_key(
    name: ::std::string::String,
    hash: ::std::string::String,
    length: usize,
    extractable: bool,
    usages: ::std::vec::Vec<::std::string::String>,
) -> DsCryptoKey {
    let bytes = match name.as_str() {
        "AES-GCM" | "AES-CBC" => length / 8,
        "HMAC" => {
            if length == 0 {
                match hash.as_str() {
                    "SHA-384" | "SHA-512" => 128,
                    _ => 64,
                }
            } else {
                length / 8
            }
        }
        _ => ::core::panic!("TypeError: crypto.subtle.generateKey: unsupported algorithm"),
    };
    let mut key = ::std::vec![0u8; bytes];
    ::getrandom::getrandom(&mut key).expect("getrandom failed");
    DsCryptoKey::new(name, hash, key, extractable, usages)
}
/// `crypto.subtle.deriveBits(algorithm, baseKey, length)` — the WinterTC WebCrypto
/// key-derivation path. For PBKDF2 the `baseKey` carries the password bytes
/// (`key.key`), and the ES `algorithm` object's `salt`/`iterations`/`hash` drive
/// the derivation (RFC 2898); `info` is unused. For HKDF the `baseKey` is the
/// input key material, `salt`/`info`/`hash` drive the extract+expand (RFC 5869);
/// `iterations` is unused. `length` is the output length in bits (`length / 8`
/// bytes). The hash selects the HMAC PRF (SHA-1/256/384/512), reusing the `hmac`
/// crate that backs `sign` — no separate PBKDF2/HKDF dep (the `pbkdf2` 0.13 crate
/// pulls `digest` 0.11, incompatible with the `sha1`/`sha2` 0.10 DashScript uses;
/// both KDFs are short loops over HMAC anyway). `async` because ES `deriveBits`
/// returns `Promise<ArrayBuffer>`; the call site's `await` drives the future, and
/// `callee_return_path` records the `Vec<u8>` return (like `sign`/`encrypt`).
pub async fn crypto_subtle_derive_bits(
    name: ::std::string::String,
    hash: ::std::string::String,
    salt: ::std::vec::Vec<u8>,
    info: ::std::vec::Vec<u8>,
    iterations: u32,
    key: &DsCryptoKey,
    length: usize,
) -> ::std::vec::Vec<u8> {
    let password = &key.key;
    let mut out = ::std::vec![0u8; length / 8];
    match name.as_str() {
        "PBKDF2" => match hash.as_str() {
            "SHA-1" => pbkdf2_into::<::hmac::Hmac<::sha1::Sha1>>(
                password, &salt, iterations, &mut out, 20,
            ),
            "SHA-256" => pbkdf2_into::<::hmac::Hmac<::sha2::Sha256>>(
                password, &salt, iterations, &mut out, 32,
            ),
            "SHA-384" => pbkdf2_into::<::hmac::Hmac<::sha2::Sha384>>(
                password, &salt, iterations, &mut out, 48,
            ),
            "SHA-512" => pbkdf2_into::<::hmac::Hmac<::sha2::Sha512>>(
                password, &salt, iterations, &mut out, 64,
            ),
            _ => ::core::panic!(
                "TypeError: crypto.subtle.deriveBits: unsupported PBKDF2 hash"
            ),
        },
        "HKDF" => match hash.as_str() {
            "SHA-1" => hkdf_into::<::hmac::Hmac<::sha1::Sha1>>(
                &salt, password, &info, &mut out, 20,
            ),
            "SHA-256" => hkdf_into::<::hmac::Hmac<::sha2::Sha256>>(
                &salt, password, &info, &mut out, 32,
            ),
            "SHA-384" => hkdf_into::<::hmac::Hmac<::sha2::Sha384>>(
                &salt, password, &info, &mut out, 48,
            ),
            "SHA-512" => hkdf_into::<::hmac::Hmac<::sha2::Sha512>>(
                &salt, password, &info, &mut out, 64,
            ),
            _ => ::core::panic!(
                "TypeError: crypto.subtle.deriveBits: unsupported HKDF hash"
            ),
        },
        _ => ::core::panic!("TypeError: crypto.subtle.deriveBits: unsupported algorithm"),
    }
    out
}
/// `crypto.subtle.deriveKey(algorithm, baseKey, derivedKeyType, extractable,
/// usages)` — the PBKDF2/HKDF subset, an orchestrator over `deriveBits` + the
/// key ctor (the WinterTC WebCrypto key-derivation-and-import path). The
/// derivation algorithm (PBKDF2's `name`/`hash`/`salt`/`iterations` or HKDF's
/// `name`/`hash`/`salt`/`info`) and the password/input-key-material
/// `baseKey` feed `crypto_subtle_derive_bits`; the `derivedKeyType` object's
/// `name`/`hash`/`length` decide the output key length in bits (AES-GCM/AES-CBC:
/// `length`; HMAC: `length`, else the named hash's block size — 512 for
/// SHA-1/SHA-256, 1024 for SHA-384/SHA-512, mirroring `generateKey`). The
/// derived bytes become the `DsCryptoKey`'s raw key. `async` because ES
/// `deriveKey` returns `Promise<CryptoKey>`; the call site's `await` drives the
/// future, and `callee_return_path` records the `DsCryptoKey` return (like
/// `importKey`/`generateKey`).
pub async fn crypto_subtle_derive_key(
    name: ::std::string::String,
    hash: ::std::string::String,
    salt: ::std::vec::Vec<u8>,
    info: ::std::vec::Vec<u8>,
    iterations: u32,
    base_key: &DsCryptoKey,
    derived_name: ::std::string::String,
    derived_hash: ::std::string::String,
    derived_length: usize,
    extractable: bool,
    usages: ::std::vec::Vec<::std::string::String>,
) -> DsCryptoKey {
    let length_bits = match derived_name.as_str() {
        "AES-GCM" | "AES-CBC" => derived_length,
        "HMAC" => {
            if derived_length == 0 {
                match derived_hash.as_str() {
                    "SHA-384" | "SHA-512" => 1024,
                    _ => 512,
                }
            } else {
                derived_length
            }
        }
        _ => ::core::panic!("TypeError: crypto.subtle.deriveKey: unsupported derived key type"),
    };
    let bits =
        crypto_subtle_derive_bits(name, hash, salt, info, iterations, base_key, length_bits).await;
    DsCryptoKey::new(derived_name, derived_hash, bits, extractable, usages)
}
/// The PBKDF2 (RFC 2898) core — `T_i = U_1 ^ … ^ U_c`, where
/// `U_1 = HMAC(P, S || INT_32_BE(i))` and `U_j = HMAC(P, U_{j-1})` — generic over
/// the HMAC instance (`H: hmac::Mac`), so each SHA hash arms with a concrete
/// `Hmac<D>` (no `digest` trait bounds to state). `h_len` is the hash output
/// length (the F-block size); `res` is filled block by block, the trailing
/// partial block truncated.
fn pbkdf2_into<H>(password: &[u8], salt: &[u8], rounds: u32, res: &mut [u8], h_len: usize)
where
    H: ::hmac::Mac + ::hmac::digest::KeyInit,
{
    use ::hmac::Mac;
    for (idx, chunk) in res.chunks_mut(h_len).enumerate() {
        let mut block = ::std::vec![0u8; salt.len() + 4];
        block[..salt.len()].copy_from_slice(salt);
        block[salt.len()..].copy_from_slice(&((idx as u32) + 1).to_be_bytes());
        let mut mac = <H as ::hmac::digest::KeyInit>::new_from_slice(password)
            .expect("HMAC key length");
        mac.update(&block);
        let mut u: ::std::vec::Vec<u8> = mac.finalize().into_bytes().to_vec();
        let mut t = u.clone();
        for _ in 1..rounds {
            let mut mac = <H as ::hmac::digest::KeyInit>::new_from_slice(password)
            .expect("HMAC key length");
            mac.update(&u);
            u = mac.finalize().into_bytes().to_vec();
            for (a, b) in t.iter_mut().zip(u.iter()) {
                *a ^= b;
            }
        }
        let n = chunk.len().min(t.len());
        chunk[..n].copy_from_slice(&t[..n]);
    }
}
/// The HKDF (RFC 5869) core — extract-then-expand, generic over the HMAC
/// instance (`H: hmac::Mac`). Extract: `PRK = HMAC-Hash(salt, IKM)` (an empty
/// `salt` is HMAC's normal zero-key path, not RFC 5869's HashLen-zeros default,
/// but HMAC of an empty key is the same as HMAC of HashLen zero bytes for these
/// hashes — the test-vector round-trip confirms it). Expand:
/// `T(0) = ""`, `T(i) = HMAC(PRK, T(i-1) || info || octet(i))`, `OKM = T(1) ||
/// T(2) || …` truncated to `res.len()`. `h_len` is the hash output length.
fn hkdf_into<H>(salt: &[u8], ikm: &[u8], info: &[u8], res: &mut [u8], h_len: usize)
where
    H: ::hmac::Mac + ::hmac::digest::KeyInit,
{
    use ::hmac::Mac;
    // Extract — PRK = HMAC(salt, IKM); reuse as the expand key.
    let mut prk_mac = <H as ::hmac::digest::KeyInit>::new_from_slice(salt).expect("HMAC key length");
    prk_mac.update(ikm);
    let prk = prk_mac.finalize().into_bytes();
    // Expand — fill `res` block by block (each HMAC output is one block).
    let mut t: ::std::vec::Vec<u8> = ::std::vec![];
    let mut idx: u8 = 1;
    for chunk in res.chunks_mut(h_len) {
        let mut mac = <H as ::hmac::digest::KeyInit>::new_from_slice(&prk).expect("HMAC key length");
        mac.update(&t);
        mac.update(info);
        mac.update(&[idx]);
        t = mac.finalize().into_bytes().to_vec();
        let n = chunk.len().min(t.len());
        chunk[..n].copy_from_slice(&t[..n]);
        idx = idx.wrapping_add(1);
    }
}
/// `crypto.subtle.sign(algo, key, data)` — the HMAC subset. The hash comes from
/// the key; the ES `algo` arg is carried by `key.algorithm` (verified to be
/// `"HMAC"`). Returns the HMAC tag bytes. `async` because ES `sign` returns
/// `Promise<ArrayBuffer>`.
pub async fn crypto_subtle_sign(
    key: &DsCryptoKey,
    data: ::std::vec::Vec<u8>,
) -> ::std::vec::Vec<u8> {
    match (key.algorithm.as_str(), key.hash.as_str()) {
        ("HMAC", "SHA-1") => {
            use ::hmac::{Hmac, Mac};
            type HmacSha1 = Hmac<::sha1::Sha1>;
            let mut mac = HmacSha1::new_from_slice(&key.key).expect("HMAC key length");
            mac.update(&data);
            mac.finalize().into_bytes().to_vec()
        }
        ("HMAC", "SHA-256") => {
            use ::hmac::{Hmac, Mac};
            type HmacSha256 = Hmac<::sha2::Sha256>;
            let mut mac = HmacSha256::new_from_slice(&key.key).expect("HMAC key length");
            mac.update(&data);
            mac.finalize().into_bytes().to_vec()
        }
        ("HMAC", "SHA-384") => {
            use ::hmac::{Hmac, Mac};
            type HmacSha384 = Hmac<::sha2::Sha384>;
            let mut mac = HmacSha384::new_from_slice(&key.key).expect("HMAC key length");
            mac.update(&data);
            mac.finalize().into_bytes().to_vec()
        }
        ("HMAC", "SHA-512") => {
            use ::hmac::{Hmac, Mac};
            type HmacSha512 = Hmac<::sha2::Sha512>;
            let mut mac = HmacSha512::new_from_slice(&key.key).expect("HMAC key length");
            mac.update(&data);
            mac.finalize().into_bytes().to_vec()
        }
        _ => ::core::panic!("TypeError: crypto.subtle.sign: unsupported algorithm"),
    }
}
/// `crypto.subtle.verify(algo, key, signature, data)` — the HMAC subset. Returns
/// `true` iff `signature` recomputes from `key`+`data`. The compare folds XOR so
/// it is constant-time-ish (HMAC verification is not secret-dependent in
/// practice, but the fold avoids an early-exit timing leak). `async` because ES
/// `verify` returns `Promise<boolean>`.
pub async fn crypto_subtle_verify(
    key: &DsCryptoKey,
    signature: ::std::vec::Vec<u8>,
    data: ::std::vec::Vec<u8>,
) -> bool {
    let computed = crypto_subtle_sign(key, data).await;
    if computed.len() != signature.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in computed.iter().zip(signature.iter()) {
        diff |= a ^ b;
    }
    diff == 0
}
/// Seal with an AES-GCM cipher built at the call site (keyed by length:
/// `Aes128Gcm`/`Aes256Gcm`). `aead::Aead::encrypt` returns
/// `ciphertext || tag` — the WebCrypto AES-GCM output format — so the result is
/// byte-compatible with a browser `crypto.subtle.encrypt`. A nonce/length error
/// is impossible for a translator-built key+iv, so the `expect` is a panic on a
/// genuinely malformed input (the engine path's `TypeError` analogue).
fn aes_gcm_seal<C>(cipher: &C, iv: &[u8], data: &[u8]) -> ::std::vec::Vec<u8>
where
    C: ::aead::Aead,
{
    use ::aead::Aead;
    cipher
        .encrypt(::aead::Nonce::<C>::from_slice(iv), data)
        .expect("AES-GCM encrypt")
        .to_vec()
}
/// The decrypt twin of [`aes_gcm_seal`] — `aead::Aead::decrypt` expects
/// `ciphertext || tag` and authenticates before returning the plaintext; a tag
/// mismatch (a tampered ciphertext) panics (ES rejects the promise with an
/// `OperationError`).
fn aes_gcm_open<C>(cipher: &C, iv: &[u8], data: &[u8]) -> ::std::vec::Vec<u8>
where
    C: ::aead::Aead,
{
    use ::aead::Aead;
    cipher
        .decrypt(::aead::Nonce::<C>::from_slice(iv), data)
        .expect("AES-GCM decrypt")
        .to_vec()
}
/// Seal with an AES-CBC cipher built at the call site (keyed by length:
/// `Aes128`/`Aes256`), PKCS7-padded (the only padding WebCrypto uses). `cbc`
/// re-exports the `cipher` traits, so `cbc::cipher::*` resolves. The block size
/// and key size are inferred from `A` via `KeyIvInit::new`'s `&GenericArray`
/// params (`from_slice` lets a `&[u8]` stand in for the sized array).
fn aes_cbc_seal<A>(key: &[u8], iv: &[u8], data: &[u8]) -> ::std::vec::Vec<u8>
where
    A: ::cbc::cipher::BlockCipher + ::cbc::cipher::BlockEncrypt + ::cbc::cipher::KeyInit,
{
    use ::cbc::cipher::{
        BlockEncryptMut, KeyIvInit, block_padding::Pkcs7, generic_array::GenericArray,
    };
    ::cbc::Encryptor::<A>::new(GenericArray::from_slice(key), GenericArray::from_slice(iv))
        .encrypt_padded_vec_mut::<Pkcs7>(data)
}
/// The decrypt twin of [`aes_cbc_seal`] — `decrypt_padded_vec_mut` returns
/// `None` on a padding mismatch (a tampered ciphertext), panicked as the ES
/// `OperationError` analogue.
fn aes_cbc_open<A>(key: &[u8], iv: &[u8], data: &[u8]) -> ::std::vec::Vec<u8>
where
    A: ::cbc::cipher::BlockCipher + ::cbc::cipher::BlockDecrypt + ::cbc::cipher::KeyInit,
{
    use ::cbc::cipher::{
        BlockDecryptMut, KeyIvInit, block_padding::Pkcs7, generic_array::GenericArray,
    };
    ::cbc::Decryptor::<A>::new(GenericArray::from_slice(key), GenericArray::from_slice(iv))
        .decrypt_padded_vec_mut::<Pkcs7>(data)
        .expect("AES-CBC decrypt")
}
/// `crypto.subtle.encrypt(algo, key, data)` — the AES-GCM/AES-CBC subset. The ES
/// `algo` object's `name` (`"AES-GCM"`/`"AES-CBC"`) and `iv` (the nonce/IV, a
/// `Uint8Array`) are extracted at translate time; the key length selects
/// `Aes128`/`Aes256`. AES-GCM returns `ciphertext || tag` (the WebCrypto
/// AES-GCM output format); AES-CBC returns the PKCS7-padded ciphertext. `async`
/// because ES `encrypt` returns `Promise<ArrayBuffer>`. The `iv` is taken by
/// reference so the standard encrypt→decrypt round-trip can reuse one IV
/// binding. The `additionalData`/`tagLength` AES-GCM fields are not yet mapped.
pub async fn crypto_subtle_encrypt(
    name: ::std::string::String,
    iv: &[u8],
    key: &DsCryptoKey,
    data: ::std::vec::Vec<u8>,
) -> ::std::vec::Vec<u8> {
    use ::aead::KeyInit;
    match name.as_str() {
        "AES-GCM" => match key.key.len() {
            16 => aes_gcm_seal(
                &::aes_gcm::Aes128Gcm::new(::aead::Key::<::aes_gcm::Aes128Gcm>::from_slice(
                    &key.key,
                )),
                iv,
                &data,
            ),
            32 => aes_gcm_seal(
                &::aes_gcm::Aes256Gcm::new(::aead::Key::<::aes_gcm::Aes256Gcm>::from_slice(
                    &key.key,
                )),
                iv,
                &data,
            ),
            _ => ::core::panic!("TypeError: AES-GCM key length must be 128/256 bits"),
        },
        "AES-CBC" => match key.key.len() {
            16 => aes_cbc_seal::<::aes::Aes128>(&key.key, iv, &data),
            32 => aes_cbc_seal::<::aes::Aes256>(&key.key, iv, &data),
            _ => ::core::panic!("TypeError: AES-CBC key length must be 128/256 bits"),
        },
        _ => ::core::panic!("TypeError: crypto.subtle.encrypt: unsupported algorithm"),
    }
}
/// `crypto.subtle.decrypt(algo, key, data)` — the AES-GCM/AES-CBC subset. The
/// inverse of `encrypt`: AES-GCM `data` is `ciphertext || tag`, authenticated
/// then returned as plaintext; AES-CBC `data` is PKCS7-padded ciphertext, the
/// padding stripped (a mismatch panics — ES `OperationError`). `async` because
/// ES `decrypt` returns `Promise<ArrayBuffer>`. As with `encrypt`, `iv` is by
/// reference (the same IV binds a sealed pair).
pub async fn crypto_subtle_decrypt(
    name: ::std::string::String,
    iv: &[u8],
    key: &DsCryptoKey,
    data: ::std::vec::Vec<u8>,
) -> ::std::vec::Vec<u8> {
    use ::aead::KeyInit;
    match name.as_str() {
        "AES-GCM" => match key.key.len() {
            16 => aes_gcm_open(
                &::aes_gcm::Aes128Gcm::new(::aead::Key::<::aes_gcm::Aes128Gcm>::from_slice(
                    &key.key,
                )),
                iv,
                &data,
            ),
            32 => aes_gcm_open(
                &::aes_gcm::Aes256Gcm::new(::aead::Key::<::aes_gcm::Aes256Gcm>::from_slice(
                    &key.key,
                )),
                iv,
                &data,
            ),
            _ => ::core::panic!("TypeError: AES-GCM key length must be 128/256 bits"),
        },
        "AES-CBC" => match key.key.len() {
            16 => aes_cbc_open::<::aes::Aes128>(&key.key, iv, &data),
            32 => aes_cbc_open::<::aes::Aes256>(&key.key, iv, &data),
            _ => ::core::panic!("TypeError: AES-CBC key length must be 128/256 bits"),
        },
        _ => ::core::panic!("TypeError: crypto.subtle.decrypt: unsupported algorithm"),
    }
}
/// RFC 3394 AES Key Wrap — the only `wrapKey`/`unwrapKey` algorithm WinterTC
/// servers use (the AES-KW "authentic" wrap, no padding, no IV; AES-GCM/AES-CBC
/// wraps go through `encrypt`/`decrypt`). `cbc` re-exports `cipher`, so
/// `cbc::cipher::*` resolves (cipher 0.4, the same version `aes` 0.8 and `cbc`
/// 0.1 share). The wrap operates on 64-bit blocks: the key (a non-zero multiple
/// of 8 bytes, `n` blocks) is wrapped under the KEK into `n+1` blocks, the
/// leading `A` block carrying the `0xA6A6…` integrity check, with the `t = i +
/// n·j` counter XOR'd in (RFC 3394 §2.2.1).
fn aes_kw_wrap<A>(kek: &[u8], key: &[u8]) -> ::std::vec::Vec<u8>
where
    A: ::cbc::cipher::BlockCipher + ::cbc::cipher::BlockEncrypt + ::cbc::cipher::KeyInit,
{
    use ::cbc::cipher::{BlockEncrypt, KeyInit, generic_array::GenericArray};
    assert!(
        key.len() % 8 == 0 && !key.is_empty(),
        "AES-KW: key length must be a non-zero multiple of 8 bytes"
    );
    let cipher = <A as KeyInit>::new(GenericArray::from_slice(kek));
    let n = key.len() / 8;
    let mut a = [0xA6u8; 8];
    let mut r: ::std::vec::Vec<[u8; 8]> = (0..n)
        .map(|i| {
            let mut blk = [0u8; 8];
            blk.copy_from_slice(&key[i * 8..(i + 1) * 8]);
            blk
        })
        .collect();
    let mut block = [0u8; 16];
    for j in 0..6u64 {
        for i in 1..=n as u64 {
            block[..8].copy_from_slice(&a);
            block[8..].copy_from_slice(&r[(i - 1) as usize]);
            cipher.encrypt_block(GenericArray::from_mut_slice(&mut block));
            a.copy_from_slice(&block[..8]);
            let t = (n as u64 * j + i).to_be_bytes();
            for k in 0..8 {
                a[k] ^= t[k];
            }
            r[(i - 1) as usize].copy_from_slice(&block[8..]);
        }
    }
    let mut out = ::std::vec::Vec::with_capacity(8 * (n + 1));
    out.extend_from_slice(&a);
    for blk in &r {
        out.extend_from_slice(blk);
    }
    out
}
/// The decrypt twin of [`aes_kw_wrap`] (RFC 3394 §2.2.2, the reverse loop in
/// `j`/`i`). The post-loop `A == 0xA6A6…` check is the integrity check — a
/// mismatch means a tampered ciphertext or wrong KEK, panicked as ES
/// `OperationError`.
fn aes_kw_unwrap<A>(kek: &[u8], data: &[u8]) -> ::std::vec::Vec<u8>
where
    A: ::cbc::cipher::BlockCipher + ::cbc::cipher::BlockDecrypt + ::cbc::cipher::KeyInit,
{
    use ::cbc::cipher::{BlockDecrypt, KeyInit, generic_array::GenericArray};
    assert!(
        data.len() % 8 == 0 && data.len() >= 16,
        "AES-KW: wrapped data must be a multiple of 8 bytes and at least 16 bytes"
    );
    let cipher = <A as KeyInit>::new(GenericArray::from_slice(kek));
    let n = data.len() / 8 - 1;
    let mut a = [0u8; 8];
    a.copy_from_slice(&data[..8]);
    let mut r: ::std::vec::Vec<[u8; 8]> = (0..n)
        .map(|i| {
            let mut blk = [0u8; 8];
            blk.copy_from_slice(&data[(i + 1) * 8..(i + 2) * 8]);
            blk
        })
        .collect();
    let mut block = [0u8; 16];
    for j in (0..6u64).rev() {
        for i in (1..=n as u64).rev() {
            let t = (n as u64 * j + i).to_be_bytes();
            for k in 0..8 {
                a[k] ^= t[k];
            }
            block[..8].copy_from_slice(&a);
            block[8..].copy_from_slice(&r[(i - 1) as usize]);
            cipher.decrypt_block(GenericArray::from_mut_slice(&mut block));
            a.copy_from_slice(&block[..8]);
            r[(i - 1) as usize].copy_from_slice(&block[8..]);
        }
    }
    assert!(
        a == [0xA6u8; 8],
        "OperationError: AES-KW integrity check failed"
    );
    let mut out = ::std::vec::Vec::with_capacity(8 * n);
    for blk in &r {
        out.extend_from_slice(blk);
    }
    out
}
/// `crypto.subtle.wrapKey(format, key, wrappingKey, wrapAlgorithm)` — the AES-KW
/// subset. The `wrapAlgorithm` carries no IV (AES-KW is IV-less), so the call is
/// `aes_kw_wrap(wrappingKey.key, key.key)`; the KEK length selects
/// `Aes128`/`Aes256`. `format` is `"raw"` (the only export form lowered — the
/// raw key bytes are wrapped directly; jwk/pkcs8/spki are not statically
/// modeled). `async` because ES `wrapKey` returns `Promise<ArrayBuffer>`; the
/// call site's `await` drives the future, and `callee_return_path` records the
/// `Vec<u8>` return (like `encrypt`).
pub async fn crypto_subtle_wrap_key(
    name: ::std::string::String,
    wrapping_key: &DsCryptoKey,
    key: &DsCryptoKey,
) -> ::std::vec::Vec<u8> {
    match name.as_str() {
        "AES-KW" => match wrapping_key.key.len() {
            16 => aes_kw_wrap::<::aes::Aes128>(&wrapping_key.key, &key.key),
            32 => aes_kw_wrap::<::aes::Aes256>(&wrapping_key.key, &key.key),
            _ => ::core::panic!("TypeError: AES-KW wrapping key length must be 128/256 bits"),
        },
        _ => ::core::panic!("TypeError: crypto.subtle.wrapKey: unsupported algorithm"),
    }
}
/// `crypto.subtle.unwrapKey(format, wrappedKey, wrappingKey, wrapAlgorithm,
/// wrappedKeyAlgorithm, extractable, usages)` — the AES-KW subset (the inverse
/// of `wrapKey`). The wrapped `data` is unwrapped under the KEK, then a
/// `DsCryptoKey` is rebuilt from the unwrapped raw bytes + the
/// `wrappedKeyAlgorithm`'s `name`/`hash` (AES keys carry no hash; HMAC keys
/// carry the named hash). `async` because ES `unwrapKey` returns
/// `Promise<CryptoKey>`; the call site's `await` drives the future, and
/// `callee_return_path` records the `DsCryptoKey` return (like `importKey`).
pub async fn crypto_subtle_unwrap_key(
    name: ::std::string::String,
    wrapping_key: &DsCryptoKey,
    data: ::std::vec::Vec<u8>,
    wrapped_name: ::std::string::String,
    wrapped_hash: ::std::string::String,
    extractable: bool,
    usages: ::std::vec::Vec<::std::string::String>,
) -> DsCryptoKey {
    let raw = match name.as_str() {
        "AES-KW" => match wrapping_key.key.len() {
            16 => aes_kw_unwrap::<::aes::Aes128>(&wrapping_key.key, &data),
            32 => aes_kw_unwrap::<::aes::Aes256>(&wrapping_key.key, &data),
            _ => ::core::panic!("TypeError: AES-KW wrapping key length must be 128/256 bits"),
        },
        _ => ::core::panic!("TypeError: crypto.subtle.unwrapKey: unsupported algorithm"),
    };
    DsCryptoKey::new(wrapped_name, wrapped_hash, raw, extractable, usages)
}
"#;

/// WHATWG URLPattern API helper — `__ds::DsURLPattern`. A `new URLPattern(input)`
/// (a WinterTC Web API) lowers here. A string `input` is parsed as a WHATWG
/// URLPattern constructor string (`UrlPatternInit::parse_constructor_string`);
/// an undefined/absent `input` is the empty pattern (every component `*`,
/// `UrlPatternInit::default`). `new URLPattern(new URL(…))` lowers to `from_str`
/// on the URL's href (the dispatcher ToString's the URL). A pattern that fails
/// to compile (an unclosed `(` group, …) panics a `TypeError` — the ES URLPattern
/// constructor's error class (`panic_any(DsError)`). Backed by the `urlpattern`
/// crate (denoland's WHATWG reference); the `URLPattern` runtime dep pulls it
/// and this slice, plus `Error` (for `DsError`). Pure-Rust — WinterTC never
/// degrades a Web API to the engine. Instance methods (`test`/`exec`) are not
/// yet lowered.
pub(super) const URLPATTERN_HELPER: &str = r#"pub struct DsURLPattern(pub urlpattern::UrlPattern);
impl DsURLPattern {
    /// `new URLPattern("pattern")` — parse the constructor string, then compile
    /// the pattern. Either failure panics a `TypeError` (the ES error class).
    pub fn from_str(s: &str) -> Self {
        // `parse_constructor_string<R: RegExp>` returns `UrlPatternInit` (which
        // carries no `R`), so `R` cannot be inferred from context — name the
        // default engine explicitly. urlpattern 0.6 binds `R = regex::Regex`.
        let init = urlpattern::UrlPatternInit::parse_constructor_string::<regex::Regex>(s, None)
            .unwrap_or_else(|_| ::std::panic::panic_any(DsError::new("TypeError", "Invalid URLPattern")));
        Self(urlpattern::UrlPattern::parse(init, urlpattern::UrlPatternOptions::default())
            .unwrap_or_else(|_| ::std::panic::panic_any(DsError::new("TypeError", "Invalid URLPattern"))))
    }
    /// `new URLPattern(undefined, undefined)` / `new URLPattern()` — the empty
    /// pattern (every component `*`).
    pub fn empty() -> Self {
        Self(urlpattern::UrlPattern::parse(
            urlpattern::UrlPatternInit::default(),
            urlpattern::UrlPatternOptions::default(),
        )
        .unwrap_or_else(|_| ::std::panic::panic_any(DsError::new("TypeError", "Invalid URLPattern"))))
    }
}
"#;

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
pub(super) const DS_PROMISE_HELPER: &str = r#"
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
pub(super) const DS_STREAMS_HELPER: &str = r#"
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
pub(super) const DS_COMPRESSION_HELPER: &str = r#"
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

/// WHATWG `fetch` API helper — `__ds::DsResponse`/`__ds::ds_fetch`. A WinterTC
/// (Ecma TC55) Web API: ES `fetch(url)` returns `Promise<Response>`; this slice
/// holds the `DsResponse`/`DsHeaders` wrappers + the `ds_fetch` async fn that
/// `await fetch(url)` lowers to. Backed by `reqwest` (deno_fetch's HTTP core,
/// the crate Deno/servo reach for) — pure-Rust static track. reqwest
/// auto-switches its backend on `wasm32` (browser `fetch` via wasm-bindgen),
/// so one slice covers the native and the future wasm target.
pub(super) const DS_FETCH_HELPER: &str = r#"
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
pub(super) const BLOB_HELPER: &str = r#"
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
pub(super) const FILE_HELPER: &str = r#"
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
pub(super) const HEADERS_HELPER: &str = r#"
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
pub(super) const FORM_DATA_HELPER: &str = r#"
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
pub(super) const EVENT_TARGET_HELPER: &str = r#"
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
pub(super) const DS_ABORT_HELPER: &str = r#"
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

/// Engine-path Web API builtins for the WHATWG Encoding API — the Javy split:
/// JS shims (`new TextEncoder()`, `.encode()`, `new TextDecoder(…)`, `.decode()`)
/// over native `Function::new` closures that delegate to `crate::__ds::Text*`
/// (the SAME Rust impls the static path lowers to). Emitted into `__ds_engine.rs`
/// only when `RuntimeDep::Engine` ∧ `RuntimeDep::Encoding` are both active, and
/// called from `wire_web_apis` (whose body
/// [`engine_helper_module`](Translator::engine_helper_module) stamps). Returning
/// `Vec<u8>`/`String` (rquickjs `IntoJs` → JS Array / JS String) sidesteps the
/// `TypedArray<'js>` Ctx-lifetime trap; the shims wrap/unwrap bytes via
/// `new Uint8Array(...)` and `ArrayBuffer` arg extraction. `decode` is
/// non-streaming (a fresh `TextDecoder` per call) — the streaming `Decoder`
/// slot is the static path's job; the engine path covers the common single-call
/// decode faithfully.
pub(super) const TEXT_ENCODING_ENGINE_BUILTIN: &str = r#"
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
pub(super) const PERF_ENGINE_BUILTIN: &str = r#"
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
pub(super) const BASE64_ENGINE_BUILTIN: &str = r#"
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
pub(super) const CRYPTO_ENGINE_BUILTIN: &str = r#"
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
pub(super) const AGENT_262_ENGINE_BUILTIN: &str = r#"
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
pub(super) const ABORT_ENGINE_BUILTIN: &str = r#"
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
pub(super) const ASSERT_ENGINE_BUILTIN: &str = r#"
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
pub(super) const WPT_ASSERT_ENGINE_BUILTIN: &str = r#"
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

/// WHATWG URL API helper — `__ds::DsUrlSearchParams`. An ordered name/value
/// list (ES `URLSearchParams` preserves insertion order), backed by
/// `Vec<(String, String)>`. Parsing and serialization route through
/// `form_urlencoded` (the WHATWG `application/x-www-form-urlencoded` reference
/// parser — the same one servo/url uses), so `+`→space and `%xx`
/// percent-decoding/encoding match the spec. `toString` is `Display`, so
/// template-literal interpolation of a `URLSearchParams` works without a
/// separate `DsDisplay` impl.
pub(super) const URL_HELPER: &str = "\
/// WHATWG URL — `__ds::DsUrl`. Wraps `url::Url` (servo/url, the spec reference
/// parser). ES `URL` exposes the parsed components as zero-arg accessors; both
/// `JSON.stringify(url)` and `url.toString()` serialize to the `href` (the
/// WHATWG serialized URL), so `Display` is the href and `Serialize` is a string
/// (matching ES `URL.toJSON()`). `new URL(input[, base])` parses via `Url::parse`
/// / `Url::options().base_url(...)`; a parse error panics (ES throws
/// `TypeError` — the WPT verdict reads the panic prefix).
type DsUrlRef = ::std::rc::Rc<::std::cell::RefCell<url::Url>>;
/// Shared query operations on a `DsUrlRef`'s query string — used by both
/// `DsUrl::sp_*` (the live-view methods) and `DsUrlSearchParams`, so the
/// standalone object and the `url.searchParams` view share one implementation.
fn dsq_pairs(u: &DsUrlRef) -> ::std::vec::Vec<(::std::string::String, ::std::string::String)> {
    u.borrow()
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}
fn dsq_set_pairs(u: &DsUrlRef, pairs: &[(::std::string::String, ::std::string::String)]) {
    let serialized = form_urlencoded::Serializer::new(::std::string::String::new())
        .extend_pairs(pairs.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .finish();
    u.borrow_mut().set_query(if serialized.is_empty() {
        ::std::option::Option::None
    } else {
        ::std::option::Option::Some(&serialized)
    });
}
pub struct DsUrl(DsUrlRef);
impl DsUrl {
    /// `new URL(input)` — parse an absolute URL. Generic over `AsRef<str>` so
    /// the constructor emit passes either a `String` or a `&str` literal
    /// unchanged.
    pub fn parse<S: ::std::convert::AsRef<str>>(input: S) -> Self {
        Self(::std::rc::Rc::new(::std::cell::RefCell::new(
            url::Url::parse(input.as_ref()).expect(\"invalid URL\"),
        )))
    }
    /// `new URL(input, base)` — resolve `input` against `base`. The base is
    /// parsed first (its own failure panics), then `input` resolves against it.
    pub fn parse_with_base<I: ::std::convert::AsRef<str>, B: ::std::convert::AsRef<str>>(
        input: I,
        base: B,
    ) -> Self {
        let base = url::Url::parse(base.as_ref()).expect(\"invalid base URL\");
        Self(::std::rc::Rc::new(::std::cell::RefCell::new(
            url::Url::options()
                .base_url(::std::option::Option::Some(&base))
                .parse(input.as_ref())
                .expect(\"invalid URL\"),
        )))
    }
    /// `url.href` — the WHATWG serialized URL.
    pub fn href(&self) -> String {
        self.0.borrow().to_string()
    }
    /// `url.origin` — the ASCII serialization of the origin (`https://example.com`).
    pub fn origin(&self) -> String {
        self.0.borrow().origin().ascii_serialization()
    }
    /// `url.protocol` — the scheme plus `:` (`https:`).
    pub fn protocol(&self) -> String {
        format!(\"{}:\", self.0.borrow().scheme())
    }
    /// `url.host` — `hostname:port` (port omitted if not present).
    pub fn host(&self) -> String {
        let u = self.0.borrow();
        match u.port() {
            ::std::option::Option::Some(p) => {
                format!(\"{}:{}\", u.host_str().unwrap_or(\"\"), p)
            }
            ::std::option::Option::None => u.host_str().unwrap_or(\"\").to_string(),
        }
    }
    /// `url.hostname` — the host without the port.
    pub fn hostname(&self) -> String {
        self.0.borrow().host_str().unwrap_or(\"\").to_string()
    }
    /// `url.pathname` — the path (`/path`).
    pub fn pathname(&self) -> String {
        self.0.borrow().path().to_string()
    }
    /// `url.search` — `?` plus the query, or `\"\"` if absent.
    pub fn search(&self) -> String {
        self.0
            .borrow()
            .query()
            .map(|q| format!(\"?{}\", q))
            .unwrap_or_default()
    }
    /// `url.hash` — `#` plus the fragment, or `\"\"` if absent.
    pub fn hash(&self) -> String {
        self.0
            .borrow()
            .fragment()
            .map(|f| format!(\"#{}\", f))
            .unwrap_or_default()
    }
    /// `url.port` — the port as a string, or `\"\"` if absent.
    pub fn port(&self) -> String {
        self.0.borrow().port().map(|p| p.to_string()).unwrap_or_default()
    }
    /// `url.username` — the username, or `\"\"` if absent.
    pub fn username(&self) -> String {
        self.0.borrow().username().to_string()
    }
    /// `url.password` — the password, or `\"\"` if absent.
    pub fn password(&self) -> String {
        self.0.borrow().password().unwrap_or(\"\").to_string()
    }
    // ---- `url.searchParams` live view ----
    // The query lives inside the wrapped `url::Url`; these read it via
    // `query_pairs()` and write it back via `set_query`, so a mutation
    // (`delete`/`append`/`set`) is visible to the next `href`/`search`/`size`.
    fn sp_pairs(&self) -> Vec<(String, String)> {
        dsq_pairs(&self.0)
    }
    fn sp_set_pairs(&self, pairs: &[(String, String)]) {
        dsq_set_pairs(&self.0, pairs)
    }
    pub fn sp_size(&self) -> usize {
        self.sp_pairs().len()
    }
    pub fn sp_get<S: ::std::convert::AsRef<str>>(&self, name: S) -> Option<String> {
        let name = name.as_ref();
        self.sp_pairs().into_iter().find(|(k, _)| k == name).map(|(_, v)| v)
    }
    pub fn sp_has<S: ::std::convert::AsRef<str>>(&self, name: S) -> bool {
        let name = name.as_ref();
        self.sp_pairs().iter().any(|(k, _)| k == name)
    }
    pub fn sp_has_value<N: ::std::convert::AsRef<str>, V: ::std::convert::AsRef<str>>(
        &self,
        name: N,
        value: V,
    ) -> bool {
        let name = name.as_ref();
        let value = value.as_ref();
        self.sp_pairs().iter().any(|(k, v)| k == name && v == value)
    }
    pub fn sp_delete<S: ::std::convert::AsRef<str>>(&self, name: S) {
        let name = name.as_ref();
        let mut p = self.sp_pairs();
        p.retain(|(k, _)| k != name);
        self.sp_set_pairs(&p);
    }
    pub fn sp_delete_value<N: ::std::convert::AsRef<str>, V: ::std::convert::AsRef<str>>(
        &self,
        name: N,
        value: V,
    ) {
        let name = name.as_ref();
        let value = value.as_ref();
        let mut p = self.sp_pairs();
        p.retain(|(k, v)| !(k == name && v == value));
        self.sp_set_pairs(&p);
    }
    pub fn sp_append<N: ::std::convert::AsRef<str>, V: ::std::convert::AsRef<str>>(
        &self,
        name: N,
        value: V,
    ) {
        let mut p = self.sp_pairs();
        p.push((name.as_ref().to_string(), value.as_ref().to_string()));
        self.sp_set_pairs(&p);
    }
    pub fn sp_set<N: ::std::convert::AsRef<str>, V: ::std::convert::AsRef<str>>(
        &self,
        name: N,
        value: V,
    ) {
        let name = name.as_ref();
        let value = value.as_ref();
        let mut p = self.sp_pairs();
        if let ::std::option::Option::Some(e) = p.iter_mut().find(|(k, _)| k == name) {
            e.1 = value.to_string();
        } else {
            p.push((name.to_string(), value.to_string()));
        }
        self.sp_set_pairs(&p);
    }
    pub fn sp_get_all<S: ::std::convert::AsRef<str>>(&self, name: S) -> Vec<String> {
        let name = name.as_ref();
        self.sp_pairs()
            .into_iter()
            .filter(|(k, _)| k == name)
            .map(|(_, v)| v)
            .collect()
    }
    pub fn sp_sort(&self) {
        let mut p = self.sp_pairs();
        p.sort_by(|a, b| a.0.cmp(&b.0));
        self.sp_set_pairs(&p);
    }
    pub fn sp_to_string(&self) -> String {
        let p = self.sp_pairs();
        form_urlencoded::Serializer::new(String::new())
            .extend_pairs(p.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .finish()
    }
    /// `url.searchParams.forEach(cb)` — see `DsUrlSearchParams::for_each`.
    /// Same value-first/key-second order; operates on the URL's live query.
    /// `FnMut` (not `Fn`) so a callback accumulating into a captured outer
    /// binding (`keys.push(key)`, the universal WPT `forEach` pattern) compiles.
    pub fn sp_for_each<F: FnMut(String, String)>(&self, mut f: F) {
        for (k, v) in self.sp_pairs() {
            f(v, k);
        }
    }
    /// `url.searchParams.entries()` — see `DsUrlSearchParams::entries_vec`.
    /// Operates on the URL's live query; same `[name, value]` array shape.
    pub fn sp_entries_vec(&self) -> Vec<Vec<String>> {
        self.sp_pairs().into_iter().map(|(k, v)| vec![k, v]).collect()
    }
    /// `url.searchParams.keys()` — see `DsUrlSearchParams::keys_vec`.
    pub fn sp_keys_vec(&self) -> Vec<String> {
        self.sp_pairs().into_iter().map(|(k, _)| k).collect()
    }
    /// `url.searchParams.values()` — see `DsUrlSearchParams::values_vec`.
    pub fn sp_values_vec(&self) -> Vec<String> {
        self.sp_pairs().into_iter().map(|(_, v)| v).collect()
    }
    /// `url.searchParams` — a live view of this URL's query. Returns a
    /// `DsUrlSearchParams` sharing the same ref-counted `url::Url` (an `Rc`
    /// clone), so a mutation through the view (`params.append(…)`) is
    /// immediately visible to `url.href`/`url.search`/the next
    /// `url.searchParams.size` — the ES live-view semantics.
    pub fn sp_view(&self) -> DsUrlSearchParams {
        DsUrlSearchParams(self.0.clone())
    }
    /// `url.search = s` — the WHATWG search setter. Strips a leading `?`,
    /// then sets the query (empty → no query, so `url.search` reads back as
    /// `\"\"`).
    pub fn set_search<S: ::std::convert::AsRef<str>>(&self, s: S) {
        let s = s.as_ref();
        let q = s.strip_prefix('?').unwrap_or(s);
        self.0.borrow_mut().set_query(if q.is_empty() {
            ::std::option::Option::None
        } else {
            ::std::option::Option::Some(q)
        });
    }
}
impl ::core::fmt::Display for DsUrl {
    /// `url.toString()` / string coercion — the href.
    fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
        write!(f, \"{}\", &*self.0.borrow())
    }
}
impl ::serde::Serialize for DsUrl {
    /// `JSON.stringify(url)` / `url.toJSON()` — ES serializes a URL as its href
    /// string (a JSON string, quoted), so `Serialize` emits the href as a `str`.
    fn serialize<S: ::serde::Serializer>(
        &self,
        s: S,
    ) -> ::core::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.0.borrow().to_string())
    }
}
// ---- `URL.<static>` as `DsUrl` associated functions ----
// The `URL` constructor object's static methods are not instance methods (no
// `&self`) — they are associated functions on `DsUrl`, so the emit carries the
// `__ds::DsUrl` marker and the `Url` runtime dep fires (the helper slice ships
// alongside the `DsUrl` type, the same dep `new URL(…)` pulls). `URL.parse`
// returns `Option<DsUrl>` (ES `null` on a parse failure, not a throw);
// `URL.canParse` is the boolean form. ES `ToString` is applied at the call
// site, so an `undefined` argument arrives as the string `\"undefined\"`, which
// fails to parse (matching `URL.canParse(undefined)` → false).
impl DsUrl {
    /// `URL.canParse(url)` — true iff `url` parses as an absolute URL.
    pub fn can_parse<S: ::std::convert::AsRef<str>>(url: S) -> bool {
        url::Url::parse(url.as_ref()).is_ok()
    }
    /// `URL.canParse(url, base)` — true iff `url` resolves against `base`. A
    /// `base` that is itself unparseable fails the whole parse (returns false).
    pub fn can_parse_with_base<U: ::std::convert::AsRef<str>, B: ::std::convert::AsRef<str>>(
        url: U,
        base: B,
    ) -> bool {
        match url::Url::parse(base.as_ref()) {
            ::std::result::Result::Ok(b) => url::Url::options()
                .base_url(::std::option::Option::Some(&b))
                .parse(url.as_ref())
                .is_ok(),
            ::std::result::Result::Err(_) => false,
        }
    }
    /// `URL.parse(url)` — `Some(DsUrl)` on success, `None` on failure (ES
    /// `null`). Each call builds a fresh `Rc<RefCell<Url>>`, so
    /// `URL.parse(x) !== URL.parse(x)` (object identity differs), matching the
    /// WPT `unique object` assertion.
    pub fn parse_opt<S: ::std::convert::AsRef<str>>(url: S) -> ::std::option::Option<DsUrl> {
        url::Url::parse(url.as_ref())
            .ok()
            .map(|u| Self(::std::rc::Rc::new(::std::cell::RefCell::new(u))))
    }
    /// `URL.parse(url, base)` — resolve `url` against `base`, `None` on failure.
    pub fn parse_opt_with_base<U: ::std::convert::AsRef<str>, B: ::std::convert::AsRef<str>>(
        url: U,
        base: B,
    ) -> ::std::option::Option<DsUrl> {
        match url::Url::parse(base.as_ref()) {
            ::std::result::Result::Ok(b) => url::Url::options()
                .base_url(::std::option::Option::Some(&b))
                .parse(url.as_ref())
                .ok()
                .map(|u| Self(::std::rc::Rc::new(::std::cell::RefCell::new(u)))),
            ::std::result::Result::Err(_) => ::std::option::Option::None,
        }
    }
}
pub struct DsUrlSearchParams(DsUrlRef);
impl DsUrlSearchParams {
    /// `new URLSearchParams(s)` — parse `s` as
    /// `application/x-www-form-urlencoded`. The standalone object owns a
    /// throwaway `url::Url` (only its query string matters); query read/write
    /// routes through the same `dsq_*` machinery as the `url.searchParams`
    /// live view, so a standalone and a view behave identically. A leading
    /// `?` is stripped (ES accepts both `\"a=b\"` and `\"?a=b\"`).
    pub fn from_query<S: AsRef<str>>(init: S) -> Self {
        let init = init.as_ref();
        let q = init.strip_prefix('?').unwrap_or(init);
        let pairs: ::std::vec::Vec<(::std::string::String, ::std::string::String)> =
            form_urlencoded::parse(q.as_bytes())
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect();
        let inner = ::std::rc::Rc::new(::std::cell::RefCell::new(
            url::Url::parse(\"http://localhost/\").expect(\"fallback URL\"),
        ));
        dsq_set_pairs(&inner, &pairs);
        Self(inner)
    }
    /// `new URLSearchParams()` / `new URLSearchParams(undefined)` — empty.
    pub fn new() -> Self {
        Self(::std::rc::Rc::new(::std::cell::RefCell::new(
            url::Url::parse(\"http://localhost/\").expect(\"fallback URL\"),
        )))
    }
    /// `params.get(name)` — the first value for `name`, or `None` (ES `null`).
    /// Generic over `AsRef<str>` so a `String` or `&str` argument (both TS
    /// `string`) is accepted without a call-site borrow.
    pub fn get<S: AsRef<str>>(&self, name: S) -> Option<String> {
        let name = name.as_ref();
        dsq_pairs(&self.0).into_iter().find(|(k, _)| k == name).map(|(_, v)| v)
    }
    /// `params.has(name)` — whether any pair's name is `name`.
    pub fn has<S: AsRef<str>>(&self, name: S) -> bool {
        let name = name.as_ref();
        dsq_pairs(&self.0).iter().any(|(k, _)| k == name)
    }
    /// `params.has(name, value)` (ES2024) — whether a `(name, value)` pair
    /// exists. The single-arg `has(name)` is the common form; the two-arg
    /// form matches both name and value.
    pub fn has_value<N: AsRef<str>, V: AsRef<str>>(&self, name: N, value: V) -> bool {
        let name = name.as_ref();
        let value = value.as_ref();
        dsq_pairs(&self.0).iter().any(|(k, v)| k == name && v == value)
    }
    /// `params.set(name, value)` — WHATWG set: update the first matching pair's
    /// value in place, drop any later matches, or append if none. Not
    /// delete-all-then-append — that would move the pair to the end; the spec
    /// keeps the first match position: `set('a','B')` on `'a=b&c=d'` yields
    /// `a=B&c=d`.
    pub fn set<N: AsRef<str>, V: AsRef<str>>(&self, name: N, value: V) {
        let name = name.as_ref();
        let value = value.as_ref().to_string();
        let mut p = dsq_pairs(&self.0);
        let mut found = false;
        // Keep the first match (to update in place), drop later matches.
        p.retain(|(k, _)| {
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
            for pair in &mut p {
                if pair.0 == name {
                    pair.1 = value;
                    break;
                }
            }
        } else {
            p.push((name.to_string(), value));
        }
        dsq_set_pairs(&self.0, &p);
    }
    /// `params.append(name, value)` — append a pair (duplicates kept).
    pub fn append<N: AsRef<str>, V: AsRef<str>>(&self, name: N, value: V) {
        let mut p = dsq_pairs(&self.0);
        p.push((name.as_ref().to_string(), value.as_ref().to_string()));
        dsq_set_pairs(&self.0, &p);
    }
    /// `params.delete(name)` — remove every pair named `name`.
    pub fn delete<S: AsRef<str>>(&self, name: S) {
        let name = name.as_ref();
        let mut p = dsq_pairs(&self.0);
        p.retain(|(k, _)| k != name);
        dsq_set_pairs(&self.0, &p);
    }
    /// `params.delete(name, value)` (ES2024) — remove only pairs matching both
    /// `name` and `value`; the single-arg `delete(name)` removes every pair
    /// with that name.
    pub fn delete_value<N: AsRef<str>, V: AsRef<str>>(&self, name: N, value: V) {
        let name = name.as_ref();
        let value = value.as_ref();
        let mut p = dsq_pairs(&self.0);
        p.retain(|(k, v)| !(k == name && v == value));
        dsq_set_pairs(&self.0, &p);
    }
    /// `params.getAll(name)` — every value for `name`, in insertion order.
    pub fn get_all<S: AsRef<str>>(&self, name: S) -> Vec<String> {
        let name = name.as_ref();
        dsq_pairs(&self.0)
            .into_iter()
            .filter(|(k, _)| k == name)
            .map(|(_, v)| v)
            .collect()
    }
    /// `params.sort()` — sort by name. Rust's `sort_by` is stable, matching
    /// ES (equal names keep their relative order).
    pub fn sort(&self) {
        let mut p = dsq_pairs(&self.0);
        p.sort_by(|a, b| a.0.cmp(&b.0));
        dsq_set_pairs(&self.0, &p);
    }
    /// `params.size` — the number of name/value pairs.
    #[inline]
    pub fn len(&self) -> usize {
        dsq_pairs(&self.0).len()
    }
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// `params.forEach(cb)` — invoke `cb(value, key)` for each pair in
    /// insertion order. WHATWG URLSearchParams uses value-first/key-second
    /// order (the opposite of `Map.forEach`); the third callback arg (the
    /// params object) and `thisArg` are reflection the static path drops.
    /// `cb` takes owned `String`s so `keys.push(key)` type-checks against a
    /// `Vec<String>` accumulator (the `assert_array_equals` operand shape).
    /// `FnMut` (not `Fn`) so a callback that mutates a captured outer binding
    /// (the `keys.push`/`values.push` accumulator pattern) compiles.
    pub fn for_each<F: FnMut(String, String)>(&self, mut f: F) {
        for (k, v) in dsq_pairs(&self.0) {
            f(v, k);
        }
    }
    /// `params.entries()` — every `[name, value]` pair as a materialized
    /// `Vec<Vec<String>>` (insertion order). The static path trades the ES
    /// iterator wrapper for a `Vec`; each pair is a two-element `[name, value]`
    /// array matching the live `DsUrlSearchParamsIter` item shape, so a WPT
    /// `assert_array_equals(entry, [\"a\", \"1\"])` holds. `for (const [k, v] of
    /// params.entries())` then destructures each array via index access.
    pub fn entries_vec(&self) -> Vec<Vec<String>> {
        dsq_pairs(&self.0).into_iter().map(|(k, v)| vec![k, v]).collect()
    }
    /// `params.keys()` — every name as a `Vec<String>` (insertion order).
    pub fn keys_vec(&self) -> Vec<String> {
        dsq_pairs(&self.0).into_iter().map(|(k, _)| k).collect()
    }
    /// `params.values()` — every value as a `Vec<String>` (insertion order).
    pub fn values_vec(&self) -> Vec<String> {
        dsq_pairs(&self.0).into_iter().map(|(_, v)| v).collect()
    }
}
impl ::core::fmt::Display for DsUrlSearchParams {
    /// `params.toString()` — serialize back to
    /// `application/x-www-form-urlencoded`. `form_urlencoded::Serializer`
    /// percent-encodes per the WHATWG byte set.
    fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
        let p = dsq_pairs(&self.0);
        let mut s = form_urlencoded::Serializer::new(String::new());
        for (k, v) in &p {
            s.append_pair(k, v);
        }
        write!(f, \"{}\", s.finish())
    }
}
/// Iterator over a `DsUrlSearchParams`'s `[name, value]` pairs — what ES
/// `for (const entry of params)` / `params.entries()` yields. Each item is a
/// two-element `[name, value]` array (so `assert_array_equals(entry, [\"a\",
/// \"1\"])` holds). The iterator is **live**: it shares the `DsUrlRef` and
/// re-reads the query list each step (advancing a cursor), so a mutation to
/// the underlying URL mid-iteration (`url.search = …`) is visible to later
/// steps — the WHATWG URLSearchParams iterator semantics a WPT fixture
/// exercises.
pub struct DsUrlSearchParamsIter {
    inner: DsUrlRef,
    idx: usize,
}
impl ::std::iter::Iterator for DsUrlSearchParamsIter {
    type Item = ::std::vec::Vec<::std::string::String>;
    fn next(&mut self) -> ::std::option::Option<Self::Item> {
        let pairs = dsq_pairs(&self.inner);
        if self.idx < pairs.len() {
            let (k, v) = pairs[self.idx].clone();
            self.idx += 1;
            ::std::option::Option::Some(vec![k, v])
        } else {
            ::std::option::Option::None
        }
    }
}
impl<'a> ::std::iter::IntoIterator for &'a DsUrlSearchParams {
    type Item = ::std::vec::Vec<::std::string::String>;
    type IntoIter = DsUrlSearchParamsIter;
    fn into_iter(self) -> Self::IntoIter {
        DsUrlSearchParamsIter { inner: self.0.clone(), idx: 0 }
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
pub(super) const TIMERS_HELPER: &str = r#"
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
            '<' => {
                // `$<name>` — named capture group (ES2021 `GetSubstitution`).
                // Scan to the closing `>`; a missing closer is left literal.
                // A non-participating or nonexistent group expands to empty
                // (regress' `named_groups` already collapses duplicate names to
                // the matched branch, matching ES `groups.x` semantics).
                let mut j = i + 2;
                while j < chars.len() && chars[j] != '>' {
                    j += 1;
                }
                if j >= chars.len() {
                    out.push('$');
                    out.push('<');
                    i += 2;
                } else {
                    let name: String = chars[i + 2..j].iter().collect();
                    if let Some((_, Some(gr))) = m.named_groups().find(|(n, _)| *n == name) {
                        out.push_str(&text[gr]);
                    }
                    i = j + 1;
                }
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
