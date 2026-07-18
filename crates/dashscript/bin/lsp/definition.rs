//! Go-to-definition: crate imports are forwarded to rust-analyzer (it resolves
//! `use crate::Symbol` to the crate's `~/.cargo` source); in-file references are
//! resolved locally against `Translator::declarations`.

use dashscript::Translator;
use lsp_types::{GotoDefinitionParams, GotoDefinitionResponse, Location, Position, Uri};
use serde_json::Value;

use super::{text, Server};

impl Server {
    /// Map a definition request to the crate source: locate the import
    /// specifier under the cursor, emit the Cargo project, map the symbol to
    /// its `use` position in `main.rs`, and let rust-analyzer resolve it.
    pub(super) fn on_definition(&mut self, params: &GotoDefinitionParams) -> Option<Value> {
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
        symbol: Option<String>,
    ) -> Option<Value> {
        self.refresh(uri, text);
        let cache = self.cache_dir(uri)?;
        let src_path = text::uri_to_path(uri)?;
        let main_rs = text::rust_file_for(&cache, &src_path);
        let main_text = std::fs::read_to_string(&main_rs).ok()?;
        // A symbol → its position in `use module::symbol`; no symbol (cursor on
        // the crate name) → the crate's own position, so RA resolves the root.
        let rust_pos = match &symbol {
            Some(sym) => map_symbol_pos(&main_text, &module, sym)?,
            None => map_module_pos(&main_text, &module)?,
        };
        let main_uri = text::path_to_uri(&main_rs).ok()?;
        let ra = self.ra.as_ref()?;
        let resp = ra.definition(main_uri.as_str(), rust_pos).ok()?;
        serde_json::to_value(resp).ok()
    }

    /// Resolve an in-file reference to a local declaration — a function, type,
    /// interface, or import binding — in the same `.ds` document.
    fn definition_local(&self, uri: &Uri, text: &str, pos: Position) -> Option<Value> {
        let byte = text::position_to_byte(text, pos)?;
        let word = text::word_at(text, byte)?;
        let decl = Translator::new()
            .declarations(text)
            .into_iter()
            .find(|d| d.name == word)?;
        let range = text::byte_range(text, decl.span.start, decl.span.end - decl.span.start);
        serde_json::to_value(GotoDefinitionResponse::Scalar(Location {
            uri: uri.clone(),
            range,
        }))
        .ok()
    }
}

/// If the cursor sits on a bare-crate import specifier (`import { X } from
/// "crate"`), return the crate module ident and the symbol name as written in
/// the emitted `use crate::X`.
fn locate_import(text: &str, pos: Position) -> Option<(String, Option<String>)> {
    let byte = text::position_to_byte(text, pos)?;
    for imp in Translator::new().crate_imports(text) {
        // A specifier under the cursor → resolve that symbol.
        for sym in &imp.symbols {
            if byte >= sym.span.start as usize && byte <= sym.span.end as usize {
                return Some((imp.module.clone(), Some(sym.name.clone())));
            }
        }
        // The import source string under the cursor → resolve the crate root.
        if byte >= imp.source_span.start as usize && byte <= imp.source_span.end as usize {
            return Some((imp.module.clone(), None));
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
        if let Some(col) = text::find_word_col(line, symbol) {
            return Some(Position {
                line: line_idx as u32,
                character: col,
            });
        }
    }
    None
}

/// Find the position of the crate (module) name within the emitted
/// `use <module>::…` line — forwarded to rust-analyzer to resolve the crate
/// root (its `lib.rs`), for a go-to-definition on the import source string.
fn map_module_pos(main_rs: &str, module: &str) -> Option<Position> {
    let needle = format!("{module}::");
    for (line_idx, line) in main_rs.lines().enumerate() {
        if line.trim_start().starts_with("use ") && line.contains(&needle) {
            if let Some(col) = text::find_word_col(line, module) {
                return Some(Position {
                    line: line_idx as u32,
                    character: col,
                });
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsp_types::Position;

    #[test]
    fn locate_import_resolves_named_specifier() {
        let text = "import { Adler32 } from \"adler\";";
        // `Adler32` starts at character 9 on line 0.
        let (module, symbol) = locate_import(
            text,
            Position {
                line: 0,
                character: 9,
            },
        )
        .unwrap();
        assert_eq!(module, "adler");
        assert_eq!(symbol.as_deref(), Some("Adler32"));
    }

    #[test]
    fn map_symbol_pos_finds_use_clause() {
        // The translator emits `use adler::Adler32;` — `Adler32` begins at
        // column 11 (`use adler::` is 11 characters).
        let main_rs = "use adler::Adler32;\n\nfn main() {}\n";
        let pos = map_symbol_pos(main_rs, "adler", "Adler32").unwrap();
        assert_eq!(
            pos,
            Position {
                line: 0,
                character: 11
            }
        );
    }

    #[test]
    fn map_symbol_pos_whole_word_only() {
        // `Adler` is a prefix of `Adler32` — it must not match.
        assert!(map_symbol_pos("use adler::Adler32;\n", "adler", "Adler").is_none());
    }

    #[test]
    fn locate_import_resolves_crate_root_on_source_string() {
        let text = "import { Adler32 } from \"adler\";";
        // Cursor inside the `"adler"` source string (character 27 = the `l`).
        let (module, symbol) = locate_import(
            text,
            Position {
                line: 0,
                character: 27,
            },
        )
        .unwrap();
        assert_eq!(module, "adler");
        assert!(
            symbol.is_none(),
            "expected no symbol (crate root): {symbol:?}"
        );
    }

    #[test]
    fn map_module_pos_finds_crate_name() {
        // `use adler::Adler32;` — `adler` begins at column 4 (`use ` is 4 chars).
        let pos = map_module_pos("use adler::Adler32;\n", "adler").unwrap();
        assert_eq!(
            pos,
            Position {
                line: 0,
                character: 4
            }
        );
    }
}
