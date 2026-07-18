//! `textDocument/signatureHelp`: parameter hints while typing inside a call.
//!
//! Two pure pieces do the work: [`enclosing_call`] walks backward from the
//! cursor to find the callee name and the **active parameter index**
//! (skipping commas inside nested `()`/`[]`/`{}`, so `foo(a, bar(b, c), d)`
//! with the cursor on `d` reports index 2, not 3); [`param_offsets`] splits a
//! signature label into per-parameter `[start, end)` ranges so the client can
//! highlight the active one. The callee is then resolved to a builtin doc
//! signature or a user function's `.ds` signature.

use dashscript::Translator;
use lsp_types::{
    Documentation, MarkupContent, MarkupKind, ParameterInformation, ParameterLabel, SignatureHelp,
    SignatureHelpParams, SignatureInformation,
};
use serde_json::Value;

use super::{builtins, text, Server};

impl Server {
    pub(super) fn on_signature_help(&self, params: &SignatureHelpParams) -> Option<Value> {
        let tdp = &params.text_document_position_params;
        let text = self.docs.get(tdp.text_document.uri.as_str())?;
        let byte = text::position_to_byte(text, tdp.position)?;
        let (callee, active) = enclosing_call(text, byte)?;
        let mut info = builtin_signature(&callee).or_else(|| user_signature(text, &callee))?;
        info.active_parameter = Some(active as u32);
        let sig = SignatureHelp {
            signatures: vec![info],
            active_signature: Some(0),
            active_parameter: Some(active as u32),
        };
        serde_json::to_value(sig).ok()
    }
}

/// Build a `SignatureInformation` for a builtin callee (`Math.round`), or
/// `None` for undocumented builtins or constants (constants take no arguments).
fn builtin_signature(callee: &str) -> Option<SignatureInformation> {
    let b = builtins::lookup(callee)?;
    if b.kind != builtins::BuiltinKind::Function {
        return None;
    }
    let label = format!("{callee}{sig}", sig = b.sig);
    Some(make_signature(&label, Some(b.doc)))
}

/// Build a `SignatureInformation` for a user function named `callee` (a bare
/// identifier — method calls like `obj.m()` are not resolved yet).
fn user_signature(text: &str, callee: &str) -> Option<SignatureInformation> {
    let sym = Translator::new()
        .declarations(text)
        .into_iter()
        .find(|d| d.name == callee && d.signature.is_some())?;
    let sig = sym.signature.as_ref()?;
    let label = format!("function {}{}", sym.name, sig.label());
    Some(make_signature(&label, None))
}

/// Wrap a signature label + optional doc into a `SignatureInformation` whose
/// parameters are `[start, end)` offsets into the label (so the client
/// highlights the active parameter). `active_parameter` is left unset — the
/// caller sets it per request.
fn make_signature(label: &str, doc: Option<&str>) -> SignatureInformation {
    let parameters: Vec<ParameterInformation> = param_offsets(label)
        .into_iter()
        .map(|(start, end)| ParameterInformation {
            label: ParameterLabel::LabelOffsets([start, end]),
            documentation: None,
        })
        .collect();
    SignatureInformation {
        label: label.to_string(),
        documentation: doc.map(|d| {
            Documentation::MarkupContent(MarkupContent {
                kind: MarkupKind::Markdown,
                value: d.to_string(),
            })
        }),
        parameters: Some(parameters),
        active_parameter: None,
    }
}

/// Walk backward from `cursor` to the enclosing call: returns the callee's
/// qualified name (`Math.round`, `greet`) and the 0-based index of the
/// parameter the cursor sits in. `None` when the cursor is not inside an
/// argument list (or is inside `[]`/`{}`, not a call).
fn enclosing_call(text: &str, cursor: usize) -> Option<(String, usize)> {
    let bytes = text.as_bytes();
    let mut depth = 0i32;
    let mut active = 0usize;
    let mut i = cursor.min(bytes.len());
    while i > 0 {
        i -= 1;
        match bytes[i] {
            b')' | b']' | b'}' => depth += 1,
            b'(' | b'[' | b'{' => {
                if depth == 0 {
                    if bytes[i] != b'(' {
                        return None; // cursor inside [] or {}, not a call's args
                    }
                    let callee = read_callee(bytes, i)?;
                    return Some((callee, active));
                }
                depth -= 1;
            }
            b',' if depth == 0 => active += 1,
            _ => {}
        }
    }
    None
}

/// The callee ending just before the `(` at `paren`: one or more identifiers
/// joined by `.` (`Math.round`, `console.log`, `greet`). Returns `None` if
/// nothing identifiable sits there (a number, an operator, …).
fn read_callee(bytes: &[u8], paren: usize) -> Option<String> {
    let mut tail = paren;
    while tail > 0 && bytes[tail - 1].is_ascii_whitespace() {
        tail -= 1;
    }
    let mut parts: Vec<&str> = Vec::new();
    let mut cursor = tail;
    loop {
        let mut start = cursor;
        while start > 0 && text::is_ident_byte(bytes[start - 1]) {
            start -= 1;
        }
        if start == cursor {
            break; // no identifier segment here
        }
        parts.push(std::str::from_utf8(&bytes[start..cursor]).ok()?);
        // A `.` (with optional surrounding whitespace) chains to the next ident.
        let mut dot = start;
        while dot > 0 && bytes[dot - 1].is_ascii_whitespace() {
            dot -= 1;
        }
        if dot > 0 && bytes[dot - 1] == b'.' {
            cursor = dot - 1;
            continue;
        }
        break;
    }
    if parts.is_empty() {
        return None;
    }
    parts.reverse();
    Some(parts.join("."))
}

