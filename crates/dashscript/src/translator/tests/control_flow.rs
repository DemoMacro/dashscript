use super::super::Translator;

    #[test]
    fn translates_if_else() {
        let src = "function main(): void { let x = 1; if (x > 0) { console.log(1); } else { console.log(2); } }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("if x > 0.0"), "got:\n{rust}");
        assert!(rust.contains(" else "), "got:\n{rust}");
    }


    #[test]
    fn translates_while_with_update() {
        let src = "function main(): void { let i = 0; while (i < 10) { i++; } }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("while i < 10.0"), "got:\n{rust}");
        assert!(rust.contains("i += 1.0"), "got:\n{rust}");
    }


    #[test]
    fn translates_for_of_as_borrow() {
        let src = "function main(): void { const xs: number[] = [1, 2]; for (const v of xs) { console.log(v); } }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("for &v in &xs"), "got:\n{rust}");
    }


    #[test]
    fn translates_switch_to_match() {
        let src = "type Status = \"pending\" | \"done\"; function f(s: Status): void { switch (s) { case \"pending\": console.log(\"p\"); break; default: console.log(\"?\"); } }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("match s"), "got:\n{rust}");
        assert!(rust.contains("Status::Pending"), "got:\n{rust}");
        assert!(rust.contains("_ =>"), "got:\n{rust}");
    }


    #[test]
    fn translates_do_while() {
        let src = "function f(): void { let i = 0; do { i++; } while (i < 3); }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("loop {"), "got:\n{rust}");
        assert!(rust.contains("break"), "got:\n{rust}");
    }


    #[test]
    fn translates_c_style_for_loop() {
        let src = "function f(): void { let total = 0; for (let i = 0; i < 5; i++) { total += i; } console.log(total); }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("while i < 5.0"), "got:\n{rust}");
        assert!(rust.contains("i += 1.0"), "got:\n{rust}");
    }


    #[test]
    fn translates_break_and_continue() {
        let src = "function f(): void { let i = 0; while (i < 10) { i++; if (i == 5) { continue; } if (i == 8) { break; } } }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("continue;"), "got:\n{rust}");
        assert!(rust.contains("break;"), "got:\n{rust}");
    }


    #[test]
    fn translates_if_collection_truthiness() {
        let src = "function f(): void { const xs: number[] = [1]; if (xs) { console.log(1); } }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("xs.is_empty()"), "got:\n{rust}");
    }


    #[test]
    fn translates_if_option_truthiness() {
        // `if (m)` on `Option<f64>` (Copy), branch unused → `if let Some(_)`.
        let src = "function f(): void { let m: number | null = 1; if (m) { console.log(1); } }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("if let Some(_) = m"), "got:\n{rust}");
    }


    #[test]
    fn translates_throw_new_error_to_panic() {
        let src = "function f(): void { throw new Error(\"boom\"); }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("panic!(\"boom\")"), "got:\n{rust}");
    }


    #[test]
    fn translates_throw_string_to_panic() {
        let src = "function f(): void { throw \"oops\"; }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("panic!(\"oops\")"), "got:\n{rust}");
    }


    #[test]
    fn translates_try_catch_to_catch_unwind() {
        let src = "function f(): void { try { throw new Error(\"oops\"); } catch (e) { console.log(e); } }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("catch_unwind"), "got:\n{rust}");
        assert!(rust.contains("AssertUnwindSafe"), "got:\n{rust}");
        // The catch param `e` is bound as the panic payload's message (String).
        assert!(rust.contains("let e ="), "got:\n{rust}");
        assert!(rust.contains("downcast_ref::<&'static str>"), "got:\n{rust}");
    }


    #[test]
    fn translates_try_finally_runs_after_match() {
        let src = "function f(): void { try { console.log(\"a\"); } catch (e) {} finally { console.log(\"b\"); } }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("catch_unwind"), "got:\n{rust}");
        assert!(rust.contains("\"b\""), "got:\n{rust}");
    }


    #[test]
    fn translates_try_block_with_return_rejected() {
        // A `return` in the try block cannot cross the catch_unwind closure
        // boundary — surfaced as a compile_error, not silent miscompilation.
        let src = "function f(): number { try { return 1; } catch (e) { return 0; } }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("compile_error!"), "got:\n{rust}");
        assert!(rust.contains("catch boundary"), "got:\n{rust}");
    }
