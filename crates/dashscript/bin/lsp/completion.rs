//! `.ds`-layer completion (`textDocument/completion`).
//!
//! rust-analyzer completes the *translated* Rust (`::std::println!`), not the
//! `.ds` semantics a developer types (`console`, `Math`). DashScript therefore
//! provides its own `.ds` completion: global built-ins + user declarations, and
//! the members of a builtin namespace after `.`.
//!
//! Both the global list and each namespace's members derive from the single
//! `BUILTINS` table in [`super::builtins`] — there is no parallel hand-written
//! member table here. The drift-guard test there asserts every entry actually
//! translates, so completion can never offer a name the translator rejects.

use dashscript::Translator;
use lsp_types::{CompletionItem, CompletionItemKind, CompletionList, CompletionParams};
use serde_json::Value;

use super::builtins::{self, BuiltinKind};
use super::text;

impl super::Server {
    pub(super) fn on_completion(&self, params: &CompletionParams) -> Option<Value> {
        let tdp = &params.text_document_position;
        let text = self.docs.get(tdp.text_document.uri.as_str())?;
        let byte = text::position_to_byte(text, tdp.position)?;
        let items = match completion_context(text, byte) {
            Context::Member(receiver) => member_items(&receiver),
            Context::Global => global_items(text),
        };
        serde_json::to_value(CompletionList {
            is_incomplete: false,
            items,
        })
        .ok()
    }
}

/// What completion should offer at `byte`: the members of a builtin namespace
/// (`console.<…>`), or the global scope (everything in scope at top level).
enum Context {
    Member(String),
    Global,
}

/// Inspect the text before the cursor. A `.` immediately preceding the partial
/// identifier (or the cursor itself) means member completion of whatever name
/// sits before the dot; otherwise it is global completion.
fn completion_context(text: &str, byte: usize) -> Context {
    let bytes = text.as_bytes();
    // Skip the partial identifier the user is still typing.
    let mut prefix_start = byte.min(bytes.len());
    while prefix_start > 0 && text::is_ident_byte(bytes[prefix_start - 1]) {
        prefix_start -= 1;
    }
    if prefix_start > 0 && bytes[prefix_start - 1] == b'.' {
        let recv_end = prefix_start - 1;
        let mut recv_start = recv_end;
        while recv_start > 0 && text::is_ident_byte(bytes[recv_start - 1]) {
            recv_start -= 1;
        }
        if recv_start < recv_end {
            return Context::Member(text[recv_start..recv_end].to_string());
        }
    }
    Context::Global
}

/// Global-scope completions: DashScript builtin namespaces + plain globals +
/// every top-level declaration in the document. The client filters by the
/// typed prefix; the server returns the full set.
fn global_items(text: &str) -> Vec<CompletionItem> {
    let mut items: Vec<CompletionItem> = Vec::new();
    // Namespaces (console, Math, …) — each appears once as a MODULE entry, in
    // first-seen order.
    let mut namespaces: Vec<String> = Vec::new();
    for b in builtins::all() {
        if let Some(ns) = &b.ns {
            if !namespaces.contains(ns) {
                namespaces.push(ns.clone());
                items.push(item(ns, CompletionItemKind::MODULE));
            }
        }
    }
    // Plain globals whose name is not also a namespace — `String`/`Number` are
    // namespaces (so their MODULE entry already covers them); `parseInt`,
    // `Boolean`, `undefined`, … are plain globals.
    for b in builtins::all().iter().filter(|b| b.ns.is_none()) {
        if !namespaces.contains(&b.name) {
            items.push(item(&b.name, global_kind(b.kind)));
        }
    }
    for decl in Translator::new().declarations(text) {
        items.push(item(&decl.name, CompletionItemKind::VARIABLE));
    }
    items
}

/// Members of the builtin namespace named by `receiver` (e.g. `console`).
/// Unknown receivers (a variable whose type is not a builtin) get nothing —
/// the translator's per-variable types are not surfaced to completion yet.
fn member_items(receiver: &str) -> Vec<CompletionItem> {
    builtins::all()
        .iter()
        .filter(|b| b.ns.as_deref() == Some(receiver))
        .map(|b| item(&b.name, member_kind(b.kind)))
        .collect()
}

