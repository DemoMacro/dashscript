use super::super::Translator;

#[test]
fn translates_map_type_to_hashmap() {
    let src = "function f(m: Map<string, number>): void {}";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("HashMap<String, f64>"), "Map type: {rust}");
}

#[test]
fn translates_set_type_to_hashset() {
    // `Set<number>` → `HashSet<DsF64Key>` — f64 lacks Eq/Hash, so a number
    // element wraps in the SameValueZero newtype.
    let src = "function f(s: Set<number>): void {}";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("HashSet<crate::__ds::DsF64Key>"),
        "Set<number>: {rust}"
    );
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
fn translates_new_uint8_array_to_zeroed_vec() {
    // `new Uint8Array(n)` — a crypto byte buffer of `n` zeroed u8 elements —
    // lowers to `vec![0u8; n as usize]`, not the generic `Uint8Array::new(n)`
    // (there is no such Rust type).
    let src = "function f(n: number): Uint8Array { return new Uint8Array(n); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("vec![0_u8;"), "new Uint8Array(n): {rust}");
}

#[test]
fn translates_new_uint8_array_empty_to_empty_vec() {
    let src = "function f(): Uint8Array { return new Uint8Array(); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("Vec::<u8>::new()"),
        "new Uint8Array(): {rust}"
    );
}

#[test]
fn translates_new_uint8_array_from_array_literal() {
    // `new Uint8Array([1, 2, 3])` — the typed-array-from-array case: copy each
    // element, casting to u8 (not the length form `vec![0_u8; n as usize]`).
    let src = "function f(): Uint8Array { return new Uint8Array([1, 2, 3]); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("map(|x| x as u8)"),
        "new Uint8Array([…]) copies with a u8 cast: {rust}"
    );
    assert!(
        !rust.contains("as usize"),
        "array literal must not take the length path: {rust}"
    );
}

#[test]
fn translates_new_uint8_array_from_member_source() {
    // `new Uint8Array(t.bytes)` — a member-access source (a `Vec<f64>`
    // property) lowers to a from-iterable copy with a u8 cast, not the
    // length path `(t.bytes) as usize` (E0605: a Vec cannot be cast to usize).
    let src = "interface T { bytes: number[]; } function f(t: T): Uint8Array { return new Uint8Array(t.bytes); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("map(|x| x as u8)"),
        "new Uint8Array(t.bytes) copies with a u8 cast: {rust}"
    );
    assert!(
        !rust.contains("as usize"),
        "member source must not take the length path: {rust}"
    );
}

#[test]
fn translates_new_uint8_array_from_vec_local() {
    // `new Uint8Array(buf)` where `buf: number[]` (a `Vec<f64>` local) takes
    // the from-iterable path. A `number` local still takes the length path
    // (covered by `translates_new_uint8_array_to_zeroed_vec`).
    let src = "function f(buf: number[]): Uint8Array { return new Uint8Array(buf); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("map(|x| x as u8)"),
        "new Uint8Array(buf) copies with a u8 cast: {rust}"
    );
    assert!(
        !rust.contains("as usize"),
        "Vec local must not take the length path: {rust}"
    );
}

#[test]
fn translates_new_int32_array_to_zeroed_vec() {
    // `new Int32Array(n)` — a 4-byte-per-element int32 typed array — reuses the
    // u8 length path with the i32 element type: `vec![0_i32; n as usize]`.
    let src = "function f(n: number): Int32Array { return new Int32Array(n); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("vec![0_i32;"), "new Int32Array(n): {rust}");
}

#[test]
fn translates_new_float64_array_to_zeroed_vec() {
    // `new Float64Array(n)` — the f64 element type — lowers to
    // `vec![0_f64; n as usize]`, the same shape with a float zero literal.
    let src = "function f(n: number): Float64Array { return new Float64Array(n); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("vec![0_f64;"), "new Float64Array(n): {rust}");
}

#[test]
fn translates_typed_array_set_to_copy_from_slice() {
    // `buf.set(src, off)` on a `Uint8Array` (`Vec<u8>`) — ES
    // `TypedArray.prototype.set` copies `src`'s bytes into `buf` starting at
    // `off`. The receiver's `Vec<u8>` type (recorded by `new Uint8Array(…)`)
    // drives the dispatch; `copy_from_slice` does the copy. `off` (a `number`)
    // is cast to `usize`.
    let src = "function f(h: Uint8Array): void {\
                 let buf = new Uint8Array(8);\
                 buf.set(h, 0);\
             }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("copy_from_slice"),
        "buf.set(src, off) → copy_from_slice: {rust}"
    );
    assert!(
        !rust.contains(".set("),
        "must not emit a plain .set( call: {rust}"
    );
}

#[test]
fn map_set_not_confused_with_typed_array_set() {
    // A `Map.set(k, v)` on a `HashMap` keeps the `insert` lowering; the
    // typed-array `set` dispatch must not grab it (different receiver type).
    let src = "function f(): void {\
                 let m: Map<string, number> = new Map();\
                 m.set(\"k\", 1);\
             }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".insert("), "Map.set → insert: {rust}");
    assert!(
        !rust.contains("copy_from_slice"),
        "Map.set must not lower to copy_from_slice: {rust}"
    );
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

#[test]
fn translates_set_number_methods_wrap_f64_key() {
    // `Set<number>` methods wrap each value in `DsF64Key` — `s.add(1)` →
    // `s.insert(DsF64Key(1))`, `s.has(1)` → `s.contains(&DsF64Key(1))`.
    let src = "function f(): void {\n\
        \x20    let s: Set<number> = new Set();\n\
        \x20    s.add(1);\n\
        \x20    console.log(s.has(1));\n\
        }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("DsF64Key"),
        "Set<number> wraps DsF64Key: {rust}"
    );
    assert!(
        rust.contains("insert(crate::__ds::DsF64Key"),
        "add wraps the value: {rust}"
    );
    // prettyplease renders `&crate::…` as `& crate ::…` (it spaces `&` before a
    // path), so verify the `has` wrap by counting `DsF64Key` (add + has = 2).
    assert!(
        rust.matches("DsF64Key").count() >= 2,
        "add + has both wrap in DsF64Key: {rust}"
    );
}

#[test]
fn translates_set_string_methods_do_not_wrap() {
    // `Set<string>` keeps `String` directly (string implements Eq+Hash), so no
    // `DsF64Key` wrap appears.
    let src = "function f(): void {\n\
        \x20    let s: Set<string> = new Set();\n\
        \x20    s.add(\"x\");\n\
        }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        !rust.contains("DsF64Key"),
        "Set<string> must not wrap: {rust}"
    );
}

#[test]
fn translates_map_number_key_wraps_f64_key() {
    // `Map<number, V>` wraps the key (not the value) in `DsF64Key`.
    let src = "function f(): void {\n\
        \x20    let m: Map<number, string> = new Map();\n\
        \x20    m.set(1, \"a\");\n\
        \x20    console.log(m.get(1));\n\
        }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("HashMap<crate::__ds::DsF64Key, String>"),
        "Map<number, string> type: {rust}"
    );
    assert!(
        rust.contains("insert(crate::__ds::DsF64Key"),
        "set wraps the key: {rust}"
    );
}

