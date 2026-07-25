// End-to-end tests for module declarations (export/import).
use super::super::Translator;

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
    assert!(rust.contains("use other::foo"), "got: {rust}");
}

#[test]
fn import_groups_multiple_names() {
    let rust = Translator::new()
        .translate("import { foo, bar } from \"./other\";")
        .expect("should translate");
    assert!(rust.contains("use other::{foo, bar}"), "got: {rust}");
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
fn bare_import_is_unsupported() {
    // A bare specifier (`lodash`) has no resolver — DashScript supports only
    // `cargo:` and relative imports. The translator emits no `use`, and
    // `check` flags it as unsupported.
    let rust = Translator::new()
        .translate("import { x } from \"lodash\";")
        .expect("should translate");
    assert!(
        !rust.contains("use lodash"),
        "bare import emitted a use: {rust}"
    );
    let diags = Translator::new().check("import { x } from \"lodash\";");
    assert!(!diags.is_empty(), "bare import not flagged by check");
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
    assert!(rust.contains("use other::{add, Point}"), "got: {rust}");
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
    assert!(rust.contains("use geom::Point"), "got: {rust}");
}

#[test]
fn import_namespace_emits_use_alias() {
    // `import * as ns from "./other"` → `use other as ns;` — a module-path
    // alias (not a group leaf). The body then reads members as the path
    // `ns::foo`, the way a Rust `use other as ns;` exposes `ns::foo`.
    let rust = Translator::new()
        .translate("import * as ns from \"./other\";")
        .expect("should translate");
    assert!(rust.contains("use other as ns"), "got: {rust}");
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
    assert!(rust.contains("pub use other::foo"), "got: {rust}");
}

#[test]
fn export_named_from_groups_multiple() {
    // Multiple re-exports group like imports: `pub use other::{foo, bar};`.
    let rust = Translator::new()
        .translate("export { foo, bar } from \"./other\";")
        .expect("should translate");
    assert!(rust.contains("pub use other::{foo, bar}"), "got: {rust}");
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
    assert!(rust.contains("pub use other::*"), "got: {rust}");
}

#[test]
fn export_all_as_namespace_emits_alias() {
    // `export * as ns from "./m"` → `pub use m as ns;` — a namespace re-export;
    // importers read its members as `ns::foo`.
    let rust = Translator::new()
        .translate("export * as ns from \"./other\";")
        .expect("should translate");
    assert!(rust.contains("pub use other as ns"), "got: {rust}");
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
fn top_level_function_referencing_top_level_var_is_unsupported() {
    // A top-level `function` reading a top-level `const` would close over a
    // `fn main` local — impossible for a Rust fn item. Flag it honestly rather
    // than letting it fail `cargo check` as a partial.
    let diags = Translator::new().check("const x: number = 5;\nfunction f(): number { return x; }");
    assert!(!diags.is_empty(), "escape not flagged: {diags:?}");
}

#[test]
fn top_level_main_call_passes_check() {
    // `function main` is renamed `__ds_main`; a top-level `main()` call is now
    // an ordinary executable statement (it invokes `__ds_main()` from the
    // implicit `fn main`) — no longer a special-cased entry to skip.
    let diags = Translator::new().check("function main(): void { console.log(1); }\nmain();");
    assert!(diags.is_empty(), "top-level main() call flagged: {diags:?}");
}
