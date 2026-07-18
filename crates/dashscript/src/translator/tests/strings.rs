use super::super::Translator;

#[test]
fn translates_string_method_call() {
    let src = "function f(): void { let s = \"hello\".toUpperCase(); console.log(s); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".to_string().to_uppercase()"), "got:\n{rust}");
}

#[test]
fn translates_to_string_to_display() {
    let src = "function f(n: number): string { return n.toString(); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".to_string()"), "got:\n{rust}");
}

#[test]
fn translates_string_concatenation_to_format() {
    let src =
        "function greet(first: string, last: string): string { return first + \" \" + last; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("format!"), "got:\n{rust}");
    // the literal " " folds into the format string; the two identifiers are placeholders
    assert!(rust.contains("\"{} {}\""), "got:\n{rust}");
}

#[test]
fn translates_string_predicate_methods() {
    let src = "function f(s: string): boolean { return s.includes(\"x\") && s.startsWith(\"a\"); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".contains(\"x\")"), "got:\n{rust}");
    assert!(rust.contains(".starts_with(\"a\")"), "got:\n{rust}");
}

#[test]
fn translates_string_repeat_and_replace() {
    let src = "function f(s: string): string { return s.repeat(3); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".repeat(3_f64 as usize)"), "got:\n{rust}");
    let src2 = "function g(s: string): string { return s.replace(\"a\", \"b\"); }";
    let rust2 = Translator::new().translate(src2).expect("should translate");
    assert!(
        rust2.contains(".replacen(\"a\", \"b\", 1)"),
        "got:\n{rust2}"
    );
}

#[test]
fn translates_string_replace_all_to_replace() {
    let src = "function f(s: string): string { return s.replaceAll(\"a\", \"b\"); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".replace(\"a\", \"b\")"), "got:\n{rust}");
    assert!(!rust.contains("replacen"), "got:\n{rust}");
}

#[test]
fn translates_string_compound_append() {
    let src = "function f(): void { let s = \"a\"; s += \"bc\"; console.log(s); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".push_str(\"bc\")"), "got:\n{rust}");
}

#[test]
fn translates_string_split_to_vec_string() {
    let src = "function f(): void { const parts = \"a,b,c\".split(\",\"); }";
    let rust = Translator::new().translate(src).expect("should translate");
    // `split` yields &str; mapped to owned so the result is Vec<String>.
    assert!(rust.contains(".split(\",\")"), "got:\n{rust}");
    assert!(rust.contains(".collect::<Vec<String>>()"), "got:\n{rust}");
}

#[test]
fn translates_string_index_of_to_find() {
    let src = "function f(): void { const s = \"hello\"; const i = s.indexOf(\"ll\"); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains(".find(\"ll\").map(|b| b as f64).unwrap_or(-1_f64)"),
        "got:\n{rust}"
    );
}

#[test]
fn translates_string_slice_to_byte_range() {
    let src = "function f(): string { return \"hello\".slice(1, 4); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("[1_f64 as usize..4_f64 as usize].to_string()"),
        "got:\n{rust}"
    );
}

#[test]
fn translates_trim_start_end_to_trim_methods() {
    let src = "function f(): void { const a = \"  x\".trimStart(); const b = \"x  \".trimEnd(); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".trim_start()"), "got:\n{rust}");
    assert!(rust.contains(".trim_end()"), "got:\n{rust}");
}

#[test]
fn translates_string_char_at_to_chars_nth() {
    let src = "function f(s: string): string { return s.charAt(0); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".chars().nth("), "got:\n{rust}");
    assert!(rust.contains(".unwrap_or_default()"), "got:\n{rust}");
}

#[test]
fn translates_string_pad_start_to_right_align() {
    let src = "function f(s: string): string { return s.padStart(5); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("format!(\"{:>1$}\""), "got:\n{rust}");
}

#[test]
fn translates_string_pad_end_to_left_align() {
    let src = "function f(s: string): string { return s.padEnd(5); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("format!(\"{:<1$}\""), "got:\n{rust}");
}

#[test]
fn translates_string_pad_start_with_fill_char() {
    let src = "function f(s: string): string { return s.padStart(5, \"0\"); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("chars().cycle().take"), "got:\n{rust}");
    assert!(rust.contains("saturating_sub"), "got:\n{rust}");
}

#[test]
fn translates_string_pad_end_with_fill_char() {
    let src = "function f(s: string): string { return s.padEnd(5, \"-\"); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("chars().cycle().take"), "got:\n{rust}");
}

#[test]
fn translates_string_char_code_at_to_code_point() {
    let src = "function f(s: string): number { return s.charCodeAt(0); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("as u32 as f64"), "got:\n{rust}");
    assert!(rust.contains("f64::NAN"), "got:\n{rust}");
}

