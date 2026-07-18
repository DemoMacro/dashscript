//! Symbol-level analysis via [`oxc_semantic`] — scope tree + symbol table +
//! resolved references. Powers LSP find-references/rename with **symbol-level
//! precision**: two same-named bindings in different scopes are distinct
//! symbols, so renaming one never touches the other (no same-name collisions,
//! which would silently corrupt code). This is how rust-analyzer and the TS
//! language service resolve references — not by global name match.
//!
//! [`analyze_symbols`] builds the full `Semantic`, harvests an owned snapshot
//! ([`SymbolTable`]) of every symbol's declaration span, kind, and resolved
//! reference spans, then drops the parse arena — the snapshot borrows nothing,
//! so it outlives the analysis.

use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_semantic::{SemanticBuilder, SymbolFlags};
use oxc_span::{SourceType, Span};

/// A flat, owned snapshot of a file's symbols and their resolved references.
#[derive(Debug, Clone, Default)]
pub struct SymbolTable {
    /// One entry per symbol (every declaration site in the file).
    pub symbols: Vec<SymbolEntry>,
}

#[derive(Debug, Clone)]
pub struct SymbolEntry {
    /// The bound name as written in `.ds` (e.g. `foo`, `Point`).
    pub name: String,
    /// Byte span of the declaration's binding identifier.
    pub span: Span,
    /// What the symbol declares. Mirrors the subset of oxc `SymbolFlags`
    /// DashScript cares about (interface/type translate to Rust struct/enum,
    /// so they are renameable alongside functions/classes/variables).
    pub kind: SymbolKind,
    /// Byte span of every resolved reference to this symbol (read or write).
    /// Excludes the declaration itself — that is [`SymbolEntry::span`].
    pub references: Vec<Span>,
}

/// The coarse kind of a symbol — enough for document-symbol icons and to
/// distinguish what is renameable. Derived from oxc `SymbolFlags`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Function,
    Class,
    Interface,
    TypeAlias,
    Variable,
    Other,
}

/// Build a symbol table for one `.ds` file. An empty result means the file
/// failed to parse (syntax errors) — the caller degrades gracefully.
#[must_use]
pub fn analyze_symbols(source: &str) -> SymbolTable {
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, SourceType::ts()).parse();
    if !ret.diagnostics.is_empty() {
        return SymbolTable::default();
    }
    // `SemanticBuilder::build` takes `&'a Program<'a>`; move the parsed program
    // into the arena so both the node data and the wrapper share one lifetime.
    let program = allocator.alloc(ret.program);
    let ret = SemanticBuilder::new().with_build_nodes(true).build(program);
    let semantic = &ret.semantic;
    let scoping = semantic.scoping();
    let symbols = scoping
        .symbol_ids()
        .map(|symbol_id| {
            let references = semantic
                .symbol_references(symbol_id)
                .map(|r| semantic.reference_span(r))
                .collect();
            SymbolEntry {
                name: scoping.symbol_name(symbol_id).to_string(),
                span: scoping.symbol_span(symbol_id),
                kind: symbol_kind(scoping.symbol_flags(symbol_id)),
                references,
            }
        })
        .collect();
    SymbolTable { symbols }
}

/// Map oxc `SymbolFlags` to the coarse [`SymbolKind`]. Check the most specific
/// declaration kinds first — a symbol may carry combined flags.
fn symbol_kind(flags: SymbolFlags) -> SymbolKind {
    if flags.contains(SymbolFlags::Function) {
        SymbolKind::Function
    } else if flags.contains(SymbolFlags::Class) {
        SymbolKind::Class
    } else if flags.contains(SymbolFlags::Interface) {
        SymbolKind::Interface
    } else if flags.contains(SymbolFlags::TypeAlias) {
        SymbolKind::TypeAlias
    } else if flags.contains(SymbolFlags::Variable) {
        SymbolKind::Variable
    } else {
        SymbolKind::Other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The core correctness guarantee for rename: same name in two scopes is
    /// two symbols, not one. If this breaks, rename would touch both.
    #[test]
    fn same_name_different_scope_are_distinct_symbols() {
        let src = "let x = 1; function foo(x: number) { return x + 1; } console.log(x);";
        let table = analyze_symbols(src);
        let xs: Vec<_> = table.symbols.iter().filter(|s| s.name == "x").collect();
        assert_eq!(xs.len(), 2, "expected two distinct `x` symbols: {xs:?}");
    }

    #[test]
    fn references_collected_per_symbol() {
        // The top-level `a` is read once inside `foo`.
        let src = "let a = 1; function foo() { return a + 1; }";
        let table = analyze_symbols(src);
        let a = table
            .symbols
            .iter()
            .find(|s| s.name == "a")
            .expect("symbol `a`");
        assert_eq!(
            a.references.len(),
            1,
            "one reference to `a`: {:?}",
            a.references
        );
    }

    /// The param `x` has one reference (`return x`); the top-level `x` has none.
    /// A rename of the param must only surface the param's references.
    #[test]
    fn rename_one_does_not_report_the_other() {
        let src = "let x = 1; function f(x: number) { return x; }";
        let table = analyze_symbols(src);
        let ref_counts: Vec<usize> = table
            .symbols
            .iter()
            .filter(|s| s.name == "x")
            .map(|s| s.references.len())
            .collect();
        assert!(
            ref_counts.contains(&1),
            "param `x` has 1 reference: {ref_counts:?}"
        );
        assert!(
            ref_counts.contains(&0),
            "top-level `x` has 0 references: {ref_counts:?}"
        );
    }

    #[test]
    fn function_and_class_kinds() {
        let src = "function fn() {} class C {}";
        let table = analyze_symbols(src);
        let fn_sym = table.symbols.iter().find(|s| s.name == "fn").expect("`fn`");
        assert_eq!(fn_sym.kind, SymbolKind::Function);
        let c_sym = table.symbols.iter().find(|s| s.name == "C").expect("`C`");
        assert_eq!(c_sym.kind, SymbolKind::Class);
    }

    #[test]
    fn interface_and_type_alias_are_collected() {
        // interface/type are TS type symbols, but oxc still binds them, so they
        // appear in the symbol table (renameable — they become Rust struct/enum).
        let src = "interface I {} type T = number;";
        let table = analyze_symbols(src);
        let i = table.symbols.iter().find(|s| s.name == "I").expect("`I`");
        assert_eq!(i.kind, SymbolKind::Interface);
        let t = table.symbols.iter().find(|s| s.name == "T").expect("`T`");
        assert_eq!(t.kind, SymbolKind::TypeAlias);
    }

    #[test]
    fn invalid_syntax_yields_empty_table() {
        let table = analyze_symbols("function ((");
        assert!(table.symbols.is_empty(), "syntax error → empty table");
    }
}
