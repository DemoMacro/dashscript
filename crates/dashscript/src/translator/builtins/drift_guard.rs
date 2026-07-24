//! Drift guard: every symbol `dashscript.d.ts` declares must be one the
//! translator maps. The declaration file is the editor's ambient types
//! (injected by `@dashscript/typescript-plugin`); this asserts it cannot
//! claim a name the translator cannot lower to Rust.
//!
//! Migrated from `bin/lsp/stdlib.rs` when the LSP was slimmed to its shared
//! core (crate-jump and translatability). Only the drift guard moved; the
//! full builtin table (signatures and docs) was LSP-only — it served
//! completion, hover, and signature-help, which now live in the editor's
//! TS LSP.

#![cfg(test)]

use oxc_allocator::Allocator;
use oxc_ast::ast::{BindingPattern, PropertyKey, Statement, TSSignature};
use oxc_parser::Parser;
use oxc_span::SourceType;

/// The standard-library declaration file (the editor's `lib.d.ts` analogue).
const SOURCE: &str = include_str!("dashscript.d.ts");

#[derive(Clone)]
struct Builtin {
    ns: Option<String>,
    name: String,
    kind: BuiltinKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BuiltinKind {
    Function,
    Const,
}

/// Parse the declaration's symbol names — just (namespace, name, kind), the
/// shape the drift guard probes. Signatures/docs were LSP-only and stay gone.
fn parse() -> Vec<Builtin> {
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, SOURCE, SourceType::ts()).parse();
    let mut out = Vec::new();
    for stmt in &ret.program.body {
        match stmt {
            Statement::TSInterfaceDeclaration(i) => {
                let ns = i.id.name.to_string();
                for sig in &i.body.body {
                    if let Some(b) = member_builtin(&ns, sig) {
                        out.push(b);
                    }
                }
            }
            Statement::FunctionDeclaration(f) => {
                if let Some(id) = &f.id {
                    out.push(Builtin {
                        ns: None,
                        name: id.name.to_string(),
                        kind: BuiltinKind::Function,
                    });
                }
            }
            Statement::VariableDeclaration(v) => {
                for d in &v.declarations {
                    let BindingPattern::BindingIdentifier(id) = &d.id else {
                        continue;
                    };
                    out.push(Builtin {
                        ns: None,
                        name: id.name.to_string(),
                        kind: BuiltinKind::Const,
                    });
                }
            }
            _ => {}
        }
    }
    out
}

/// One interface member → a namespace builtin. A method is a `Function`; a
/// property is a `Const`. Other signature kinds are skipped.
fn member_builtin(ns: &str, sig: &TSSignature) -> Option<Builtin> {
    let name = match sig {
        TSSignature::TSMethodSignature(m) => key_name(&m.key)?,
        TSSignature::TSPropertySignature(p) => key_name(&p.key)?,
        _ => return None,
    };
    let kind = match sig {
        TSSignature::TSMethodSignature(_) => BuiltinKind::Function,
        TSSignature::TSPropertySignature(_) => BuiltinKind::Const,
        _ => return None,
    };
    Some(Builtin {
        ns: Some(ns.to_string()),
        name,
        kind,
    })
}

fn key_name(key: &PropertyKey) -> Option<String> {
    match key {
        PropertyKey::StaticIdentifier(id) => Some(id.name.to_string()),
        PropertyKey::StringLiteral(s) => Some(s.value.to_string()),
        _ => None,
    }
}

fn qualified(b: &Builtin) -> String {
    match &b.ns {
        Some(ns) => format!("{ns}.{}", b.name),
        None => b.name.clone(),
    }
}

/// The drift guard: every symbol the standard library *declares* must be one
/// the translator *maps*. For each entry we translate a one-line call and
/// assert the output has no "fall-through" marker — the shape an unmapped
/// name lowers to (`Math.foo` → `math.foo`, a bare `foo(`). Global constants
/// (`undefined`/`Infinity`/`NaN`) are skipped: literals handled outside the
/// builtin mapping, with no call form.
#[test]
fn drift_guard_every_entry_translates() {
    use crate::Translator;

    for b in parse() {
        if b.ns.is_none() && b.kind == BuiltinKind::Const {
            continue;
        }
        let src = probe_source(&b);
        let rust = match Translator::new().translate(&src) {
            Ok(r) => r,
            Err(e) => panic!("{} failed to translate: {e}", qualified(&b)),
        };
        let marker = marker_for(&b);
        assert!(
            !has_fall_through(&rust, &marker),
            "{} is declared in stdlib but not mapped by the translator — \
             output contains the fall-through marker `{marker}`:\n{rust}",
            qualified(&b)
        );
    }
}

/// A minimal program that exercises `b` once, inside `main`. Arguments are
/// chosen so the mapping fires: Math/Object take two (covers `pow`/`atan2`/
/// `imul`/`is`/`assign`), Array.isArray needs an identifier receiver, the
/// rest take a literal.
fn probe_source(b: &Builtin) -> String {
    match (b.ns.as_deref(), b.kind, b.name.as_str()) {
        (Some(_), _, "isArray") => {
            "function main(): void { const x = 1; Array.isArray(x); }".to_string()
        }
        (Some(ns), BuiltinKind::Const, _) => {
            format!("function main(): void {{ {ns}.{}; }}", b.name)
        }
        (Some("Math"), _, _) => format!("function main(): void {{ Math.{}(1, 2); }}", b.name),
        (Some("console"), _, _) => format!("function main(): void {{ console.{}(1); }}", b.name),
        (Some("Number"), _, _) => format!("function main(): void {{ Number.{}(1); }}", b.name),
        (Some("String"), _, _) => format!("function main(): void {{ String.{}(65); }}", b.name),
        (Some("Array"), _, _) => format!("function main(): void {{ Array.{}([1]); }}", b.name),
        (Some("Object"), _, _) => format!("function main(): void {{ Object.{}(1, 2); }}", b.name),
        (Some(ns), _, _) => format!("function main(): void {{ {ns}.{}(1); }}", b.name),
        (None, _, _) => format!("function main(): void {{ {}(1); }}", b.name),
    }
}

/// The fall-through marker for `b` — the substring an unmapped name leaves in
/// the translated Rust. A namespace member `Math.foo` lowers to `math.foo`;
/// a global `foo(` lowers to `foo(` (snake-cased callee).
fn marker_for(b: &Builtin) -> String {
    use crate::translator::bindings;
    match (b.ns.as_deref(), b.kind) {
        (Some(ns), _) => format!("{}.{}", bindings::snake(ns), bindings::snake(&b.name)),
        (None, BuiltinKind::Function) => format!("{}(", bindings::snake(&b.name)),
        (None, BuiltinKind::Const) => String::new(),
    }
}

/// True when `marker` appears as a standalone callee in `rust` — preceded by
/// neither `.` nor an identifier byte. A `.`-preceded hit (`.is_nan(`) is a
/// method call that happens to share the name, not a fall-through.
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
