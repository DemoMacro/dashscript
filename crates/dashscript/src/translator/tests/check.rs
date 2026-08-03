// End-to-end tests for `Translator::check` — the translatability layer.
use super::super::{FileRole, Translator};

#[test]
fn check_passes_a_translatable_file() {
    let src = "function f(x: number): number { return x + 1; }";
    assert!(Translator::new().check(src).is_empty());
}

#[test]
fn check_passes_a_basic_class() {
    // A field-only class is translatable (struct + fn new).
    let diags = Translator::new().check("class C { x: number; }");
    assert!(diags.is_empty());
}

#[test]
fn check_passes_namespace_and_bare_import() {
    // A namespace import (`import * as ns`) lowers to `use m as ns;`, and a
    // bare specifier (`"m"`) resolves via `node_modules` — both mapped
    // constructs, so `check` passes (resolution is the build pipeline's layer,
    // not translatability).
    let diags = Translator::new().check("import * as ns from \"m\";");
    assert!(diags.is_empty(), "namespace/bare import flagged: {diags:?}");
}

#[test]
fn check_flags_a_syntax_error() {
    // Missing `:` — oxc_parser surfaces a syntax diagnostic.
    let diags = Translator::new().check("function f(x number) { return x; }");
    assert!(!diags.is_empty());
}

// Low-compatibility constructs — ECMAScript reflection/dynamic features with no
// Rust mapping. `check` flags them as `unsupported` (one diagnostic) rather
// than letting the translator lower them to broken Rust that fails `cargo
// check` (which would read as `partial` in the matrix). See `unsupported_pattern`.

#[test]
fn check_flags_instanceof() {
    let diags = Translator::new().check("function f(): boolean { return a instanceof B; }");
    assert!(
        diags.iter().any(|d| d.message.contains("instanceof")),
        "{diags:?}"
    );
}

#[test]
fn check_flags_symbol_call() {
    let diags = Translator::new().check("function f(): void { const s = Symbol(); }");
    assert!(diags.iter().any(|d| d.message.contains("Symbol")));
}

#[test]
fn check_flags_new_proxy() {
    let diags = Translator::new().check("function f(): void { const p = new Proxy({}, {}); }");
    assert!(diags.iter().any(|d| d.message.contains("Proxy")));
}

#[test]
fn check_flags_reflect_namespace() {
    let diags = Translator::new().check("function f(): boolean { return Reflect.has({}, \"x\"); }");
    assert!(diags.iter().any(|d| d.message.contains("Reflect")));
}

#[test]
fn check_flags_object_define_property() {
    let diags =
        Translator::new().check("function f(): void { Object.defineProperty({}, \"x\", {}); }");
    assert!(diags
        .iter()
        .any(|d| d.message.contains("Object.defineProperty")));
}

#[test]
fn check_flags_object_create() {
    let diags = Translator::new().check("function f(): void { Object.create(null); }");
    assert!(diags.iter().any(|d| d.message.contains("Object.create")));
}

#[test]
fn check_flags_has_own_property() {
    let diags =
        Translator::new().check("function f(): boolean { return {}.hasOwnProperty(\"x\"); }");
    assert!(diags.iter().any(|d| d.message.contains("hasOwnProperty")));
}

#[test]
fn check_flags_constructor_reflection() {
    let diags = Translator::new().check("function f(): unknown { return (1).constructor; }");
    assert!(diags.iter().any(|d| d.message.contains("constructor")));
}

#[test]
fn check_flags_arguments_object() {
    let diags = Translator::new().check("function f(): unknown { return arguments[0]; }");
    assert!(diags.iter().any(|d| d.message.contains("arguments")));
}

#[test]
fn check_flags_delete_operator() {
    let diags = Translator::new().check("function f(): void { delete o.x; }");
    assert!(diags.iter().any(|d| d.message.contains("delete")));
}

#[test]
fn check_flags_bigint_literal() {
    let diags = Translator::new().check("function f(): void { const n = 1n; }");
    assert!(diags.iter().any(|d| d.message.contains("BigInt")));
}

#[test]
fn check_flags_object_accessor() {
    let diags =
        Translator::new().check("function f(): void { var o = { get x() { return 1; } }; }");
    assert!(
        diags.iter().any(|d| d.message.contains("accessor")),
        "{diags:?}"
    );
}