#[test]
fn translates_string_from_char_code_to_char() {
    let src = "function f(): string { return String.fromCharCode(65); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("char::from_u32("), "got:\n{rust}");
}

#[test]
fn translates_string_code_point_at() {
    let src = "function f(s: string): number { return s.codePointAt(0); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("as u32 as f64"), "got:\n{rust}");
}

#[test]
fn translates_string_concat_to_format() {
    let src = "function f(s: string): string { return s.concat(\"a\", \"b\"); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("format!("), "got:\n{rust}");
    assert!(rust.contains("\"{}{}{}\""), "got:\n{rust}");
}

#[test]
fn translates_string_at_to_chars_nth() {
    let src = "function f(s: string): string { return s.at(0); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".chars().nth("), "got:\n{rust}");
}

#[test]
fn translates_string_last_index_of_to_rfind() {
    let src = "function f(s: string): number { return s.lastIndexOf(\"l\"); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".rfind("), "got:\n{rust}");
    assert!(rust.contains("unwrap_or(-1_f64)"), "got:\n{rust}");
}

#[test]
fn translates_string_lower_trim_methods() {
    let src = "function f(s: string): string { return s.trim().toLowerCase(); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".trim()"), "got:\n{rust}");
    assert!(rust.contains(".to_lowercase()"), "got:\n{rust}");
}

#[test]
fn translates_string_ends_with_to_ends_with() {
    let src = "function f(s: string): boolean { return s.endsWith(\"x\"); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".ends_with("), "got:\n{rust}");
}

#[test]
fn translates_string_replace_substring_methods() {
    let src = "function f(s: string): string { return s.replace(\"a\", \"b\").substring(1); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".replacen("), "got:\n{rust}");
    assert!(rust.contains("[1_f64 as usize..]"), "got:\n{rust}");
}

#[test]
fn translates_string_from_code_point() {
    let src = "function f(): string { return String.fromCodePoint(65); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("char::from_u32"), "got:\n{rust}");
}

#[test]
fn translates_string_value_of() {
    let src = "function f(s: string): string { return s.valueOf(); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(!rust.contains("valueOf"), "got:\n{rust}");
}

#[test]
fn translates_string_is_well_formed() {
    let src = "function f(s: string): boolean { return s.isWellFormed(); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("true"), "got:\n{rust}");
}

#[test]
fn translates_string_to_well_formed() {
    let src = "function f(s: string): string { return s.toWellFormed(); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".to_string()"), "got:\n{rust}");
}

#[test]
fn translates_string_index_of_with_position() {
    // `.indexOf(needle, from)` starts the search at byte offset `from`.
    let src = "function f(s: string): number { return s.indexOf(\"l\", 2); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("__from"), "got:\n{rust}");
    assert!(rust.contains(".find(\"l\")"), "got:\n{rust}");
}

#[test]
fn translates_string_includes_with_position() {
    let src = "function f(s: string): boolean { return s.includes(\"x\", 1); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("__p..].contains(\"x\")"), "got:\n{rust}");
}

#[test]
fn translates_string_starts_ends_with_position() {
    let src =
        "function f(s: string): boolean { return s.startsWith(\"a\", 1) || s.endsWith(\"z\", 4); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("get(__p..).is_some_and(|t| t.starts_with(\"a\"))"),
        "got:\n{rust}"
    );
    assert!(rust.contains("[..__p].ends_with(\"z\")"), "got:\n{rust}");
}

#[test]
fn translates_string_bracket_index_to_chars_nth() {
    // `s[i]` — Rust `str` cannot be indexed by `usize`; lower to
    // `chars().nth(i)` (the char as a `String`, "" if out of range).
    let src = "function f(s: string): string { return s[0]; }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".chars().nth("), "got:\n{rust}");
    assert!(rust.contains("map(|c| c.to_string())"), "got:\n{rust}");
}

#[test]
fn translates_string_pad_undefined_fill_uses_space_default() {
    // `.padEnd(n, undefined)` falls back to the space default (same as
    // `.padEnd(n)`), not the dynamic-fill cycle path.
    let src = "function f(s: string): string { return s.padEnd(5, undefined); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains("format!(\"{:<1$}\""), "got:\n{rust}");
    assert!(!rust.contains("cycle"), "got:\n{rust}");
}

#[test]
fn translates_string_prototype_trim_call_to_method() {
    // `String.prototype.trim.call(x)` — the JS borrow-via-.call idiom — lowers
    // to ToString(x).trim() (a scalar receiver is format!-coerced first).
    let src = "function f(): void { console.log(String.prototype.trim.call(\"  x  \")); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".trim()"), "got:\n{rust}");
    assert!(!rust.contains("prototype"), "got:\n{rust}");
}

#[test]
fn translates_string_prototype_touppercase_call() {
    let src = "function f(): void { console.log(String.prototype.toUpperCase.call(\"ab\")); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".to_uppercase()"), "got:\n{rust}");
}
