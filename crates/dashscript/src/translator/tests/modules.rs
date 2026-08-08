// End-to-end tests for module declarations (export/import).
use super::super::{FileRole, Translator};

#[test]
fn exports_function_as_pub() {
    let rust = Translator::new()
        .translate("export function foo(): number { return 1; }")
        .expect("should translate");
    assert!(rust.contains("pub fn foo"), "got: {rust}");
}

#[test]
fn exports_interface_as_pub_struct() {
    let rust = Translator::new()
        .translate("export interface P { x: number; }")
        .expect("should translate");
    assert!(rust.contains("pub struct P"), "got: {rust}");
}

#[test]
fn exports_type_alias_as_pub_type() {
    let rust = Translator::new()
        .translate("export type Id = number;")
        .expect("should translate");
    assert!(rust.contains("pub type Id"), "got: {rust}");
}

#[test]
fn import_emits_use() {
    let rust = Translator::new()
        .translate("import { foo } from \"./other\";")
        .expect("should translate");
    assert!(rust.contains("use crate::app::other::foo"), "got: {rust}");
}

#[test]
fn import_groups_multiple_names() {
    let rust = Translator::new()
        .translate("import { foo, bar } from \"./other\";")
        .expect("should translate");
    assert!(
        rust.contains("use crate::app::other::{foo, bar}"),
        "got: {rust}"
    );
}

#[test]
fn import_cargo_emits_use() {
    // `cargo:serde` is a Cargo crate (the `cargo:` family marker, aligned
    // with Deno's `npm:`/`jsr:`); it lowers to `use serde::foo`.
    let rust = Translator::new()
        .translate("import { foo } from \"cargo:serde\";")
        .expect("should translate");
    assert!(rust.contains("use serde::foo"), "got: {rust}");
}

#[test]
fn import_cargo_hyphen_to_underscore() {
    // A crate name may contain a hyphen, but a `use` path / module ident may
    // not — `cargo:cfg-if` becomes `use cfg_if::x`.
    let rust = Translator::new()
        .translate("import { x } from \"cargo:cfg-if\";")
        .expect("should translate");
    assert!(rust.contains("use cfg_if::x"), "got: {rust}");
    assert!(!rust.contains("cfg-if"), "hyphen leaked: {rust}");
}

#[test]
fn collect_skips_cargo_import() {
    // A `cargo:` import is a crate, not a local `.ts` file — it must not be
    // collected for module assembly (only relative imports are).
    let imports = Translator::new().imports("import { foo } from \"cargo:serde\";");
    assert!(imports.is_empty(), "cargo import collected: {imports:?}");
}

#[test]
fn bare_import_emits_use_for_npm_resolution() {
    // A bare specifier (`lodash`) is an npm import — the translator emits
    // `use ds_lodash::x;` (the `ds_` prefix + injective escape separates
    // npm-origin crates from cargo-native crates) and `check` passes.
    // Resolution is the build pipeline's job (the third correctness layer);
    // whether it lowers depends on the target. `ds build` resolves `lodash`
    // under `node_modules/`: a `.ts` entry translates, a `.js` entry errors
    // honestly.
    let rust = Translator::new()
        .translate("import { x } from \"lodash\";")
        .expect("should translate");
    assert!(
        rust.contains("use third_party::ds_lodash::x"),
        "bare import emitted no use: {rust}"
    );
    let diags = Translator::new().check("import { x } from \"lodash\";");
    assert!(diags.is_empty(), "bare import flagged by check: {diags:?}");
}

#[test]
fn bare_import_normalizes_scope_and_hyphen() {
    // `@scope/pkg-name` → `ds_scopeSpkg_name`: a valid Rust module ident. The
    // leading `@` is dropped; the scope separator `/` escapes to `S` and the
    // hyphen to `_`, so the map is injective (distinct npm names never share
    // one ident) and `ds_`-prefixed (never collides with a cargo-native crate).
    let rust = Translator::new()
        .translate("import { x } from \"@scope/pkg-name\";")
        .expect("should translate");
    assert!(
        rust.contains("use third_party::ds_scopeSpkg_name::x"),
        "scope/hyphen not normalized: {rust}"
    );
}

#[test]
fn collect_includes_bare_import() {
    // A bare specifier is assembled into a `mod` decl (resolved via
    // `node_modules`), the way a relative import is — unlike a `cargo:`
    // import, which names a Rust crate and is excluded from assembly. Its
    // module name is the injective `ds_`-prefixed ident (`my-pkg` →
    // `ds_my_pkg`).
    let imports = Translator::new().imports("import { foo } from \"my-pkg\";");
    assert_eq!(imports.len(), 1);
    assert_eq!(imports[0].module, "ds_my_pkg");
    assert_eq!(imports[0].source, "my-pkg");
}

#[test]
fn collect_local_imports() {
    let imports = Translator::new().imports("import { foo, bar } from \"./other\";");
    assert_eq!(imports.len(), 1);
    assert_eq!(imports[0].module, "other");
    assert_eq!(imports[0].source, "./other");
}

#[test]
fn import_keeps_type_name_pascalcase() {
    // A type binding (uppercase) is kept as-is so it matches the PascalCase
    // struct/type the module exports; a value binding is snake_cased.
    let rust = Translator::new()
        .translate("import { add, Point } from \"./other\";")
        .expect("should translate");
    assert!(
        rust.contains("use crate::app::other::{add, Point}"),
        "got: {rust}"
    );
}

#[test]
fn import_default_value_emits_use() {
    // A default import (`import foo`) lowers like a named one: Rust crates have
    // no default export, so the local name names the crate item directly.
    let rust = Translator::new()
        .translate("import foo from \"cargo:serde\";")
        .expect("should translate");
    assert!(rust.contains("use serde::foo"), "got: {rust}");
}

#[test]
fn import_default_type_keeps_pascalcase() {
    // A default import naming a type keeps PascalCase, like a named type import.
    let rust = Translator::new()
        .translate("import Foo from \"cargo:serde\";")
        .expect("should translate");
    assert!(rust.contains("use serde::Foo"), "got: {rust}");
}

#[test]
fn import_type_emits_type_use() {
    // `import type { T } from "./geom"` is type-only (zero runtime) — it lowers
    // to a type `use`, same as a named type import (the `type` keyword does not
    // change the emit; a Rust `use` of a type is already zero-runtime).
    let rust = Translator::new()
        .translate("import type { Point } from \"./geom\";")
        .expect("should translate");
    assert!(rust.contains("use crate::app::geom::Point"), "got: {rust}");
}

