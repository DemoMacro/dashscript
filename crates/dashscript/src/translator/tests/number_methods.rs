use super::super::Translator;

    #[test]
    fn translates_number_to_fixed_to_format_precision() {
        let src = "function f(): string { const pi = 3.14159; return pi.toFixed(2); }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("format!(\"{:.*}\", 2.0 as usize, pi)"), "got:\n{rust}");
    }


    #[test]
    fn translates_number_to_string_radix_hex() {
        let src = "function f(n: number): string { return n.toString(16); }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("\"{:x}\""), "got:\n{rust}");
        assert!(rust.contains("as u32"), "got:\n{rust}");
    }


    #[test]
    fn translates_number_to_string_radix_binary() {
        let src = "function f(): string { return (255).toString(2); }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("\"{:b}\""), "got:\n{rust}");
    }


    #[test]
    fn translates_number_to_string_no_arg_is_display() {
        let src = "function f(n: number): string { return n.toString(); }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains(".to_string()"), "got:\n{rust}");
        assert!(!rust.contains("as u32"), "got:\n{rust}");
    }
