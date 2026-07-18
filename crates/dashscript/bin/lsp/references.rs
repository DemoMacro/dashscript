//! `textDocument/references`: find every reference to the symbol under the
//! cursor — its declaration (when requested) plus each read/write, resolved at
//! the **symbol level** by `oxc_semantic`. Two same-named bindings in
//! different scopes are distinct symbols, so a reference query on one never
//! returns the other's sites (the core correctness guarantee — no same-name
//! collisions). Single-file for now; cross-file resolution needs a
//! project-level symbol index.

use dashscript::translator::semantic::{SymbolEntry, SymbolTable};
use dashscript::Translator;
use lsp_types::{Location, ReferenceParams, Uri};
use oxc_span::Span;
use serde_json::Value;

use super::{text, Server};

impl Server {
    pub(super) fn on_references(&self, params: &ReferenceParams) -> Option<Value> {
        let tdp = &params.text_document_position;
        let uri = &tdp.text_document.uri;
        let text = self.docs.get(uri.as_str())?;
        let byte = text::position_to_byte(text, tdp.position)?;
        let table = Translator::new().symbols(text);
        let sym = find_symbol(&table, byte)?;
        let mut locs = Vec::new();
        if params.context.include_declaration {
            locs.push(to_location(uri, text, &sym.span));
        }
        for r in &sym.references {
            locs.push(to_location(uri, text, r));
        }
        serde_json::to_value(locs).ok()
    }
}

/// The symbol whose declaration — or a resolved reference — spans `byte`, or
/// `None` when the cursor sits on no symbol. Declarations are checked first,
/// so a cursor on the declaration site resolves even when the symbol has no
/// references.
pub(super) fn find_symbol(table: &SymbolTable, byte: usize) -> Option<SymbolEntry> {
    table
        .symbols
        .iter()
        .find(|s| covers(&s.span, byte))
        .or_else(|| {
            table
                .symbols
                .iter()
                .find(|s| s.references.iter().any(|r| covers(r, byte)))
        })
        .cloned()
}

/// Whether `byte` falls inside `span` (half-open `[start, end)`).
pub(super) fn covers(span: &Span, byte: usize) -> bool {
    byte >= span.start as usize && byte < span.end as usize
}

/// A `Location` for a `.ds` byte span within the given document.
fn to_location(uri: &Uri, text: &str, span: &Span) -> Location {
    Location {
        uri: uri.clone(),
        range: text::byte_range(text, span.start, span.end - span.start),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table_of(src: &str) -> SymbolTable {
        Translator::new().symbols(src)
    }

    #[test]
    fn find_symbol_on_declaration() {
        // `foo`'s declaration starts at byte 9 (`function ` is 9 chars).
        let table = table_of("function foo() { foo(); }");
        let sym = find_symbol(&table, 9).expect("symbol at declaration");
        assert_eq!(sym.name, "foo");
        // One reference: the `foo()` call.
        assert_eq!(sym.references.len(), 1);
    }

    #[test]
    fn find_symbol_on_reference_resolves_owner() {
        let text = "function foo() { foo(); }";
        // The call's `foo` — the last occurrence.
        let call_foo = text.rfind("foo").unwrap();
        let table = table_of(text);
        let sym = find_symbol(&table, call_foo).expect("symbol at reference");
        assert_eq!(sym.name, "foo");
    }

    #[test]
    fn find_symbol_none_on_non_identifier() {
        let text = "let x = 1;";
        // The `=` is not part of any symbol.
        let eq = text.find('=').unwrap();
        let table = table_of(text);
        assert!(find_symbol(&table, eq).is_none());
    }

    /// The core guarantee: a reference query on the parameter `x` returns only
    /// the parameter's sites — never the top-level `let x`. If this breaks,
    /// find-references would silently corrupt code by jumping across scopes.
    #[test]
    fn find_symbol_same_name_does_not_cross_scopes() {
        let text = "let x = 1; function f(x: number) { return x; }";
        let table = table_of(text);
        // The `x` inside `return x` — find its byte.
        let ret_x = text.find("return ").map(|i| i + "return ".len()).unwrap();
        let sym = find_symbol(&table, ret_x).expect("param x");
        assert_eq!(sym.name, "x");
        // The parameter has exactly one reference (`return x`); the top-level
        // `let x` (which has no references) must not be mixed in.
        assert_eq!(sym.references.len(), 1, "params only: {sym:?}");
        // And the declaration span is the parameter, not the top-level `let x`:
        // the param binding sits right after `f(`.
        let before = text[..sym.span.start as usize].trim_end();
        assert!(
            before.ends_with('('),
            "declaration should be the parameter, got context: {before:?}"
        );
    }
}