#[test]
fn import_namespace_emits_use_alias() {
    // `import * as ns from "./other"` → `use other as ns;` — a module-path
    // alias (not a group leaf). The body then reads members as the path
    // `ns::foo`, the way a Rust `use other as ns;` exposes `ns::foo`.
    let rust = Translator::new()
        .translate("import * as ns from \"./other\";")
        .expect("should translate");
    assert!(rust.contains("use crate::app::other as ns"), "got: {rust}");
}

#[test]
fn import_namespace_member_call_is_path() {
    // `ns.foo(1)` where `ns` is a namespace import → the free function
    // `ns::foo(1)`. The callee reuses `member_expr`'s namespace branch, and the
    // call is guarded before method dispatch so a name colliding with a mapped
    // method is not mis-routed. Runs in the implicit `fn main`.
    let rust = Translator::new()
        .translate("import * as ns from \"./other\";\nns.foo(1);")
        .expect("should translate");
    assert!(rust.contains("ns::foo"), "got: {rust}");
}

#[test]
fn import_namespace_member_read_is_path() {
    // `ns.foo` (a read, not a call) → `ns::foo` via `member_expr`'s namespace
    // branch. A `console.log` argument routes the member through `translate_expr`.
    let rust = Translator::new()
        .translate("import * as ns from \"./other\";\nconsole.log(ns.foo);")
        .expect("should translate");
    assert!(rust.contains("ns::foo"), "got: {rust}");
}

#[test]
fn import_named_rename_emits_as() {
    // `import { add as sum }` → `use other::add as sum;` — the imported name
    // (`add`, the source-module path) aliased to the local binding (`sum`).
    // Without this the translator would emit `use other::sum`, which cannot
    // resolve (`sum` does not exist in `other`). A value alias keeps its
    // snake_case form.
    let rust = Translator::new()
        .translate("import { add as sum } from \"./other\";")
        .expect("should translate");
    assert!(rust.contains("add as sum"), "got: {rust}");
}

#[test]
fn import_type_rename_keeps_pascalcase() {
    // `import { Point as P }` → `use geom::Point as P;` — both names are types,
    // so PascalCase is kept (the same rule as a named type import).
    let rust = Translator::new()
        .translate("import { Point as P } from \"./geom\";")
        .expect("should translate");
    assert!(rust.contains("Point as P"), "got: {rust}");
}

#[test]
fn namespace_member_access_passes_check() {
    // `ns.foo(…)` on a namespace import is a mapped construct (module-path
    // call), so `check` must not flag it unsupported — otherwise valid
    // namespace code would be rejected by `ds lint` / the conformance harness.
    let diags = Translator::new().check("import * as ns from \"./other\";\nns.foo(1);");
    assert!(diags.is_empty(), "namespace access flagged: {diags:?}");
}

#[test]
fn export_named_from_emits_pub_use() {
    // `export { foo } from "./m"` → `pub use m::foo;` — a re-export surfaces
    // another module's item on this module's public surface. prettyplease
    // drops the braces for a single item.
    let rust = Translator::new()
        .translate("export { foo } from \"./other\";")
        .expect("should translate");
    assert!(
        rust.contains("pub use crate::app::other::foo"),
        "got: {rust}"
    );
}

#[test]
fn export_named_from_groups_multiple() {
    // Multiple re-exports group like imports: `pub use other::{foo, bar};`.
    let rust = Translator::new()
        .translate("export { foo, bar } from \"./other\";")
        .expect("should translate");
    assert!(
        rust.contains("pub use crate::app::other::{foo, bar}"),
        "got: {rust}"
    );
}

#[test]
fn export_named_from_rename_emits_as() {
    // `export { foo as bar } from "./m"` → `pub use m::foo as bar;` — the
    // source path (`foo`) aliased to the exported name (`bar`).
    let rust = Translator::new()
        .translate("export { foo as bar } from \"./other\";")
        .expect("should translate");
    assert!(rust.contains("foo as bar"), "got: {rust}");
}

#[test]
fn export_all_emits_pub_glob() {
    // `export * from "./m"` → `pub use m::*;` — re-export every item.
    let rust = Translator::new()
        .translate("export * from \"./other\";")
        .expect("should translate");
    assert!(rust.contains("pub use crate::app::other::*"), "got: {rust}");
}

#[test]
fn export_all_as_namespace_emits_alias() {
    // `export * as ns from "./m"` → `pub use m as ns;` — a namespace re-export;
    // importers read its members as `ns::foo`.
    let rust = Translator::new()
        .translate("export * as ns from \"./other\";")
        .expect("should translate");
    assert!(
        rust.contains("pub use crate::app::other as ns"),
        "got: {rust}"
    );
}

#[test]
fn export_re_exports_pass_check() {
    // Re-exports are mapped constructs, so `check` must not flag them.
    let diags = Translator::new().check(
        "export { foo } from \"./a\";\nexport * from \"./b\";\nexport * as ns from \"./c\";",
    );
    assert!(diags.is_empty(), "re-export flagged: {diags:?}");
}

#[test]
fn export_default_function_emits_pub_fn() {
    // `export default function foo()` lowers the declaration `pub` — a default
    // export is a public item like any named export (which file is "the entry"
    // is a build-pipeline concern, not the translator's).
    let rust = Translator::new()
        .translate("export default function foo(): number { return 1; }")
        .expect("should translate");
    assert!(rust.contains("pub fn foo"), "got: {rust}");
}

#[test]
fn export_default_class_emits_pub_struct() {
    // `export default class Foo` lowers the `struct` (and its `impl`) as `pub`.
    let rust = Translator::new()
        .translate("export default class Foo { x: number; }")
        .expect("should translate");
    assert!(rust.contains("pub struct Foo"), "got: {rust}");
}

#[test]
fn export_default_expression_is_unsupported() {
    // `export default <expression>` names no item — Rust has no anonymous
    // default value, so it stays unsupported (a default function/class works).
    let diags = Translator::new().check("export default 42;");
    assert!(
        !diags.is_empty(),
        "expression default not flagged: {diags:?}"
    );
}

#[test]
fn declarations_list_local_bindings() {
    let decls = Translator::new().declarations(
        "function foo() {}\ninterface Bar {}\ntype Baz = number\nimport { qux } from \"./other\";",
    );
    let names: Vec<&str> = decls.iter().map(|d| d.name.as_str()).collect();
    assert!(names.contains(&"foo"), "missing foo: {names:?}");
    assert!(names.contains(&"Bar"), "missing Bar: {names:?}");
    assert!(names.contains(&"Baz"), "missing Baz: {names:?}");
    assert!(names.contains(&"qux"), "missing qux: {names:?}");
}

