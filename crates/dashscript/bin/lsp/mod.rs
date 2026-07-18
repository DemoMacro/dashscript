//! `ds lsp` — the DashScript language server over stdio.
//!
//! Layering (mirrors a conventional LSP server):
//! - [`Server`] owns the connection and document state, routes requests, and
//!   refreshes the rust-analyzer backend ([`backend`]).
//! - [`diagnostics`] publishes translatability diagnostics (`Translator::check`).
//! - [`definition`] resolves go-to-definition (crate imports via rust-analyzer,
//!   in-file symbols locally).
//! - [`text`] holds the byte↔position↔URI helpers shared across the above.
//!
//! Stage 2: translatability diagnostics (`Translator::check`).
//! Stage 3: crate go-to-definition — a cursor on `import { X } from "crate"`
//! is mapped to the emitted `use crate::X` position, then forwarded to a
//! rust-analyzer backend that resolves it to the crate's `~/.cargo` source.

use std::{collections::HashMap, error::Error, path::Path};

use lsp_server::{Connection, Message, Request, Response};
use lsp_types::{
    CompletionOptions, HoverProviderCapability, InitializeParams, OneOf, ServerCapabilities,
    SignatureHelpOptions, TextDocumentSyncCapability, TextDocumentSyncKind, Uri,
};
use serde_json::Value;

mod backend;
mod builtins;
mod completion;
mod definition;
mod diagnostics;
mod document_symbols;
mod hover;
mod references;
mod signatures;
mod text;

use backend::RaClient;

/// Run the language server on stdio until the client requests shutdown.
pub fn run() -> Result<(), Box<dyn Error>> {
    let (connection, io_threads) = Connection::stdio();
    let init_value = connection.initialize(serde_json::to_value(server_capabilities())?)?;
    let init: InitializeParams = serde_json::from_value(init_value)?;
    // The backend binary path comes from the client's `initializationOptions`
    // (the extension forwards `dashscript.rustAnalyzerPath`); default to PATH.
    let ra_path = init
        .initialization_options
        .as_ref()
        .and_then(|o| o.get("rustAnalyzerPath"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| "rust-analyzer".to_string());

    let mut server = Server {
        conn: connection,
        docs: HashMap::new(),
        ra_path,
        ra: None,
    };
    server.main_loop()?;
    if let Some(ra) = server.ra.take() {
        ra.shutdown();
    }
    io_threads.join()?;
    Ok(())
}

fn server_capabilities() -> ServerCapabilities {
    ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        definition_provider: Some(OneOf::Left(true)),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        document_symbol_provider: Some(OneOf::Left(true)),
        references_provider: Some(OneOf::Left(true)),
        signature_help_provider: Some(SignatureHelpOptions {
            trigger_characters: Some(vec!["(".to_string()]),
            retrigger_characters: Some(vec![",".to_string()]),
            ..Default::default()
        }),
        completion_provider: Some(CompletionOptions {
            // `.` triggers member completion (`console.`, `Math.`).
            trigger_characters: Some(vec![".".to_string()]),
            resolve_provider: Some(false),
            ..Default::default()
        }),
        ..Default::default()
    }
}

struct Server {
    conn: Connection,
    /// uri string → latest `.ds` text. String keys sidestep `Uri`'s interior
    /// mutability (which trips clippy::mutable_key_type in a HashMap).
    docs: HashMap<String, String>,
    ra_path: String,
    ra: Option<RaClient>,
}

impl Server {
    fn main_loop(&mut self) -> Result<(), Box<dyn Error>> {
        while let Ok(message) = self.conn.receiver.recv() {
            match message {
                Message::Request(req) => {
                    if self.conn.handle_shutdown(&req)? {
                        break;
                    }
                    self.handle_request(req);
                }
                Message::Notification(not) => self.handle_notification(&not),
                Message::Response(_) => {}
            }
        }
        Ok(())
    }

