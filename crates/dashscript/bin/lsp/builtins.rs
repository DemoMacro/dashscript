//! Hover/signature-help data for DashScript builtins — signature + a short
//! doc per name. Mirrors the methods/constants the translator actually maps;
//! keep in sync with `translator/builtins/*.rs` and the member tables in
//! `completion.rs`. (A future stage derives these from the translator match
//! arms so the table cannot drift.)
//!
//! Coverage is intentionally partial: undocumented builtins return `None`
//! rather than a guessed doc — no silent wrong information. High-traffic names
//! (`console.*`, `Math` core, global conversions) come first.

/// One builtin's signature and a one-line doc, shown in hover and signature
/// help. The [`BuiltinKind`] decides how the signature is rendered.
#[derive(Clone, Copy)]
pub(super) struct Builtin {
    pub sig: &'static str,
    pub doc: &'static str,
    pub kind: BuiltinKind,
}

/// A function (`Math.round`) renders as `name(sig)`; a constant (`Math.PI`)
/// renders as `const name: sig`. Signature help only fires for functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BuiltinKind {
    Function,
    Const,
}

/// Lookup by fully-qualified name — `Math.round`, `console.log`, or a global
/// like `parseInt`. `None` for builtins not yet in the table.
pub(super) fn lookup(name: &str) -> Option<&'static Builtin> {
    BUILTINS
        .iter()
        .find(|(key, _)| *key == name)
        .map(|(_, v)| v)
}

const BUILTINS: &[(&str, Builtin)] = &[
    // console → println!/eprintln!
    (
        "console.log",
        Builtin {
            sig: "(...data: any[]): void",
            doc: "Print to stdout (lowers to `println!`).",
            kind: BuiltinKind::Function,
        },
    ),
    (
        "console.warn",
        Builtin {
            sig: "(...data: any[]): void",
            doc: "Print to stderr (lowers to `eprintln!`).",
            kind: BuiltinKind::Function,
        },
    ),
    (
        "console.error",
        Builtin {
            sig: "(...data: any[]): void",
            doc: "Print to stderr (lowers to `eprintln!`).",
            kind: BuiltinKind::Function,
        },
    ),
    // Math constants
    (
        "Math.PI",
        Builtin {
            sig: "number",
            doc: "The ratio π ≈ 3.14159.",
            kind: BuiltinKind::Const,
        },
    ),
    (
        "Math.E",
        Builtin {
            sig: "number",
            doc: "Euler's number e ≈ 2.71828.",
            kind: BuiltinKind::Const,
        },
    ),
    (
        "Math.SQRT2",
        Builtin {
            sig: "number",
            doc: "Square root of 2 ≈ 1.41421.",
            kind: BuiltinKind::Const,
        },
    ),
    (
        "Math.LN2",
        Builtin {
            sig: "number",
            doc: "Natural log of 2 ≈ 0.69315.",
            kind: BuiltinKind::Const,
        },
    ),
    (
        "Math.LN10",
        Builtin {
            sig: "number",
            doc: "Natural log of 10 ≈ 2.30259.",
            kind: BuiltinKind::Const,
        },
    ),
    // Math methods
    (
        "Math.round",
        Builtin {
            sig: "(x: number): number",
            doc: "Round to the nearest integer (halves toward +∞).",
            kind: BuiltinKind::Function,
        },
    ),
    (
        "Math.floor",
        Builtin {
            sig: "(x: number): number",
            doc: "Round toward −∞.",
            kind: BuiltinKind::Function,
        },
    ),
    (
        "Math.ceil",
        Builtin {
            sig: "(x: number): number",
            doc: "Round toward +∞.",
            kind: BuiltinKind::Function,
        },
    ),
    (
        "Math.trunc",
        Builtin {
            sig: "(x: number): number",
            doc: "Drop the fractional part (round toward 0).",
            kind: BuiltinKind::Function,
        },
    ),
    (
        "Math.abs",
        Builtin {
            sig: "(x: number): number",
            doc: "Absolute value.",
            kind: BuiltinKind::Function,
        },
    ),
    (
        "Math.sqrt",
        Builtin {
            sig: "(x: number): number",
            doc: "Square root.",
            kind: BuiltinKind::Function,
        },
    ),
    (
        "Math.pow",
        Builtin {
            sig: "(base: number, exp: number): number",
            doc: "`base` raised to the power `exp`.",
            kind: BuiltinKind::Function,
        },
    ),
    (
        "Math.max",
        Builtin {
            sig: "(...values: number[]): number",
            doc: "The largest argument (−∞ if empty).",
            kind: BuiltinKind::Function,
        },
    ),
    (
        "Math.min",
        Builtin {
            sig: "(...values: number[]): number",
            doc: "The smallest argument (+∞ if empty).",
            kind: BuiltinKind::Function,
        },
    ),
    (
        "Math.sign",
        Builtin {
            sig: "(x: number): number",
            doc: "−1, 0, or +1 indicating the sign.",
            kind: BuiltinKind::Function,
        },
    ),
    // global conversions
    (
        "parseInt",
        Builtin {
            sig: "(s: string, radix?: number): number",
            doc: "Parse an integer from a string.",
            kind: BuiltinKind::Function,
        },
    ),
    (
        "parseFloat",
        Builtin {
            sig: "(s: string): number",
            doc: "Parse a floating-point number from a string.",
            kind: BuiltinKind::Function,
        },
    ),
    (
        "isNaN",
        Builtin {
            sig: "(x: number): boolean",
            doc: "True when `x` is NaN.",
            kind: BuiltinKind::Function,
        },
    ),
    (
        "isFinite",
        Builtin {
            sig: "(x: number): boolean",
            doc: "True when `x` is finite (not ±∞ or NaN).",
            kind: BuiltinKind::Function,
        },
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_finds_qualified_function() {
        let b = lookup("Math.round").expect("Math.round");
        assert_eq!(b.sig, "(x: number): number");
        assert_eq!(b.kind, BuiltinKind::Function);
        assert!(b.doc.contains("nearest"));
    }

    #[test]
    fn lookup_finds_qualified_constant() {
        let b = lookup("Math.PI").expect("Math.PI");
        assert_eq!(b.sig, "number");
        assert_eq!(b.kind, BuiltinKind::Const);
    }

    #[test]
    fn lookup_finds_global() {
        assert_eq!(
            lookup("parseInt").unwrap().sig,
            "(s: string, radix?: number): number"
        );
    }

    #[test]
    fn lookup_none_for_undocumented() {
        // Math.imul is mapped but not yet documented — None, not a guess.
        assert!(lookup("Math.imul").is_none());
        assert!(lookup("notABuiltin").is_none());
    }
}
