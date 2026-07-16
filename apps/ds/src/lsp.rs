//! `ds lsp` — the DashScript language server over stdio.
//!
//! Stage 2: publishes translatability diagnostics (`Translator::check`) on
//! textDocument open/change. Crate go-to-definition (a rust-analyzer backend,
//! stage 3) and in-file definition (stage 4) arrive next.

use std::error::Error;

use dashscript::Translator;
use lsp_server::{Connection, Message, Notification, Response};
use lsp_types::{
    Diagnostic, DiagnosticSeverity, DidChangeTextDocumentParams, DidOpenTextDocumentParams,
    Position, PublishDiagnosticsParams, Range, ServerCapabilities, TextDocumentSyncCapability,
    TextDocumentSyncKind, Uri,
};
use oxc_diagnostics::OxcDiagnostic;
use serde_json::Value;

/// Run the language server on stdio until the client requests shutdown.
pub fn run() -> Result<(), Box<dyn Error>> {
    let (connection, io_threads) = Connection::stdio();
    let capabilities = serde_json::to_value(server_capabilities())?;
    let _initialize_params = connection.initialize(capabilities)?;

    while let Ok(message) = &connection.receiver.recv() {
        match message {
            Message::Request(request) => {
                if connection.handle_shutdown(request)? {
                    break;
                }
                // definition/hover return null until stages 3/4.
                let _ = connection.sender.send(Message::Response(Response::new_ok(
                    request.id.clone(),
                    Value::Null,
                )));
            }
            Message::Notification(notification) => {
                handle_notification(&connection, notification);
            }
            Message::Response(_) => {}
        }
    }
    io_threads.join()?;
    Ok(())
}

fn server_capabilities() -> ServerCapabilities {
    ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        ..Default::default()
    }
}

fn handle_notification(connection: &Connection, notification: &Notification) {
    let params = notification.params.clone();
    match notification.method.as_str() {
        "textDocument/didOpen" => {
            if let Ok(params) = serde_json::from_value::<DidOpenTextDocumentParams>(params) {
                publish_diagnostics(
                    connection,
                    &params.text_document.uri,
                    &params.text_document.text,
                );
            }
        }
        "textDocument/didChange" => {
            if let Ok(params) = serde_json::from_value::<DidChangeTextDocumentParams>(params) {
                // Full sync: the single change carries the whole new text.
                if let Some(change) = params.content_changes.into_iter().next() {
                    publish_diagnostics(connection, &params.text_document.uri, &change.text);
                }
            }
        }
        _ => {}
    }
}

/// Run `Translator::check` on the document and push the diagnostics.
fn publish_diagnostics(connection: &Connection, uri: &Uri, text: &str) {
    let diagnostics = Translator::new()
        .check(text)
        .iter()
        .map(|diag| to_lsp_diagnostic(diag, text))
        .collect();
    let params = PublishDiagnosticsParams { uri: uri.clone(), diagnostics, version: None };
    let _ = connection.sender.send(Message::Notification(Notification::new(
        "textDocument/publishDiagnostics".into(),
        params,
    )));
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

/// An oxc byte (offset, length) → an LSP `Range` (0-based line/character).
fn byte_range(text: &str, offset: u32, len: u32) -> Range {
    let start = offset as usize;
    let end = start.saturating_add(len as usize).min(text.len());
    Range { start: byte_to_position(text, start), end: byte_to_position(text, end) }
}

fn byte_to_position(text: &str, byte_offset: usize) -> Position {
    let byte_offset = byte_offset.min(text.len());
    let prefix = &text[..byte_offset];
    let line = prefix.bytes().filter(|&byte| byte == b'\n').count() as u32;
    let line_start = prefix.rfind('\n').map(|idx| idx + 1).unwrap_or(0);
    let character = text[line_start..byte_offset].chars().count() as u32;
    Position { line, character }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_tracks_lines_and_columns() {
        let text = "abc\ndef\nghi";
        assert_eq!(byte_to_position(text, 0), Position { line: 0, character: 0 });
        assert_eq!(byte_to_position(text, 3), Position { line: 0, character: 3 });
        assert_eq!(byte_to_position(text, 4), Position { line: 1, character: 0 });
        assert_eq!(byte_to_position(text, 8), Position { line: 2, character: 0 });
    }

    #[test]
    fn range_from_byte_span() {
        let text = "hello\nworld";
        // "world" spans bytes 6..11 → line 1, characters 0..5.
        let range = byte_range(text, 6, 5);
        assert_eq!(range.start, Position { line: 1, character: 0 });
        assert_eq!(range.end, Position { line: 1, character: 5 });
    }

    #[test]
    fn range_clamps_past_end_of_text() {
        let range = byte_range("ab", 0, 100);
        assert_eq!(range.end, Position { line: 0, character: 2 });
    }
}
