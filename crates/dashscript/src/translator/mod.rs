//! oxc AST → idiomatic Rust source, emitted through `syn` + `prettyplease`.
//!
//! Translation is one file per AST category — `declarations`, `functions`,
//! `types`, `expressions`, `bindings` — so each oxc node maps to a `syn` node
//! one-to-one. The `syn` tree is the project's hub: the translator builds it
//! (oxc → syn), `prettyplease` prints it, and the future `bindgen` parses
//! Rust crates into the same `syn` tree (syn → .ds) — one AST, two
//! directions. Parsing reuses `oxc_parser`; DashScript never parses itself.

mod analysis;
pub mod bindings;
mod builtins;
mod check;
mod class;
pub mod context;
pub mod declarations;
pub mod expressions;
pub mod functions;
pub mod imports;
pub mod name_table;
pub mod registry;
pub mod semantic;
pub mod types;

use oxc_allocator::Allocator;
use oxc_codegen::{Codegen, CodegenOptions, IndentChar};
use oxc_diagnostics::OxcDiagnostic;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::SourceType;

/// Runtime dependencies a translated file pulls in — extra crates or helper
/// modules the generated Cargo project must include. Collected during
/// translation so `ds build` only links what the source actually uses: a file
/// that never formats a number to an ES string pulls in no `ryu_js`.
#[derive(Default, Debug, Clone)]
pub struct RuntimeDeps {
    /// Some emit point routes an `f64` through `__ds::number_to_string`, so the
    /// generated crate needs the `ryu_js` crate and the `__ds` helper module.
    pub needs_ryu_js: bool,
}

impl RuntimeDeps {
    /// Union another dep set into this one — a project links a runtime dep if
    /// any of its translated files does.
    pub fn merge(&mut self, other: &RuntimeDeps) {
        self.needs_ryu_js |= other.needs_ryu_js;
    }

    /// The `__ds` helper module source — `number_to_string(f64) -> String` via
    /// `ryu_js` (ES `Number::toString`) — when this dep set flags `ryu_js`.
    /// `None` when no runtime dep is needed, so the caller writes nothing.
    pub fn helper_module(&self) -> Option<&'static str> {
        self.needs_ryu_js.then_some(DS_HELPER_MODULE)
    }

    /// Append `ryu-js = "1.0"` to a generated `Cargo.toml`, creating the
    /// `[dependencies]` section if absent. A no-op when no runtime dep is needed
    /// or `ryu-js` is already declared (e.g. the project declared `rust:ryu_js`)
    /// — so a consumer can call this unconditionally and let the dep set gate it.
    /// A string-level post-process keeps the dep out of the user's
    /// `manifest.json` — it is a DashScript-internal runtime need, not a
    /// declared project dependency.
    pub fn apply_to_cargo_toml(&self, cargo_toml: &mut String) {
        if !self.needs_ryu_js || cargo_toml.contains("ryu-js") {
            return;
        }
        // The crates.io package is `ryu-js` (hyphen); Rust exposes it as
        // `ryu_js` (underscore) in `use`, so the Cargo.toml key uses the
        // package name.
        const RYU_JS_LINE: &str = "ryu-js = \"1.0\"";
        if let Some(pos) = cargo_toml.find("[dependencies]\n") {
            cargo_toml.insert_str(pos + "[dependencies]\n".len(), &format!("{RYU_JS_LINE}\n"));
        } else {
            cargo_toml.push_str(&format!("\n[dependencies]\n{RYU_JS_LINE}\n"));
        }
    }
}

/// The DashScript runtime helper module, written to `src/__ds.rs` and declared
/// `mod __ds;` at each crate root when a translated file references it. The
/// single source for the `__ds` helpers — consumed by both `ds build` (bin) and
/// the conformance harness (lib test) — so the helper text lives in the library
/// rather than either consumer. Today only `number_to_string` (ES
/// `Number::toString` via `ryu_js`); signed zero is normalized to `"0"` first
/// (`ryu_js` covers NaN and ±Infinity already).
const DS_HELPER_MODULE: &str = "\
//! DashScript runtime helper: ES-correct `Number::toString` via `ryu_js`.
use ryu_js::Buffer;