#[test]
fn translates_inferred_set_literal_wraps_f64_key() {
    // An unannotated `let s = new Set([1, 2])` infers `HashSet<DsF64Key>` (the
    // element type from the literal), so `s.add(…)` / `s.has(…)` wrap each
    // value too — the f64-Eq/Hash gap closed for the inferred-literal path, not
    // just the annotated `Set<number>` one.
    let src = "function f(): void {\n\
        \x20    let s = new Set([1, 2]);\n\
        \x20    s.add(3);\n\
        \x20    console.log(s.has(3));\n\
        }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("insert"),
        "add → insert on an inferred HashSet: {rust}"
    );
    assert!(
        rust.matches("DsF64Key").count() >= 3,
        "inferred Set([1, 2]) + add(3) + has(3) all thread DsF64Key: {rust}"
    );
}

#[test]
fn translates_bare_new_map_methods_dispatch() {
    // An unannotated `let map = new Map()` records the bare `HashMap` type, so
    // `map.set/get/has/size` dispatch on the receiver — K/V are inferred at the
    // insert sites by Rust (`HashMap::new()` + `insert("a", 1_f64)` ⇒
    // `HashMap<String, f64>`).
    let src = "function f(): void {\
                 let map = new Map();\
                 map.set(\"a\", 1);\
                 console.log(map.get(\"a\"));\
                 console.log(map.has(\"a\"));\
                 console.log(map.size);\
             }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".insert("), "bare Map.set → insert: {rust}");
    assert!(rust.contains(".contains_key("), "bare Map.has: {rust}");
    assert!(rust.contains(".len() as f64"), "bare Map.size: {rust}");
}

#[test]
fn translates_new_map_from_array_literal() {
    // `new Map([["k", 1], ["k2", 2]])` → `HashMap::from([("k".to_string(), 1_f64), …])`
    // — a literal initial map of [key, value] pairs, mirroring `new Set([a, b])`.
    let src = "function f(): void { let m = new Map([[\"k\", 1], [\"k2\", 2]]); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("HashMap::from(["),
        "new Map([[k,v],…]) → HashMap::from: {rust}"
    );
    assert!(
        !rust.contains("Map::new"),
        "must not fall through to Map::new: {rust}"
    );
}

#[test]
fn translates_new_map_from_number_key_array_wraps_f64_key() {
    // `new Map([[1, 1], [2, 2]])` — a number key wraps in `DsF64Key` (f64 lacks
    // Eq/Hash), so the inferred `HashMap<DsF64Key, f64>` compiles.
    let src = "function f(): void { let m = new Map([[1, 1], [2, 2]]); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("DsF64Key"),
        "new Map([[number, …]]) wraps the key: {rust}"
    );
}

#[test]
fn translates_bare_map_number_key_wraps_f64_key() {
    // An unannotated `new Map()` whose first `set` key is a number wraps the
    // key in `DsF64Key` — the inferred `HashMap<f64, V>` would otherwise fail.
    let src = "function f(): void {\
                 let map = new Map();\
                 map.set(0, 1);\
                 console.log(map.has(0));\
             }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("insert(crate::__ds::DsF64Key"),
        "bare Map number key wraps DsF64Key: {rust}"
    );
}