fn item(label: &str, kind: CompletionItemKind) -> CompletionItem {
    CompletionItem {
        label: label.to_string(),
        kind: Some(kind),
        ..Default::default()
    }
}

/// A plain global's completion kind: a function (`parseInt`) or a constant
/// (`undefined`).
fn global_kind(kind: BuiltinKind) -> CompletionItemKind {
    match kind {
        BuiltinKind::Function => CompletionItemKind::FUNCTION,
        BuiltinKind::Const => CompletionItemKind::CONSTANT,
    }
}

/// A namespace member's completion kind: a method (`Math.round`) or a constant
/// (`Math.PI`). Methods render with the METHOD icon — a namespace member is a
/// method of that namespace, never a free function.
fn member_kind(kind: BuiltinKind) -> CompletionItemKind {
    match kind {
        BuiltinKind::Function => CompletionItemKind::METHOD,
        BuiltinKind::Const => CompletionItemKind::CONSTANT,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(items: &[CompletionItem]) -> Vec<&str> {
        items.iter().map(|i| i.label.as_str()).collect()
    }

    #[test]
    fn global_offers_builtins_and_declarations() {
        let text = "function greet() {}\ninterface Point {}\n";
        let items = global_items(text);
        let labels = labels(&items);
        assert!(labels.contains(&"console"), "missing console: {labels:?}");
        assert!(labels.contains(&"parseInt"), "missing parseInt: {labels:?}");
        assert!(labels.contains(&"greet"), "missing user fn: {labels:?}");
        assert!(labels.contains(&"Point"), "missing user type: {labels:?}");
    }

    #[test]
    fn console_dot_offers_log_warn_error() {
        let text = "console.";
        // cursor at end (byte == len) → member completion of `console`.
        let ctx = completion_context(text, text.len());
        assert!(matches!(ctx, Context::Member(ref r) if r == "console"));
        let items = member_items("console");
        let labels = labels(&items);
        assert!(labels.contains(&"log") && labels.contains(&"warn") && labels.contains(&"error"));
        // `console` maps exactly these three — no speculative extras.
        assert_eq!(labels.len(), 3, "console members drifted: {labels:?}");
    }

    #[test]
    fn math_dot_offers_constants_and_methods() {
        let items = member_items("Math");
        let labels = labels(&items);
        assert!(labels.contains(&"PI"), "missing PI: {labels:?}");
        assert!(labels.contains(&"round"), "missing round: {labels:?}");
        assert!(labels.contains(&"floor"), "missing floor: {labels:?}");
        assert!(labels.contains(&"cos"), "missing cos: {labels:?}");
    }

    #[test]
    fn number_dot_offers_constants_and_statics() {
        let items = member_items("Number");
        let labels = labels(&items);
        assert!(labels.contains(&"EPSILON"), "missing EPSILON: {labels:?}");
        assert!(labels.contains(&"isNaN"), "missing isNaN: {labels:?}");
        assert!(
            labels.contains(&"isInteger"),
            "missing isInteger: {labels:?}"
        );
        // Instance methods (toFixed) are NOT namespace members.
        assert!(!labels.contains(&"toFixed"));
    }

    #[test]
    fn array_dot_offers_statics() {
        let items = member_items("Array");
        let labels = labels(&items);
        assert!(labels.contains(&"from") && labels.contains(&"of") && labels.contains(&"isArray"));
    }

    #[test]
    fn object_dot_offers_statics() {
        let items = member_items("Object");
        let labels = labels(&items);
        assert!(labels.contains(&"keys") && labels.contains(&"entries") && labels.contains(&"is"));
    }

    #[test]
    fn unknown_receiver_offers_nothing() {
        assert!(member_items("localVar").is_empty());
    }

    #[test]
    fn member_context_after_partial_member() {
        // `console.lo` (cursor after `lo`) → still member completion of console.
        let text = "console.lo";
        let ctx = completion_context(text, text.len());
        assert!(matches!(ctx, Context::Member(ref r) if r == "console"));
    }

    #[test]
    fn global_context_without_dot() {
        let text = "cons";
        let ctx = completion_context(text, text.len());
        assert!(matches!(ctx, Context::Global));
    }
}