#[test]
fn has_main_detects_function_main() {
    assert!(Translator::new().has_main("function main() { console.log(1); }"));
}

#[test]
fn has_main_detects_export_function_main() {
    assert!(Translator::new().has_main("export function main(): number { return 0; }"));
}

#[test]
fn has_main_false_when_absent() {
    assert!(!Translator::new().has_main("function helper() {}"));
}

#[test]
fn has_main_ignores_main_loop_helper() {
    // `main_loop` is a common helper name; a substring scan of `fn main` in the
    // translated Rust would trip on it. AST-level matching only counts an
    // identifier literally named `main`.
    assert!(!Translator::new().has_main("function main_loop() {}"));
}

#[test]
fn has_main_ignores_string_literal() {
    // A `"fn main"` string literal must not count as declaring `main` — the
    // reason `has_main` walks the AST rather than scanning the source text.
    assert!(!Translator::new().has_main("const s = \"fn main\";"));
}

#[test]
fn top_level_function_main_renames_to_ds_main() {
    // Pure-TS execution semantics: `function main` is an ordinary declaration,
    // not the cargo entry. Rename the root scope's `main` to `__ds_main` so the
    // implicit `fn main` the translator emits cannot collide with it. The
    // rename lives in `NameTable`, so every call site follows automatically.
    let rust = Translator::new()
        .translate("function main(): void { console.log(1); }")
        .expect("should translate");
    assert!(rust.contains("fn __ds_main"), "got: {rust}");
}

#[test]
fn top_level_executable_collected_into_implicit_main() {
    // Pure-TS execution semantics: a top-level `const` and expression statement
    // run in source order, so they land inside the implicit `fn main` the
    // translator emits — not as Rust items. `function greet` is a declaration,
    // so it stays a top-level `fn`.
    let rust = Translator::new()
        .translate(
            "function greet(n: string): string { return n; }\nconst m = greet(\"hi\");\nconsole.log(m);",
        )
        .expect("should translate");
    assert!(rust.contains("fn greet"), "decl stayed an item: {rust}");
    assert!(rust.contains("fn main()"), "implicit main emitted: {rust}");
    assert!(
        rust.contains("let m"),
        "top-level const collected into main body: {rust}"
    );
}

#[test]
fn declaration_only_program_emits_empty_fn_main() {
    // A file with only declarations (no executable statements) still needs a
    // binary entry point, so the translator emits an empty `fn main {}` — the
    // way Node runs a script that defines functions but never calls them.
    let rust = Translator::new()
        .translate("function helper(): number { return 1; }")
        .expect("should translate");
    assert!(rust.contains("fn helper"), "decl stayed an item: {rust}");
    assert!(
        rust.contains("fn main()"),
        "empty implicit main emitted: {rust}"
    );
}

#[test]
fn top_level_main_call_invokes_renamed_ds_main() {
    // `function main` is renamed `__ds_main`; a top-level `main()` call (the
    // explicit way to run it under pure-TS semantics) lands in the implicit
    // `fn main` body as a `__ds_main()` call.
    let rust = Translator::new()
        .translate("function main(): void { console.log(1); }\nmain();")
        .expect("should translate");
    assert!(rust.contains("fn __ds_main"), "main renamed: {rust}");
    assert!(
        rust.contains("__ds_main();"),
        "call site followed rename: {rust}"
    );
}

#[test]
fn top_level_variable_declaration_passes_check() {
    // Pure-TS execution semantics: a top-level `const` runs in the implicit
    // `fn main`, so it is legitimate top-level — not an unmapped statement.
    let diags = Translator::new().check("const x: number = 5;");
    assert!(diags.is_empty(), "top-level const flagged: {diags:?}");
}

#[test]
fn top_level_expression_statement_passes_check() {
    // A top-level expression (a call, a side effect) runs in the implicit
    // `fn main` — legitimate top-level under pure-TS semantics.
    let diags = Translator::new().check("console.log(1);");
    assert!(diags.is_empty(), "top-level expression flagged: {diags:?}");
}

#[test]
fn top_level_function_referencing_non_mutated_let_promotes() {
    // B3-1a: a top-level `function` reading a non-mutated top-level `let`
    // (const-expression literal) promotes to a crate `const` item — previously
    // `unsupported` (check_escape). Only a *mutated* `let` is still flagged
    // (it needs a `thread_local!` `RefCell`, B3-2).
    let diags = Translator::new().check("let x: number = 5;\nfunction f(): number { return x; }");
    assert!(
        diags.is_empty(),
        "non-mutated let escape flagged: {diags:?}"
    );
}

#[test]
fn top_level_number_const_referenced_by_fn_promotes_to_const_item() {
    // Escape promotion (A3): a top-level `const` number literal referenced from
    // a top-level `function` is hoisted to a crate-level `const` item — it
    // cannot stay in `fn main` (a Rust fn item cannot capture a `main` local).
    // The item keeps the snake-case rust name so the reference resolves to it
    // unchanged; `#[allow(non_upper_case_globals)]` silences the lowercase-const
    // lint.
    let rust = Translator::new()
        .translate("const N = 5;\nfunction f(): number { return N; }")
        .expect("should translate");
    assert!(rust.contains("const n: f64"), "no const item: {rust}");
    assert!(
        rust.contains("#[allow(non_upper_case_globals)]"),
        "no lint allow: {rust}"
    );
    // The initializer must NOT also appear as a `let` inside `fn main`.
    assert!(
        !rust.contains("let n"),
        "promoted const leaked into main: {rust}"
    );
}

#[test]
fn top_level_bool_const_referenced_by_fn_promotes_to_const_item() {
    // A `const` boolean literal escapes the same way → a crate-level `bool`
    // `const` item.
    let rust = Translator::new()
        .translate("const B = true;\nfunction f(): boolean { return B; }")
        .expect("should translate");
    assert!(rust.contains("const b: bool"), "no bool const item: {rust}");
    assert!(
        !rust.contains("let b"),
        "promoted bool leaked into main: {rust}"
    );
}

#[test]
fn promoted_number_const_passes_check() {
    // A promoted const-expr `const` is a mapped construct (it lowers to a
    // `const` item), so `check` must not flag it — otherwise valid escape code
    // would be rejected by `ds lint`.
    let diags = Translator::new().check("const N = 5;\nfunction f(): number { return N; }");
    assert!(diags.is_empty(), "promoted const flagged: {diags:?}");
}