/// Format an `f64` as ECMAScript `Number::toString` would. `ryu_js` covers NaN
/// and ±Infinity; signed zero is normalized to `\"0\"` (ES prints both `+0`
/// and `-0` that way).
pub fn number_to_string(x: f64) -> String {
    if x == 0.0 {
        return \"0\".to_string();
    }
    Buffer::new().format(x).to_string()
}
";

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
    /// Convenience wrapper around [`Self::translate_with_deps`] that drops the
    /// runtime-dependency report — for callers (tests, LSP) that only want the
    /// Rust text. `ds build` uses [`Self::translate_with_deps`] so the project
    /// links only what the source uses.
    ///
    /// # Errors
    /// Returns an error string if oxc reports parse diagnostics.
    pub fn translate(&self, source: &str) -> Result<String, String> {
        Ok(self.translate_with_deps(source)?.0)
    }

    /// Parse `.ds` source, translate the AST to Rust source, and report the
    /// runtime dependencies the generated code needs.
    ///
    /// The Rust text matches [`Self::translate`]; the second return value is the
    /// set of extra crates / helper modules the translated code references, so
    /// the project emitter can add them to `Cargo.toml` and write the helper
    /// module only when needed.
    ///
    /// # Errors
    /// Returns an error string if oxc reports parse diagnostics.
    pub fn translate_with_deps(&self, source: &str) -> Result<(String, RuntimeDeps), String> {
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, source, SourceType::ts()).parse();

        if !ret.diagnostics.is_empty() {
            return Err(format!(
                "dashscript: oxc reported {} parse diagnostic(s)",
                ret.diagnostics.len()
            ));
        }

        // Move the program into the arena so the arena, the program, and the
        // semantic analysis all share one lifetime `'a` (the same trick
        // `semantic::analyze_symbols` uses). `with_build_nodes(true)` fills the
        // `symbol_id` / `reference_id` cells on each `BindingIdentifier` /
        // `IdentifierReference` so the translator can resolve any identifier to
        // its `SymbolId` — the identity `NameTable` keys on, replacing the lossy
        // `snake(name)` string fold.
        let program = allocator.alloc(ret.program);
        let sret = SemanticBuilder::new().with_build_nodes(true).build(program);
        let names = name_table::build(sret.semantic.scoping());

        // First pass: collect discriminated-union enum shapes so later
        // expression translation can build variant constructors.
        let registry = registry::build_registry(&program.body);
        let items = program
            .body
            .iter()
            .flat_map(|s| functions::translate_statement(s, &registry, &names))
            .collect();
        let file = syn::File {
            shebang: None,
            attrs: Vec::new(),
            items,
        };
        // An emit point that routes an `f64` through the ES NumberToString
        // helper writes a `crate::__ds::number_to_string` call into the Rust
        // text; its presence means the generated crate needs the `ryu_js` crate
        // and the `__ds` helper module. Scanning the emitted text (rather than
        // threading a `RefCell<RuntimeDeps>` through every expression) keeps the
        // dep report a pure function of the output — the `__ds::` prefix is a
        // DashScript-reserved namespace a `.ds` source cannot produce any other
        // way.
        let rust = prettyplease::unparse(&file);
        let deps = RuntimeDeps {
            needs_ryu_js: rust.contains("__ds::number_to_string"),
        };
        Ok((rust, deps))
    }

    /// Check `.ds` source for translatability without emitting Rust.
    ///
    /// Returns syntax errors from `oxc_parser` plus one diagnostic per
    /// top-level statement the translator cannot map. An empty `Vec` means the
    /// file is translatable to valid Rust (as far as DashScript can tell).
    #[must_use]
    pub fn check(&self, source: &str) -> Vec<OxcDiagnostic> {
        check::check(source)
    }

    /// Format `.ds` source with `oxc_codegen` (pretty-print, 2-space indent,
    /// not minified) — the same indentation style as prettier / `vp fmt`, so
    /// `.ds` written by hand (TypeScript-style) is already formatted.
    ///
    /// # Errors
    /// Returns an error string if `oxc_parser` reports syntax diagnostics — a
    /// file with syntax errors cannot be formatted.
    pub fn format(&self, source: &str) -> Result<String, String> {
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, source, SourceType::ts()).parse();
        if !ret.diagnostics.is_empty() {
            return Err(format!(
                "dashscript: oxc reported {} parse diagnostic(s) — fix syntax before formatting",
                ret.diagnostics.len()
            ));
        }
        Ok(Codegen::new()
            .with_options(CodegenOptions {
                indent_char: IndentChar::Space,
                indent_width: 2,
                ..CodegenOptions::default()
            })
            .build(&ret.program)
            .code)
    }

    /// The local `.ds` modules this file imports (`import { x } from "./other"`
    /// → `other`), for `ds build` to assemble one Rust module per dependency.
    #[must_use]
    pub fn imports(&self, source: &str) -> Vec<imports::ImportRef> {
        imports::collect_imports(source)
    }

    /// The bare-crate imports in a `.ds` file (`import { X } from "crate"`),
    /// each with its `.ds` byte span. Used by `ds lsp` to resolve
    /// go-to-definition on an import specifier to the crate's `~/.cargo` source.
    #[must_use]
    pub fn crate_imports(&self, source: &str) -> Vec<imports::CrateImport> {
        imports::collect_crate_imports(source)
    }

    /// The locally declarable names in a `.ds` file (`function`, `interface`,
    /// `type`, `export`, `import`), each with its binding byte span. Used by
    /// `ds lsp` for in-file go-to-definition (everything but crate imports).
    #[must_use]
    pub fn declarations(&self, source: &str) -> Vec<imports::LocalSymbol> {
        imports::collect_declarations(source)
    }

    /// Whether the `.ds` source declares a top-level `function main()` — the
    /// entry point a `[[bin]]` target needs. AST-level (not a substring scan),
    /// so a `main_loop` helper or a `"fn main"` string literal cannot trip it.
    #[must_use]
    pub fn has_main(&self, source: &str) -> bool {
        imports::has_main(source)
    }

    /// Symbol-level analysis for one `.ds` file: every declaration's span,
    /// kind, and resolved references (read/write). Powers LSP find-references /
    /// rename with **symbol-level precision** — two same-named bindings in
    /// different scopes are distinct symbols, so renaming one never touches the
    /// other. Returns an owned snapshot that borrows nothing (the parse arena is
    /// released). An empty table means the file failed to parse.
    #[must_use]
    pub fn symbols(&self, source: &str) -> semantic::SymbolTable {
        semantic::analyze_symbols(source)
    }
}

#[cfg(test)]
mod tests;
