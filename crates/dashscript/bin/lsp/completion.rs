//! `.ds`-layer completion (`textDocument/completion`).
//!
//! rust-analyzer completes the *translated* Rust (`::std::println!`), not the
//! `.ds` semantics a developer types (`console`, `Math`). DashScript therefore
//! provides its own `.ds` completion: global built-ins + user declarations, and
//! the members of a builtin namespace after `.`.
//!
//! The completion tables mirror the methods/constants the translator actually
//! maps — keep them in sync with `translator/builtins/{console,math,number,
//! string,array,object,global}.rs`. (A future stage will derive them from those
//! match arms so this table cannot drift.)

use dashscript::Translator;
use lsp_types::{CompletionItem, CompletionItemKind, CompletionList, CompletionParams};
use serde_json::Value;

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

/// Global-scope completions: DashScript builtin globals + every top-level
/// declaration in the document. The client filters by the typed prefix; the
/// server returns the full set.
fn global_items(text: &str) -> Vec<CompletionItem> {
    let mut items: Vec<CompletionItem> = GLOBALS
        .iter()
        .map(|&(label, kind)| item(label, kind))
        .collect();
    for decl in Translator::new().declarations(text) {
        items.push(item(&decl.name, CompletionItemKind::VARIABLE));
    }
    items
}

/// Members of the builtin namespace named by `receiver` (e.g. `console`).
/// Unknown receivers (a variable whose type is not a builtin) get nothing —
/// the translator's per-variable types are not surfaced to completion yet.
fn member_items(receiver: &str) -> Vec<CompletionItem> {
    let members: &[&[(&str, CompletionItemKind)]] = match receiver {
        "console" => &[CONSOLE_MEMBERS],
        "Math" => &[MATH_CONSTANTS, MATH_METHODS],
        "Number" => &[NUMBER_CONSTANTS, NUMBER_STATIC],
        "String" => &[STRING_STATIC],
        "Array" => &[ARRAY_STATIC],
        "Object" => &[OBJECT_STATIC],
        _ => return Vec::new(),
    };
    members
        .iter()
        .flat_map(|slice| slice.iter().copied())
        .map(|(label, kind)| item(label, kind))
        .collect()
}

fn item(label: &str, kind: CompletionItemKind) -> CompletionItem {
    CompletionItem {
        label: label.to_string(),
        kind: Some(kind),
        ..Default::default()
    }
}

// === builtin tables (mirror translator/builtins/*.rs mapped members) ===

