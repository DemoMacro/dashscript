use super::super::Translator;

#[test]
fn translates_map_type_to_hashmap() {
    let src = "function f(m: Map<string, number>): void {}";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("HashMap<String, f64>"), "Map type: {rust}");
}

#[test]
fn translates_set_type_to_hashset() {
    let src = "function f(s: Set<number>): void {}";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("HashSet<f64>"), "Set type: {rust}");
}

#[test]
fn translates_new_map_to_hashmap_new() {
    let src = "function f(): void { let m: Map<string, number> = new Map(); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("HashMap::new()"), "new Map: {rust}");
}

#[test]
fn translates_new_set_to_hashset_new() {
    let src = "function f(): void { let s: Set<number> = new Set(); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("HashSet::new()"), "new Set: {rust}");
}

#[test]
fn translates_map_methods() {
    // `m.set`/`get`/`has`/`delete`/`size` map to HashMap insert/get(Option)/
    // contains_key/remove(.is_some)/len. `set` is a mutator, so `m` is `let mut`.
    let src = "function f(): void {\n\
        \x20    let m: Map<string, number> = new Map();\n\
        \x20    m.set(\"a\", 1);\n\
        \x20    console.log(m.get(\"a\"));\n\
        \x20    console.log(m.has(\"a\"));\n\
        \x20    console.log(m.size);\n\
        \x20    m.delete(\"a\");\n\
        }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("let mut m"), "set marks m mut: {rust}");
    assert!(rust.contains(".insert("), "set→insert: {rust}");
    assert!(rust.contains(".get(&"), "get: {rust}");
    assert!(rust.contains(".cloned()"), "get cloned (Option<V>): {rust}");
    assert!(rust.contains(".contains_key("), "has: {rust}");
    assert!(rust.contains(".remove("), "delete→remove: {rust}");
    assert!(rust.contains(".len() as f64"), "size→len: {rust}");
}

#[test]
fn translates_set_methods() {
    let src = "function f(): void {\n\
        \x20    let s: Set<number> = new Set();\n\
        \x20    s.add(1);\n\
        \x20    console.log(s.has(1));\n\
        \x20    console.log(s.size);\n\
        }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("let mut s"), "add marks s mut: {rust}");
    assert!(rust.contains(".insert("), "add→insert: {rust}");
    assert!(rust.contains(".contains("), "has→contains: {rust}");
    assert!(rust.contains(".len() as f64"), "size→len: {rust}");
}
