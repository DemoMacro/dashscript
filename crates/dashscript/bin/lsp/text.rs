//! Text/position/URI helpers: byte offsets ↔ LSP `Position`, identifier
//! extraction, and cache-file resolution. Pure and unit-tested.

use std::{
    error::Error,
    path::{Path, PathBuf},
};

use lsp_types::{Position, Range, Uri};

/// Column (in characters) of the first whole-word occurrence of `word`.
pub(super) fn find_word_col(line: &str, word: &str) -> Option<u32> {
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
pub(super) fn word_at(text: &str, byte: usize) -> Option<String> {
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

pub(super) fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// An LSP `Position` (0-based line/character) → a `.ds` byte offset. The
/// character column is counted in Unicode scalars; `.ds` sources are ASCII
/// where this matters, so it agrees with the UTF-16 the protocol specifies.
pub(super) fn position_to_byte(text: &str, pos: Position) -> Option<usize> {
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
pub(super) fn byte_range(text: &str, offset: u32, len: u32) -> Range {
    let start = offset as usize;
    let end = start.saturating_add(len as usize).min(text.len());
    Range {
        start: byte_to_position(text, start),
        end: byte_to_position(text, end),
    }
}

fn byte_to_position(text: &str, byte_offset: usize) -> Position {
    let byte_offset = byte_offset.min(text.len());
    let prefix = &text[..byte_offset];
    let line = prefix.bytes().filter(|&b| b == b'\n').count() as u32;
    let line_start = prefix.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let character = text[line_start..byte_offset].chars().count() as u32;
    Position { line, character }
}

pub(super) fn uri_to_path(uri: &Uri) -> Option<PathBuf> {
    url::Url::parse(uri.as_str()).ok()?.to_file_path().ok()
}

/// The cache Rust file for a `.ds` source: `src/<stem>.rs` (project mode — one
/// Rust file per bin) when present, else `src/main.rs` (lone-file mode). Lets
/// the language server point rust-analyzer at the right file in either mode.
pub(super) fn rust_file_for(cache: &Path, src_path: &Path) -> PathBuf {
    let stem = src_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("main");
    let project_rs = cache.join("src").join(format!("{stem}.rs"));
    if project_rs.exists() {
        project_rs
    } else {
        cache.join("src").join("main.rs")
    }
}

pub(super) fn path_to_uri(path: &Path) -> Result<Uri, Box<dyn Error>> {
    let url = url::Url::from_file_path(path)
        .map_err(|_| format!("not an absolute file path: {}", path.display()))?;
    Ok(url.as_str().parse::<Uri>()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn rust_file_for_prefers_project_stem() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = tmp.path();
        std::fs::create_dir_all(cache.join("src")).unwrap();
        std::fs::write(cache.join("src").join("numbers.rs"), "").unwrap();
        assert_eq!(
            rust_file_for(cache, Path::new("numbers.ds")),
            cache.join("src").join("numbers.rs")
        );
    }

    #[test]
    fn rust_file_for_falls_back_to_main_rs() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = tmp.path();
        std::fs::create_dir_all(cache.join("src")).unwrap();
        // No src/<stem>.rs → lone-file mode picks src/main.rs.
        assert_eq!(
            rust_file_for(cache, Path::new("numbers.ds")),
            cache.join("src").join("main.rs")
        );
    }

    #[test]
    fn position_tracks_lines_and_columns() {
        let text = "abc\ndef\nghi";
        assert_eq!(
            byte_to_position(text, 0),
            Position {
                line: 0,
                character: 0
            }
        );
        assert_eq!(
            byte_to_position(text, 3),
            Position {
                line: 0,
                character: 3
            }
        );
        assert_eq!(
            byte_to_position(text, 4),
            Position {
                line: 1,
                character: 0
            }
        );
        assert_eq!(
            byte_to_position(text, 8),
            Position {
                line: 2,
                character: 0
            }
        );
    }

    #[test]
    fn range_from_byte_span() {
        let text = "hello\nworld";
        // "world" spans bytes 6..11 → line 1, characters 0..5.
        let range = byte_range(text, 6, 5);
        assert_eq!(
            range.start,
            Position {
                line: 1,
                character: 0
            }
        );
        assert_eq!(
            range.end,
            Position {
                line: 1,
                character: 5
            }
        );
    }

    #[test]
    fn range_clamps_past_end_of_text() {
        let range = byte_range("ab", 0, 100);
        assert_eq!(
            range.end,
            Position {
                line: 0,
                character: 2
            }
        );
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