#[test]
fn top_level_string_const_referenced_by_fn_promotes() {
    // A `const` string literal lowers to `&'static str` (a Rust const), so it
    // promotes to a crate `const` item just like a number/boolean when a
    // function escapes onto it — `check` must not flag it.
    let diags = Translator::new().check("const S = \"hi\";\nfunction f(): string { return S; }");
    assert!(diags.is_empty(), "promoted string const flagged: {diags:?}");
    let rust = Translator::new()
        .translate("const S = \"hi\";\nfunction f(): string { return S; }")
        .unwrap();
    assert!(
        rust.contains("const s: &'static str") && rust.contains("\"hi\""),
        "string const not promoted to a crate item: {rust}"
    );
}

#[test]
fn non_escaped_top_level_const_stays_in_main() {
    // Promotion happens only on escape (a function reading the binding). A
    // top-level `const` read only at the top level stays a `let` inside `fn
    // main` — it is not hoisted to a crate item.
    let rust = Translator::new()
        .translate("const N = 5;\nconsole.log(N);")
        .expect("should translate");
    assert!(
        !rust.contains("const n"),
        "non-escaped const hoisted: {rust}"
    );
    assert!(
        rust.contains("let n"),
        "top-level const not in main: {rust}"
    );
}

#[test]
fn promoted_number_const_readable_at_top_level() {
    // A promoted numeric `const` is registered in the name table so a top-level
    // read routes through `__ds::number_to_string` (ES Number rendering), the
    // way a numeric local does — not left as a bare `f64` that `Display`s with
    // the wrong format.
    let rust = Translator::new()
        .translate("const N = 5;\nfunction f(): number { return N; }\nconsole.log(N);")
        .expect("should translate");
    assert!(
        rust.contains("number_to_string(n as f64)"),
        "top-level read of promoted const not routed: {rust}"
    );
}

#[test]
fn module_non_mutated_let_literal_promotes_to_const_item() {
    // B3-1a: a module-level non-mutated `let` whose initializer is a
    // const-expression literal lowers to a crate `const` item, just like a
    // `const` — a module has no `fn main` to run a `let` in, and an immutable
    // literal is a Rust const. `check` must not flag it.
    let src = "let config: number = 42;\nexport function get(): number { return config; }";
    let diags = Translator::new().check_as(src, FileRole::Module);
    assert!(
        diags.is_empty(),
        "non-mutated module let flagged: {diags:?}"
    );
    let rust = Translator::new()
        .translate_with_deps_as(src, FileRole::Module)
        .expect("should translate")
        .0;
    assert!(
        rust.contains("const config"),
        "non-mutated let not promoted to const item: {rust}"
    );
    assert!(
        !rust.contains("OnceLock"),
        "literal let should not use OnceLock: {rust}"
    );
}

#[test]
fn module_non_mutated_let_runtime_init_emits_lazy_static() {
    // B3-1a (lazy-static path): a module-level non-mutated `let` whose
    // initializer is NOT a const-expression literal (here a `number[]`, which
    // builds at runtime) lowers to a `static OnceLock<T>` + accessor `fn`, just
    // like a `const` — a module has no `fn main` to run a `let` in, and the
    // value is constructed once at first use. Previously a `let` here was
    // rejected; only `const` qualified. (An object-literal initializer with an
    // interface annotation hits the B5 type-propagation gap — the annotation
    // does not yet steer the literal to the struct ctor — so this case uses a
    // `number[]` init, whose Vec ctor matches the OnceLock type.)
    let src =
        "let nums: number[] = [1, 2, 3];\nexport function first(): number { return nums[0]; }";
    let diags = Translator::new().check_as(src, FileRole::Module);
    assert!(
        diags.is_empty(),
        "non-mutated let runtime init flagged: {diags:?}"
    );
    let rust = Translator::new()
        .translate_with_deps_as(src, FileRole::Module)
        .expect("should translate")
        .0;
    assert!(
        rust.contains("OnceLock") && rust.contains("get_or_init"),
        "non-mutated let runtime init not lowered to lazy static: {rust}"
    );
    assert!(
        rust.contains("fn nums()"),
        "lazy-static accessor not emitted: {rust}"
    );
}

#[test]
fn module_new_textencoder_referenced_by_fn_hoists_to_oncelock() {
    // A module-level `new TextEncoder()` (the WHATWG Encoding API, a WinterTC
    // Web API) referenced from a top-level function hoists behind a
    // `static OnceLock<crate::__ds::TextEncoder>` + accessor `fn`, just like a
    // `number[]` runtime init — the constructor is not a const-expression
    // literal, and the stateless builtin ctor's Rust type is known
    // (`encoding_ctor_type`). Previously `is_inferable_new` only recognized
    // `new Set([…])`, so this fell through to `unsupported`. The OnceLock
    // carries the `__ds::TextEncoder` type, which also flags the Encoding dep
    // via the marker probe.
    let src =
        "const encoder = new TextEncoder();\nexport function encLen(s: string): number { return encoder.encode(s).length; }";
    let diags = Translator::new().check_as(src, FileRole::Module);
    assert!(
        diags.is_empty(),
        "module-level new TextEncoder flagged: {diags:?}"
    );
    let rust = Translator::new()
        .translate_with_deps_as(src, FileRole::Module)
        .expect("should translate")
        .0;
    assert!(
        rust.contains("OnceLock") && rust.contains("get_or_init"),
        "module-level new TextEncoder not lowered to lazy static: {rust}"
    );
    assert!(
        rust.contains("crate::__ds::TextEncoder"),
        "OnceLock type is not the encoding ctor type: {rust}"
    );
    assert!(
        rust.contains("fn encoder()"),
        "lazy-static accessor for the encoder not emitted: {rust}"
    );
}

#[test]
fn module_mutable_let_value_reassigned_by_fn_emits_thread_local_refcell() {
    // B3-2a: a module-level `let` (a value type: a number) rebound from a
    // top-level function cannot live in `fn main` (a Rust fn item cannot close
    // over a `main` local) and cannot live behind an immutable OnceLock (B3-1),
    // so it hoists behind a thread-local `RefCell` with a get/set accessor pair —
    // matching TS's single-threaded module-global semantics. The `x++` in the
    // function rewrites to a block that reads via the get accessor, writes via
    // the set accessor, and yields the old value (the get accessor returns a
    // clone, not an lvalue, so `x() += 1` would not compile).
    let src = "let counter = 0;\nexport function next(): number { return counter++; }";
    let diags = Translator::new().check_as(src, FileRole::Module);
    assert!(
        diags.is_empty(),
        "mutable module-global value let flagged: {diags:?}"
    );
    let rust = Translator::new()
        .translate_with_deps_as(src, FileRole::Module)
        .expect("should translate")
        .0;
    assert!(
        rust.contains("thread_local!") && rust.contains("RefCell"),
        "mutable module-global not lowered to thread_local RefCell: {rust}"
    );
    assert!(
        rust.contains("fn counter()") && rust.contains("fn set_counter"),
        "mutable-static get/set accessors not emitted: {rust}"
    );
    // `counter++` rewrites to a block that calls set_counter.
    assert!(
        rust.contains("set_counter"),
        "x++ not rewritten to the set accessor: {rust}"
    );
}

