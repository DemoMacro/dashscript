//! `ds lsp` — the DashScript language server over stdio.
//!
//! Stage 2: translatability diagnostics (`Translator::check`).
//! Stage 3: crate go-to-definition — a cursor on `import { X } from "crate"`
//! is mapped to the emitted `use crate::X` position, then forwarded to a
//! rust-analyzer backend that resolves it to the crate's `~/.cargo` source.

use std::{
    collections::HashMap,
    error::Error,
    io::BufReader,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicI32, Ordering},
    thread::{self, JoinHandle},
};

use crossbeam_channel::{unbounded, Receiver, Sender};
use dashscript::Translator;
use lsp_server::{Connection, Message, Notification, Request, RequestId, Response};
use lsp_types::{
    ClientCapabilities, Diagnostic, DiagnosticSeverity,
    DidChangeTextDocumentParams, DidOpenTextDocumentParams, GotoDefinitionParams,
    GotoDefinitionResponse, InitializeParams, Location,
    OneOf, Position, PublishDiagnosticsParams, Range, ServerCapabilities,
    TextDocumentItem, TextDocumentSyncCapability, TextDocumentSyncKind, Uri,
};
use oxc_diagnostics::OxcDiagnostic;
use serde_json::Value;

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

    let mut server = Server { conn: connection, docs: HashMap::new(), ra_path, ra: None };
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
                    .extract::<GotoDefinitionParams>("textDocument/definition")
                    .ok()
                    .and_then(|(_, params)| self.on_definition(&params))
                    .unwrap_or(Value::Null);
                Response::new_ok(id, result)
            }
            // Other requests (hover, completion, …) return null until wired.
            _ => Response::new_ok(id, Value::Null),
        };
        let _ = self.conn.sender.send(resp.into());
    }

    fn handle_notification(&mut self, not: &Notification) {
        let params = not.params.clone();
        match not.method.as_str() {
            "textDocument/didOpen" => {
                if let Ok(p) = serde_json::from_value::<DidOpenTextDocumentParams>(params) {
                    let key = p.text_document.uri.as_str().to_string();
                    let text = p.text_document.text.clone();
                    self.docs.insert(key, text.clone());
                    self.refresh(&p.text_document.uri, &text);
                    publish_diagnostics(&self.conn, &p.text_document.uri, &text);
                }
            }
            "textDocument/didChange" => {
                if let Ok(p) = serde_json::from_value::<DidChangeTextDocumentParams>(params) {
                    // Full sync: the single change carries the whole new text.
                    if let Some(change) = p.content_changes.into_iter().next() {
                        let key = p.text_document.uri.as_str().to_string();
                        self.docs.insert(key, change.text.clone());
                        self.refresh(&p.text_document.uri, &change.text);
                        publish_diagnostics(&self.conn, &p.text_document.uri, &change.text);
                    }
                }
            }
            _ => {}
        }
    }

    /// Emit the `.ds` text to a cache Cargo project and tell rust-analyzer
    /// about the resulting `main.rs`. Errors are swallowed — diagnostics and
    /// go-to-definition degrade gracefully when emission or the backend fails.
    fn refresh(&mut self, uri: &Uri, text: &str) {
        let Some(src_path) = uri_to_path(uri) else { return };
        let Some(cache) = self.cache_dir(uri) else { return };
        if crate::emit_cargo_project(text, &src_path, &cache).is_err() {
            return;
        }
        let main_rs = cache.join("src").join("main.rs");
        let Ok(main_text) = std::fs::read_to_string(&main_rs) else { return };
        let Ok(main_uri) = path_to_uri(&main_rs) else { return };
        if self.ensure_ra(&cache).is_ok() {
            if let Some(ra) = self.ra.as_ref() {
                ra.notify(
                    "textDocument/didOpen",
                    DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
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
            let root_uri = path_to_uri(root)?.as_str().to_string();
            self.ra = Some(RaClient::spawn(&self.ra_path, &root_uri)?);
        }
        Ok(())
    }

    fn cache_dir(&self, uri: &Uri) -> Option<PathBuf> {
        let path = uri_to_path(uri)?;
        let stem = path.file_stem()?.to_str()?.to_string();
        Some(std::env::temp_dir().join("dashscript-lsp").join(stem))
    }

    /// Map a definition request to the crate source: locate the import
    /// specifier under the cursor, emit the Cargo project, map the symbol to
    /// its `use` position in `main.rs`, and let rust-analyzer resolve it.
    fn on_definition(&mut self, params: &GotoDefinitionParams) -> Option<Value> {
        let tdp = &params.text_document_position_params;
        let uri = &tdp.text_document.uri;
        let text = self.docs.get(uri.as_str())?.clone();
        // A crate import specifier is resolved by rust-analyzer; anything else
        // (a local function/type/interface/import binding) is resolved in-file.
        if let Some((module, symbol)) = locate_import(&text, tdp.position) {
            self.definition_via_ra(uri, &text, module, symbol)
        } else {
            self.definition_local(uri, &text, tdp.position)
        }
    }

    /// Forward an import specifier to rust-analyzer → the crate's `~/.cargo` source.
    fn definition_via_ra(
        &mut self,
        uri: &Uri,
        text: &str,
        module: String,
        symbol: String,
    ) -> Option<Value> {
        self.refresh(uri, text);
        let cache = self.cache_dir(uri)?;
        let main_rs = cache.join("src").join("main.rs");
        let main_text = std::fs::read_to_string(&main_rs).ok()?;
        let rust_pos = map_symbol_pos(&main_text, &module, &symbol)?;
        let main_uri = path_to_uri(&main_rs).ok()?;
        let ra = self.ra.as_ref()?;
        let resp = ra.definition(main_uri.as_str(), rust_pos).ok()?;
        serde_json::to_value(resp).ok()
    }

    /// Resolve an in-file reference to a local declaration — a function, type,
    /// interface, or import binding — in the same `.ds` document.
    fn definition_local(&self, uri: &Uri, text: &str, pos: Position) -> Option<Value> {
        let byte = position_to_byte(text, pos)?;
        let word = word_at(text, byte)?;
        let decl = Translator::new().declarations(text).into_iter().find(|d| d.name == word)?;
        let range = byte_range(text, decl.span.start, decl.span.end - decl.span.start);
        serde_json::to_value(GotoDefinitionResponse::Scalar(Location { uri: uri.clone(), range })).ok()
    }
}

// === rust-analyzer backend ===

/// A synchronous LSP client driving a rust-analyzer subprocess over its
/// stdin/stdout. We are the *client* here (VS Code is the client of `ds lsp`).
struct RaClient {
    tx: Sender<Message>,
    rx: Receiver<Message>,
    next_id: AtomicI32,
    _child: Child,
    _reader: JoinHandle<()>,
    _writer: JoinHandle<()>,
}

impl RaClient {
    fn spawn(path: &str, root_uri: &str) -> Result<Self, Box<dyn Error>> {
        let mut child = Command::new(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("cannot spawn rust-analyzer '{path}': {e}"))?;
        let stdout = child.stdout.take().ok_or("rust-analyzer: no stdout")?;
        let stdin = child.stdin.take().ok_or("rust-analyzer: no stdin")?;

        // Writer thread: drain our outgoing channel onto the child's stdin.
        let (tx_write, rx_write) = unbounded::<Message>();
        let mut stdin = stdin;
        let writer = thread::spawn(move || {
            for msg in rx_write.iter() {
                if msg.write(&mut stdin).is_err() {
                    break;
                }
            }
        });

        // Reader thread: frame the child's stdout into our incoming channel.
        let (tx_read, rx_read) = unbounded::<Message>();
        let reader = thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            while let Ok(Some(msg)) = Message::read(&mut reader) {
                if tx_read.send(msg).is_err() {
                    break;
                }
            }
        });

        let client = RaClient {
            tx: tx_write,
            rx: rx_read,
            next_id: AtomicI32::new(1),
            _child: child,
            _reader: reader,
            _writer: writer,
        };

        let params = serde_json::json!({
            "processId": std::process::id(),
            "rootUri": root_uri,
            "capabilities": ClientCapabilities::default(),
        });
        let _ = client.request("initialize", params)?;
        client.notify("initialized", serde_json::json!({}));
        Ok(client)
    }

    fn next_id(&self) -> RequestId {
        RequestId::from(self.next_id.fetch_add(1, Ordering::SeqCst))
    }

    fn notify(&self, method: &str, params: impl serde::Serialize) {
        let _ = self.tx.send(Notification::new(method.into(), params).into());
    }

    /// Send a request and block for its response, skipping any notifications
    /// rust-analyzer emits in the meantime (progress, diagnostics, …).
    fn request(&self, method: &str, params: impl serde::Serialize) -> Result<Value, Box<dyn Error>> {
        let id = self.next_id();
        self.tx.send(Request::new(id.clone(), method.into(), params).into())?;
        loop {
            match self.rx.recv() {
                Ok(Message::Response(r)) if r.id == id => {
                    return r.response_result.map_err(|e| format!("rust-analyzer {method}: {e:?}").into());
                }
                Ok(_) => continue,
                Err(e) => return Err(format!("rust-analyzer {method}: channel {e}").into()),
            }
        }
    }

    fn definition(
        &self,
        uri: &str,
        position: Position,
    ) -> Result<GotoDefinitionResponse, Box<dyn Error>> {
        let params = serde_json::json!({
            "textDocument": { "uri": uri },
            "position": position,
        });
        let value = self.request("textDocument/definition", params)?;
        if value.is_null() {
            return Err("rust-analyzer returned no definition".into());
        }
        Ok(serde_json::from_value(value)?)
    }

    /// Best-effort shutdown: send `shutdown` + `exit`; the child is dropped on
    /// `RaClient` drop regardless.
    fn shutdown(self) {
        let id = self.next_id();
        let _ = self.tx.send(Request::new(id, "shutdown".into(), Value::Null).into());
        let _ = self.tx.send(Notification::new("exit".into(), Value::Null).into());
    }
}

