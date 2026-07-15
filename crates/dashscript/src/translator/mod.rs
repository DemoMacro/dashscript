//! oxc AST → idiomatic Rust source, emitted through `syn` + `prettyplease`.
//!
//! Translation is one file per AST category — `declarations`, `functions`,
//! `types`, `expressions`, `bindings` — so each oxc node maps to a `syn` node
//! one-to-one. The `syn` tree is the project's hub: the translator builds it
//! (oxc → syn), `prettyplease` prints it, and the future `bindgen` parses
//! Rust crates into the same `syn` tree (syn → .ds) — one AST, two
//! directions. Parsing reuses `oxc_parser`; DashScript never parses itself.

pub mod bindings;
pub mod declarations;
pub mod expressions;
pub mod functions;
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

        let items = ret
            .program
            .body
            .iter()
            .filter_map(functions::translate_statement)
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
}
