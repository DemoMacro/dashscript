//! DashScript's standard-library declarations — the `lib.d.ts` analogue.
//!
//! Each built-in's declaration lives beside its implementation, as a `.ds` file
//! in `translator/builtins/` (`console.ds` ↔ `console.rs`, `math.ds` ↔
//! `math.rs`, …). One `.rs` carries the Rust mapping; the matching `.ds`
//! carries the names, signatures, and docs the language server surfaces — so a
//! gap between "declared" and "implemented" is a gap you can see in one place.
//!
//! [`SOURCES`] is the list of those files (via `include_str!`); [`parse`] reads
//! each one through oxc into the [`Builtin`] table (the single source of truth
//! for completion/hover/signature-help in [`super::builtins`]). A declaration's
//! trailing `// doc` is read back as that symbol's hover text — authored next to
//! the signature it describes. The drift-guard test asserts every declared
//! symbol actually translates, so the table cannot claim a name the translator
//! cannot lower to Rust.
//!
//! `interface`s declare namespace members (`console`, `Math`, …: methods and
//! constants); `declare function`s declare globals (`parseInt`, `Boolean`);
//! `declare var`s declare global constants (`undefined`, `Infinity`, `NaN`).
//! These are ambient declarations, exactly as TypeScript's `lib.es5.d.ts` does
//! (`declare var NaN: number`), so they carry a type but no initializer.
//!
//! Unlike a crate added with `ds add rust:<crate>` (whose `.ds` declaration
//! bindgen can derive from the crate's `~/.cargo` source), these built-ins have
//! no `.rs` file a user compiles — the translator generates their Rust inline.
//! So this declaration is hand-written, like `lib.d.ts`.

use oxc_allocator::Allocator;
use oxc_ast::ast::{BindingPattern, PropertyKey, Statement, TSSignature, VariableDeclarator};
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType};

use super::builtins::{Builtin, BuiltinKind};

/// The standard-library declaration files — each beside its `.rs` counterpart
/// in `translator/builtins/`. Parsed individually so each AST span is relative
/// to its own file (and the trailing-`// doc` scan stays local to that text).
const SOURCES: &[&str] = &[
    include_str!("../../src/translator/builtins/console.ds"),
    include_str!("../../src/translator/builtins/math.ds"),
    include_str!("../../src/translator/builtins/number.ds"),
    include_str!("../../src/translator/builtins/string.ds"),
    include_str!("../../src/translator/builtins/array.ds"),
    include_str!("../../src/translator/builtins/object.ds"),
    include_str!("../../src/translator/builtins/global.ds"),
];

/// Parse every standard-library declaration into the builtin table. Called once
/// (lazily, via the `PARSED` static in [`super::builtins`]); the result is the
/// single source of truth for completion/hover/signature-help.
pub(super) fn parse() -> Vec<Builtin> {
    let mut out = Vec::new();
    for source in SOURCES {
        parse_file(source, &mut out);
    }
    out
}

/// Parse one declaration file, appending its builtins. Kept per-file so the
/// AST spans (and the trailing-doc scan) are relative to that file's text.
fn parse_file(source: &str, out: &mut Vec<Builtin>) {
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, SourceType::ts()).parse();
    for stmt in &ret.program.body {
        collect(stmt, source, out);
    }
}

/// One top-level declaration → its builtins.
fn collect(stmt: &Statement, source: &str, out: &mut Vec<Builtin>) {
    match stmt {
        // `interface <ns> { …members… }` → one builtin per member.
        Statement::TSInterfaceDeclaration(i) => {
            let ns = i.id.name.to_string();
            for sig in &i.body.body {
                if let Some(b) = member_builtin(&ns, sig, source) {
                    out.push(b);
                }
            }
        }
        // `function <name>(…): T {}` (incl. `declare function …;`) → a global.
        Statement::FunctionDeclaration(f) => {
            if let Some(id) = &f.id {
                out.push(Builtin {
                    ns: None,
                    name: id.name.to_string(),
                    kind: BuiltinKind::Function,
                    sig: function_sig(f, source),
                    doc: trailing_doc(source, f.span().end),
                });
            }
        }
        // `const <name> = …` / `declare var <name>: T;` → a global constant.
        Statement::VariableDeclaration(v) => {
            for d in &v.declarations {
                if let Some(b) = const_builtin(d, source) {
                    out.push(b);
                }
            }
        }
        _ => {}
    }
}