// === diagnostics + mapping helpers ===

fn publish_diagnostics(connection: &Connection, uri: &Uri, text: &str) {
    let diagnostics = Translator::new()
        .check(text)
        .iter()
        .map(|diag| to_lsp_diagnostic(diag, text))
        .collect();
    let params = PublishDiagnosticsParams { uri: uri.clone(), diagnostics, version: None };
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

/// If the cursor sits on a bare-crate import specifier (`import { X } from
/// "crate"`), return the crate module ident and the symbol name as written in
/// the emitted `use crate::X`.
fn locate_import(text: &str, pos: Position) -> Option<(String, String)> {
    let byte = position_to_byte(text, pos)?;
    for imp in Translator::new().crate_imports(text) {
        for sym in &imp.symbols {
            if byte >= sym.span.start as usize && byte <= sym.span.end as usize {
                return Some((imp.module.clone(), sym.name.clone()));
            }
        }
    }
    None
}

/// Find the position of `symbol` within the `use <module>::…` line of the
/// emitted Rust source — the position forwarded to rust-analyzer.
fn map_symbol_pos(main_rs: &str, module: &str, symbol: &str) -> Option<Position> {
    let needle = format!("{module}::");
    for (line_idx, line) in main_rs.lines().enumerate() {
        if !line.trim_start().starts_with("use ") || !line.contains(&needle) {
            continue;
        }
        if let Some(col) = find_word_col(line, symbol) {
            return Some(Position { line: line_idx as u32, character: col });
        }
    }
    None
}

/// Column (in characters) of the first whole-word occurrence of `word`.
fn find_word_col(line: &str, word: &str) -> Option<u32> {
    let bytes = line.as_bytes();
    let mut from = 0;
    while let Some(rel) = line[from..].find(word) {
        let start = from + rel;
        let end = start + word.len();
        let before = if start == 0 { b' ' } else { bytes[start - 1] };
        let after = bytes.get(end).copied().unwrap_or(b' ');
        if !is_ident_byte(before) && !is_ident_byte(after) {
            return Some(line[..start].chars().count() as u32);
        }
        from = start + word.len();
    }
    None
}

/// The identifier covering `byte` in `text`, if the cursor sits on one.
fn word_at(text: &str, byte: usize) -> Option<String> {
    let bytes = text.as_bytes();
    if byte >= bytes.len() || !is_ident_byte(bytes[byte]) {
        return None;
    }
    let mut start = byte;
    while start > 0 && is_ident_byte(bytes[start - 1]) {
        start -= 1;
    }
    let mut end = byte;
    while end < bytes.len() && is_ident_byte(bytes[end]) {
        end += 1;
    }
    Some(text[start..end].to_string())
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// An LSP `Position` (0-based line/character) → a `.ds` byte offset. The
/// character column is counted in Unicode scalars; `.ds` sources are ASCII
/// where this matters, so it agrees with the UTF-16 the protocol specifies.
fn position_to_byte(text: &str, pos: Position) -> Option<usize> {
    let mut line = 0u32;
    let mut col = 0u32;
    for (i, ch) in text.char_indices() {
        if line == pos.line && col == pos.character {
            return Some(i);
        }
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    (line == pos.line && col == pos.character).then_some(text.len())
}

/// An oxc byte (offset, length) → an LSP `Range`.
fn byte_range(text: &str, offset: u32, len: u32) -> Range {
    let start = offset as usize;
    let end = start.saturating_add(len as usize).min(text.len());
    Range { start: byte_to_position(text, start), end: byte_to_position(text, end) }
}

fn byte_to_position(text: &str, byte_offset: usize) -> Position {
    let byte_offset = byte_offset.min(text.len());
    let prefix = &text[..byte_offset];
    let line = prefix.bytes().filter(|&b| b == b'\n').count() as u32;
    let line_start = prefix.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let character = text[line_start..byte_offset].chars().count() as u32;
    Position { line, character }
}

fn uri_to_path(uri: &Uri) -> Option<PathBuf> {
    url::Url::parse(uri.as_str()).ok()?.to_file_path().ok()
}

fn path_to_uri(path: &Path) -> Result<Uri, Box<dyn Error>> {
    let url = url::Url::from_file_path(path)
        .map_err(|_| format!("not an absolute file path: {}", path.display()))?;
    Ok(url.as_str().parse::<Uri>()?)
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

    #[test]
    fn locate_import_resolves_named_specifier() {
        let text = "import { Adler32 } from \"adler\";";
        // `Adler32` starts at character 9 on line 0.
        let (module, symbol) = locate_import(text, Position { line: 0, character: 9 }).unwrap();
        assert_eq!(module, "adler");
        assert_eq!(symbol, "Adler32");
    }

    #[test]
    fn map_symbol_pos_finds_use_clause() {
        // The translator emits `use adler::Adler32;` — `Adler32` begins at
        // column 11 (`use adler::` is 11 characters).
        let main_rs = "use adler::Adler32;\n\nfn main() {}\n";
        let pos = map_symbol_pos(main_rs, "adler", "Adler32").unwrap();
        assert_eq!(pos, Position { line: 0, character: 11 });
    }

    #[test]
    fn map_symbol_pos_whole_word_only() {
        // `Adler` is a prefix of `Adler32` — it must not match.
        assert!(map_symbol_pos("use adler::Adler32;\n", "adler", "Adler").is_none());
    }

    #[test]
    fn path_to_uri_has_no_verbatim_prefix() {
        let uri = path_to_uri(&std::env::temp_dir()).unwrap();
        let s = uri.as_str();
        assert!(s.starts_with("file:///"), "bad scheme: {s}");
        assert!(!s.contains("//?/"), "verbatim prefix leaked: {s}");
    }

    #[test]
    fn word_at_extracts_identifier_under_cursor() {
        let text = "const x = foo();";
        // `foo` spans bytes 10..13.
        assert_eq!(word_at(text, 10).as_deref(), Some("foo"));
        assert_eq!(word_at(text, 12).as_deref(), Some("foo"));
        assert_eq!(word_at(text, 13), None); // `(` is not an ident char
    }
}