/// The `[start, end)` ranges of each parameter within a signature `label`
/// (`Math.round(x: number): number`, `function greet(a: T, b: U): R`). Splits
/// on top-level commas only — nested `()`/`[]`/`<>` are kept intact, so
/// `(cb: (x: number) => void)` is one parameter. Empty for a no-arg signature.
fn param_offsets(label: &str) -> Vec<(u32, u32)> {
    let bytes = label.as_bytes();
    let Some(open) = label.find('(') else {
        return Vec::new();
    };
    let mut depth = 0i32;
    let mut close = None;
    for (i, &b) in bytes.iter().enumerate().skip(open) {
        match b {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }
    let Some(close) = close else {
        return Vec::new();
    };
    let mut offsets = Vec::new();
    let mut depth = 0i32;
    let mut seg_start = open + 1;
    for i in (open + 1)..close {
        match bytes[i] {
            b'(' | b'[' | b'<' => depth += 1,
            b')' | b']' | b'>' => depth -= 1,
            b',' if depth == 0 => {
                push_trimmed(bytes, &mut offsets, seg_start, i);
                seg_start = i + 1;
            }
            _ => {}
        }
    }
    push_trimmed(bytes, &mut offsets, seg_start, close);
    offsets
}

/// Push the whitespace-trimmed `[start, end)` of one parameter segment.
fn push_trimmed(bytes: &[u8], out: &mut Vec<(u32, u32)>, start: usize, end: usize) {
    let mut s = start;
    while s < end && bytes[s].is_ascii_whitespace() {
        s += 1;
    }
    let mut e = end;
    while e > s && bytes[e - 1].is_ascii_whitespace() {
        e -= 1;
    }
    if s < e {
        out.push((s as u32, e as u32));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enclosing_call_second_argument() {
        // cursor on `b` (byte 8) → active index 1.
        assert_eq!(enclosing_call("foo(a, b)", 8), Some(("foo".to_string(), 1)));
    }

    #[test]
    fn enclosing_call_at_open_paren() {
        // cursor right after `(` → active index 0.
        assert_eq!(
            enclosing_call("Math.round(", 11),
            Some(("Math.round".to_string(), 0))
        );
    }

    #[test]
    fn enclosing_call_skips_nested_call_commas() {
        // foo(a, bar(b, c), d) — cursor on `d` (byte 18) → index 2 (bar's
        // inner comma does not count).
        assert_eq!(
            enclosing_call("foo(a, bar(b, c), d)", 18),
            Some(("foo".to_string(), 2))
        );
    }

    #[test]
    fn enclosing_call_skips_array_literal_commas() {
        // foo(a, [1, 2], b) — cursor on `b` (byte 15) → index 2.
        assert_eq!(
            enclosing_call("foo(a, [1, 2], b)", 15),
            Some(("foo".to_string(), 2))
        );
    }

    #[test]
    fn enclosing_call_none_outside_a_call() {
        assert!(enclosing_call("let x = 1;", 4).is_none());
    }

    #[test]
    fn enclosing_call_none_inside_object_literal() {
        // cursor inside `{ … }` is not a call argument list.
        assert!(enclosing_call("let o = { a: 1 }", 10).is_none());
    }

    #[test]
    fn read_callee_qualified_name() {
        // `Math.round(` — the `(` is at byte 10.
        assert_eq!(
            read_callee("Math.round(".as_bytes(), 10).as_deref(),
            Some("Math.round")
        );
    }

    #[test]
    fn read_callee_handles_space_before_paren() {
        // `foo (` — the `(` is at byte 4.
        assert_eq!(read_callee("foo (".as_bytes(), 4).as_deref(), Some("foo"));
    }

    #[test]
    fn param_offsets_single_argument() {
        // `x: number` spans label bytes 11..20.
        let offsets = param_offsets("Math.round(x: number): number");
        assert_eq!(offsets, vec![(11, 20)]);
    }

    #[test]
    fn param_offsets_multiple_arguments() {
        let offsets = param_offsets("add(a: number, b: number): number");
        // `a: number` occupies bytes 4..13, `b: number` 15..24.
        assert_eq!(offsets, vec![(4, 13), (15, 24)]);
    }

    #[test]
    fn param_offsets_empty_for_no_args() {
        assert!(param_offsets("now(): number").is_empty());
    }

    #[test]
    fn param_offsets_keeps_nested_parens_intact() {
        // `(cb: (x: number) => void)` is one parameter.
        let offsets = param_offsets("each(cb: (x: number) => void): void");
        assert_eq!(offsets.len(), 1);
    }

    #[test]
    fn builtin_signature_renders_function() {
        let info = builtin_signature("Math.round").expect("Math.round sig");
        assert_eq!(info.label, "Math.round(x: number): number");
        assert_eq!(info.parameters.as_ref().map(Vec::len), Some(1));
        assert!(info.documentation.is_some());
    }

    #[test]
    fn builtin_signature_none_for_constant() {
        // Math.PI is a constant — no signature help.
        assert!(builtin_signature("Math.PI").is_none());
    }

    #[test]
    fn user_signature_renders_function_label() {
        let text = "function greet(name: string, times?: number): string { return name; }";
        let info = user_signature(text, "greet").expect("greet sig");
        assert_eq!(
            info.label,
            "function greet(name: string, times?: number): string"
        );
        assert_eq!(info.parameters.as_ref().map(Vec::len), Some(2));
    }

    #[test]
    fn user_signature_none_for_non_function() {
        // `Point` is an interface — no signature.
        let text = "interface Point { x: number }";
        assert!(user_signature(text, "Point").is_none());
    }
}
