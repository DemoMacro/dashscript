//! Translatability diagnostics: run `Translator::check` on a `.ts` document
//! and publish the result as LSP diagnostics.

use dashscript::Translator;
use lsp_server::{Connection, Notification};
use lsp_types::{Diagnostic, DiagnosticSeverity, PublishDiagnosticsParams, Uri};
use oxc_diagnostics::OxcDiagnostic;

use super::text::byte_range;

pub(super) fn publish_diagnostics(connection: &Connection, uri: &Uri, text: &str) {
    // Only check DashScript source — a file that imports no `cargo:` crate is
    // plain TypeScript (the VS Code extension's own sources, an ordinary npm
    // project, …); reporting translatability on it would intrude on the TS/Node
    // language server. `ds lint` still checks any file on request.
    let translator = Translator::new();
    if translator.crate_imports(text).is_empty() {
        return;
    }
    let diagnostics = translator
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
