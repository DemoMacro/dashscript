use super::super::Translator;

#[test]
fn translates_number_to_fixed_to_format_precision() {
    let src = "function f(): string { const pi = 3.14159; return pi.toFixed(2); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("format!(\"{:.*}\", 2_f64 as usize, pi)"),
        "got:\n{rust}"
    );
}

#[test]
fn translates_number_to_string_radix_hex() {
    let src = "function f(n: number): string { return n.toString(16); }";
    let rust = Translator::new().translate(src).expect("should translate");
    // Any radix (2-36) lowers to a runtime base-N conversion over digits
    // `0-9a-z` — see `translates_number_to_string_variable_radix`.
    assert!(
        rust.contains("0123456789abcdefghijklmnopqrstuvwxyz"),
        "got:\n{rust}"
    );
}

#[test]
fn translates_number_to_string_radix_binary() {
    let src = "function f(): string { return (255).toString(2); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("0123456789abcdefghijklmnopqrstuvwxyz"),
        "got:\n{rust}"
    );
}

#[test]
fn translates_number_to_string_variable_radix() {
    // A non-literal radix — `i.toString(radix)` in a loop — lowers to the same
    // base-N conversion as a literal radix: digits 0-9a-z, the receiver cast to
    // `i64` (TS truncates the fraction), and a negative value prefixed '-'.
    // Previously this returned `None` and fell through to a 0-arg toString
    // (E0061). Mirrors test262's `number.prototype.toString.a-z`.
    let src = "function f(i: number, radix: number): string { return i.toString(radix); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("0123456789abcdefghijklmnopqrstuvwxyz"),
        "got:\n{rust}"
    );
    assert!(rust.contains("as i64"), "got:\n{rust}");
    assert!(rust.contains("(radix) as u32"), "got:\n{rust}");
}

#[test]
fn translates_number_to_string_no_arg_is_display() {
    let src = "function f(n: number): string { return n.toString(); }";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(rust.contains(".to_string()"), "got:\n{rust}");
    assert!(!rust.contains("as u32"), "got:\n{rust}");
}
