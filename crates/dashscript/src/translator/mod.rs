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
mod globals;
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
    /// A `JSON.parse`/`JSON.stringify` call inlines a `serde_json::` call into
    /// the Rust text, so the generated crate needs the `serde_json` crate (no
    /// helper module — the calls are direct).
    pub needs_serde_json: bool,
    /// The source uses ES dynamic reflection (`Object.defineProperty`/
    /// `getOwnPropertyDescriptor`/`create`/`getPrototypeOf`, accessor
    /// properties, …) the static translator cannot lower to idiomatic Rust.
    /// The generated crate then runs the whole program under an embedded
    /// QuickJS engine via the `__ds_engine` helper module (the `rquickjs`
    /// crate) — a gated compat fallback, the same pattern as `ryu_js`. Default
    /// `ds build` output stays pure Rust; only programs that actually use such
    /// a construct pull the engine (and its C build dependency).
    pub needs_engine: bool,
}

impl RuntimeDeps {
    /// Union another dep set into this one — a project links a runtime dep if
    /// any of its translated files does.
    pub fn merge(&mut self, other: &RuntimeDeps) {
        self.needs_ryu_js |= other.needs_ryu_js;
        self.needs_serde_json |= other.needs_serde_json;
        self.needs_engine |= other.needs_engine;
    }

    /// The `__ds` helper module source — `number_to_string(f64) -> String` via
    /// `ryu_js` (ES `Number::toString`) — when this dep set flags `ryu_js`.
    /// `None` when no runtime dep is needed, so the caller writes nothing.
    pub fn helper_module(&self) -> Option<&'static str> {
        self.needs_ryu_js.then_some(DS_HELPER_MODULE)
    }

    /// The `__ds_engine` compat module source — runs a `.ds` source under an
    /// embedded QuickJS engine — when this dep set flags `needs_engine`. `None`
    /// otherwise, so the caller writes nothing and the default build pulls no
    /// engine dependency.
    pub fn engine_helper_module(&self) -> Option<&'static str> {
        self.needs_engine.then_some(ENGINE_HELPER_MODULE)
    }

    /// Append `ryu-js = "1.0"` to a generated `Cargo.toml`, creating the
    /// `[dependencies]` section if absent. A no-op when no runtime dep is needed
    /// or `ryu-js` is already declared (e.g. the project declared `rust:ryu_js`)
    /// — so a consumer can call this unconditionally and let the dep set gate it.
    /// A string-level post-process keeps the dep out of the user's
    /// `manifest.json` — it is a DashScript-internal runtime need, not a
    /// declared project dependency.
    pub fn apply_to_cargo_toml(&self, cargo_toml: &mut String) {
        // The crates.io package is `ryu-js` (hyphen); Rust exposes it as
        // `ryu_js` (underscore) in `use`, so the Cargo.toml key uses the
        // package name.
        append_dep(cargo_toml, "ryu-js", "\"1.0\"", self.needs_ryu_js);
        append_dep(cargo_toml, "serde_json", "\"1\"", self.needs_serde_json);
        // `rquickjs` bundles QuickJS-NG C sources (compiled via `cc`), so this
        // is only emitted for programs that opt into the engine compat path.
        append_dep(cargo_toml, "rquickjs", "\"0.12\"", self.needs_engine);
    }
}

