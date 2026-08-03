//! Temporary WPT fixture probe: translate one fixture to inspect the emitted
//! Rust. `cargo run --example wpt_probe -- <id-substr> [category]` (category
//! defaults to `url`). Used to localize a runtime AssertionError to its
//! translated construct without running the full conformance matrix.

use dashscript::Translator;

fn main() {
    let filter = std::env::args()
        .nth(1)
        .expect("usage: wpt_probe <id-substr> [category]");
    let cat = std::env::args().nth(2).unwrap_or_else(|| "url".to_string());
    let path = format!("crates/dashscript/tests/conformance/data/wpt/{cat}.json");
    let json = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    let feats = v.get("features").and_then(|f| f.as_array()).unwrap();
    let raw = feats
        .iter()
        .find(|x| {
            x.get("id")
                .and_then(|i| i.as_str())
                .is_some_and(|s| s.contains(&filter))
        })
        .unwrap_or_else(|| panic!("no fixture matching {filter}"));
    let fixture = raw.get("fixture").and_then(|f| f.as_str()).unwrap();
    let t = Translator::new();
    let diags = t.check(fixture);
    println!("=== diags ({})", diags.len());
    for d in &diags {
        println!("  {d}");
    }
    match t.translate_with_deps(fixture) {
        Ok((rust, deps)) => {
            println!("=== needs_engine: {}", deps.needs_engine());
            println!("=== RUST ===\n{rust}");
        }
        Err(e) => println!("=== translate err: {e}"),
    }
}
