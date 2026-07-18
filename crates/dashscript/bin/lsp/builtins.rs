//! Builtin metadata — derived from the standard-library `.ds` declaration in
//! [`super::stdlib`] (the `lib.d.ts` analogue). `completion`, `hover`, and
//! `signature-help` all read [`all`]; the drift-guard test (in
//! [`super::stdlib`]) asserts every entry actually translates, so this table
//! cannot claim a name the translator cannot lower to Rust.
//!
//! There is no hand-written member table here — the `.ds` declaration is the
//! single source of truth, parsed once at first use.

use std::sync::LazyLock;

use super::stdlib;

/// One built-in: its qualified name, kind, and a signature (+ optional doc).
/// The [`BuiltinKind`] decides how the signature is rendered and whether
/// signature-help fires.
#[derive(Clone)]
pub(super) struct Builtin {
    /// `None` for a global (`parseInt`); `Some("Math")` for a namespace member.
    pub ns: Option<String>,
    pub name: String,
    pub kind: BuiltinKind,
    /// Signature text (`(x: number): number`), sliced from the stdlib source.
    pub sig: String,
    /// One-line doc; empty when undocumented (hover omits the doc paragraph).
    pub doc: String,
}

impl Builtin {
    /// The fully-qualified name (`Math.round`, `parseInt`) used by hover.
    pub(super) fn qualified(&self) -> String {
        match &self.ns {
            Some(ns) => format!("{ns}.{}", self.name),
            None => self.name.clone(),
        }
    }
}

/// A function (`Math.round`) renders as `name(sig)`; a constant (`Math.PI`)
/// renders as `const name: sig`. Signature help only fires for functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BuiltinKind {
    Function,
    Const,
}

static PARSED: LazyLock<Vec<Builtin>> = LazyLock::new(stdlib::parse);

/// The full builtin table, parsed once from the stdlib declaration. This is
/// the single source of truth for completion/hover/signature-help.
pub(super) fn all() -> &'static [Builtin] {
    &PARSED
}

/// Lookup a built-in by qualified name — `Math.round`, `console.log`, or a
/// global like `parseInt`. `None` for unknown names.
pub(super) fn lookup(qualified: &str) -> Option<&'static Builtin> {
    all().iter().find(|b| b.qualified() == qualified)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_finds_qualified_function() {
        let b = lookup("Math.round").expect("Math.round");
        assert_eq!(b.sig, "(x: number): number");
        assert_eq!(b.kind, BuiltinKind::Function);
    }

    #[test]
    fn lookup_finds_qualified_constant() {
        let b = lookup("Math.PI").expect("Math.PI");
        assert_eq!(b.sig, "number");
        assert_eq!(b.kind, BuiltinKind::Const);
    }

    #[test]
    fn lookup_finds_global() {
        let b = lookup("parseInt").expect("parseInt");
        assert!(b.sig.contains("radix?"), "sig: {}", b.sig);
        assert_eq!(b.kind, BuiltinKind::Function);
    }

    #[test]
    fn lookup_none_for_unknown() {
        assert!(lookup("notABuiltin").is_none());
    }
}
