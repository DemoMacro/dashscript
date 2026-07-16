//! oxc AST → idiomatic Rust source, emitted through `syn` + `prettyplease`.
//!
//! Translation is one file per AST category — `declarations`, `functions`,
//! `types`, `expressions`, `bindings` — so each oxc node maps to a `syn` node
//! one-to-one. The `syn` tree is the project's hub: the translator builds it
//! (oxc → syn), `prettyplease` prints it, and the future `bindgen` parses
//! Rust crates into the same `syn` tree (syn → .ds) — one AST, two
//! directions. Parsing reuses `oxc_parser`; DashScript never parses itself.

pub mod bindings;
pub mod context;
pub mod declarations;
pub mod expressions;
pub mod functions;
pub mod registry;
pub mod types;

use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_span::SourceType;

/// Translates a TypeScript-flavored `.ds` program into Rust source.
#[derive(Default)]
pub struct Translator;

impl Translator {
    /// Create a translator with default options.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Parse `.ds` source with oxc and translate the AST to Rust source.
    ///
    /// # Errors
    /// Returns an error string if oxc reports parse diagnostics.
    pub fn translate(&self, source: &str) -> Result<String, String> {
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, source, SourceType::ts()).parse();

        if !ret.diagnostics.is_empty() {
            return Err(format!(
                "dashscript: oxc reported {} parse diagnostic(s)",
                ret.diagnostics.len()
            ));
        }

        // First pass: collect discriminated-union enum shapes so later
        // expression translation can build variant constructors.
        let registry = registry::build_registry(&ret.program.body);
        let items = ret
            .program
            .body
            .iter()
            .filter_map(|s| functions::translate_statement(s, &registry))
            .collect();
        let file = syn::File { shebang: None, attrs: Vec::new(), items };
        Ok(prettyplease::unparse(&file))
    }
}

#[cfg(test)]
mod tests {
    use super::Translator;

    #[test]
    fn translates_a_typed_function_returning_a_string() {
        let src = "function greet(name: string): string { return \"Hello\"; }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("fn greet(name: String) -> String"), "got:\n{rust}");
        assert!(rust.contains("\"Hello\".to_string()"), "got:\n{rust}");
    }

    #[test]
    fn reports_parse_diagnostics() {
        assert!(Translator::new().translate("function (").is_err());
    }

    #[test]
    fn translates_interface_to_struct() {
        let src = "interface Point { x: number; y: number; }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("struct Point"), "got:\n{rust}");
        assert!(rust.contains("pub x: f64"), "got:\n{rust}");
        assert!(rust.contains("pub y: f64"), "got:\n{rust}");
    }

    #[test]
    fn translates_array_type_to_vec() {
        let src = "interface Box { items: number[]; ids: Array<string>; }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("Vec<f64>"), "got:\n{rust}");
        assert!(rust.contains("Vec<String>"), "got:\n{rust}");
    }

    #[test]
    fn translates_locals_object_literal_and_field_access() {
        let src =
            "interface Point { x: number } function main(): void { const p: Point = { x: 1 }; console.log(p.x); }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("Point { x: 1.0 }"), "got:\n{rust}");
        assert!(rust.contains("p.x"), "got:\n{rust}");
    }

    #[test]
    fn translates_mutable_let_as_let_mut() {
        let src = "function main(): void { let n: number = 0; console.log(n); }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("let mut n"), "got:\n{rust}");
    }

    #[test]
    fn translates_array_literal_to_vec_macro() {
        let src = "function main(): void { const xs: number[] = [1, 2, 3]; console.log(xs); }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("vec![1.0, 2.0, 3.0]"), "got:\n{rust}");
    }

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
    fn translates_nullable_to_option() {
        let src = "function main(): void { let x: number | null = null; console.log(x); }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("Option<f64>"), "got:\n{rust}");
        assert!(rust.contains("= None"), "got:\n{rust}");
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
    fn translates_nullable_return_type() {
        let src = "function f(): number | null { return null; }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("-> Option<f64>"), "got:\n{rust}");
        assert!(rust.contains("return None"), "got:\n{rust}");
    }

    #[test]
    fn translates_string_union_to_enum() {
        let src = "type Status = \"pending\" | \"active\" | \"done\";";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("enum Status"), "got:\n{rust}");
        assert!(rust.contains("Pending"), "got:\n{rust}");
        assert!(rust.contains("Active"), "got:\n{rust}");
        assert!(rust.contains("Done"), "got:\n{rust}");
    }

