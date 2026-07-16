use super::super::Translator;

    #[test]
    fn translates_optional_chain_to_as_ref_map() {
        let src = "interface V { x: number } function f(): void { const v: V | null = null; const x = v?.x; }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("v.as_ref().map(|__c| __c.x)"), "got:\n{rust}");
    }


    #[test]
    fn translates_optional_chain_coalesce_to_unwrap_or() {
        let src = "interface V { x: number } function f(): number { const v: V | null = null; return v?.x ?? -1; }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("unwrap_or(-1.0)"), "got:\n{rust}");
        assert!(rust.contains("__c.x"), "got:\n{rust}");
    }


    #[test]
    fn translates_some_wrapping() {
        let src = "function main(): void { let x: number | null = 5; console.log(x); }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("Option<f64>"), "got:\n{rust}");
        assert!(rust.contains("Some(5.0)"), "got:\n{rust}");
    }


    #[test]
    fn translates_non_null_assertion() {
        let src = "function f(x: number | null): void { console.log(x!); }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("x: Option<f64>"), "got:\n{rust}");
        assert!(rust.contains("x.unwrap()"), "got:\n{rust}");
    }


    #[test]
    fn narrows_option_truthiness_branch_binding() {
        // The branch reads `m!`, so the inner value binds and `m!` needs no unwrap.
        let src = "function f(): void { let m: number | null = 1; if (m) { console.log(m!); } }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("if let Some(m) = m"), "got:\n{rust}");
        assert!(!rust.contains(".unwrap()"), "got:\n{rust}");
    }


    #[test]
    fn non_copy_option_truthiness_keeps_is_some() {
        // `Option<String>` inner is not Copy: narrowing would move out of it.
        let src = "function f(): void { let m: string | null = \"a\"; if (m) { console.log(1); } }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("m.is_some()"), "got:\n{rust}");
    }


    #[test]
    fn mutated_option_truthiness_keeps_is_some() {
        // `m` is reassigned: an `if let` binding cannot be reassigned.
        let src = "function f(): void { let m: number | null = 1; if (m) { m = 2; } }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("m.is_some()"), "got:\n{rust}");
    }


    #[test]
    fn translates_null_equality_to_is_none() {
        let src = "function f(m: number | null): boolean { return m === null; }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("m.is_none()"), "got:\n{rust}");
    }


    #[test]
    fn translates_null_inequality_to_is_some() {
        let src = "function f(m: number | null): boolean { return m !== null; }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("m.is_some()"), "got:\n{rust}");
    }


    #[test]
    fn translates_nullish_coalescing_to_unwrap_or_else() {
        let src = "function f(m: number | null): number { return m ?? 0; }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("m.unwrap_or_else(|| 0.0)"), "got:\n{rust}");
    }


    #[test]
    fn translates_logical_or_value_returns_left_when_truthy() {
        let src = "function f(s: string): string { return s || \"default\"; }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("let __l = "), "got:\n{rust}");
        assert!(rust.contains("!__l.is_empty()"), "got:\n{rust}");
    }


    #[test]
    fn translates_logical_or_bool_short_circuits() {
        let src = "function f(a: boolean, b: boolean): boolean { return a || b; }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("a || b"), "got:\n{rust}");
        assert!(!rust.contains("__l"), "bool should short-circuit, not block");
    }


    #[test]
    fn translates_logical_nullish_assign() {
        let src = "function f(x: number | null): void { x ??= 5; }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("is_none()"), "got:\n{rust}");
        assert!(rust.contains("Some("), "got:\n{rust}");
    }


    #[test]
    fn translates_logical_or_assign() {
        let src = "function f(x: number): void { x ||= 5; }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("if !"), "got:\n{rust}");
    }
