//! rust-analyzer backend: a synchronous LSP client driving a rust-analyzer
//! subprocess over its stdin/stdout. We are the *client* here (VS Code is the
//! client of `ds lsp`).

use std::{
    error::Error,
    io::BufReader,
    path::Path,
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicI32, Ordering},
    thread::{self, JoinHandle},
};

use crossbeam_channel::{unbounded, Receiver, Sender};
use lsp_server::{Message, Notification, Request, RequestId};
use lsp_types::{ClientCapabilities, GotoDefinitionResponse, Position};
use serde_json::Value;

pub(super) struct RaClient {
    tx: Sender<Message>,
    rx: Receiver<Message>,
    next_id: AtomicI32,
    _child: Child,
    _reader: JoinHandle<()>,
    _writer: JoinHandle<()>,
}

impl RaClient {
    pub(super) fn spawn(
        path: &str,
        root_uri: &str,
        root_dir: &Path,
    ) -> Result<Self, Box<dyn Error>> {
        // `current_dir(root_dir)` is load-bearing: rust-analyzer otherwise inherits
        // `ds lsp`'s cwd (the VS Code workspace folder). When that folder is itself a
        // Cargo workspace — e.g. opening a file from this repo's root — RA analyses
        // the workspace's own `Cargo.toml` instead of the emitted cache project, so
        // crate imports (`use adler::Adler32`) fail to resolve.
        let mut child = Command::new(path)
            .current_dir(root_dir)
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

    pub(super) fn notify(&self, method: &str, params: impl serde::Serialize) {
        let _ = self
            .tx
            .send(Notification::new(method.into(), params).into());
    }

    /// Send a request and block for its response, skipping any notifications
    /// rust-analyzer emits in the meantime (progress, diagnostics, …).
    pub(super) fn request(
        &self,
        method: &str,
        params: impl serde::Serialize,
    ) -> Result<Value, Box<dyn Error>> {
        let id = self.next_id();
        self.tx
            .send(Request::new(id.clone(), method.into(), params).into())?;
        loop {
            match self.rx.recv() {
                Ok(Message::Response(r)) if r.id == id => {
                    return r
                        .response_result
                        .map_err(|e| format!("rust-analyzer {method}: {e:?}").into());
                }
                Ok(_) => continue,
                Err(e) => return Err(format!("rust-analyzer {method}: channel {e}").into()),
            }
        }
    }

    pub(super) fn definition(
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
    pub(super) fn shutdown(self) {
        let id = self.next_id();
        let _ = self
            .tx
            .send(Request::new(id, "shutdown".into(), Value::Null).into());
        let _ = self
            .tx
            .send(Notification::new("exit".into(), Value::Null).into());
    }
}