    #[test]
    fn translates_enum_variant_construction() {
        let src = "type Status = \"pending\" | \"done\"; function f(): void { let s: Status = \"done\"; }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("Status::Done"), "got:\n{rust}");
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
    fn translates_switch_to_match() {
        let src = "type Status = \"pending\" | \"done\"; function f(s: Status): void { switch (s) { case \"pending\": console.log(\"p\"); break; default: console.log(\"?\"); } }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("match s"), "got:\n{rust}");
        assert!(rust.contains("Status::Pending"), "got:\n{rust}");
        assert!(rust.contains("_ =>"), "got:\n{rust}");
    }

    #[test]
    fn translates_string_method_call() {
        let src = "function f(): void { let s = \"hello\".toUpperCase(); console.log(s); }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains(".to_string().to_uppercase()"), "got:\n{rust}");
    }

    #[test]
    fn translates_length_to_len() {
        let src = "function f(): void { let n = \"hi\".length; }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains(".len()"), "got:\n{rust}");
    }

    #[test]
    fn translates_array_index() {
        let src = "function f(): void { const xs: number[] = [1, 2]; console.log(xs[0]); }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("xs[0.0 as usize]"), "got:\n{rust}");
    }

    #[test]
    fn translates_string_concatenation_to_format() {
        let src = "function greet(first: string, last: string): string { return first + \" \" + last; }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("format!"), "got:\n{rust}");
        // three operands → three placeholders
        assert!(rust.contains("\"{}{}{}\""), "got:\n{rust}");
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
    fn translates_do_while() {
        let src = "function f(): void { let i = 0; do { i++; } while (i < 3); }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("loop {"), "got:\n{rust}");
        assert!(rust.contains("break"), "got:\n{rust}");
    }

    #[test]
    fn translates_string_predicate_methods() {
        let src = "function f(s: string): boolean { return s.includes(\"x\") && s.startsWith(\"a\"); }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains(".contains(\"x\")"), "got:\n{rust}");
        assert!(rust.contains(".starts_with(\"a\")"), "got:\n{rust}");
    }

    #[test]
    fn translates_string_repeat_and_replace() {
        let repeat = "function f(s: string): string { return s.repeat(3); }";
        let rust = Translator::new().translate(repeat).expect("should translate");
        assert!(rust.contains(".repeat(3.0 as usize)"), "got:\n{rust}");
        let replace = "function g(s: string): string { return s.replace(\"a\", \"b\"); }";
        let rust = Translator::new().translate(replace).expect("should translate");
        assert!(rust.contains(".replacen(\"a\", \"b\", 1)"), "got:\n{rust}");
    }

    #[test]
    fn translates_c_style_for_loop() {
        let src = "function f(): void { let total = 0; for (let i = 0; i < 5; i++) { total += i; } console.log(total); }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("while i < 5.0"), "got:\n{rust}");
        assert!(rust.contains("i += 1.0"), "got:\n{rust}");
    }

    #[test]
    fn translates_arrow_function_expression_body() {
        let src = "function f(): void { const double = (x: number) => x * 2; console.log(double(5)); }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("|x| x * 2.0"), "got:\n{rust}");
    }

    #[test]
    fn translates_break_and_continue() {
        let src = "function f(): void { let i = 0; while (i < 10) { i++; if (i == 5) { continue; } if (i == 8) { break; } } }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("continue;"), "got:\n{rust}");
        assert!(rust.contains("break;"), "got:\n{rust}");
    }

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
    fn translates_multi_arg_console_log() {
        let src = "function f(): void { console.log(\"x\", 1, true); }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("\"{} {} {}\""), "got:\n{rust}");
        assert!(!rust.contains("todo!"), "got:\n{rust}");
    }

    #[test]
    fn translates_if_collection_truthiness() {
        let src = "function f(): void { const xs: number[] = [1]; if (xs) { console.log(1); } }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("xs.is_empty()"), "got:\n{rust}");
    }

    #[test]
    fn translates_if_option_truthiness() {
        let src = "function f(): void { let m: number | null = 1; if (m) { console.log(1); } }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("m.is_some()"), "got:\n{rust}");
    }

