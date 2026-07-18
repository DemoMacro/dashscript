//! `textDocument/documentSymbol` — the outline view: every top-level
//! declaration (function / interface / type alias / import) becomes one
//! `DocumentSymbol`. DashScript symbols are file-top-level today, so the
//! outline is flat (no nested children).

use dashscript::translator::imports::{ParamInfo, Signature};
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
                // `deprecated` field is the LSP 3.x way to flag it; we never set it.
                DocumentSymbol {
                    name: s.name,
                    detail: s.signature.as_ref().map(format_signature),
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

/// `(name: type, opt?: type): return` — the one-line signature shown in the
/// outline detail. Reused by hover and signature-help (the same label).
fn format_signature(sig: &Signature) -> String {
    let params: Vec<String> = sig.params.iter().map(format_param).collect();
    let ret = sig
        .return_type
        .clone()
        .unwrap_or_else(|| "void".to_string());
    format!("({}): {}", params.join(", "), ret)
}

/// One parameter rendered as `name: type` (or `name?: type`, `name: any`).
fn format_param(p: &ParamInfo) -> String {
    let ty = p.type_text.clone().unwrap_or_else(|| "any".to_string());
    if p.optional {
        format!("{}?: {}", p.name, ty)
    } else {
        format!("{}: {}", p.name, ty)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_signature_renders_params_and_return() {
        let sig = Signature {
            params: vec![
                ParamInfo {
                    name: "name".into(),
                    type_text: Some("string".into()),
                    optional: false,
                },
                ParamInfo {
                    name: "times".into(),
                    type_text: Some("number".into()),
                    optional: true,
                },
            ],
            return_type: Some("string".into()),
        };
        assert_eq!(
            format_signature(&sig),
            "(name: string, times?: number): string"
        );
    }

    #[test]
    fn format_signature_void_when_no_return() {
        let sig = Signature {
            params: vec![],
            return_type: None,
        };
        assert_eq!(format_signature(&sig), "(): void");
    }

    #[test]
    fn format_param_untyped_is_any() {
        let p = ParamInfo {
            name: "x".into(),
            type_text: None,
            optional: false,
        };
        assert_eq!(format_param(&p), "x: any");
    }

    #[test]
    fn doc_kind_maps_each_variant() {
        assert_eq!(doc_kind(DsSymbolKind::Function), SymbolKind::FUNCTION);
        assert_eq!(doc_kind(DsSymbolKind::Interface), SymbolKind::INTERFACE);
        assert_eq!(doc_kind(DsSymbolKind::TypeAlias), SymbolKind::CLASS);
        assert_eq!(doc_kind(DsSymbolKind::Variable), SymbolKind::VARIABLE);
        assert_eq!(doc_kind(DsSymbolKind::Other), SymbolKind::VARIABLE);
    }
}