/// Append `<pkg> = <req>` to a generated `Cargo.toml`'s `[dependencies]`,
/// creating the section if absent. A no-op when `needed` is false or the dep is
/// already declared — so a consumer can call this per dep and let the flag gate
/// it. A string-level post-process keeps these deps out of the user's
/// `manifest.json` — they are DashScript-internal runtime needs.
fn append_dep(cargo_toml: &mut String, pkg: &str, req: &str, needed: bool) {
    let needle = format!("{pkg} =");
    if !needed || cargo_toml.contains(&needle) {
        return;
    }
    let line = format!("{pkg} = {req}\n");
    if let Some(pos) = cargo_toml.find("[dependencies]\n") {
        cargo_toml.insert_str(pos + "[dependencies]\n".len(), &line);
    } else {
        cargo_toml.push_str(&format!("\n[dependencies]\n{line}"));
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

/// The DashScript compat engine module, written to `src/__ds_engine.rs` and
/// declared `mod __ds_engine;` at the crate root when a translated file uses ES
/// dynamic reflection the static translator cannot lower. It runs the whole
/// `.ds` source under an embedded QuickJS engine (`rquickjs`), with a
/// `console.log` wired to stdout. Number stringification uses the engine's own
/// `String()` (ES `Number::toString`), so output matches Node for primitives.
///
/// Gated: only emitted for `needs_engine` programs, so a plain `ds build` pulls
/// no engine dependency (and no QuickJS C compile). The single source for the
/// engine helper — consumed by both `ds build` (project.rs) and the conformance
/// harness — so the helper text lives in the library rather than either
/// consumer.
///
/// Note: the engine evaluates the source as plain ECMAScript, so a `.ds` source
/// with TypeScript type annotations is not yet handled on this path (today it
/// serves the conformance oracle, whose test262 fixtures are annotation-free
/// JS). Stripping annotations for real `.ds` sources is a follow-up.
const ENGINE_HELPER_MODULE: &str = r##"//! DashScript compat engine: run a `.ds` source under an embedded QuickJS
//! engine (`rquickjs`) when it uses ES dynamic reflection
//! (`Object.defineProperty`, `Reflect.*`, `Symbol`, `Proxy`, …) the static
//! translator cannot lower to idiomatic Rust. Gated — only present when
//! `RuntimeDeps::needs_engine`.
use rquickjs::{Context, Ctx, Runtime};

/// Run a `.ds` source under QuickJS with `console.log` wired to stdout, then
/// call `main()` (the fixture entry — the same `main();` the conformance
/// harness appends for the Node oracle). `console.log` joins its arguments with
/// spaces, stringified by the engine's own `String()` coercion — ES
/// `Number::toString` for numbers — so the output matches Node for primitives.
pub fn run(source: &str) {
    use rquickjs::context::EvalOptions;
    let runtime = Runtime::new().expect("rquickjs Runtime");
    let ctx = Context::full(&runtime).expect("rquickjs Context");
    // Sloppy-mode eval (strict=false): test262 fixtures use `this` at the top
    // of `main` for property-attribute setup (`this.configurable = true`), the
    // sloppy-mode `this`=global. Node runs the oracle the same way (a plain
    // script, not a strict module); strict eval would make `this`=undefined
    // and throw before the first console.log.
    let sloppy = || {
        let mut o = EvalOptions::default();
        o.strict = false;
        o
    };
    let result = ctx.with(|ctx: Ctx<'_>| -> rquickjs::Result<()> {
        // A native line-print primitive; `console.log` (defined in JS below)
        // joins its arguments with spaces and hands each finished line here.
        let print_line = rquickjs::Function::new(ctx.clone(), |s: String| {
            println!("{s}");
        })?;
        ctx.globals().set("__ds_print_line", print_line)?;
        // Define `console.log` in JS so argument stringification uses the
        // engine's own `String()` coercion (ES NumberToString for numbers),
        // matching Node's `console.log` output for primitives. A plain number
        // arg prints `1e+21` (not Rust's `f64` Display spelling).
        ctx.eval_with_options::<(), _>(
            r#"this.console = { log: function () {
                for (var i = 0, out = []; i < arguments.length; i++) {
                    out.push(String(arguments[i]));
                }
                __ds_print_line(out.join(" "));
            } };"#,
            sloppy(),
        )?;
        // Eval the source (defines `main`), then invoke it.
        ctx.eval_with_options::<(), _>(source, sloppy())?;
        ctx.eval_with_options::<(), _>("main();", sloppy())?;
        Ok(())
    });
    result.expect("rquickjs eval");
}
"##;

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

        // Engine-gated compat path: a source using ES dynamic reflection
        // (`Object.defineProperty`, `Reflect.*`, `Symbol`, `Proxy`,
        // `instanceof`, …) the static translator cannot lower is run whole
        // under an embedded QuickJS engine instead of being lowered to Rust.
        // The same `collect_unsupported` walk that flags these as
        // `unsupported` in `ds check` here flips the file to the engine path —
        // a single source of truth for what the engine covers, so the lint and
        // the lowering cannot drift. Default `ds build` output stays pure Rust;
        // only a program that actually uses such a construct pulls the
        // `rquickjs` engine dep (and its C compile).
        if check::program_uses_engine(program) {
            // The engine evaluates ECMAScript, so strip the TS type annotations
            // a `.ds` source carries — QuickJS parses JS, not TS. test262
            // fixtures annotate only the wrapped `main` (their bodies are the
            // test262 file's original JS); a real `.ds` source with richer
            // annotations (`as`, generics, ...) needs fuller stripping — a
            // follow-up. Regenerate via oxc codegen so the embedded source is
            // annotation-free JS, then `syn::LitStr` escapes it for embedding.
            for stmt in &mut program.body {
                if let oxc_ast::ast::Statement::FunctionDeclaration(f) = stmt {
                    f.return_type = None;
                }
            }
            let js_source = Codegen::new().build(&*program).code;
            let src_lit = syn::LitStr::new(&js_source, proc_macro2::Span::call_site());
            let main_item: syn::Item = syn::parse_quote! {
                fn main() {
                    crate::__ds_engine::run(#src_lit);
                }
            };
            let rust = prettyplease::unparse(&syn::File {
                shebang: None,
                attrs: Vec::new(),
                items: vec![main_item],
            });
            return Ok((
                rust,
                RuntimeDeps {
                    needs_ryu_js: false,
                    needs_serde_json: false,
                    needs_engine: true,
                },
            ));
        }

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
        // text; a `JSON.parse`/`JSON.stringify` call inlines `serde_json::`.
        // Either prefix means the generated crate needs the matching crate (and
        // the `__ds` helper module, for ryu_js). Scanning the emitted text
        // (rather than threading a `RefCell<RuntimeDeps>` through every
        // expression) keeps the dep report a pure function of the output — the
        // `__ds::` prefix is a DashScript-reserved namespace a `.ds` source
        // cannot produce any other way, and `serde_json::` likewise only
        // appears via the `JSON` builtin.
        let rust = prettyplease::unparse(&file);
        let deps = RuntimeDeps {
            needs_ryu_js: rust.contains("__ds::number_to_string"),
            needs_serde_json: rust.contains("serde_json::"),
            needs_engine: false,
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
