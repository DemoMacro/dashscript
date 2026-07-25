//! Hover: `cargo:` import symbols are forwarded to rust-analyzer, which
//! returns the type/doc markdown resolved from the crate's `~/.cargo` source.
//! In-file (TS) symbols are left to the editor's TS LSP — `on_hover` returns
//! `None` unless the cursor is on a `cargo:` import, so VS Code falls back to
//! the TS LSP hover for everything else. This preserves the
//! zero-stub model: no `.d.ts`, crate types come straight from RA.

use lsp_types::HoverParams;
use serde_json::Value;

use super::{definition::locate_import, Server};

impl Server {
    /// Hover a crate import symbol via rust-analyzer. Returns `None` for any
    /// non-`cargo:` position, so the editor's TS LSP owns the hover there.
    pub(super) fn on_hover(&mut self, params: &HoverParams) -> Option<Value> {
        let tdp = &params.text_document_position_params;
        let uri = &tdp.text_document.uri;
        let text = self.docs.get(uri.as_str())?.clone();
        let (module, symbol) = locate_import(&text, tdp.position)?;
        let (main_uri, rust_pos) =
            self.emitted_rust_position(uri, &text, &module, symbol.as_deref())?;
        let ra = self.ra.as_ref()?;
        ra.hover(main_uri.as_str(), rust_pos).ok()
    }
}
