//! Binding-name disambiguation: two `.ds` bindings that fold to one Rust
//! snake-name (`N` and `n` both → `n`) get distinct Rust names, and the
//! per-symbol keying means a mutation of one never leaks onto the other's
//! `let mut` decision.
use super::super::Translator;

#[test]
fn disambiguates_same_scope_n_and_n() {
    // `N` and `n` are distinct `SymbolId`s in the same scope; without
    // disambiguation both would emit as `n` and silently shadow.
    let src = "function main(): void { const N = 1; const n = 2; console.log(N + n); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("n_2"),
        "the second `n` must disambiguate, got:\n{rust}"
    );
}

#[test]
fn disambiguated_reassigned_binding_keeps_mut() {
    // `let n` (→ `n_2`) is reassigned; the mutation is keyed by `SymbolId`,
    // so it still flags `mut` despite the disambiguated name — without the
    // per-symbol key this would be `let n_2` (no mut) and fail E0384.
    let src = "function main(): void { const N = 1; let n = 2; n = N; console.log(n); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("let mut n_2"),
        "reassigned disambiguated binding must be `mut`, got:\n{rust}"
    );
}

#[test]
fn different_scope_same_name_does_not_disambiguate() {
    // Two `for (let i)` loops are separate Rust blocks, so each `i` shadows
    // the other legally — no `_2` suffix.
    let src = "function main(): void { for (let i = 0; i < 1; i = i + 1) {} for (let i = 0; i < 1; i = i + 1) {} }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        !rust.contains("i_2"),
        "different-scope bindings should not disambiguate, got:\n{rust}"
    );
}
