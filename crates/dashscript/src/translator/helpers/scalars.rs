pub const RYUJS_HELPERS: &str = "\
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
pub const ARRAY_HELPER: &str = "\
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
pub const ENCODING_HELPER: &str = "\
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
pub const BASE64_HELPER: &str = r#"use base64::prelude::{Engine as _, BASE64_STANDARD};
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
pub const URI_HELPER: &str = r#"
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
pub const SUM_PRECISE_HELPER: &str = r#"
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
pub const PERF_HELPER: &str = r#"
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