#[test]
fn module_delayed_binding_optional_let_hoists_to_refcell_option() {
    // B3-2c: a module-level `let x: T | undefined;` with no initializer is a
    // delayed-binding slot (TS breaks a circular dependency by declaring first,
    // assigning later — e.g. a lazy parse-table). It hoists behind a thread-local
    // `RefCell<Option<T>>` seeded `None`; a later `x = v` rewrites to
    // `set_x(Some(v))`, and `if (x)` truthiness to `x().is_some()`. The
    // annotation `T | undefined` lowers to `Option<T>`, so the cell holds it
    // directly. Here T is `number` → `Option<f64>`.
    let src = "let slot: number | undefined;\nexport function has(): boolean { if (slot) { return true; } return false; }\nexport function fill(): void { slot = 1; }";
    let diags = Translator::new().check_as(src, FileRole::Module);
    assert!(
        diags.is_empty(),
        "delayed-binding optional let flagged: {diags:?}"
    );
    let rust = Translator::new()
        .translate_with_deps_as(src, FileRole::Module)
        .expect("should translate")
        .0;
    assert!(
        rust.contains("RefCell") && rust.contains("None"),
        "delayed-binding not RefCell<Option> None-seeded: {rust}"
    );
    assert!(
        rust.contains("fn slot()") && rust.contains("fn set_slot"),
        "delayed-binding get/set accessors missing: {rust}"
    );
    // `slot = 1` → `set_slot(Some(1.0))`.
    assert!(
        rust.contains("set_slot(::std::option::Option::Some"),
        "assign not rewritten to set_slot(Some(..)): {rust}"
    );
    // `if (slot)` truthiness → `slot().is_some()`.
    assert!(
        rust.contains(".is_some()"),
        "truthiness not lowered to is_some(): {rust}"
    );
}

#[test]
fn module_delayed_binding_fn_optional_call_unwraps() {
    // B3-2c (callable slot): a delayed-binding `Option<fn …>` slot called as
    // `parse(args)` lowers to `(parse().expect("parse"))(args)` — ES throws on a
    // nullish call, here it panics with the source name (fail-loud). The slot is
    // assigned a fn item (`parse = inc`), which coerces to the
    // `Option<fn(f64) -> f64>` the cell holds.
    let src = "let parse: ((n: number) => number) | undefined;\nfunction inc(n: number): number { return n + 1; }\nexport function run(n: number): number { parse = inc; if (parse) { return parse(n); } return 0; }";
    let diags = Translator::new().check_as(src, FileRole::Module);
    assert!(diags.is_empty(), "callable optional let flagged: {diags:?}");
    let rust = Translator::new()
        .translate_with_deps_as(src, FileRole::Module)
        .expect("should translate")
        .0;
    assert!(
        rust.contains(".expect(\"parse\")"),
        "callable slot not unwrapped via expect: {rust}"
    );
    assert!(
        rust.contains("set_parse(::std::option::Option::Some"),
        "fn assign not Some-wrapped: {rust}"
    );
}

#[test]
fn module_as_const_array_lazy_static_infers_vec_type() {
    // `const X = [...] as const` — the `as const` is a TS literal-type marker
    // (runtime no-op), not a real type: oxc parses it as a `TSTypeReference`
    // named `const` (a Rust keyword that would panic `reference_type`). The
    // OnceLock now holds the inner array's inferred element type — `Vec<String>`
    // for a string-literal array, `Vec<Vec<…>>` for a nested one — instead.
    let src =
        "const NAMES = [\"a\", \"b\"] as const;\nexport function has(): boolean { return true; }";
    let diags = Translator::new().check_as(src, FileRole::Module);
    assert!(diags.is_empty(), "as const lazy static flagged: {diags:?}");
    let rust = Translator::new()
        .translate_with_deps_as(src, FileRole::Module)
        .expect("should translate")
        .0;
    assert!(
        rust.contains("OnceLock<Vec<String>>"),
        "as const array not lowered to OnceLock<Vec<String>>: {rust}"
    );
}

#[test]
fn module_export_const_object_lazy_static_emits_accessor() {
    // `export const M = {...}` is an `ExportNamedDeclaration` wrapping the
    // `VariableDeclaration` — without unwrapping it, the lazy-static pre-pass
    // skipped the binding entirely (the module emitted no accessor at all, so a
    // cross-file `use crate::m::M` resolved to nothing). Both shapes lower to
    // the same `OnceLock` + `pub fn` accessor; the `export` only adds visibility.
    let src = "export const M: Record<string, string> = { a: \"1\", b: \"2\" };\nexport function has(): boolean { return true; }";
    let diags = Translator::new().check_as(src, FileRole::Module);
    assert!(
        diags.is_empty(),
        "export const lazy static flagged: {diags:?}"
    );
    let rust = Translator::new()
        .translate_with_deps_as(src, FileRole::Module)
        .expect("should translate")
        .0;
    assert!(
        rust.contains("static M_CELL") && rust.contains("pub fn m()"),
        "export const not lowered to OnceLock accessor: {rust}"
    );
}

