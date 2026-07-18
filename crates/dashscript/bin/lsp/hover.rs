//! `textDocument/hover`: show type/signature/docs on hover. Three sources,
//! tried in order — (1) a crate import symbol → forwarded to rust-analyzer
//! (the crate's own Rust types are the source of truth); (2) a DashScript
//! builtin (`console.log`, `Math.round`, …) → the builtin doc table;
//! (3) a user declaration → its `.ds` signature from `LocalSymbol`.

use dashscript::translator::imports::LocalSymbol;
use dashscript::translator::semantic::SymbolKind as DsSymbolKind;
use dashscript::Translator;
use lsp_types::{Hover, HoverContents, HoverParams, MarkupContent, MarkupKind, Uri};
use serde_json::Value;

use super::{builtins, text, Server};

impl Server {
    pub(super) fn on_hover(&mut self, params: &HoverParams) -> Option<Value> {
        let tdp = &params.text_document_position_params;
        let uri = &tdp.text_document.uri;
        let text = self.docs.get(uri.as_str())?.clone();
        let byte = text::position_to_byte(&text, tdp.position)?;

        // (1) crate import → rust-analyzer. The crate's Rust types are the
        // truth, so we surface RA's own hover verbatim.
        if let Some((module, symbol)) = super::definition::locate_import(&text, tdp.position) {
            if let Some(hover) = self.hover_via_ra(uri, &text, module, symbol) {
                return Some(hover);
            }
        }

        // (2) builtin → doc table. (3) user declaration → .ds signature.
        let markdown = builtin_hover(&text, byte).or_else(|| user_hover(&text, byte))?;
        serde_json::to_value(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: markdown,
            }),
            range: None,
        })
        .ok()
    }

    /// Forward a crate-symbol hover to rust-analyzer: refresh the cache
    /// project, map the symbol to its `use` position in the emitted Rust, and
    /// request hover there. `None` if RA has nothing to say.
    fn hover_via_ra(
        &mut self,
        uri: &Uri,
        text: &str,
        module: String,
        symbol: Option<String>,
    ) -> Option<Value> {
        self.refresh(uri, text);
        let cache = self.cache_dir(uri)?;
        let src_path = text::uri_to_path(uri)?;
        let main_rs = text::rust_file_for(&cache, &src_path);
        let main_text = std::fs::read_to_string(&main_rs).ok()?;
        let rust_pos = match &symbol {
            Some(sym) => super::definition::map_symbol_pos(&main_text, &module, sym)?,
            None => super::definition::map_module_pos(&main_text, &module)?,
        };
        let main_uri = text::path_to_uri(&main_rs).ok()?;
        let ra = self.ra.as_ref()?;
        let value = ra
            .request(
                "textDocument/hover",
                serde_json::json!({
                    "textDocument": { "uri": main_uri.as_str() },
                    "position": rust_pos,
                }),
            )
            .ok()?;
        if value.is_null() {
            return None;
        }
        Some(value)
    }
}

/// Hover for a DashScript builtin: the qualified name under the cursor
/// (`Math.round`, `parseInt`) looked up in the builtin doc table.
fn builtin_hover(text: &str, byte: usize) -> Option<String> {
    let qualified = qualified_at(text, byte)?;
    let b = builtins::lookup(&qualified)?;
    let header = match b.kind {
        builtins::BuiltinKind::Function => format!("{qualified}{sig}", sig = b.sig),
        builtins::BuiltinKind::Const => format!("const {qualified}: {sig}", sig = b.sig),
    };
    Some(format!("```ts\n{header}\n```\n\n{doc}", doc = b.doc))
}

/// Hover for a user-declared symbol: its `.ds` signature (functions) or kind
/// header (interface/type/class/variable). Imports get no hover.
fn user_hover(text: &str, byte: usize) -> Option<String> {
    let word = text::word_at(text, byte)?;
    let sym = Translator::new()
        .declarations(text)
        .into_iter()
        .find(|d| d.name == word)?;
    render_symbol(&sym, text)
}

/// Format a `LocalSymbol` as a markdown hover header, or `None` for kinds with
/// no useful hover (imports).
fn render_symbol(sym: &LocalSymbol, text: &str) -> Option<String> {
    let body = match sym.kind {
        DsSymbolKind::Function => {
            let sig = sym
                .signature
                .as_ref()
                .map(|s| s.label())
                .unwrap_or_else(|| "(): void".to_string());
            format!("function {}{}", sym.name, sig)
        }
        // Show the full declaration source — `interface Point { x: number }`
        // — matching how TS surfaces a type on hover, not just the header.
        DsSymbolKind::Interface | DsSymbolKind::TypeAlias => match sym.decl_span {
            Some(s) => text[s.start as usize..s.end as usize].to_string(),
            None => type_header(sym),
        },
        DsSymbolKind::Class => format!("class {}", sym.name),
        DsSymbolKind::Variable => format!("let {}", sym.name),
        DsSymbolKind::Other => return None, // import — no hover
    };
    Some(format!("```ts\n{body}\n```"))
}

