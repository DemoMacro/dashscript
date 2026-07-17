use super::super::Translator;

    #[test]
    fn translates_object_destructure_default_to_unwrap_or() {
        let src = "interface User { name?: string; } function f(u: User): void { const { name = \"anon\" } = u; }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains(".unwrap_or(\"anon\".to_string())"), "got:\n{rust}");
    }


    #[test]
    fn translates_discriminated_union_switch_destructure() {
        let src = "type Shape = { kind: \"circle\"; radius: number } | { kind: \"square\"; side: number }; function area(s: Shape): number { switch (s.kind) { case \"circle\": return s.radius * s.radius; case \"square\": return s.side * s.side; } }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("match s"), "got:\n{rust}");
        assert!(rust.contains("Shape::Circle { radius }"), "got:\n{rust}");
        assert!(rust.contains("Shape::Square { side }"), "got:\n{rust}");
        // narrowing: each `s.radius` reads as the destructured `radius` binding.
        assert!(rust.contains("radius * radius"), "got:\n{rust}");
    }


    #[test]
    fn translates_object_destructuring_to_struct_pattern() {
        let src = "interface V { x: number; y: number; } function f(v: V): number { const { x, y } = v; return x + y; }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("let V { x, y, .. } = v;"), "got:\n{rust}");
    }


    #[test]
    fn translates_array_destructuring_to_indexed_lets() {
        let src = "function f(): void { const xs: number[] = [1, 2]; const [a, b] = xs; }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("let a = xs[0];"), "got:\n{rust}");
        assert!(rust.contains("let b = xs[1];"), "got:\n{rust}");
    }


    #[test]
    fn translates_array_destructure_rest_to_slice() {
        let src = "function f(): void { const xs: number[] = [1, 2, 3]; const [a, ...rest] = xs; }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("let rest = xs[1..].to_vec()"), "got:\n{rust}");
    }


    #[test]
    fn translates_object_spread_to_struct_update() {
        let src = "interface Vector { x: number; y: number; } function f(v: Vector): Vector { return { ...v, y: 9 }; }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("Vector { y: 9.0, ..v }"), "got:\n{rust}");
    }


    #[test]
    fn translates_array_spread_to_slice_concat() {
        let src = "function f(): void { const xs: number[] = [1, 2]; const ys = [...xs, 3]; }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("[xs.as_slice(), &[3.0][..]].concat()"), "got:\n{rust}");
    }


    #[test]
    fn translates_array_destructure_skips_holes() {
        let src = "function f(xs: number[]): void { const [a, , c] = xs; }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("xs[0]"), "got:\n{rust}");
        assert!(rust.contains("xs[2]"), "got:\n{rust}");
        assert!(!rust.contains("xs[1]"), "hole must be skipped");
    }


    #[test]
    fn translates_object_destructure_rename() {
        let src = "interface Vector { x: number; } function f(v: Vector): void { const { x: renamed } = v; }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("x: renamed"), "got:\n{rust}");
    }
