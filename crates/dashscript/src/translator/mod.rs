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
}
