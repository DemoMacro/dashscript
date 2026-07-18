//! `textDocument/prepareRename` + `textDocument/rename`: rename the symbol
//! under the cursor across its declaration and every resolved reference — at
//! the symbol level, the same resolution find-references uses. Same-name
//! bindings in other scopes are a different symbol and are left untouched.

use dashscript::translator::semantic::SymbolEntry;
use dashscript::Translator;
use lsp_types::{
    PrepareRenameResponse, RenameParams, TextDocumentPositionParams, TextEdit, WorkspaceEdit,
};
use oxc_span::Span;
use serde_json::Value;

use super::{references, text, Server};

impl Server {
    pub(super) fn on_prepare_rename(&self, params: &TextDocumentPositionParams) -> Option<Value> {
        let text = self.docs.get(params.text_document.uri.as_str())?;
        let byte = text::position_to_byte(text, params.position)?;
        let table = Translator::new().symbols(text);
        let sym = references::find_symbol(&table, byte)?;
        let here = cursor_span(&sym, byte);
        let range = text::byte_range(text, here.start, here.end - here.start);
        serde_json::to_value(PrepareRenameResponse::RangeWithPlaceholder {
            range,
            placeholder: sym.name,
        })
        .ok()
    }

    pub(super) fn on_rename(&self, params: &RenameParams) -> Option<Value> {
        let tdp = &params.text_document_position;
        let uri = &tdp.text_document.uri;
        let text = self.docs.get(uri.as_str())?;
        let byte = text::position_to_byte(text, tdp.position)?;
        let table = Translator::new().symbols(text);
        let sym = references::find_symbol(&table, byte)?;
        let edits = rename_edits(&sym, text, &params.new_name);
        // `changes` is keyed by `Uri` (LSP-fixed); Uri's interior mutability
        // trips clippy::mutable_key_type, but the key is never mutated here.
        #[allow(clippy::mutable_key_type)]
        let mut changes = std::collections::HashMap::new();
        changes.insert(uri.clone(), edits);
        serde_json::to_value(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        })
        .ok()
    }
}

/// The edits to rename `sym` to `new_name`: one at the declaration plus one
/// per resolved reference. Same-name symbols in other scopes are already
/// excluded — `sym` is scoped to a single binding.
fn rename_edits(sym: &SymbolEntry, text: &str, new_name: &str) -> Vec<TextEdit> {
    let mut edits = vec![to_edit(text, &sym.span, new_name)];
    for r in &sym.references {
        edits.push(to_edit(text, r, new_name));
    }
    edits
}

/// The span the cursor actually sits on — the declaration, or the specific
/// reference it hit — so prepare-rename highlights exactly that token.
fn cursor_span(sym: &SymbolEntry, byte: usize) -> Span {
    if references::covers(&sym.span, byte) {
        return sym.span;
    }
    sym.references
        .iter()
        .copied()
        .find(|r| references::covers(r, byte))
        .unwrap_or(sym.span)
}

/// A `TextEdit` replacing the text at `span` with `new_name`.
fn to_edit(text: &str, span: &Span, new_name: &str) -> TextEdit {
    TextEdit {
        range: text::byte_range(text, span.start, span.end - span.start),
        new_text: new_name.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn symbol_at(text: &str, byte: usize) -> SymbolEntry {
        let table = Translator::new().symbols(text);
        references::find_symbol(&table, byte).expect("symbol under cursor")
    }

    #[test]
    fn rename_edits_cover_declaration_and_references() {
        let text = "function foo() { foo(); }";
        let sym = symbol_at(text, 9); // declaration
        let edits = rename_edits(&sym, text, "bar");
        // 1 declaration + 1 reference.
        assert_eq!(edits.len(), 2);
        assert!(edits.iter().all(|e| e.new_text == "bar"));
    }

    /// Renaming the parameter `x` touches only the parameter — never the
    /// top-level `let x`. If this breaks, F2 would silently corrupt code.
    #[test]
    fn rename_does_not_cross_same_name_scope() {
        let text = "let x = 1; function f(x: number) { return x; }";
        let ret_x = text.find("return ").map(|i| i + "return ".len()).unwrap();
        let sym = symbol_at(text, ret_x);
        let edits = rename_edits(&sym, text, "y");
        // Parameter `x`: 1 declaration + 1 reference (`return x`).
        assert_eq!(edits.len(), 2, "param-only edits: {edits:?}");
        // The top-level `let x` is at line 0, char 4 — no edit may land there.
        for e in &edits {
            assert!(
                !(e.range.start.line == 0 && e.range.start.character == 4),
                "edit crossed into the top-level `let x`: {e:?}"
            );
        }
    }

    #[test]
    fn cursor_span_picks_declaration_when_on_it() {
        let text = "function foo() { foo(); }";
        let sym = symbol_at(text, 9); // on declaration
        let here = cursor_span(&sym, 9);
        // Declaration `foo` spans bytes 9..12.
        assert_eq!((here.start, here.end), (9, 12));
    }

    #[test]
    fn cursor_span_picks_reference_when_on_it() {
        let text = "function foo() { foo(); }";
        let call_foo = text.rfind("foo").unwrap();
        let sym = symbol_at(text, call_foo);
        let here = cursor_span(&sym, call_foo);
        assert_eq!(here.start as usize, call_foo);
    }
}
