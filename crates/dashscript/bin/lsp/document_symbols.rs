//! `textDocument/documentSymbol` — the outline view: every top-level
//! declaration (function / interface / type alias / import) becomes one
//! `DocumentSymbol`. DashScript symbols are file-top-level today, so the
//! outline is flat (no nested children).

use dashscript::translator::semantic::SymbolKind as DsSymbolKind;
use dashscript::Translator;
use lsp_types::{DocumentSymbol, DocumentSymbolParams, DocumentSymbolResponse, SymbolKind};
use serde_json::Value;

use super::text;

impl super::Server {
    pub(super) fn on_document_symbol(&self, params: &DocumentSymbolParams) -> Option<Value> {
        let text = self.docs.get(params.text_document.uri.as_str())?;
        let docs: Vec<DocumentSymbol> = Translator::new()
            .declarations(text)
            .into_iter()
            .map(|s| {
                let range = text::byte_range(text, s.span.start, s.span.end - s.span.start);
                #[allow(deprecated)]
                DocumentSymbol {
                    name: s.name,
                    detail: s.signature.as_ref().map(|sig| sig.label()),
                    kind: doc_kind(s.kind),
                    tags: None,
                    range,
                    selection_range: range,
                    children: None,
                    deprecated: None,
                }
            })
            .collect();
        serde_json::to_value(DocumentSymbolResponse::Nested(docs)).ok()
    }
}

/// Map DashScript's coarse symbol kind to an LSP `SymbolKind` icon. `type`
/// aliases (no dedicated LSP kind) map to `Class`, matching how VS Code renders
/// struct-like declarations in the outline.
fn doc_kind(kind: DsSymbolKind) -> SymbolKind {
    match kind {
        DsSymbolKind::Function => SymbolKind::FUNCTION,
        DsSymbolKind::Interface => SymbolKind::INTERFACE,
        DsSymbolKind::TypeAlias => SymbolKind::CLASS,
        DsSymbolKind::Class => SymbolKind::CLASS,
        DsSymbolKind::Variable => SymbolKind::VARIABLE,
        DsSymbolKind::Other => SymbolKind::VARIABLE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doc_kind_maps_each_variant() {
        assert_eq!(doc_kind(DsSymbolKind::Function), SymbolKind::FUNCTION);
        assert_eq!(doc_kind(DsSymbolKind::Interface), SymbolKind::INTERFACE);
        assert_eq!(doc_kind(DsSymbolKind::TypeAlias), SymbolKind::CLASS);
        assert_eq!(doc_kind(DsSymbolKind::Variable), SymbolKind::VARIABLE);
        assert_eq!(doc_kind(DsSymbolKind::Other), SymbolKind::VARIABLE);
    }

    #[test]
    fn function_detail_is_its_one_line_signature() {
        // The outline detail of a function is its signature label.
        let text = "function greet(name: string, times?: number): string { return name; }";
        let greet = Translator::new()
            .declarations(text)
            .into_iter()
            .find(|d| d.name == "greet")
            .expect("greet");
        assert_eq!(
            greet.signature.as_ref().expect("sig").label(),
            "(name: string, times?: number): string"
        );
    }
}