type Member = (&'static str, CompletionItemKind);

const GLOBALS: &[Member] = &[
    ("console", CompletionItemKind::MODULE),
    ("Math", CompletionItemKind::MODULE),
    ("Number", CompletionItemKind::MODULE),
    ("String", CompletionItemKind::MODULE),
    ("Object", CompletionItemKind::MODULE),
    ("Array", CompletionItemKind::MODULE),
    ("parseInt", CompletionItemKind::FUNCTION),
    ("parseFloat", CompletionItemKind::FUNCTION),
    ("isNaN", CompletionItemKind::FUNCTION),
    ("isFinite", CompletionItemKind::FUNCTION),
    ("Boolean", CompletionItemKind::FUNCTION),
    ("undefined", CompletionItemKind::CONSTANT),
    ("Infinity", CompletionItemKind::CONSTANT),
    ("NaN", CompletionItemKind::CONSTANT),
];

const CONSOLE_MEMBERS: &[Member] = &[
    ("log", CompletionItemKind::METHOD),
    ("warn", CompletionItemKind::METHOD),
    ("error", CompletionItemKind::METHOD),
];

const MATH_CONSTANTS: &[Member] = &[
    ("PI", CompletionItemKind::CONSTANT),
    ("E", CompletionItemKind::CONSTANT),
    ("LN10", CompletionItemKind::CONSTANT),
    ("LN2", CompletionItemKind::CONSTANT),
    ("LOG10E", CompletionItemKind::CONSTANT),
    ("LOG2E", CompletionItemKind::CONSTANT),
    ("SQRT2", CompletionItemKind::CONSTANT),
    ("SQRT1_2", CompletionItemKind::CONSTANT),
];

const MATH_METHODS: &[Member] = &[
    ("abs", CompletionItemKind::FUNCTION),
    ("round", CompletionItemKind::FUNCTION),
    ("floor", CompletionItemKind::FUNCTION),
    ("ceil", CompletionItemKind::FUNCTION),
    ("trunc", CompletionItemKind::FUNCTION),
    ("sqrt", CompletionItemKind::FUNCTION),
    ("cbrt", CompletionItemKind::FUNCTION),
    ("exp", CompletionItemKind::FUNCTION),
    ("expm1", CompletionItemKind::FUNCTION),
    ("log", CompletionItemKind::FUNCTION),
    ("log2", CompletionItemKind::FUNCTION),
    ("log10", CompletionItemKind::FUNCTION),
    ("log1p", CompletionItemKind::FUNCTION),
    ("pow", CompletionItemKind::FUNCTION),
    ("sign", CompletionItemKind::FUNCTION),
    ("sin", CompletionItemKind::FUNCTION),
    ("cos", CompletionItemKind::FUNCTION),
    ("tan", CompletionItemKind::FUNCTION),
    ("asin", CompletionItemKind::FUNCTION),
    ("acos", CompletionItemKind::FUNCTION),
    ("atan", CompletionItemKind::FUNCTION),
    ("atan2", CompletionItemKind::FUNCTION),
    ("sinh", CompletionItemKind::FUNCTION),
    ("cosh", CompletionItemKind::FUNCTION),
    ("tanh", CompletionItemKind::FUNCTION),
    ("asinh", CompletionItemKind::FUNCTION),
    ("acosh", CompletionItemKind::FUNCTION),
    ("atanh", CompletionItemKind::FUNCTION),
    ("hypot", CompletionItemKind::FUNCTION),
    ("max", CompletionItemKind::FUNCTION),
    ("min", CompletionItemKind::FUNCTION),
    ("clz32", CompletionItemKind::FUNCTION),
    ("fround", CompletionItemKind::FUNCTION),
    ("imul", CompletionItemKind::FUNCTION),
    ("sumPrecise", CompletionItemKind::FUNCTION),
];

const NUMBER_CONSTANTS: &[Member] = &[
    ("EPSILON", CompletionItemKind::CONSTANT),
    ("MAX_SAFE_INTEGER", CompletionItemKind::CONSTANT),
    ("MAX_VALUE", CompletionItemKind::CONSTANT),
    ("MIN_SAFE_INTEGER", CompletionItemKind::CONSTANT),
    ("MIN_VALUE", CompletionItemKind::CONSTANT),
    ("NaN", CompletionItemKind::CONSTANT),
    ("NEGATIVE_INFINITY", CompletionItemKind::CONSTANT),
    ("POSITIVE_INFINITY", CompletionItemKind::CONSTANT),
];

const NUMBER_STATIC: &[Member] = &[
    ("isNaN", CompletionItemKind::FUNCTION),
    ("isFinite", CompletionItemKind::FUNCTION),
    ("isInteger", CompletionItemKind::FUNCTION),
    ("isSafeInteger", CompletionItemKind::FUNCTION),
    ("parseFloat", CompletionItemKind::FUNCTION),
    ("parseInt", CompletionItemKind::FUNCTION),
];

const STRING_STATIC: &[Member] = &[
    ("fromCharCode", CompletionItemKind::FUNCTION),
    ("fromCodePoint", CompletionItemKind::FUNCTION),
];

const ARRAY_STATIC: &[Member] = &[
    ("from", CompletionItemKind::FUNCTION),
    ("of", CompletionItemKind::FUNCTION),
    ("isArray", CompletionItemKind::FUNCTION),
];

const OBJECT_STATIC: &[Member] = &[
    ("keys", CompletionItemKind::FUNCTION),
    ("values", CompletionItemKind::FUNCTION),
    ("entries", CompletionItemKind::FUNCTION),
    ("assign", CompletionItemKind::FUNCTION),
    ("fromEntries", CompletionItemKind::FUNCTION),
    ("getOwnPropertyNames", CompletionItemKind::FUNCTION),
    ("is", CompletionItemKind::FUNCTION),
    ("freeze", CompletionItemKind::FUNCTION),
    ("seal", CompletionItemKind::FUNCTION),
    ("preventExtensions", CompletionItemKind::FUNCTION),
    ("isFrozen", CompletionItemKind::FUNCTION),
    ("isSealed", CompletionItemKind::FUNCTION),
    ("isExtensible", CompletionItemKind::FUNCTION),
];

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