#[test]
fn collect_lazy_static_exports_maps_accessor_to_cell_type() {
    // A cross-file consumer recognizes an imported lazy static by its accessor
    // name (`snake(export name)`) and cell type — `collect_lazy_static_exports`
    // is the per-file half `project::translate_sources` aggregates across the
    // import graph. `export const M = {...}` lowers to accessor `m` holding a
    // `HashMap<String, String>`.
    let src = "export const M: Record<string, string> = { a: \"1\", b: \"2\" };";
    let map = Translator::new().collect_lazy_static_exports(src);
    let ty = map
        .get("m")
        .expect("accessor name `m` (snake of export `M`)");
    let ty_text = quote::quote!(#ty).to_string();
    assert!(
        ty_text.contains("HashMap"),
        "cell type not a HashMap: {ty_text}"
    );
}

#[test]
fn imported_lazy_static_uses_accessor_name_and_hashmap_get() {
    // With the cross-file lazy-static export table published (the way
    // `project::translate_sources` publishes it before a package translate), a
    // consumer's `import { M }` lowers to `use crate::app::a::m` — the accessor fn,
    // snake-folded, not the type-cased `M` — a reference emits the accessor
    // call, and a `HashMap` index lowers to `.get(…)`. Without the table the
    // use path was `M` (unresolved), the reference a bare `m`, and the index a
    // `usize` cast.
    let mut exports: std::collections::HashMap<String, syn::Type> =
        std::collections::HashMap::new();
    exports.insert(
        "m".to_string(),
        syn::parse_quote!(::std::collections::HashMap<String, String>),
    );
    crate::translator::imports::set_lazy_static_exports(exports);
    let src = "import { M } from \"./a\";\nexport function use(): string { return M[\"a\"]; }";
    let result = Translator::new().translate_with_deps_as(src, FileRole::Module);
    crate::translator::imports::clear_lazy_static_exports();
    let rust = result.expect("should translate").0;
    assert!(
        rust.contains("use crate::app::a::m;"),
        "use path not the accessor name: {rust}"
    );
    assert!(
        rust.contains("m().get(\"a\")"),
        "not a HashMap get on the accessor call: {rust}"
    );
}

#[test]
fn file_local_lazy_static_alias_indexes_via_get() {
    // A file-local alias of an imported lazy static (`const N = M;`) lowers to
    // its own OnceLock accessor whose cell type is the aliased static's cell
    // type (read from the cross-file export table), initialized by cloning
    // through the accessor's `&'static T` return. An index `N["k"]` then routes
    // to `n().get(k)` — the alias's cell type is not in the cross-file export
    // table, so it is resolved via the per-symbol NameTable recorded in the
    // lazy-static pre-pass. Previously the index fell through to a numeric
    // index because the alias was not recognized as a HashMap local.
    let mut exports: std::collections::HashMap<String, syn::Type> =
        std::collections::HashMap::new();
    exports.insert(
        "m".to_string(),
        syn::parse_quote!(::std::collections::HashMap<String, String>),
    );
    crate::translator::imports::set_lazy_static_exports(exports);
    let src = "import { M } from \"./a\";\nexport const N = M;\nexport function use(): string { return N[\"a\"]; }";
    let result = Translator::new().translate_with_deps_as(src, FileRole::Module);
    crate::translator::imports::clear_lazy_static_exports();
    let rust = result.expect("should translate").0;
    assert!(
        rust.contains("use crate::app::a::m;"),
        "use path not the accessor name: {rust}"
    );
    assert!(
        rust.contains("(*m()).clone()"),
        "alias init not cloning through the accessor ref: {rust}"
    );
    assert!(
        rust.contains("n().get(\"a\")"),
        "alias index not a HashMap get: {rust}"
    );
}

#[test]
fn entry_non_mutated_let_literal_referenced_by_fn_promotes() {
    // B3-1a (entry path, via promotable relaxation): a top-level non-mutated
    // `let` literal referenced from a function promotes to a crate `const` item
    // — previously this was `unsupported` (check_escape). The promotable
    // relaxation plus the check_escape flaggable exclusion make it legal.
    let src = "let n: number = 5;\nfunction f(): number { return n; }";
    let diags = Translator::new().check(src);
    assert!(
        diags.is_empty(),
        "non-mutated let escape flagged: {diags:?}"
    );
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("const n") && !rust.contains("let n"),
        "non-mutated let not promoted: {rust}"
    );
}

#[test]
fn entry_non_mutated_let_runtime_init_referenced_by_fn_hoists() {
    // B3-1b: an entry-file non-mutated `let` (runtime initializer) referenced
    // from a function hoists to a `static OnceLock<T>` + accessor, just like a
    // module — previously `unsupported` (check_escape), since an entry left it
    // as an `fn main` local a Rust fn item cannot close over.
    let src = "let nums: number[] = [1, 2, 3];\nfunction first(): number { return nums[0]; }";
    let diags = Translator::new().check(src);
    assert!(
        diags.is_empty(),
        "entry non-mutated let escape flagged: {diags:?}"
    );
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("OnceLock") && rust.contains("fn nums()"),
        "entry non-mutated let not hoisted to lazy static: {rust}"
    );
}

#[test]
fn entry_const_array_literal_no_annotation_referenced_by_fn_hoists() {
    // A top-level `const`/`let` whose initializer is an array literal with no
    // type annotation, referenced from a function, hoists to a
    // `static OnceLock<T>` + accessor — `cell_type::infer_array_type` infers
    // `Vec<T>` from the first element (falling back to `Vec<serde_json::Value>`
    // for non-scalar/object elements). Previously `unsupported`
    // (check_escape): an unannotated array literal was not in
    // `lazy_static_candidate`'s inferable set, so a WPT fixture whose inlined
    // `// META: script=` defined `const encodings_table = […]` (then read from
    // a `test()` callback) was rejected.
    let src = "const words = [\"a\", \"b\", \"c\"];\nfunction first() { return words[0]; }";
    let diags = Translator::new().check(src);
    assert!(
        diags.is_empty(),
        "entry const array escape flagged: {diags:?}"
    );
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        rust.contains("OnceLock<Vec<String>>") && rust.contains("fn words()"),
        "entry const array not hoisted to OnceLock<Vec<String>>: {rust}"
    );
}

#[test]
fn entry_non_mutated_let_runtime_init_unreferenced_stays_local() {
    // B3-1b: an entry-file non-mutated `let` NOT referenced from any function
    // stays a plain `fn main` local (source-order, zero-cost) — only the
    // referenced ones hoist. The `console.log` reads it from `fn main` without a
    // function closing over it, so no OnceLock is needed.
    let src = "let nums: number[] = [1, 2, 3];\nconsole.log(nums[0]);";
    let rust = Translator::new().translate(src).expect("should translate");
    assert!(
        !rust.contains("OnceLock"),
        "unreferenced entry let hoisted unnecessarily: {rust}"
    );
}

#[test]
fn mutated_top_level_let_referenced_by_fn_still_unsupported() {
    // A top-level `let` mutated from a function cannot lower to a `const` item
    // or an immutable OnceLock — it needs a `thread_local!` `RefCell` (B3-2).
    // Until then `check_escape` still flags it.
    let diags = Translator::new().check("let n: number = 0;\nfunction f(): void { n = 1; }");
    assert!(
        !diags.is_empty(),
        "mutated let escape not flagged: {diags:?}"
    );
}