/// One interface member → a namespace builtin. A method (`abs(x: number):
/// number`) is a `Function`; a property (`PI: number`) is a `Const` whose
/// signature is its type. The method signature is sliced from the first `(`
/// onward (the AST span includes the member name + trailing `;`).
fn member_builtin(ns: &str, sig: &TSSignature, source: &str) -> Option<Builtin> {
    match sig {
        TSSignature::TSMethodSignature(m) => {
            let full = slice(source, sig.span());
            let sig_text = full
                .find('(')
                .map(|i| full[i..].trim_end_matches(';').to_string())
                .unwrap_or(full);
            Some(Builtin {
                ns: Some(ns.to_string()),
                name: key_name(&m.key)?,
                kind: BuiltinKind::Function,
                sig: sig_text,
                doc: trailing_doc(source, sig.span().end),
            })
        }
        TSSignature::TSPropertySignature(p) => {
            let ty = p
                .type_annotation
                .as_ref()
                .map(|t| slice(source, t.type_annotation.span()))
                .unwrap_or_else(|| "any".to_string());
            Some(Builtin {
                ns: Some(ns.to_string()),
                name: key_name(&p.key)?,
                kind: BuiltinKind::Const,
                sig: ty,
                doc: trailing_doc(source, sig.span().end),
            })
        }
        _ => None,
    }
}

/// `function <name>(params): T` → its signature `(params): T`, reconstructed by
/// slicing each parameter's and the return type's span. Works for `declare
/// function` too (its body is empty; only the params/return are read).
fn function_sig(f: &oxc_ast::ast::Function, source: &str) -> String {
    let params: Vec<String> = f
        .params
        .items
        .iter()
        .map(|p| {
            let name = match &p.pattern {
                BindingPattern::BindingIdentifier(id) => id.name.to_string(),
                _ => "_".to_string(),
            };
            let ty = p
                .type_annotation
                .as_ref()
                .map(|t| slice(source, t.type_annotation.span()))
                .unwrap_or_else(|| "any".to_string());
            if p.optional {
                format!("{name}?: {ty}")
            } else {
                format!("{name}: {ty}")
            }
        })
        .collect();
    let ret = f
        .return_type
        .as_ref()
        .map(|t| slice(source, t.type_annotation.span()))
        .unwrap_or_else(|| "void".to_string());
    format!("({}): {ret}", params.join(", "))
}

/// `const <name>` / `declare var <name>: T` → a `Const` builtin. The type comes
/// from the declarator's `type_annotation` (`declare var NaN: number` →
/// `number`); `any` when no annotation is present.
fn const_builtin(d: &VariableDeclarator, source: &str) -> Option<Builtin> {
    let BindingPattern::BindingIdentifier(id) = &d.id else {
        return None;
    };
    let ty = d
        .type_annotation
        .as_ref()
        .map(|t| slice(source, t.type_annotation.span()))
        .unwrap_or_else(|| "any".to_string());
    Some(Builtin {
        ns: None,
        name: id.name.to_string(),
        kind: BuiltinKind::Const,
        sig: ty,
        doc: trailing_doc(source, d.span().end),
    })
}

/// The original `.ds` spelling of a property key — `round`, `sumPrecise`,
/// `PI` (NOT snake-cased; `bindings::property_key_name` snake-cases for Rust,
/// but this table mirrors what the developer types).
fn key_name(key: &PropertyKey) -> Option<String> {
    match key {
        PropertyKey::StaticIdentifier(id) => Some(id.name.to_string()),
        PropertyKey::StringLiteral(s) => Some(s.value.to_string()),
        _ => None,
    }
}

