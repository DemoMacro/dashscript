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
    assert!(rust.contains("use crate::other::foo"), "got: {rust}");
}

#[test]
fn import_groups_multiple_names() {
    let rust = Translator::new()
        .translate("import { foo, bar } from \"./other\";")
        .expect("should translate");
    assert!(rust.contains("use crate::other::{foo, bar}"), "got: {rust}");
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
    // `use lodash::x;` and `check` passes. Resolution is the build pipeline's
    // job (the third correctness layer); whether it lowers depends on the
    // target. `ds build` resolves `lodash` under `node_modules/`: a `.ts`
    // entry translates, a `.js` entry errors honestly.
    let rust = Translator::new()
        .translate("import { x } from \"lodash\";")
        .expect("should translate");
    assert!(
        rust.contains("use lodash::x"),
        "bare import emitted no use: {rust}"
    );
    let diags = Translator::new().check("import { x } from \"lodash\";");
    assert!(diags.is_empty(), "bare import flagged by check: {diags:?}");
}

#[test]
fn bare_import_normalizes_scope_and_hyphen() {
    // `@scope/pkg-name` → `scope_pkg_name`: a valid Rust module ident (`@` and
    // `-` are illegal in a `use` path). The leading `@` of a scoped package is
    // dropped; hyphens and the scope separator fold to `_`.
    let rust = Translator::new()
        .translate("import { x } from \"@scope/pkg-name\";")
        .expect("should translate");
    assert!(
        rust.contains("use scope_pkg_name::x"),
        "scope/hyphen not normalized: {rust}"
    );
}

#[test]
fn collect_includes_bare_import() {
    // A bare specifier is assembled into a `mod` decl (resolved via
    // `node_modules`), the way a relative import is — unlike a `cargo:`
    // import, which names a Rust crate and is excluded from assembly.
    let imports = Translator::new().imports("import { foo } from \"my-pkg\";");
    assert_eq!(imports.len(), 1);
    assert_eq!(imports[0].module, "my_pkg");
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
        rust.contains("use crate::other::{add, Point}"),
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
    assert!(rust.contains("use crate::geom::Point"), "got: {rust}");
}

#[test]
fn import_namespace_emits_use_alias() {
    // `import * as ns from "./other"` → `use other as ns;` — a module-path
    // alias (not a group leaf). The body then reads members as the path
    // `ns::foo`, the way a Rust `use other as ns;` exposes `ns::foo`.
    let rust = Translator::new()
        .translate("import * as ns from \"./other\";")
        .expect("should translate");
    assert!(rust.contains("use crate::other as ns"), "got: {rust}");
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
    assert!(rust.contains("pub use crate::other::foo"), "got: {rust}");
}

#[test]
fn export_named_from_groups_multiple() {
    // Multiple re-exports group like imports: `pub use other::{foo, bar};`.
    let rust = Translator::new()
        .translate("export { foo, bar } from \"./other\";")
        .expect("should translate");
    assert!(
        rust.contains("pub use crate::other::{foo, bar}"),
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
    assert!(rust.contains("pub use crate::other::*"), "got: {rust}");
}

#[test]
fn export_all_as_namespace_emits_alias() {
    // `export * as ns from "./m"` → `pub use m as ns;` — a namespace re-export;
    // importers read its members as `ns::foo`.
    let rust = Translator::new()
        .translate("export * as ns from \"./other\";")
        .expect("should translate");
    assert!(rust.contains("pub use crate::other as ns"), "got: {rust}");
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
fn translate_with_deps_module_rejects_top_level_executable() {
    // Module role + a top-level executable statement (`console.log`) → Err: a
    // module does not execute, and top-level statements have no entry to run in
    // (a Node module only exports; it does not run top-level statements). Reject
    // rather than silently drop.
    let err = Translator::new()
        .translate_with_deps_as(
            "export function helper(): void {}\nconsole.log(1);",
            FileRole::Module,
        )
        .expect_err("module with top-level executable should error");
    assert!(
        err.contains("module file may only declare"),
        "wrong error: {err}"
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
    // `bool` regardless of the predicate's target.
    let rust = Translator::new()
        .translate("export const isStr = (s: unknown): s is string => typeof s === \"string\";")
        .expect("should translate");
    assert!(rust.contains("pub fn is_str("), "no pub fn: {rust}");
    assert!(rust.contains("-> bool"), "predicate not bool: {rust}");
}