#[test]
fn top_level_main_call_passes_check() {
    // `function main` is renamed `__ds_main`; a top-level `main()` call is now
    // an ordinary executable statement (it invokes `__ds_main()` from the
    // implicit `fn main`) — no longer a special-cased entry to skip.
    let diags = Translator::new().check("function main(): void { console.log(1); }\nmain();");
    assert!(diags.is_empty(), "top-level main() call flagged: {diags:?}");
}

#[test]
fn translate_with_deps_module_emits_no_main() {
    // Module role (arch decision point 8): a module only declares, never
    // executes — no `fn main`. A module file exporting a function translates to
    // a crate-internal module (src/<stem>.rs), brought in by the entry via `mod`.
    let rust = Translator::new()
        .translate_with_deps_as(
            "export function helper(x: number): number { return x * 2; }",
            FileRole::Module,
        )
        .expect("should translate")
        .0;
    assert!(!rust.contains("fn main"), "module emitted fn main: {rust}");
    assert!(
        rust.contains("pub fn helper"),
        "module lost its declaration: {rust}"
    );
}

#[test]
fn translate_with_deps_bin_entry_emits_main_for_declarations_only() {
    // A bin entry emits an empty `fn main` even when it is declarations-only
    // (a cargo bin target requires an entry).
    let rust = Translator::new()
        .translate_with_deps_as(
            "function helper(x: number): number { return x * 2; }",
            FileRole::BinEntry,
        )
        .expect("should translate")
        .0;
    assert!(
        rust.contains("fn main"),
        "bin entry missing fn main: {rust}"
    );
}

#[test]
fn translate_with_deps_module_degrades_top_level_executable() {
    // Module role + a top-level executable statement (`console.log`) →
    // whole-module degrade (degrade-over-reject; arch decision point 8). A
    // module declares, so a top-level statement has no `fn main` to run in, but
    // degrading keeps the side effect: every top-level function routes to
    // `call_fn` and the module source (carrying the `console.log`) is eval'd
    // before each degraded invocation — the executable is not dropped.
    let (rust, deps) = Translator::new()
        .translate_with_deps_as(
            "export function helper(): void {}\nconsole.log(1);",
            FileRole::Module,
        )
        .expect("module with top-level executable should degrade, not error");
    assert!(
        rust.contains("__ds::engine::call_fn(\"helper\""),
        "top-level function should degrade to call_fn: {rust}"
    );
    assert!(
        rust.contains("console.log(1)"),
        "top-level executable should survive in the module JS: {rust}"
    );
    assert!(
        deps.needs_engine(),
        "degraded module should pull the engine dep: {deps:?}"
    );
}

#[test]
fn new_worker_emits_isolate_spawn() {
    // Direction D (D1): `new Worker(handler)` lowers to `__ds::Worker::new(handler)`
    // — an isolate thread runs the handler, fed by the main thread via postMessage.
    // `postMessage` goes through the generic member-name mapping (snake_case →
    // post_message); the runtime flags RuntimeDep::Worker.
    let (rust, deps) = Translator::new()
        .translate_with_deps(
            "const w = new Worker((msg: number): void => { console.log(msg); });\n\
             w.postMessage(42);",
        )
        .expect("should translate");
    assert!(
        rust.contains("__ds::Worker::new"),
        "worker spawn not emitted: {rust}"
    );
    assert!(
        rust.contains("w.post_message"),
        "postMessage not snake_cased to post_message: {rust}"
    );
    assert!(deps.needs_worker(), "worker runtime dep not flagged");
}

#[test]
fn new_worker_with_reply_emits_bidirectional_spawn() {
    // Direction D (D2): a handler with a second `reply` parameter →
    // `new_with_reply` (bidirectional). The worker replies via `reply.send(v)`;
    // main blocks on `recv()`. The arch's D2 mapping of `worker.on('message')`
    // ← `mpsc::Receiver::recv()` (synchronous fn main, no event loop).
    let (rust, deps) = Translator::new()
        .translate_with_deps(
            "const w = new Worker((msg: number, reply: unknown): void => {\n\
             \x20    reply.send(msg * 2);\n\
             \x20});\n\
             w.postMessage(21);\n\
             const r: number = w.recv();\n\
             console.log(r);",
        )
        .expect("should translate");
    assert!(
        rust.contains("__ds::Worker::new_with_reply"),
        "bidirectional spawn not emitted: {rust}"
    );
    assert!(rust.contains("reply.send"), "reply.send not mapped: {rust}");
    assert!(rust.contains("w.recv()"), "recv not emitted: {rust}");
    assert!(deps.needs_worker(), "worker runtime dep not flagged");
}

#[test]
fn new_worker_one_arg_handler_stays_d1() {
    // A one-argument handler stays on the D1 one-way `new` (not mis-promoted to
    // new_with_reply).
    let (rust, _deps) = Translator::new()
        .translate_with_deps("const w = new Worker((msg: number): void => { console.log(msg); });")
        .expect("should translate");
    assert!(
        rust.contains("__ds::Worker::new(") && !rust.contains("new_with_reply"),
        "one-arg handler should stay D1 new: {rust}"
    );
}

#[test]
fn new_worker_reply_turbofish_anchors_message_type() {
    // Direction D (D2): the handler's first-param type annotation (`: number`)
    // → turbofish `new_with_reply::<f64, _>`. The worker deserializes each
    // message to A, but the closure body alone cannot pin A (the generic
    // `reply.send(msg * 2)` does not anchor `msg`'s type), so the annotation is
    // the anchor — avoids E0283.
    let (rust, _deps) = Translator::new()
        .translate_with_deps(
            "const w = new Worker((msg: number, reply: unknown): void => {\n\
             \x20    reply.send(msg * 2);\n\
             \x20});",
        )
        .expect("should translate");
    // prettyplease expands the turbofish across multiple lines (`::<\n    f64,\n    _,\n>`),
    // so whitespace is normalized before asserting `new_with_reply::<f64,_>`
    // to avoid indent sensitivity.
    let flat: String = rust.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        flat.contains("new_with_reply::<f64"),
        "turbofish not anchoring message type f64: {rust}"
    );
}

#[test]
fn export_const_arrow_lowers_to_pub_fn() {
    // `export const name = (params): ret => expr` is a named function (the
    // binding names it), so it lowers to a `pub fn` item — not dropped or left
    // as an executable statement. An expression body becomes the block's
    // trailing expression.
    let rust = Translator::new()
        .translate("export const isEmpty = (arr: number[]): boolean => arr.length === 0;")
        .expect("should translate");
    assert!(rust.contains("pub fn is_empty("), "no pub fn: {rust}");
    assert!(rust.contains("-> bool"), "no bool return: {rust}");
}

