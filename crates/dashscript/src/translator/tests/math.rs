use super::super::Translator;

#[test]
fn translates_math_methods() {
    let src =
        "function f(x: number): number { return Math.floor(x) + Math.max(x, 0) + Math.pow(x, 2); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("x.floor()"), "got:\n{rust}");
    // Math.max(x, 0) — the 0 now goes through math_receiver (literal → 0_f64)
    // since max folds every arg through it.
    assert!(rust.contains("x.max(0_f64)"), "got:\n{rust}");
    assert!(rust.contains("x.powf(2.0)"), "got:\n{rust}");
}

#[test]
fn translates_math_constants() {
    let src = "function f(): number { return Math.PI; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("f64::consts::PI"), "got:\n{rust}");
}

#[test]
fn translates_math_trig_and_log_methods() {
    let src =
        "function f(x: number): number { return Math.sin(x) + Math.log10(x) + Math.cbrt(x); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".sin("), "got:\n{rust}");
    assert!(rust.contains(".log10("), "got:\n{rust}");
    assert!(rust.contains(".cbrt("), "got:\n{rust}");
}

#[test]
fn translates_math_log_to_ln() {
    let src = "function f(x: number): number { return Math.log(x); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".ln("), "got:\n{rust}");
    assert!(!rust.contains("Math.log"), "got:\n{rust}");
}

#[test]
fn translates_math_atan2_to_atan2() {
    let src = "function f(y: number, x: number): number { return Math.atan2(y, x); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".atan2("), "got:\n{rust}");
}

#[test]
fn translates_math_hypot_to_pythagoras() {
    let src = "function f(a: number, b: number): number { return Math.hypot(a, b); }";
    let rust = Translator::new().translate(src).expect("should translate");
    // hypot now binds args and guards ±∞ (JS hypot(∞, NaN) = ∞): the finite
    // path is still √(Σ aᵢ²), so powi(2) + sqrt() appear, plus the is_infinite
    // guard and the +∞ short-circuit.
    assert!(rust.contains(".powi(2)"), "got:\n{rust}");
    assert!(rust.contains(".sqrt()"), "got:\n{rust}");
    assert!(rust.contains("is_infinite()"), "got:\n{rust}");
}

#[test]
fn translates_math_log1p_to_ln_1p() {
    let src = "function f(x: number): number { return Math.log1p(x); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".ln_1p("), "got:\n{rust}");
}

#[test]
fn translates_math_expm1_to_exp_m1() {
    let src = "function f(x: number): number { return Math.expm1(x); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".exp_m1("), "got:\n{rust}");
}

#[test]
fn translates_math_clz32_to_leading_zeros() {
    let src = "function f(x: number): number { return Math.clz32(x); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".leading_zeros()"), "got:\n{rust}");
    // ToUint32 (mod 2³²), not Rust's saturating `as u32` — clz32(2³²) = 32.
    assert!(rust.contains("4294967296.0"), "got:\n{rust}");
}

#[test]
fn translates_math_fround_to_f32_round_trip() {
    let src = "function f(x: number): number { return Math.fround(x); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("as f32) as f64"), "got:\n{rust}");
}

#[test]
fn translates_math_imul_to_wrapping_mul() {
    let src = "function f(a: number, b: number): number { return Math.imul(a, b); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".wrapping_mul("), "got:\n{rust}");
    // ToInt32 (ToUint32 mod 2³², reinterpreted signed) — imul wraps like JS.
    assert!(rust.contains("4294967296.0"), "got:\n{rust}");
}

#[test]
fn translates_math_sign_to_signum() {
    let src = "function f(x: number): number { return Math.sign(x); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".signum("), "got:\n{rust}");
    assert!(!rust.contains(".sign("), "got:\n{rust}");
}

#[test]
fn translates_math_hyperbolic_methods() {
    let src =
        "function f(x: number): number { return Math.sinh(x) + Math.cosh(x) + Math.tanh(x); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".sinh("), "got:\n{rust}");
    assert!(rust.contains(".cosh("), "got:\n{rust}");
    assert!(rust.contains(".tanh("), "got:\n{rust}");
}

#[test]
fn translates_math_inverse_hyperbolic_methods() {
    let src =
        "function f(x: number): number { return Math.asinh(x) + Math.acosh(x) + Math.atanh(x); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".asinh("), "got:\n{rust}");
    assert!(rust.contains(".acosh("), "got:\n{rust}");
    assert!(rust.contains(".atanh("), "got:\n{rust}");
}

#[test]
fn translates_math_inverse_trig_methods() {
    let src =
        "function f(x: number): number { return Math.asin(x) + Math.acos(x) + Math.atan(x); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".asin("), "got:\n{rust}");
    assert!(rust.contains(".acos("), "got:\n{rust}");
    assert!(rust.contains(".atan("), "got:\n{rust}");
}

#[test]
fn translates_math_extra_constants() {
    let src = "function f(): number { return Math.LN10 + Math.LN2 + Math.LOG10E + Math.LOG2E + Math.SQRT2 + Math.SQRT1_2; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("f64::consts::LN_10"), "got:\n{rust}");
    assert!(rust.contains("f64::consts::LN_2"), "got:\n{rust}");
    assert!(rust.contains("f64::consts::LOG10_E"), "got:\n{rust}");
    assert!(rust.contains("f64::consts::LOG2_E"), "got:\n{rust}");
    assert!(rust.contains("f64::consts::SQRT_2"), "got:\n{rust}");
    assert!(rust.contains("f64::consts::FRAC_1_SQRT_2"), "got:\n{rust}");
}

#[test]
fn translates_math_rounding_and_root_methods() {
    let src = "function f(x: number): number { return Math.abs(x) + Math.ceil(x) + Math.round(x) + Math.sqrt(x) + Math.trunc(x); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".abs()"), "got:\n{rust}");
    assert!(rust.contains(".ceil()"), "got:\n{rust}");
    // Math.round rounds half toward +∞, but avoids `(x + 0.5).floor()` (which
    // reproduces V8's 0.49999999999999994 bug): the half check is
    // `x - floor(x) >= 0.5`, with a 2^52 guard for huge integral doubles.
    assert!(rust.contains(">= 0.5"), "round-half check: {rust}");
    assert!(rust.contains("4503599627370496.0"), "2^52 guard: {rust}");
    assert!(rust.contains(".sqrt()"), "got:\n{rust}");
    assert!(rust.contains(".trunc()"), "got:\n{rust}");
}

#[test]
fn translates_math_exp_log_trig_methods() {
    let src = "function f(x: number): number { return Math.exp(x) + Math.log2(x) + Math.cos(x) + Math.tan(x); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".exp()"), "got:\n{rust}");
    assert!(rust.contains(".log2()"), "got:\n{rust}");
    assert!(rust.contains(".cos()"), "got:\n{rust}");
    assert!(rust.contains(".tan()"), "got:\n{rust}");
}

#[test]
fn translates_math_min_and_e_constant() {
    let src = "function f(a: number, b: number): number { return Math.min(a, b) + Math.E; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".min("), "got:\n{rust}");
    assert!(rust.contains("f64::consts::E"), "got:\n{rust}");
}

#[test]
fn translates_math_max_min_hypot_variadic() {
    // Math.max()/min() with no args: -∞/+∞ (the JS identity element).
    let none = "function f(): number { return Math.max() + Math.min(); }";
    let rust = Translator::new().translate(none).expect("should translate");
    assert!(rust.contains("NEG_INFINITY"), "max(): {rust}");
    assert!(rust.contains("INFINITY"), "min(): {rust}");
    // Math.hypot() = 0.
    let rust = Translator::new()
        .translate("function f(): number { return Math.hypot(); }")
        .expect("should translate");
    assert!(rust.contains("0.0"), "hypot(): {rust}");
    // Math.max(a, b, c) folds binary f64::max left to right.
    let rust = Translator::new()
        .translate(
            "function f(a: number, b: number, c: number): number { return Math.max(a, b, c); }",
        )
        .expect("should translate");
    assert!(rust.contains("a.max(b)"), "fold: {rust}");
    // Math.hypot(a) = |a| = a.powi(2).sqrt().
    let rust = Translator::new()
        .translate("function f(a: number): number { return Math.hypot(a); }")
        .expect("should translate");
    assert!(
        rust.contains(".powi(2)") && rust.contains(".sqrt()"),
        "hypot(a): {rust}"
    );
}

#[test]
fn translates_math_round_avoids_add_half_bug() {
    // `(x + 0.5).floor()` reproduces V8's old bug at 0.49999999999999994
    // (x + 0.5 rounds to 1.0 in double → 1, not 0) and breaks huge doubles;
    // the spec-faithful form uses `x - floor(x) >= 0.5` plus a 2^52 guard.
    let src = "function f(x: number): number { return Math.round(x); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(">= 0.5"), "half check: {rust}");
    assert!(rust.contains("4503599627370496.0"), "2^52 guard: {rust}");
    assert!(!rust.contains("+ 0.5).floor"), "no add-half form: {rust}");
}

#[test]
fn translates_math_sign_keeps_signed_zero() {
    // JS `Math.sign(-0) = -0`, `Math.sign(0) = +0`; Rust's `signum` gives
    // -1/+1, so ±0 is special-cased (return the input, preserving its sign).
    let src = "function f(x: number): number { return Math.sign(x); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("== 0.0"), "zero special-case: {rust}");
    assert!(rust.contains(".signum()"), "non-zero signum: {rust}");
}