#[test]
fn check_flags_low_compat_nested_in_callback() {
    // A construct buried inside a callback body is still surfaced — the walk
    // recurses through every expression kind the translator itself walks.
    let diags =
        Translator::new().check("function f(): void { xs.forEach((x) => x instanceof B); }");
    assert!(diags.iter().any(|d| d.message.contains("instanceof")));
}

#[test]
fn check_does_not_flag_typeof_symbol() {
    // `typeof` has its own mapping (a global constructor → "function"), so its
    // operand is not walked — `typeof Symbol` stays supported.
    let diags = Translator::new().check("function f(): void { console.log(typeof Symbol); }");
    assert!(diags.is_empty(), "{diags:?}");
}

#[test]
fn check_does_not_flag_object_keys() {
    // `Object.keys` (and values/entries/is/freeze/…) is mapped — it must not
    // trip the reflection rule (only the named reflection surface is flagged).
    let diags = Translator::new().check("function f(): void { Object.keys({ a: 1 }); }");
    assert!(diags.is_empty(), "{diags:?}");
}

#[test]
fn check_does_not_flag_supported_code() {
    // A plain supported body has no low-compat construct — the walk adds nothing.
    let diags = Translator::new().check(
        "function f(): void { const xs: number[] = [1, 2]; console.log(Math.round(xs[0])); }",
    );
    assert!(diags.is_empty(), "{diags:?}");
}

#[test]
fn check_flags_reflection_in_function_expression() {
    // A reflection call inside an IIFE body `(function () { … })()` is still
    // surfaced — the walk recurses function-expression bodies, not just arrows.
    let diags = Translator::new()
        .check("function f(): void { (function () { Object.defineProperty({}, \"x\", {}); })(); }");
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("Object.defineProperty")),
        "{diags:?}"
    );
}

#[test]
fn check_flags_reflection_in_try_catch() {
    // A construct in the try body or the catch handler (`e.constructor`) is
    // surfaced — the walk recurses both the try block and the catch body.
    let diags = Translator::new().check(
        "function f(): void { try { Object.create(null); } catch (e) { console.log(e.constructor); } }",
    );
    assert!(
        diags.iter().any(|d| d.message.contains("Object.create")),
        "{diags:?}"
    );
    assert!(
        diags.iter().any(|d| d.message.contains("constructor")),
        "{diags:?}"
    );
}

#[test]
fn check_flags_symbol_in_assignment_index() {
    // `obj[Symbol.X] = v` buries a `Symbol` reference in the assignment target
    // — the walk recurses the lvalue's index, so it is surfaced (not lost).
    let diags = Translator::new().check(
        "function f(): void { const o: Record<string, number> = {}; o[Symbol.iterator] = 1; }",
    );
    assert!(
        diags.iter().any(|d| d.message.contains("Symbol")),
        "{diags:?}"
    );
}

#[test]
fn check_flags_prototype_mutation() {
    // `Array.prototype[k] = v` mutates a builtin's prototype — reflection the
    // static model cannot express.
    let diags = Translator::new().check("function f(): void { Array.prototype[0] = 9; }");
    assert!(
        diags.iter().any(|d| d.message.contains("prototype")),
        "{diags:?}"
    );
}

#[test]
fn check_does_not_flag_plain_member_assignment() {
    // `xs[i] = v` is a legitimate mutation (no reflection) — the walk adds
    // nothing, so the body stays supported (a later borrow-check is `partial`,
    // not `unsupported`).
    let diags = Translator::new()
        .check("function f(): void { let xs: number[] = [1]; xs[0] = 5; console.log(xs[0]); }");
    assert!(diags.is_empty(), "{diags:?}");
}

#[test]
fn check_flags_locale_aware_casing_with_locale() {
    // `s.toLocaleUpperCase("tr")` carries an explicit locale DashScript has no
    // ICU table for — reported as unsupported (the locale-less form lowers to
    // the default casing, but a locale cannot be honored without ICU).
    let diags = Translator::new()
        .check("function f(s: string): string { return s.toLocaleUpperCase(\"tr\"); }");
    assert!(
        diags.iter().any(|d| d.message.contains("locale")),
        "{diags:?}"
    );
}

