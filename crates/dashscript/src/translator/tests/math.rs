use super::super::Translator;

    #[test]
    fn translates_math_methods() {
        let src =
            "function f(x: number): number { return Math.floor(x) + Math.max(x, 0) + Math.pow(x, 2); }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("x.floor()"), "got:\n{rust}");
        assert!(rust.contains("x.max(0.0)"), "got:\n{rust}");
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
        let src = "function f(x: number): number { return Math.sin(x) + Math.log10(x) + Math.cbrt(x); }";
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
        assert!(
            rust.contains("a.powi(2) + b.powi(2)).sqrt()"),
            "got:\n{rust}"
        );
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
        assert!(rust.contains("as u32).leading_zeros()"), "got:\n{rust}");
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
        assert!(rust.contains("as i32).wrapping_mul("), "got:\n{rust}");
    }