    fn handle_request(&mut self, req: Request) {
        let id = req.id.clone();
        let method = req.method.clone();
        let resp = match method.as_str() {
            "textDocument/definition" => {
                let result = req
                    .extract::<lsp_types::GotoDefinitionParams>("textDocument/definition")
                    .ok()
                    .and_then(|(_, params)| self.on_definition(&params))
                    .unwrap_or(Value::Null);
                Response::new_ok(id, result)
            }
            "textDocument/hover" => {
                let result = req
                    .extract::<lsp_types::HoverParams>("textDocument/hover")
                    .ok()
                    .and_then(|(_, params)| self.on_hover(&params))
                    .unwrap_or(Value::Null);
                Response::new_ok(id, result)
            }
            "textDocument/completion" => {
                let result = req
                    .extract::<lsp_types::CompletionParams>("textDocument/completion")
                    .ok()
                    .and_then(|(_, params)| self.on_completion(&params))
                    .unwrap_or(Value::Null);
                Response::new_ok(id, result)
            }
            "textDocument/documentSymbol" => {
                let result = req
                    .extract::<lsp_types::DocumentSymbolParams>("textDocument/documentSymbol")
                    .ok()
                    .and_then(|(_, params)| self.on_document_symbol(&params))
                    .unwrap_or(Value::Null);
                Response::new_ok(id, result)
            }
            "textDocument/signatureHelp" => {
                let result = req
                    .extract::<lsp_types::SignatureHelpParams>("textDocument/signatureHelp")
                    .ok()
                    .and_then(|(_, params)| self.on_signature_help(&params))
                    .unwrap_or(Value::Null);
                Response::new_ok(id, result)
            }
            "textDocument/references" => {
                let result = req
                    .extract::<lsp_types::ReferenceParams>("textDocument/references")
                    .ok()
                    .and_then(|(_, params)| self.on_references(&params))
                    .unwrap_or(Value::Null);
                Response::new_ok(id, result)
            }
            // Other requests (hover, …) return null until wired.
            _ => Response::new_ok(id, Value::Null),
        };
        let _ = self.conn.sender.send(resp.into());
    }

    fn handle_notification(&mut self, not: &lsp_server::Notification) {
        let params = not.params.clone();
        match not.method.as_str() {
            "textDocument/didOpen" => {
                if let Ok(p) =
                    serde_json::from_value::<lsp_types::DidOpenTextDocumentParams>(params)
                {
                    let key = p.text_document.uri.as_str().to_string();
                    let text = p.text_document.text.clone();
                    self.docs.insert(key, text.clone());
                    self.refresh(&p.text_document.uri, &text);
                    diagnostics::publish_diagnostics(&self.conn, &p.text_document.uri, &text);
                }
            }
            "textDocument/didChange" => {
                if let Ok(p) =
                    serde_json::from_value::<lsp_types::DidChangeTextDocumentParams>(params)
                {
                    // Full sync: the single change carries the whole new text.
                    if let Some(change) = p.content_changes.into_iter().next() {
                        let key = p.text_document.uri.as_str().to_string();
                        self.docs.insert(key, change.text.clone());
                        self.refresh(&p.text_document.uri, &change.text);
                        diagnostics::publish_diagnostics(
                            &self.conn,
                            &p.text_document.uri,
                            &change.text,
                        );
                    }
                }
            }
            _ => {}
        }
    }

    /// Emit the `.ds` text to a cache Cargo project and tell rust-analyzer
    /// about the resulting Rust file (`src/<stem>.rs` in project mode,
    /// `src/main.rs` for a lone file). Errors are swallowed — diagnostics and
    /// go-to-definition degrade gracefully when emission or the backend fails.
    fn refresh(&mut self, uri: &Uri, text: &str) {
        let Some(src_path) = text::uri_to_path(uri) else {
            return;
        };
        let Some(cache) = self.cache_dir(uri) else {
            return;
        };
        if crate::commands::project::emit_cargo_project(text, &src_path, &cache).is_err() {
            return;
        }
        let main_rs = text::rust_file_for(&cache, &src_path);
        let Ok(main_text) = std::fs::read_to_string(&main_rs) else {
            return;
        };
        let Ok(main_uri) = text::path_to_uri(&main_rs) else {
            return;
        };
        if self.ensure_ra(&cache).is_ok() {
            if let Some(ra) = self.ra.as_ref() {
                ra.notify(
                    "textDocument/didOpen",
                    lsp_types::DidOpenTextDocumentParams {
                        text_document: lsp_types::TextDocumentItem {
                            uri: main_uri,
                            language_id: "rust".to_string(),
                            version: 0,
                            text: main_text,
                        },
                    },
                );
            }
        }
    }

    /// Spawn rust-analyzer once, rooted at the cache Cargo project.
    fn ensure_ra(&mut self, root: &Path) -> Result<(), Box<dyn Error>> {
        if self.ra.is_none() {
            let root_uri = text::path_to_uri(root)?.as_str().to_string();
            self.ra = Some(RaClient::spawn(&self.ra_path, &root_uri, root)?);
        }
        Ok(())
    }

    fn cache_dir(&self, uri: &Uri) -> Option<std::path::PathBuf> {
        let path = text::uri_to_path(uri)?;
        let stem = path.file_stem()?.to_str()?.to_string();
        Some(std::env::temp_dir().join("dashscript-lsp").join(stem))
    }
}