    #[test]
    fn translates_string_compound_append() {
        let src = "function f(): void { let s = \"a\"; s += \"bc\"; console.log(s); }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains(".push_str(\"bc\")"), "got:\n{rust}");
    }

    #[test]
    fn translates_type_union_to_tagged_enum() {
        let src = "interface Circle { radius: number } interface Square { side: number } type Shape = Circle | Square;";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("enum Shape"), "got:\n{rust}");
        assert!(rust.contains("Circle(Circle)"), "got:\n{rust}");
        assert!(rust.contains("Square(Square)"), "got:\n{rust}");
    }

    #[test]
    fn translates_discriminated_union_to_field_variants() {
        let src = "type Shape = { kind: \"circle\"; radius: number } | { kind: \"square\"; side: number };";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("enum Shape"), "got:\n{rust}");
        assert!(rust.contains("Circle { radius: f64 }"), "got:\n{rust}");
        assert!(rust.contains("Square { side: f64 }"), "got:\n{rust}");
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
    fn translates_discriminated_union_variant_construction() {
        let src = "type Shape = { kind: \"circle\"; radius: number } | { kind: \"square\"; side: number }; function f(): void { const s: Shape = { kind: \"circle\", radius: 3 }; }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("Shape::Circle { radius: 3.0 }"), "got:\n{rust}");
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
    fn translates_array_map_to_iter_copied_map_collect() {
        let src = "function f(): void { const xs: number[] = [1, 2, 3]; const ys = xs.map(n => n * 2); }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(
            rust.contains("xs.iter().copied().map(|n| n * 2.0).collect::<Vec<_>>()"),
            "got:\n{rust}"
        );
    }

    #[test]
    fn translates_array_filter_to_iter_copied_filter_collect() {
        let src = "function f(): void { const xs: number[] = [1, 2, 3]; const ys = xs.filter(n => n > 1); }";
        let rust = Translator::new().translate(src).expect("should translate");
        // `.filter`'s closure receives &Item after `.copied()`, so the param is
        // destructured (`|&n|`) and the body reads the owned value.
        assert!(
            rust.contains("xs.iter().copied().filter(|&n| n > 1.0).collect::<Vec<_>>()"),
            "got:\n{rust}"
        );
    }

    #[test]
    fn translates_array_slice_to_index_range_to_vec() {
        let src = "function f(): void { const xs: number[] = [1, 2, 3, 4]; const ys = xs.slice(1, 3); const zs = xs.slice(2); }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("xs[1.0 as usize..3.0 as usize].to_vec()"), "got:\n{rust}");
        assert!(rust.contains("xs[2.0 as usize..].to_vec()"), "got:\n{rust}");
    }

    #[test]
    fn translates_return_object_literal_to_struct_init() {
        let src = "interface V { x: number; y: number; } function f(): V { return { x: 1, y: 2 }; }";
        let rust = Translator::new().translate(src).expect("should translate");
        // `return { … }` borrows the struct name from the return-type annotation.
        assert!(rust.contains("V { x: 1.0, y: 2.0 }"), "got:\n{rust}");
    }

    #[test]
    fn translates_string_split_to_vec_string() {
        let src = "function f(): void { const parts = \"a,b,c\".split(\",\"); }";
        let rust = Translator::new().translate(src).expect("should translate");
        // `split` yields &str; mapped to owned so the result is Vec<String>.
        assert!(rust.contains(".split(\",\")"), "got:\n{rust}");
        assert!(rust.contains(".collect::<Vec<String>>()"), "got:\n{rust}");
    }

    #[test]
    fn translates_object_destructuring_to_struct_pattern() {
        let src = "interface V { x: number; y: number; } function f(v: V): number { const { x, y } = v; return x + y; }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("let V { x, y } = v;"), "got:\n{rust}");
    }

    #[test]
    fn translates_object_literal_argument_to_struct_init() {
        let src = "interface V { x: number; y: number; } function g(v: V): number { return v.x; } function f(): number { return g({ x: 1, y: 2 }); }";
        let rust = Translator::new().translate(src).expect("should translate");
        // `f({ x, y })` borrows the struct name from the callee's parameter type.
        assert!(rust.contains("g(V { x: 1.0, y: 2.0 })"), "got:\n{rust}");
    }

    #[test]
    fn translates_array_index_of_to_position() {
        let src = "function f(): void { const xs: number[] = [1, 2, 3]; const i = xs.indexOf(2); }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains(".position(|y| y == 2.0)"), "got:\n{rust}");
        assert!(rust.contains(".unwrap_or(-1.0)"), "got:\n{rust}");
    }

    #[test]
    fn translates_string_index_of_to_find() {
        let src = "function f(): void { const s = \"hello\"; const i = s.indexOf(\"ll\"); }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains(".find(\"ll\").map(|b| b as f64).unwrap_or(-1.0)"), "got:\n{rust}");
    }

    #[test]
    fn translates_record_to_hashmap_literal() {
        let src = "function f(): void { const m: Record<string, number> = { a: 1, b: 2 }; }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains("HashMap<String, f64>"), "got:\n{rust}");
        assert!(rust.contains("HashMap::from"), "got:\n{rust}");
        assert!(rust.contains("\"a\".to_string()"), "got:\n{rust}");
    }

    #[test]
    fn translates_hashmap_index_to_get() {
        let src = "function f(): number { const m: Record<string, number> = { a: 1 }; return m[\"a\"]; }";
        let rust = Translator::new().translate(src).expect("should translate");
        assert!(rust.contains(".get(\"a\").copied().unwrap()"), "got:\n{rust}");
    }
}
