use super::super::Translator;

#[test]
fn translates_string_global_numeric_to_helper() {
    let src = "function f(): string { return String(42); }";
    let rust = Translator::new().translate(src).expect("should translate");
    // A numeric arg is ES NumberToString — routed through the ryu-js helper,
    // not `format!` (Rust's `f64` Display differs from ECMAScript).
    assert!(
        rust.contains("__ds::number_to_string(42_f64)"),
        "got:\n{rust}"
    );
}

#[test]
fn translates_parse_int_to_truncating_closure() {
    // parseInt truncates at the first non-digit (ES semantics), inlined as a
    // closure — not a whole-string `parse` (which would turn "12ab" into NaN).
    let src = "function f(): number { return parseInt(\"100\"); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("__pi"), "inlined parse-int closure: {rust}");
    assert!(
        rust.contains("to_digit"),
        "per-digit truncating parse: {rust}"
    );
}

#[test]
fn translates_parse_int_with_radix_and_hex_prefix() {
    let src = "function f(s: string): number { return parseInt(s, 16); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("__pi"), "got:\n{rust}");
    // the 0x/0X auto-detection rides alongside the radix parse.
    assert!(rust.contains("b'x'"), "hex prefix detection: {rust}");
}

#[test]
fn translates_parse_float_to_truncating_closure() {
    // parseFloat takes the longest valid decimal prefix (truncation) —
    // "3.14abc" → 3.14, not NaN. Inlined as a closure.
    let src = "function f(): number { return parseFloat(\"3.14\"); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("__pf"), "inlined parse-float closure: {rust}");
    assert!(
        rust.contains("starts_with(\"Infinity\")"),
        "Infinity handling: {rust}"
    );
}

#[test]
fn translates_number_global_string_to_parse() {
    let src = "function f(): number { return Number(\"42\"); }";
    let rust = Translator::new().translate(src).expect("should translate");
    // ToNumber coercion: an empty/whitespace string is 0, otherwise parse.
    assert!(rust.contains("is_empty()"), "got:\n{rust}");
    assert!(rust.contains("0_f64"), "got:\n{rust}");
    assert!(rust.contains("parse::<f64>()"), "got:\n{rust}");
}

#[test]
fn translates_number_global_string_var_to_parse() {
    let src = "function f(s: string): number { return Number(s); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("is_empty()"), "got:\n{rust}");
    assert!(rust.contains("0_f64"), "got:\n{rust}");
}

#[test]
fn translates_number_global_empty_string_to_zero() {
    // ToNumber("") / ToNumber("  ") is 0, not NaN — `Number("")` must not
    // fall through to `parse` (which rejects an empty string). Mirrors
    // test262's `number.s9.3.1` (ToNumber on a string).
    let src = "function f(): number { return Number(\"\"); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("is_empty()"), "got:\n{rust}");
}

#[test]
fn translates_number_global_number_passes_through() {
    let src = "function f(n: number): number { return Number(n); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        !rust.contains("Number") && !rust.contains("return"),
        "Number(n) passes through to n, got:\n{rust}"
    );
}

#[test]
fn translates_boolean_global_zero_to_false() {
    let src = "function f(): boolean { return Boolean(0); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("false") && !rust.contains("return"),
        "Boolean(0) -> false, got:\n{rust}"
    );
}

#[test]
fn translates_boolean_global_nonzero_to_true() {
    let src = "function f(): boolean { return Boolean(42); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("true") && !rust.contains("return"),
        "Boolean(42) -> true, got:\n{rust}"
    );
}

#[test]
fn translates_boolean_global_string_literal_to_is_empty() {
    let src = "function f(): boolean { return Boolean(\"\"); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("!\"\".to_string().is_empty()"),
        "got:\n{rust}"
    );
}

#[test]
fn translates_boolean_global_vec_to_is_empty() {
    let src = "function f(xs: number[]): boolean { return Boolean(xs); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("!xs.is_empty()"), "got:\n{rust}");
}

#[test]
fn translates_boolean_global_number_var_to_ne_zero() {
    let src = "function f(n: number): boolean { return Boolean(n); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("n != 0_f64"), "got:\n{rust}");
}

#[test]
fn translates_boolean_global_option_to_is_some() {
    let src = "function f(m: number | null): boolean { return Boolean(m); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("m.is_some()"), "got:\n{rust}");
}

#[test]
fn translates_number_static_type_checks() {
    let src = "function f(n: number): boolean { return Number.isInteger(n) && Number.isFinite(n) && Number.isNaN(n); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".fract() == 0_f64"), "got:\n{rust}");
    assert!(rust.contains(".is_finite()"), "got:\n{rust}");
    assert!(rust.contains(".is_nan()"), "got:\n{rust}");
}

#[test]
fn translates_number_is_safe_integer() {
    let src = "function f(n: number): boolean { return Number.isSafeInteger(n); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("9_007_199_254_740_991_f64"), "got:\n{rust}");
}

#[test]
fn translates_number_constants() {
    let src = "function f(): number { return Number.EPSILON + Number.MAX_SAFE_INTEGER + Number.MAX_VALUE + Number.MIN_SAFE_INTEGER + Number.MIN_VALUE + Number.NaN + Number.NEGATIVE_INFINITY + Number.POSITIVE_INFINITY; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("f64::EPSILON"), "got:\n{rust}");
    assert!(rust.contains("9_007_199_254_740_991f64"), "got:\n{rust}");
    assert!(rust.contains("f64::MAX"), "got:\n{rust}");
    assert!(rust.contains("-9_007_199_254_740_991f64"), "got:\n{rust}");
    assert!(rust.contains("f64::MIN_POSITIVE"), "got:\n{rust}");
    assert!(rust.contains("f64::NAN"), "got:\n{rust}");
    assert!(rust.contains("f64::NEG_INFINITY"), "got:\n{rust}");
    assert!(rust.contains("f64::INFINITY"), "got:\n{rust}");
}

#[test]
fn translates_number_parse_float() {
    // Number.parseFloat ≡ the global parseFloat — full truncating semantics.
    let src = "function f(s: string): number { return Number.parseFloat(s); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("__pf"), "inlined parse-float closure: {rust}");
}

#[test]
fn translates_number_parse_int_radix() {
    let src = "function f(s: string): number { return Number.parseInt(s, 16); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("__pi"), "got:\n{rust}");
    assert!(rust.contains("b'x'"), "hex prefix detection: {rust}");
}

#[test]
fn translates_number_to_exponential() {
    let src = "function f(n: number): string { return n.toExponential(2); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("{:.*e}"), "got:\n{rust}");
    // TS signs the exponent (`1e+4`); Rust's `{:e}` prints `1e4` — a bare
    // exponent gets a `+` prepended via a `split_once('e')` fixup.
    assert!(
        rust.contains("split_once('e')"),
        "exponent sign fixup: {rust}"
    );
    assert!(rust.contains("\"{}e+{}\""), "got:\n{rust}");
}

#[test]
fn translates_number_value_of() {
    let src = "function f(n: number): number { return n.valueOf(); }";
    let rust = Translator::new().translate(src).expect("should translate");
    // valueOf is an identity on f64 — the receiver passes through, no `Number`.
    assert!(
        !rust.contains("Number") && !rust.contains("valueOf"),
        "got:\n{rust}"
    );
}

#[test]
fn translates_global_is_nan() {
    let src = "function f(n: number): boolean { return isNaN(n); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".is_nan()"), "got:\n{rust}");
}

#[test]
fn translates_global_is_finite() {
    let src = "function f(n: number): boolean { return isFinite(n); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".is_finite()"), "got:\n{rust}");
}
