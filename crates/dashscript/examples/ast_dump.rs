//! Temporary AST dumper: `cargo run --example ast_dump -- <file.ts>`.
//! Inspects oxc's parse of a construct before writing its translator mapping,
//! so the mapping follows the real AST shape rather than guessing.

use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_span::SourceType;

fn main() {
    let path = std::env::args().nth(1).expect("usage: ast_dump <file.ts>");
    let src = std::fs::read_to_string(&path).unwrap();
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, &src, SourceType::ts()).parse();
    for e in &parsed.diagnostics {
        eprintln!("parse error: {e}");
    }
    println!("{:#?}", parsed.program);
}