/// Fallback `interface X` / `type X` header when no declaration span exists.
fn type_header(sym: &LocalSymbol) -> String {
    let kw = match sym.kind {
        DsSymbolKind::Interface => "interface",
        DsSymbolKind::TypeAlias => "type",
        _ => "type",
    };
    format!("{kw} {}", sym.name)
}

/// The qualified identifier under `byte` — `Math.round` when the cursor sits
/// on a member preceded by `<ident>.`, otherwise the bare word (`parseInt`).
/// `None` when the cursor is not on an identifier at all.
fn qualified_at(text: &str, byte: usize) -> Option<String> {
    let bytes = text.as_bytes();
    if byte >= bytes.len() || !text::is_ident_byte(bytes[byte]) {
        return None;
    }
    let mut start = byte;
    while start > 0 && text::is_ident_byte(bytes[start - 1]) {
        start -= 1;
    }
    let mut end = byte;
    while end < bytes.len() && text::is_ident_byte(bytes[end]) {
        end += 1;
    }
    let member = &text[start..end];
    // A preceding `.` + receiver → qualify it (`Math.round`).
    if start > 0 && bytes[start - 1] == b'.' {
        let recv_end = start - 1;
        let mut recv_start = recv_end;
        while recv_start > 0 && text::is_ident_byte(bytes[recv_start - 1]) {
            recv_start -= 1;
        }
        if recv_start < recv_end {
            return Some(format!("{}.{}", &text[recv_start..recv_end], member));
        }
    }
    Some(member.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qualified_at_member_after_dot() {
        // `Math.round` — cursor on `r` (byte 5).
        assert_eq!(qualified_at("Math.round", 5).as_deref(), Some("Math.round"));
    }

    #[test]
    fn qualified_at_bare_global() {
        // `parseInt` — cursor on `p` (byte 0).
        assert_eq!(qualified_at("parseInt", 0).as_deref(), Some("parseInt"));
    }

    #[test]
    fn qualified_at_none_on_punctuation() {
        assert!(qualified_at("a.b", 1).is_none()); // `.` is not an ident
    }

    #[test]
    fn builtin_hover_renders_function_signature_and_doc() {
        // `Math.round` — cursor on `round` (byte 5).
        let md = builtin_hover("Math.round", 5).expect("hover");
        assert!(
            md.contains("Math.round(x: number): number"),
            "missing sig: {md}"
        );
        assert!(md.contains("nearest"), "missing doc: {md}");
    }

    #[test]
    fn builtin_hover_renders_constant_as_const() {
        // `Math.PI` — cursor on `PI` (byte 5).
        let md = builtin_hover("Math.PI", 5).expect("hover");
        assert!(
            md.contains("const Math.PI: number"),
            "missing const header: {md}"
        );
    }

    #[test]
    fn builtin_hover_none_for_unknown() {
        // A name the standard library does not declare — no hover (no guess).
        assert!(builtin_hover("Math.nonexistent", 5).is_none());
    }

    #[test]
    fn user_hover_shows_function_signature() {
        let text = "function greet(name: string): string { return name; }";
        // `greet` starts at byte 9.
        let md = user_hover(text, 9).expect("hover");
        assert!(
            md.contains("function greet(name: string): string"),
            "missing sig: {md}"
        );
    }

    #[test]
    fn user_hover_shows_full_interface_definition() {
        let text = "interface Point { x: number; y: number }";
        // `Point` starts at byte 10.
        let md = user_hover(text, 10).expect("hover");
        assert!(md.contains("interface Point"), "missing header: {md}");
        // The full definition — both fields — not just the header.
        assert!(md.contains("x: number"), "missing field x: {md}");
        assert!(md.contains("y: number"), "missing field y: {md}");
    }

    #[test]
    fn user_hover_shows_full_type_alias_definition() {
        let text = "type Id = number;";
        // `Id` starts at byte 5.
        let md = user_hover(text, 5).expect("hover");
        assert!(md.contains("type Id = number"), "missing full alias: {md}");
    }

    #[test]
    fn user_hover_none_for_import() {
        let text = "import { foo } from \"./other\";";
        // `foo` starts at byte 9.
        assert!(user_hover(text, 9).is_none());
    }
}