#[test]
fn check_does_not_flag_localeless_tolocale() {
    // `s.toLocaleUpperCase()` has no locale — per spec it is equivalent to
    // `toUpperCase`, so it lowers to the default casing and stays supported.
    let diags =
        Translator::new().check("function f(s: string): string { return s.toLocaleUpperCase(); }");
    assert!(diags.is_empty(), "{diags:?}");
}

#[test]
fn check_flags_string_raw() {
    // `String.raw` is the tagged-template runtime form — no static mapping;
    // without this the translator snake-cases `String` → `string` (E0425
    // `partial`). Reported honestly as `unsupported`.
    let diags =
        Translator::new().check("function f(): string { return String.raw({ raw: [\"a\"] }); }");
    assert!(
        diags.iter().any(|d| d.message.contains("String.raw")),
        "{diags:?}"
    );
}

#[test]
fn check_as_module_flags_top_level_executable() {
    // Module role (arch decision point 8): a module only declares; a top-level
    // executable statement (`console.log`) has no entry to run in → unsupported
    // (rather than silently dropping its side effect).
    let diags = Translator::new().check_as(
        "export function f(): void {}\nconsole.log(1);",
        FileRole::Module,
    );
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("module file may only declare")),
        "module top-level executable not flagged: {diags:?}"
    );
}

#[test]
fn check_as_module_passes_declarations_only() {
    // Module role + declarations only → no diagnostics (declaring is a module's job).
    let diags = Translator::new().check_as(
        "export function f(x: number): number { return x; }",
        FileRole::Module,
    );
    assert!(diags.is_empty(), "module declarations flagged: {diags:?}");
}

#[test]
fn check_as_bin_entry_allows_top_level_executable() {
    // A bin entry allows top-level executable statements (they go into the
    // implicit `fn main`) — unchanged behavior.
    let diags = Translator::new().check_as("console.log(1);", FileRole::BinEntry);
    assert!(diags.is_empty(), "bin entry executable flagged: {diags:?}");
}

#[test]
fn check_flags_await() {
    // `await` needs an async runtime DashScript does not have (`fn main` is
    // sync; decision point 1). Reported honestly rather than lowered to a
    // run-time `todo!()` that panics.
    let diags = Translator::new().check("async function f(): Promise<void> { await foo(); }");
    assert!(
        diags.iter().any(|d| d.message.contains("await")),
        "await not flagged: {diags:?}"
    );
}

#[test]
fn nested_fn_declaration_lowers_to_nested_item() {
    // A nested `function` declaration (the test262/WPT helper convention)
    // lowers to a Rust nested fn item — `fn main { fn helper(..) {..} }` is
    // valid Rust, and a sibling call (`caller` → `helper`) resolves at the
    // enclosing scope.
    let src = "function main(): void {\n  function helper(x: number): number { return x + 1; }\n  function caller(): number { return helper(41); }\n  caller();\n}";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("fn helper("),
        "nested helper not emitted: {rust}"
    );
    assert!(
        rust.contains("fn caller("),
        "nested caller not emitted: {rust}"
    );
    assert!(
        !rust.contains("todo!"),
        "no todo! in nested fn lowering: {rust}"
    );
}

#[test]
fn check_passes_nested_fn_declaration() {
    // A nested fn declaration is no longer flagged `unsupported` — it lowers
    // to a Rust nested fn item. (A construct inside its body that cannot
    // lower — reflection, await, … — is still flagged by the recursive walk.)
    let diags = Translator::new()
        .check("function main(): void {\n  function helper(x: number): number { return x + 1; }\n  helper(1);\n}");
    assert!(diags.is_empty(), "nested fn flagged: {diags:?}");
}

#[test]
fn check_flags_unmappable_inside_nested_fn() {
    // A nested fn itself lowers, but an unmappable construct inside its body
    // (`instanceof` reflection) is still surfaced by the recursive walk.
    let diags = Translator::new()
        .check("function main(): void {\n  function f(x: unknown): boolean { return x instanceof Array; }\n}");
    assert!(
        diags.iter().any(|d| d.message.contains("instanceof")),
        "instanceof inside nested fn not flagged: {diags:?}"
    );
    assert!(
        !diags
            .iter()
            .any(|d| d.message.contains("nested function declaration")),
        "nested fn itself should not be flagged: {diags:?}"
    );
}
