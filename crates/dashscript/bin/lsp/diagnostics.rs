//! Translatability diagnostics: run `Translator::check` on a `.ds` document
//! and publish the result as LSP diagnostics.

use dashscript::Translator;
use lsp_server::{Connection, Notification};
use lsp_types::{Diagnostic, DiagnosticSeverity, PublishDiagnosticsParams, Uri};
use oxc_diagnostics::OxcDiagnostic;

use super::text::byte_range;

pub(super) fn publish_diagnostics(connection: &Connection, uri: &Uri, text: &str) {
    let diagnostics = Translator::new()
        .check(text)
        .iter()
        .map(|diag| to_lsp_diagnostic(diag, text))
        .collect();
    let params = PublishDiagnosticsParams {
        uri: uri.clone(),
        diagnostics,
        version: None,
    };
    let _ = connection
        .sender
        .send(Notification::new("textDocument/publishDiagnostics".into(), params).into());
}

fn to_lsp_diagnostic(diag: &OxcDiagnostic, text: &str) -> Diagnostic {
    let range = diag
        .labels
        .as_slice()
        .first()
        .map(|span| byte_range(text, span.offset(), span.len()))
        .unwrap_or_default();
    Diagnostic {
        range,
        severity: Some(DiagnosticSeverity::ERROR),
        message: diag.message.to_string(),
        source: Some("dashscript".to_string()),
        ..Default::default()
    }
}