/// `source[span]` as an owned string — the verbatim `.ds` text of a node.
fn slice(source: &str, span: oxc_span::Span) -> String {
    source[span.start as usize..span.end as usize].to_string()
}

/// The `// doc` text trailing a declaration on the same line — the doc is
/// authored inline in the declaration file, next to the signature it describes.
/// Empty when no trailing comment is present.
fn trailing_doc(source: &str, span_end: u32) -> String {
    let bytes = source.as_bytes();
    let start = span_end as usize;
    let line_end = bytes[start..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(bytes.len(), |p| start + p);
    let rest = &source[start.min(source.len())..line_end];
    rest.find("//")
        .map(|i| rest[i + 2..].trim().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_console_members_with_doc() {
        let table = parse();
        let log = table
            .iter()
            .find(|b| b.qualified() == "console.log")
            .expect("console.log");
        assert_eq!(log.kind, BuiltinKind::Function);
        assert!(log.sig.contains("data"), "sig: {}", log.sig);
        assert!(log.sig.contains("void"), "sig: {}", log.sig);
        assert!(log.doc.contains("stdout"), "doc: {}", log.doc);
    }

    #[test]
    fn parses_math_method_and_constant() {
        let table = parse();
        let round = table
            .iter()
            .find(|b| b.qualified() == "Math.round")
            .expect("Math.round");
        assert_eq!(round.kind, BuiltinKind::Function);
        // The signature is the params + return, WITHOUT the member name or `;`.
        assert_eq!(round.sig, "(x: number): number");
        assert!(round.doc.contains("nearest"), "doc: {}", round.doc);
        let pi = table
            .iter()
            .find(|b| b.qualified() == "Math.PI")
            .expect("Math.PI");
        assert_eq!(pi.kind, BuiltinKind::Const);
        assert_eq!(pi.sig, "number");
        assert!(pi.doc.contains("π"), "doc: {}", pi.doc);
    }

    #[test]
    fn keeps_original_member_name_not_snake_cased() {
        let table = parse();
        // `sumPrecise` stays as written, not `sum_precise`.
        assert!(table.iter().any(|b| b.qualified() == "Math.sumPrecise"));
    }

    #[test]
    fn parses_global_function_and_constants() {
        let table = parse();
        let parse_int = table
            .iter()
            .find(|b| b.ns.is_none() && b.name == "parseInt")
            .expect("global parseInt");
        assert_eq!(parse_int.kind, BuiltinKind::Function);
        assert!(parse_int.sig.contains("radix?"), "sig: {}", parse_int.sig);
        assert!(parse_int.doc.contains("integer"), "doc: {}", parse_int.doc);
        // `declare var` carries its type from the annotation, not `any`.
        let nan = table
            .iter()
            .find(|b| b.ns.is_none() && b.name == "NaN")
            .expect("global NaN");
        assert_eq!(nan.kind, BuiltinKind::Const);
        assert_eq!(nan.sig, "number", "declare var type should be `number`");
        let inf = table
            .iter()
            .find(|b| b.ns.is_none() && b.name == "Infinity")
            .expect("global Infinity");
        assert_eq!(inf.kind, BuiltinKind::Const);
        assert!(inf.doc.contains("infinity"), "doc: {}", inf.doc);
    }

    #[test]
    fn number_namespace_and_global_function_coexist() {
        // `interface Number { … }` and `declare function Number(x)` both exist.
        let table = parse();
        assert!(table.iter().any(|b| b.qualified() == "Number.parseInt"));
        assert!(table.iter().any(|b| b.ns.is_none() && b.name == "Number"));
    }

    #[test]
    fn every_entry_has_a_unique_qualified_name() {
        let mut names: Vec<String> = parse().iter().map(Builtin::qualified).collect();
        names.sort();
        let before = names.len();
        names.dedup();
        assert_eq!(names.len(), before, "duplicate qualified name");
    }

    /// The drift guard: every symbol the standard library *declares* must be
    /// one the translator *maps*. For each entry we translate a one-line call
    /// and assert the output has no "fall-through" marker — the shape an
    /// unmapped name lowers to (`Math.foo` → `math.foo`, a bare `foo(`).
    /// Global constants (`undefined`/`Infinity`/`NaN`) are skipped: they are
    /// literals handled outside the builtin mapping, with no call form.
    #[test]
    fn drift_guard_every_entry_translates() {
        use dashscript::Translator;

        for b in parse() {
            if b.ns.is_none() && b.kind == BuiltinKind::Const {
                continue;
            }
            let src = probe_source(&b);
            let rust = match Translator::new().translate(&src) {
                Ok(r) => r,
                Err(e) => panic!("{} failed to translate: {e}", b.qualified()),
            };
            let marker = marker_for(&b);
            assert!(
                !has_fall_through(&rust, &marker),
                "{} is declared in stdlib but not mapped by the translator — \
                 output contains the fall-through marker `{marker}`:\n{rust}",
                b.qualified()
            );
        }
    }

    /// A minimal `.ds` program that exercises `b` once, inside `main` (a
    /// top-level expression is rejected). Arguments are chosen so the mapping
    /// fires: Math/Object take two (covers `pow`/`atan2`/`imul`/`is`/`assign`),
    /// Array.isArray needs an identifier receiver, the rest take a literal.
    fn probe_source(b: &Builtin) -> String {
        match (b.ns.as_deref(), b.kind, b.name.as_str()) {
            (Some(_), _, "isArray") => {
                "function main(): void { const x = 1; Array.isArray(x); }".to_string()
            }
            (Some(ns), BuiltinKind::Const, _) => {
                format!("function main(): void {{ {ns}.{}; }}", b.name)
            }
            (Some("Math"), _, _) => format!("function main(): void {{ Math.{}(1, 2); }}", b.name),
            (Some("console"), _, _) => {
                format!("function main(): void {{ console.{}(1); }}", b.name)
            }
            (Some("Number"), _, _) => {
                format!("function main(): void {{ Number.{}(1); }}", b.name)
            }
            (Some("String"), _, _) => {
                format!("function main(): void {{ String.{}(65); }}", b.name)
            }
            (Some("Array"), _, _) => {
                format!("function main(): void {{ Array.{}([1]); }}", b.name)
            }
            (Some("Object"), _, _) => {
                format!("function main(): void {{ Object.{}(1, 2); }}", b.name)
            }
            (Some(ns), _, _) => format!("function main(): void {{ {ns}.{}(1); }}", b.name),
            (None, _, _) => format!("function main(): void {{ {}(1); }}", b.name),
        }
    }

    /// The fall-through marker for `b` — the substring an unmapped name leaves
    /// in the translated Rust. A namespace member `Math.foo` lowers to
    /// `math.foo`; a global `foo(` lowers to `foo(` (snake-cased callee).
    fn marker_for(b: &Builtin) -> String {
        use dashscript::translator::bindings;
        match (b.ns.as_deref(), b.kind) {
            (Some(ns), _) => format!("{}.{}", bindings::snake(ns), bindings::snake(&b.name)),
            (None, BuiltinKind::Function) => format!("{}(", bindings::snake(&b.name)),
            (None, BuiltinKind::Const) => String::new(),
        }
    }

    /// True when `marker` appears as a standalone callee in `rust` — preceded
    /// by neither `.` nor an identifier byte. A `.`-preceded hit (`.is_nan(`)
    /// is a method call that happens to share the name, not a fall-through.
    fn has_fall_through(rust: &str, marker: &str) -> bool {
        if marker.is_empty() {
            return false;
        }
        let bytes = rust.as_bytes();
        let mut from = 0;
        while let Some(idx) = rust[from..].find(marker) {
            let pos = from + idx;
            let standalone = pos == 0 || {
                let prev = bytes[pos - 1];
                prev != b'.' && !prev.is_ascii_alphanumeric() && prev != b'_'
            };
            if standalone {
                return true;
            }
            from = pos + marker.len();
        }
        false
    }
}