#[test]
fn export_const_generic_arrow_keeps_type_params() {
    // `export const f = <T>(…) => …` keeps the generic `<T>` so a call site
    // monomorphizes — the const-arrow lowering mirrors `translate_function`'s
    // generic pass-through.
    let rust = Translator::new()
        .translate("export const firstOrDefault = <T>(arr: T[]): T => arr[0];")
        .expect("should translate");
    assert!(
        rust.contains("pub fn first_or_default<T>("),
        "generic lost: {rust}"
    );
}

#[test]
fn export_const_arrow_type_predicate_returns_bool() {
    // A type predicate (`arg is X`) is a TS type guard; its runtime shape is
    // `bool` (the narrowed type is a type-level fiction), so the fn returns
    // `bool` regardless of the predicate's target. A typed (`string`) param
    // and a runtime-typeof-free body keep the signature statically expressible
    // so the predicate-to-bool mapping is what this exercises (an `unknown`
    // param or a `typeof` in the body would force a per-function engine
    // degrade under the B6d #312 const-arrow rule).
    let rust = Translator::new()
        .translate("export const isStr = (s: string): s is string => s.length > 0;")
        .expect("should translate");
    assert!(rust.contains("pub fn is_str("), "no pub fn: {rust}");
    assert!(rust.contains("-> bool"), "predicate not bool: {rust}");
}

#[test]
fn const_arrow_signature_with_unmappable_type_degrades() {
    // B6d #312 extension: a const-arrow fn (`const f = <T>(x): T => …`) whose
    // signature carries a type the static translator cannot express (an
    // indexed access on a generic, `unknown`, …) degrades to the engine the
    // same way a `function` declaration does. The unmappable param/return
    // marshals as `serde_json::Value`, and the body runs under QuickJS via
    // `call_fn`. Without this, the const-arrow would emit `_` for the
    // untypable type and fail cargo check.
    let rust = Translator::new()
        .translate("export const f = (v: unknown): unknown => v;")
        .expect("should translate");
    assert!(
        rust.contains("call_fn"),
        "unmappable signature → engine stub:\n{rust}"
    );
    assert!(
        rust.contains("pub fn f(v: ::serde_json::Value) -> ::serde_json::Value"),
        "degraded signature marshals as Value:\n{rust}"
    );
}

#[test]
fn per_function_degraded_module_with_esm_import_uses_call_module_fn() {
    // A per-function-degraded `.ts` module whose annotation-stripped JS still
    // carries an ESM `import` cannot run under `call_fn`'s script-mode `eval`
    // (ESM imports are not parsed in script mode), so its degraded body routes
    // to `call_module_fn` keyed by the module's import specifier, and the
    // module source lands in the runtime dep's static source table. An
    // `unknown` param forces per-function degradation (the signature is
    // unmappable); the `import` is the ESM binding that triggers module mode.
    use crate::translator::imports;
    imports::set_current_module_specifier(Some("mod_spec".to_string()));
    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            imports::clear_current_module_specifier();
        }
    }
    let _g = Guard;
    let src = "import { x } from \"./other\";\nexport function f(v: unknown): number { return 1; }";
    let (rust, deps) = Translator::new()
        .translate_with_deps_as(src, FileRole::Module)
        .expect("should translate");
    assert!(
        rust.contains("call_module_fn"),
        "module mode should route to call_module_fn:\n{rust}"
    );
    assert!(
        !rust.contains("__DS_MODULE_JS"),
        "module mode should not emit the script-eval const:\n{rust}"
    );
    assert!(
        deps.js_module_sources()
            .iter()
            .any(|(s, _)| s == "mod_spec"),
        "module source should be in the static table: {:?}",
        deps.js_module_sources()
    );
}

#[test]
fn per_function_degraded_without_specifier_keeps_call_fn() {
    // No import specifier (an entry, or a translate outside `ds build`) keeps
    // the script-eval `call_fn` path — module mode needs a specifier to key
    // the loader. So a degraded file with no specifier still emits
    // `__DS_MODULE_JS` + `call_fn` (the established per-function behavior).
    let rust = Translator::new()
        .translate("export function f(v: unknown): number { return 1; }")
        .expect("should translate");
    assert!(rust.contains("call_fn"), "no specifier → call_fn:\n{rust}");
    assert!(
        rust.contains("__DS_MODULE_JS"),
        "no specifier → script-eval const:\n{rust}"
    );
}

#[test]
fn whole_module_degrade_routes_all_functions_to_call_module_fn() {
    // B6-5c: a `.ts` file that imports a degraded `.js` module (an npm package
    // whose export is a generic-callable the translator cannot specialize into a
    // stub) degrades the *whole* module: every top-level function — even ones
    // whose signatures map statically — routes to `call_module_fn`, so the
    // engine resolves the import itself. The project emitter sets the
    // whole-module-degrade flag from `src_imports_degraded_js` plus the import
    // specifier; here both are set directly, then two statically-mappable
    // functions translate.
    use crate::translator::imports;
    imports::set_current_module_specifier(Some("mod_spec".to_string()));
    struct SpecGuard;
    impl Drop for SpecGuard {
        fn drop(&mut self) {
            imports::clear_current_module_specifier();
        }
    }
    let _sg = SpecGuard;
    Translator::set_whole_module_degrade(true);
    struct DegradeGuard;
    impl Drop for DegradeGuard {
        fn drop(&mut self) {
            Translator::set_whole_module_degrade(false);
        }
    }
    let _dg = DegradeGuard;
    let src = "import { sha512 } from \"./other\";\n\
               export function a(): number { return 1; }\n\
               export function b(): number { return 2; }";
    let (rust, deps) = Translator::new()
        .translate_with_deps_as(src, FileRole::Module)
        .expect("should translate");
    assert!(
        rust.contains("call_module_fn"),
        "whole-module degrade should route to call_module_fn:\n{rust}"
    );
    assert!(
        !rust.contains("__DS_MODULE_JS"),
        "module mode should not emit the script-eval const:\n{rust}"
    );
    // Whole-module degrade unions *all* top-level functions, not just one with
    // an unmappable signature — both `a` and `b` route through the loader.
    let module_fn_count = rust.matches("call_module_fn").count();
    assert!(
        module_fn_count >= 2,
        "both functions should route through call_module_fn, found {module_fn_count}:\n{rust}"
    );
    assert!(
        deps.js_module_sources()
            .iter()
            .any(|(s, _)| s == "mod_spec"),
        "module source should be in the static table: {:?}",
        deps.js_module_sources()
    );
}
