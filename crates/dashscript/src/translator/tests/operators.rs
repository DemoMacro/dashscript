use super::super::Translator;

    #[test]
    fn translates_arithmetic_and_comparison() {
        let src = "function f(): void { console.log(1 + 2 * 3); console.log(4 >= 2); }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("1.0 + 2.0 * 3.0"), "got:\n{rust}");
        assert!(rust.contains("4.0 >= 2.0"), "got:\n{rust}");
    }


    #[test]
    fn translates_logical_and_unary() {
        let src = "function f(): void { let b = true; console.log(b && !b); console.log(-5); }";
        let rust = Translator::new().translate(src).expect("should translate");
        // `prettyplease` prints a space after unary `!`/`-` (`! b`, `- 5.0`); the
        // structure — `&&` then logical-not, and unary negation — is what we check.
        assert!(rust.contains("b && "), "got:\n{rust}");
        assert!(rust.contains("! b"), "got:\n{rust}");
        assert!(rust.contains("- 5.0"), "got:\n{rust}");
    }


    #[test]
    fn translates_compound_assignment() {
        let src = "function f(): void { let n = 0; n += 5; n = n * 2; }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("n += 5.0"), "got:\n{rust}");
        assert!(rust.contains("n = n * 2.0"), "got:\n{rust}");
    }


    #[test]
    fn translates_template_literal() {
        let src = "function greet(name: string): string { return `Hello, ${name}!`; }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("format!"), "got:\n{rust}");
        assert!(rust.contains("\"Hello, {}!\""), "got:\n{rust}");
        assert!(rust.contains("name)"), "got:\n{rust}");
    }


    #[test]
    fn translates_ternary_to_if_expression() {
        let src = "function f(x: number): string { return x > 0 ? \"pos\" : \"neg\"; }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("if x > 0.0 {"), "got:\n{rust}");
        assert!(rust.contains("\"pos\".to_string()"), "got:\n{rust}");
        assert!(rust.contains("} else {"), "got:\n{rust}");
        assert!(rust.contains("\"neg\".to_string()"), "got:\n{rust}");
    }


    #[test]
    fn translates_length_to_len() {
        let src = "function f(): void { let n = \"hi\".length; }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains(".len()"), "got:\n{rust}");
    }


    #[test]
    fn unwraps_parenthesized_expression() {
        let src = "function f(a: number, b: number, c: number): number { return (a + b) * c; }";
        let rust = Translator::new().translate(src).expect("should translate");
        // parens are unwrapped, then prettyplease re-adds them for precedence
        assert!(rust.contains("(a + b) * c"), "got:\n{rust}");
    }


    #[test]
    fn translates_exponent_operator() {
        let src = "function f(x: number): number { return x ** 2; }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains(".powf(2.0)"), "got:\n{rust}");
    }


    #[test]
    fn translates_in_operator_to_contains_key() {
        let src = "function f(m: Record<string, number>): boolean { return \"k\" in m; }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains(".contains_key(\"k\")"), "got:\n{rust}");
    }


    #[test]
    fn translates_arrow_function_expression_body() {
        let src = "function f(): void { const double = (x: number) => x * 2; console.log(double(5)); }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("|x| x * 2.0"), "got:\n{rust}");
    }


    #[test]
    fn translates_field_assign_to_field() {
        let src = "function f(v: Vector): void { v.x = 5; }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("v.x = 5.0"), "got:\n{rust}");
    }


    #[test]
    fn translates_bitwise_and_or_xor() {
        let src = "function f(a: number, b: number): number { return (a & b) + (a | b) + (a ^ b); }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("as i32) &"), "got:\n{rust}");
        assert!(rust.contains("as i32) |"), "got:\n{rust}");
        assert!(rust.contains("as i32) ^"), "got:\n{rust}");
    }


    #[test]
    fn translates_bitwise_shifts() {
        let src = "function f(a: number, b: number): number { return (a << b) + (a >> b) + (a >>> b); }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains(".wrapping_shl("), "got:\n{rust}");
        assert!(rust.contains(".wrapping_shr("), "got:\n{rust}");
        assert!(rust.contains("as u32).wrapping_shr"), "got:\n{rust}");
    }


    #[test]
    fn translates_bitwise_not() {
        let src = "function f(a: number): number { return ~a; }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("!(a as i32)"), "got:\n{rust}");
    }


    #[test]
    fn translates_bitwise_compound_assign() {
        let src = "function f(a: number, b: number): void { a &= b; a <<= b; a **= 2; }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("a = ((a as i32) &"), "got:\n{rust}");
        assert!(rust.contains(".wrapping_shl("), "got:\n{rust}");
        assert!(rust.contains("a = a.powf(2.0)"), "got:\n{rust}");
    }
