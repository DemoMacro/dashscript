use super::*;
use std::fs;

fn write(dir: &Path, name: &str, body: &str) {
    fs::write(dir.join(name), body).unwrap();
}

fn package_at(root: &Path) -> Package {
    read_package(&root.join("package.json")).unwrap()
}

/// Recursively collect every `*.rs` file under `dir`. The emit-tree layout
/// (`app/`, `third_party/`, `__ds/`, synthesized `mod.rs`) puts dep
/// artifacts in subdirectories, so tests that scan `src/` for a marker
/// must walk the whole tree, not just the top level.
fn collect_rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            collect_rust_files(&p, out);
        } else if p.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(p);
        }
    }
}

/// Create a directory symlink, cross-platform. Used by the workspace-dep
/// tests to model a pnpm/npm `node_modules/<pkg>` entry pointing at a
/// sibling package. Returns an error where symlinks are unsupported or
/// (Windows) without developer mode / admin — the caller treats that as a
/// skip, not a failure.
fn make_symlink(original: &Path, link: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(original, link)
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_dir(original, link)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (original, link);
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "symlinks unsupported on this platform",
        ))
    }
}

#[test]
fn translate_project_emits_per_file_bins() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(
        root,
        "package.json",
        r#"{ "name": "app", "bin": { "a": "a.ts", "b": "b.ts" } }"#,
    );
    write(root, "a.ts", "function main() { console.log(1); }");
    write(root, "b.ts", "function main() { console.log(2); }");

    let out = tmp.path().join("out");
    let ((bins, lib), _deps) = translate_project(root, &package_at(root), &out, None).unwrap();
    let names: Vec<&str> = bins.iter().map(|(n, _)| n.as_str()).collect();
    assert!(names.contains(&"a"), "bins: {bins:?}");
    assert!(names.contains(&"b"), "bins: {bins:?}");
    assert!(lib.is_none());
    assert!(out.join("src").join("a.rs").exists(), "src/a.rs missing");
    assert!(out.join("src").join("b.rs").exists(), "src/b.rs missing");
}

#[test]
fn translate_project_preserves_nested_same_stem() {
    // The source directory tree is preserved, so two files sharing a stem in
    // different directories no longer collide: dup.ts → src/app/dup.rs and
    // sub/dup.ts → src/app/sub/dup.rs (with src/app/sub/mod.rs declaring it).
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("sub")).unwrap();
    write(
        root,
        "package.json",
        r#"{ "name": "app", "bin": "main.ts" }"#,
    );
    write(
            root,
            "main.ts",
            "import { helper } from \"./dup\";\nimport { other } from \"./sub/dup\";\nfunction main() { helper(); other(); }",
        );
    write(root, "dup.ts", "export function helper() {}");
    write(&root.join("sub"), "dup.ts", "export function other() {}");

    let out = tmp.path().join("out");
    translate_project(root, &package_at(root), &out, None).unwrap();
    assert!(
        out.join("src").join("app").join("dup.rs").exists(),
        "src/app/dup.rs missing"
    );
    assert!(
        out.join("src")
            .join("app")
            .join("sub")
            .join("dup.rs")
            .exists(),
        "src/app/sub/dup.rs missing"
    );
    assert!(
        out.join("src")
            .join("app")
            .join("sub")
            .join("mod.rs")
            .exists(),
        "src/app/sub/mod.rs missing"
    );
}

#[test]
fn translate_project_detects_bin_imports_bin() {
    // cargo forbids one bin from mod-ing another; shared code must go
    // through a lib module. The guard surfaces this before cargo does.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(
        root,
        "package.json",
        r#"{ "name": "app", "bin": { "a": "a.ts", "b": "b.ts" } }"#,
    );
    write(
        root,
        "a.ts",
        "import { x } from \"./b\";\nfunction main() {}",
    );
    write(root, "b.ts", "export function x() {}\nfunction main() {}");

    let out = tmp.path().join("out");
    let err = translate_project(root, &package_at(root), &out, None).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("bin 'a' imports bin 'b'"), "got: {msg}");
}

#[test]
fn resolve_local_module_finds_bare_stem() {
    // `./foo` resolves to `foo.ts` (extensionless, bundler-style).
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(root, "foo.ts", "export function foo() {}");
    let (resolved, _) = resolve_local_module(root, "./foo").unwrap();
    assert_eq!(resolved, root.join("foo.ts"));
}

#[test]
fn translate_dep_degrades_js_class_extends() {
    // A `.js` module whose class `extends` another cannot lower statically,
    // so translate_dep emits engine-forwarding stub fns and flags the
    // engine runtime dep — instead of the static translator's
    // `compile_error!`.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(
        root,
        "dep.js",
        "class A extends B {}\nexport function f(x) { return x; }",
    );
    let translator = Translator::new();
    let mut deps = RuntimeDeps::empty();
    let rust = translate_dep(
        &translator,
        &root.join("dep.js"),
        DepKind::Js,
        "dep.js",
        None,
        &mut deps,
    )
    .expect("a class-extends .js degrades");
    assert!(rust.contains("pub fn f"), "stub fn emitted: {rust}");
    assert!(
        rust.contains("call_module_fn"),
        "stub forwards to the engine: {rust}"
    );
    assert!(
        !rust.contains("compile_error"),
        "no static compile_error leaked: {rust}"
    );
    assert!(deps.needs_engine(), "engine runtime dep flagged");
}

#[test]
fn translate_dep_keeps_static_js_without_extends() {
    // A `.js` module without `extends` stays on the static path (no stub).
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(
        root,
        "dep.js",
        "export function add(a, b) { return a + b; }",
    );
    let translator = Translator::new();
    let mut deps = RuntimeDeps::empty();
    let rust = translate_dep(
        &translator,
        &root.join("dep.js"),
        DepKind::Js,
        "dep.js",
        None,
        &mut deps,
    )
    .expect("a plain .js transpiles");
    assert!(
        !rust.contains("call_module_fn"),
        "no engine stub for a static .js: {rust}"
    );
    assert!(!deps.needs_engine());
}

#[test]
fn degrade_registers_under_import_specifier_not_path() {
    // A degraded `.js` registers under its import specifier (the runtime
    // `DsResolver` finds bare specifiers verbatim), not its filesystem path —
    // a `package.json` `exports` map may diverge the two.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(
        root,
        "dep.js",
        "class A extends B {}\nexport function f(x) { return x; }",
    );
    let translator = Translator::new();
    let mut deps = RuntimeDeps::empty();
    let rust = translate_dep(
        &translator,
        &root.join("dep.js"),
        DepKind::Js,
        "@scope/pkg/dep.js",
        None,
        &mut deps,
    )
    .expect("a class-extends .js degrades");
    assert!(
        rust.contains("\"@scope/pkg/dep.js\""),
        "stub registers under the import specifier: {rust}"
    );
    assert!(
        !rust.contains(&root.display().to_string().replace('\\', "/")),
        "stub does not use the filesystem path: {rust}"
    );
}

#[test]
fn degrade_registers_transitive_js_under_resolver_specifier() {
    // `a.js` (class extends → degrades) imports `b.js` (only `export const`,
    // no `export function`). Both must land in the build-time source table
    // under their DsResolver specifiers — `a.js` under the bare import
    // specifier, `b.js` under the joined specifier (`pkg/` + `b.js`) — so the
    // runtime loader resolves the transitive import even though `b.js` emits
    // no stub fn and is never runtime-registered.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(
        root,
        "a.js",
        "import { x } from \"./b.js\";\nclass A extends B {}\nexport function f() { return x; }",
    );
    write(root, "b.js", "export const x = 42;");
    let translator = Translator::new();
    let mut deps = RuntimeDeps::empty();
    translate_dep(
        &translator,
        &root.join("a.js"),
        DepKind::Js,
        "pkg/a.js",
        None,
        &mut deps,
    )
    .expect("a.js degrades");
    let sources = deps.js_module_sources();
    assert!(
        sources.iter().any(|(s, _)| s == "pkg/a.js"),
        "a.js registered under its specifier: {sources:?}"
    );
    assert!(
            sources.iter().any(|(s, _)| s == "pkg/b.js"),
            "b.js registered under the joined specifier (pkg/b.js), not \"./b.js\" or a path: {sources:?}"
        );
}

#[test]
fn record_workspace_dep_marks_node_modules_js_as_local() {
    // A bare specifier resolving to a node_modules `.js` degrades to an
    // in-crate mod stub, so it must be recorded as local — the consumer's
    // `use` path is `crate::…`, not a bare `mod` (Rust 2018 path clarity
    // rejects the bare form from a sibling module).
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let pkg = root.join("node_modules").join("my-pkg");
    fs::create_dir_all(&pkg).unwrap();
    write(
        &pkg,
        "package.json",
        r#"{ "name": "my-pkg", "main": "index.js" }"#,
    );
    write(&pkg, "index.js", "export function f(x) { return x; }");
    let mut deps = std::collections::HashSet::new();
    record_workspace_dep("my-pkg", root, &mut deps);
    assert!(
        deps.contains("my-pkg"),
        "node_modules .js should be recorded as a local mod: {deps:?}"
    );
}

#[test]
fn emit_cargo_project_degrades_js_dep_with_class_extends() {
    // End-to-end: an entry importing a `.js` dep with a class `extends`
    // emits engine-forwarding stubs for that dep and the `__ds::engine`
    // helper module (the stubs call into it), flagged via the engine
    // runtime dep on the emitted Cargo.toml.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(
        root,
        "package.json",
        r#"{ "name": "probe", "bin": "main.ts" }"#,
    );
    write(
        root,
        "main.ts",
        "import { f } from \"./dep.js\";\nfunction main() { console.log(f(3)); }",
    );
    write(
        root,
        "dep.js",
        "class A extends B {}\nexport function f(x) { return x + 1; }",
    );
    let out = tmp.path().join("out");
    let main_path = root.join("main.ts");
    let src = fs::read_to_string(&main_path).unwrap();
    emit_cargo_project(&src, &main_path, &out).unwrap();
    let mut files = Vec::new();
    collect_rust_files(&out.join("src"), &mut files);
    let mut stub = String::new();
    for p in files {
        let body = fs::read_to_string(&p).unwrap();
        if body.contains("crate::__ds::engine::call_module_fn") {
            stub = body;
        }
    }
    assert!(stub.contains("pub fn f"), "stub fn emitted: {stub}");
    assert!(
        !stub.contains("register_js_module"),
        "stub does not re-inline source (single copy in __DS_MODULE_SOURCES): {stub}"
    );
    assert!(
        out.join("src").join("__ds").join("engine.rs").exists(),
        "__ds::engine helper emitted"
    );
    let engine = fs::read_to_string(out.join("src").join("__ds").join("engine.rs")).unwrap();
    assert!(
        engine.contains("__DS_MODULE_SOURCES"),
        "build-time source table emitted: {engine}"
    );
    assert!(
        engine.contains("class A extends B"),
        "dep source embedded once in the build-time table: {engine}"
    );
    let cargo = fs::read_to_string(out.join("Cargo.toml")).unwrap();
    assert!(
        cargo.contains("rquickjs") || cargo.contains("rquickjs-sys"),
        "engine crate dep on Cargo.toml: {cargo}"
    );
}

#[test]
fn emit_cargo_project_specializes_stub_from_marshal_safe_dts() {
    // A `.js` dep with a class `extends` degrades to engine stubs, and a
    // sibling `.d.ts` with a marshal-safe `declare function` signature
    // specializes the stub: `bytesToHex(b: Uint8Array): string` becomes
    // `pub fn bytesToHex(__ds_p0: Vec<u8>) -> String`, marshaling via
    // serde_json rather than `Value` end to end. (The entry only imports
    // it; whether the call site stays type-correct is a separate concern.)
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(
        root,
        "package.json",
        r#"{ "name": "probe", "bin": "main.ts" }"#,
    );
    write(
        root,
        "main.ts",
        "import { bytesToHex } from \"./dep.js\";\nfunction main() {}",
    );
    write(
        root,
        "dep.js",
        "class A extends B {}\nexport function bytesToHex(b) { return b; }",
    );
    write(
        root,
        "dep.d.ts",
        "declare function bytesToHex(b: Uint8Array): string;",
    );
    let out = tmp.path().join("out");
    let main_path = root.join("main.ts");
    let src = fs::read_to_string(&main_path).unwrap();
    emit_cargo_project(&src, &main_path, &out).unwrap();
    let mut files = Vec::new();
    collect_rust_files(&out.join("src"), &mut files);
    let mut stub = String::new();
    for p in files {
        let body = fs::read_to_string(&p).unwrap();
        if body.contains("crate::__ds::engine::call_module_fn") {
            stub = body;
        }
    }
    assert!(stub.contains("pub fn bytesToHex"), "stub emitted: {stub}");
    assert!(
        stub.contains("Vec<u8>") && stub.contains("-> String"),
        "Uint8Array param + string return specialized: {stub}"
    );
    assert!(
        stub.contains("serde_json::from_value") && stub.contains("serde_json::to_value"),
        "marshal via serde_json: {stub}"
    );
}

#[test]
fn emit_cargo_project_falls_back_to_value_for_unmappable_dts() {
    // A `.d.ts` signature with a non-marshal-safe type (a `string | number`
    // union lowers to a generated enum, not a JSON scalar) does not
    // specialize the stub — it stays `serde_json::Value` end to end.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(
        root,
        "package.json",
        r#"{ "name": "probe", "bin": "main.ts" }"#,
    );
    write(
        root,
        "main.ts",
        "import { pick } from \"./dep.js\";\nfunction main() {}",
    );
    write(
        root,
        "dep.js",
        "class A extends B {}\nexport function pick(x) { return x; }",
    );
    write(
        root,
        "dep.d.ts",
        "declare function pick(x: string | number): string;",
    );
    let out = tmp.path().join("out");
    let main_path = root.join("main.ts");
    let src = fs::read_to_string(&main_path).unwrap();
    emit_cargo_project(&src, &main_path, &out).unwrap();
    let mut files = Vec::new();
    collect_rust_files(&out.join("src"), &mut files);
    let mut stub = String::new();
    for p in files {
        let body = fs::read_to_string(&p).unwrap();
        if body.contains("crate::__ds::engine::call_module_fn") {
            stub = body;
        }
    }
    assert!(
        stub.contains("pub fn pick(__ds_p0: serde_json::Value) -> serde_json::Value"),
        "Value stub for an unmappable signature: {stub}"
    );
}

#[test]
fn resolve_local_module_honors_explicit_extension() {
    // An explicit `./foo.ts` is honored as-is.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(root, "foo.ts", "export function foo() {}");
    let (resolved, _) = resolve_local_module(root, "./foo.ts").unwrap();
    assert_eq!(resolved, root.join("foo.ts"));
}

#[test]
fn resolve_local_module_falls_back_to_index_barrel() {
    // No `foo.ts` → fall back to `foo/index.ts` (bundler barrel).
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("foo")).unwrap();
    write(&root.join("foo"), "index.ts", "export function foo() {}");
    let (resolved, _) = resolve_local_module(root, "./foo").unwrap();
    assert_eq!(resolved, root.join("foo").join("index.ts"));
}

#[test]
fn resolve_local_module_prefers_direct_file_over_barrel() {
    // `foo.ts` wins over `foo/index.ts` (bundler order).
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("foo")).unwrap();
    write(root, "foo.ts", "export function direct() {}");
    write(&root.join("foo"), "index.ts", "export function barrel() {}");
    let (resolved, _) = resolve_local_module(root, "./foo").unwrap();
    assert_eq!(resolved, root.join("foo.ts"));
}

#[test]
fn resolve_node_module_package_root_via_main() {
    // A bare `my-pkg` resolves under `node_modules/my-pkg/`, its entry from
    // `package.json` `main`. A `.ts` entry translates.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let pkg = root.join("node_modules").join("my-pkg");
    fs::create_dir_all(&pkg).unwrap();
    write(
        &pkg,
        "package.json",
        r#"{ "name": "my-pkg", "main": "index.ts" }"#,
    );
    write(
        &pkg,
        "index.ts",
        "export function foo(): number { return 1; }",
    );
    let (resolved, _) = resolve_local_module(root, "my-pkg").unwrap();
    assert_eq!(resolved, pkg.join("index.ts"));
}

#[test]
fn resolve_node_module_subpath_direct_file() {
    // A bare subpath `my-pkg/util` resolves directly to `util.ts` under the
    // package dir (no `package.json` consultation).
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let pkg = root.join("node_modules").join("my-pkg");
    fs::create_dir_all(&pkg).unwrap();
    write(&pkg, "util.ts", "export function u(): number { return 1; }");
    let (resolved, _) = resolve_local_module(root, "my-pkg/util").unwrap();
    assert_eq!(resolved, pkg.join("util.ts"));
}

#[test]
fn resolve_node_module_js_entry_is_js_kind() {
    // A package whose entry is `.js` (no `.ts`/`.d.ts`) resolves to a `Js`
    // kind. Resolve no longer errors on `.js`; the translate loop transpiles
    // it first (a `.js` is JS-flavored TS) and falls back to the engine only
    // for dynamic JS the table cannot lower.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let pkg = root.join("node_modules").join("js-pkg");
    fs::create_dir_all(&pkg).unwrap();
    write(
        &pkg,
        "package.json",
        r#"{ "name": "js-pkg", "main": "index.js" }"#,
    );
    write(&pkg, "index.js", "export function x() { return 1; }");
    let (resolved, kind) = resolve_local_module(root, "js-pkg").unwrap();
    assert_eq!(resolved, pkg.join("index.js"));
    assert!(
        matches!(kind, DepKind::Js),
        "js entry should be Js kind: {kind:?}"
    );
}

#[test]
fn resolve_workspace_dep_follows_symlink_to_src() {
    // A workspace-local package: `node_modules/@scope/b` symlinks to a
    // sibling package dir; its `src/` source is resolved (bypassing
    // `exports`/`main`, which would point at an unbuilt `dist/`).
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let pkg_b = root.join("packages").join("b");
    fs::create_dir_all(pkg_b.join("src").join("sub")).unwrap();
    write(&pkg_b.join("src"), "index.ts", "export function b() {}");
    write(
        &pkg_b.join("src").join("sub"),
        "index.ts",
        "export function sub() {}",
    );
    let scope_nm = root.join("node_modules").join("@scope");
    fs::create_dir_all(&scope_nm).unwrap();
    if make_symlink(&pkg_b, &scope_nm.join("b")).is_err() {
        eprintln!("resolve_workspace_dep_follows_symlink_to_src: skipped (no symlink privilege)");
        return;
    }
    // Resolve from a sibling package `packages/a/src` — the resolver walks
    // up to the shared `node_modules`.
    let base = root.join("packages").join("a").join("src");
    fs::create_dir_all(&base).unwrap();
    let (resolved, kind) = resolve_workspace_dep(&base, "@scope/b").expect("local pkg");
    assert!(matches!(kind, DepKind::Ts), "got: {kind:?}");
    assert_eq!(
        fs::canonicalize(&resolved).unwrap(),
        fs::canonicalize(pkg_b.join("src").join("index.ts")).unwrap()
    );
    // Subpath → `src/sub/index.ts` (directory barrel).
    let (resolved, _) = resolve_workspace_dep(&base, "@scope/b/sub").expect("subpath");
    assert_eq!(
        fs::canonicalize(&resolved).unwrap(),
        fs::canonicalize(pkg_b.join("src").join("sub").join("index.ts")).unwrap()
    );
    // Also reachable through the public `resolve_local_module`.
    let (resolved, _) = resolve_local_module(&base, "@scope/b").unwrap();
    assert_eq!(
        fs::canonicalize(&resolved).unwrap(),
        fs::canonicalize(pkg_b.join("src").join("index.ts")).unwrap()
    );
}

#[test]
fn translate_sources_disambiguates_cross_package_same_stem() {
    // Two workspace packages both ship a `types.ts`. Without the member
    // prefix both lower to `crate::types` and the second clobbers the first
    // (the cross-package stem collision between two same-stem members); with it, package
    // `b`'s file becomes `crate::scope_b_types` while the entry's own stays
    // `crate::types`. The emit filename, `mod` decl, and `use` path all
    // derive from `dep_mod_name` so they agree.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let pkg_a = root.join("packages").join("a");
    let pkg_b = root.join("packages").join("b");
    fs::create_dir_all(pkg_a.join("src")).unwrap();
    fs::create_dir_all(pkg_b.join("src")).unwrap();
    write(&pkg_a, "package.json", r#"{ "name": "@scope/a" }"#);
    write(&pkg_b, "package.json", r#"{ "name": "@scope/b" }"#);
    write(
        &pkg_a.join("src"),
        "index.ts",
        "import { X } from \"./types\";\nimport { Y } from \"@scope/b\";\nfunction main() {}",
    );
    write(
        &pkg_a.join("src"),
        "types.ts",
        "export interface X { a: number }",
    );
    write(&pkg_b.join("src"), "index.ts", "export * from \"./types\";");
    write(
        &pkg_b.join("src"),
        "types.ts",
        "export interface Y { b: number }",
    );
    let scope_nm = root.join("node_modules").join("@scope");
    fs::create_dir_all(&scope_nm).unwrap();
    if make_symlink(&pkg_b, &scope_nm.join("b")).is_err() {
        eprintln!(
                "translate_sources_disambiguates_cross_package_same_stem: skipped (no symlink privilege)"
            );
        return;
    }
    let out = tmp.path().join("out");
    fs::create_dir_all(out.join("src")).unwrap();
    let entry = pkg_a.join("src").join("index.ts");
    let src = fs::read_to_string(&entry).unwrap();
    translate_sources(&src, &entry, &out).expect("translate");
    let dir = out.join("src");
    // Entry's own types.ts → src/types.rs (no prefix).
    assert!(dir.join("types.rs").exists(), "entry types.rs missing");
    // Package b's types.ts → src/scope_b_types.rs (member prefix).
    assert!(
        dir.join("scope_b_types.rs").exists(),
        "cross-package types.rs should be prefixed; src has: {:?}",
        fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
    );
    let main = fs::read_to_string(dir.join("main.rs")).unwrap();
    assert!(main.contains("mod types;"), "entry mod decl: {main}");
    assert!(
        main.contains("mod scope_b_types;"),
        "prefixed mod decl: {main}"
    );
    // The barrel (b/index.ts) reaches its own `./types` via the prefixed
    // path too — `use crate::scope_b_types::*`, not `crate::types::*`.
    let barrel = fs::read_to_string(dir.join("scope_b.rs")).unwrap();
    assert!(
        barrel.contains("crate::scope_b_types"),
        "barrel uses the member-prefixed use path: {barrel}"
    );
}

#[test]
fn translate_project_skips_cross_member_emit_and_records_path_dep() {
    // Independent-crate model on the translate_project path: a bare import
    // of a sibling workspace member becomes a cargo path dep, not a merged
    // local module. translate_project records the dep and emits nothing for
    // the member — its source lives in its own crate. (translate_sources
    // still merges cross-member when called directly, but `ds build` now
    // routes workspace members here, so that path is dormant on live builds;
    // its cleanup was deferred — see translate_sources's routing note.)
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let pkg_a = root.join("packages").join("a");
    let pkg_b = root.join("packages").join("b");
    fs::create_dir_all(pkg_a.join("src")).unwrap();
    fs::create_dir_all(pkg_b.join("src")).unwrap();
    write(
        &pkg_a,
        "package.json",
        r#"{ "name": "@scope/a", "main": "src/index.ts" }"#,
    );
    write(&pkg_b, "package.json", r#"{ "name": "@scope/b" }"#);
    write(
        &pkg_a.join("src"),
        "index.ts",
        "import { Y } from \"@scope/b\";\nexport const Z = Y;",
    );
    write(
        &pkg_b.join("src"),
        "index.ts",
        "export const Y: number = 1;",
    );
    let scope_nm = root.join("node_modules").join("@scope");
    fs::create_dir_all(&scope_nm).unwrap();
    if make_symlink(&pkg_b, &scope_nm.join("b")).is_err() {
        eprintln!(
            "translate_project_skips_cross_member_emit_and_records_path_dep: \
                 skipped (no symlink privilege)"
        );
        return;
    }
    let out = tmp.path().join("out");
    let package = read_package(&pkg_a.join("package.json")).unwrap();
    let (_targets, deps) = translate_project(&pkg_a, &package, &out, None).expect("translate");
    let dir = out.join("src");
    assert!(
        !dir.join("scope_b.rs").exists(),
        "cross-member must not emit into this crate; src has: {:?}",
        fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
    );
    assert!(
        deps.path_deps().contains("scope_b"),
        "path_deps should record the member crate: {:?}",
        deps.path_deps()
    );
}

#[test]
fn translate_project_subpath_import_records_member_crate_not_subpath() {
    // A sub-path bare specifier (`@scope/b/sub`) is one cargo dep on the
    // member crate `scope_b` — the sub-path is a module inside that crate,
    // not a separate crate. Without splitting the package root off the
    // specifier, `module_ident` would sanitize the whole string and the
    // path dep would name a phantom `scope_b_sub` crate that does not exist.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let pkg_a = root.join("packages").join("a");
    let pkg_b = root.join("packages").join("b");
    fs::create_dir_all(pkg_a.join("src")).unwrap();
    fs::create_dir_all(pkg_b.join("src").join("sub")).unwrap();
    write(
        &pkg_a,
        "package.json",
        r#"{ "name": "@scope/a", "main": "src/index.ts" }"#,
    );
    write(&pkg_b, "package.json", r#"{ "name": "@scope/b" }"#);
    write(
        &pkg_a.join("src"),
        "index.ts",
        "import { Y } from \"@scope/b/sub\";\nexport const Z = Y;",
    );
    write(
        &pkg_b.join("src").join("sub"),
        "index.ts",
        "export const Y: number = 1;",
    );
    let scope_nm = root.join("node_modules").join("@scope");
    fs::create_dir_all(&scope_nm).unwrap();
    if make_symlink(&pkg_b, &scope_nm.join("b")).is_err() {
        eprintln!(
            "translate_project_subpath_import_records_member_crate_not_subpath: \
                 skipped (no symlink privilege)"
        );
        return;
    }
    let out = tmp.path().join("out");
    let package = read_package(&pkg_a.join("package.json")).unwrap();
    let (_targets, deps) = translate_project(&pkg_a, &package, &out, None).expect("translate");
    assert!(
        deps.path_deps().contains("scope_b"),
        "sub-path import must record the member crate: {:?}",
        deps.path_deps()
    );
    assert!(
        !deps.path_deps().contains("scope_b_sub"),
        "sub-path must not synthesize a phantom crate: {:?}",
        deps.path_deps()
    );
}

#[test]
fn walk_ts_skips_co_located_test_files() {
    // `.spec.ts`/`.test.ts` are co-located tests — they exercise the crate,
    // not part of it. walk_ts must skip them so a package's own test
    // assertions (top-level executable statements) don't land in a module
    // file, which has no entry to run them.
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    fs::create_dir_all(&src).unwrap();
    write(&src, "escape.ts", "export function f() {}");
    write(&src, "escape.spec.ts", "assert(true);");
    write(&src, "escape.test.ts", "assert(true);");
    write(&src, "escape.bench.ts", "bench(() => 1);");
    let mut out = Vec::new();
    walk_ts(&src, &mut out);
    let names: Vec<String> = out
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert!(
        names.contains(&"escape.ts".to_string()),
        "source kept: {names:?}"
    );
    assert!(
        !names.iter().any(|n| n.contains(".spec")),
        "spec skipped: {names:?}"
    );
    assert!(
        !names.iter().any(|n| n.contains(".test")),
        "test skipped: {names:?}"
    );
    assert!(
        !names.iter().any(|n| n.contains(".bench")),
        "bench skipped: {names:?}"
    );
}

#[test]
fn resolve_workspace_dep_ignores_pnpm_store() {
    // A pnpm-store package is also a symlink, but its target is under
    // `node_modules/.pnpm/` — it must NOT be treated as a local source
    // package (it is a registry package; left to the standard resolver).
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let store_pkg = root
        .join("node_modules")
        .join(".pnpm")
        .join("reg@1.0.0")
        .join("node_modules")
        .join("reg");
    fs::create_dir_all(store_pkg.join("src")).unwrap();
    write(&store_pkg.join("src"), "index.ts", "export function r() {}");
    let nm = root.join("node_modules").join("reg");
    if make_symlink(&store_pkg, &nm).is_err() {
        eprintln!("resolve_workspace_dep_ignores_pnpm_store: skipped (no symlink privilege)");
        return;
    }
    let base = root.join("packages").join("a").join("src");
    fs::create_dir_all(&base).unwrap();
    assert!(
        resolve_workspace_dep(&base, "reg").is_none(),
        "a .pnpm-store symlink must not be treated as a local source package"
    );
}

#[test]
fn resolve_node_module_dts_with_js_pair() {
    // A package with both `.d.ts` and `.js` (a typed npm package) resolves
    // to `DtsWithJs` — the `.d.ts` entry (the `types` field wins over
    // `main`) carries its sibling `.js` as the implementation path. The
    // translate loop then emits the `.d.ts` types alongside the transpiled
    // `.js`.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let pkg = root.join("node_modules").join("typed-pkg");
    fs::create_dir_all(&pkg).unwrap();
    write(
        &pkg,
        "package.json",
        r#"{ "name": "typed-pkg", "main": "index.js", "types": "index.d.ts" }"#,
    );
    write(
        &pkg,
        "index.d.ts",
        "export declare function f(a: number): number;",
    );
    write(&pkg, "index.js", "export function f(a) { return a; }");
    let (resolved, kind) = resolve_local_module(root, "typed-pkg").unwrap();
    assert_eq!(resolved, pkg.join("index.d.ts"));
    match kind {
        DepKind::DtsWithJs { dts_path, js_path } => {
            assert_eq!(dts_path, pkg.join("index.d.ts"));
            assert_eq!(js_path, pkg.join("index.js"));
        }
        other => panic!("expected DtsWithJs, got {other:?}"),
    }
}

#[test]
fn resolve_node_module_walks_up_for_node_modules() {
    // Node resolution walks up for `node_modules/`: a package imported from
    // a nested source dir resolves to the project-root `node_modules/`.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let pkg = root.join("node_modules").join("shared");
    fs::create_dir_all(&pkg).unwrap();
    write(
        &pkg,
        "package.json",
        r#"{ "name": "shared", "main": "index.ts" }"#,
    );
    write(
        &pkg,
        "index.ts",
        "export function s(): number { return 1; }",
    );
    let nested = root.join("src").join("deep");
    fs::create_dir_all(&nested).unwrap();
    let (resolved, _) = resolve_local_module(&nested, "shared").unwrap();
    assert_eq!(resolved, pkg.join("index.ts"));
}

#[test]
fn resolve_node_module_scoped_package() {
    // A scoped package `@scope/pkg` resolves under
    // `node_modules/@scope/pkg/`. The hand-written resolver split on the
    // first `/` and mishandled scopes; oxc_resolver handles them natively.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let pkg = root.join("node_modules").join("@scope").join("pkg");
    fs::create_dir_all(&pkg).unwrap();
    write(
        &pkg,
        "package.json",
        r#"{ "name": "@scope/pkg", "main": "index.ts" }"#,
    );
    write(
        &pkg,
        "index.ts",
        "export function s(): number { return 1; }",
    );
    let (resolved, _) = resolve_local_module(root, "@scope/pkg").unwrap();
    assert_eq!(resolved, pkg.join("index.ts"));
}

#[test]
fn resolve_node_module_exports_field_import_condition() {
    // A modern package with an `exports` field: the `import` condition
    // points at the ESM entry, `require` at the CJS one. oxc_resolver reads
    // `exports` with the configured conditions — the hand-written resolver
    // could not (it only scanned `main`/`module`/`types`).
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let pkg = root.join("node_modules").join("mod-pkg");
    fs::create_dir_all(&pkg).unwrap();
    write(
        &pkg,
        "package.json",
        r#"{ "name": "mod-pkg", "exports": { ".": { "import": "./esm.mjs", "require": "./cjs.cjs" } } }"#,
    );
    write(&pkg, "esm.mjs", "export function x() { return 1; }");
    write(&pkg, "cjs.cjs", "module.exports = {};");
    let (resolved, kind) = resolve_local_module(root, "mod-pkg").unwrap();
    // The `import` condition wins (configured `condition_names`), so the
    // ESM entry resolves. `.mjs` is a `Js` kind — transpiled first (ESM
    // `export function` lowers like a `.ts` module); the CJS `.cjs` arm is
    // reached only under a `require` condition.
    assert_eq!(resolved, pkg.join("esm.mjs"));
    assert!(
        matches!(kind, DepKind::Js),
        ".mjs entry should be Js kind: {kind:?}"
    );
}

#[test]
fn stem_of_index_in_subdir_is_parent_dir() {
    // foo/index.ts is a barrel → module name "foo" (the parent dir).
    assert_eq!(stem_of(Path::new("foo/index.ts")), "foo");
}

#[test]
fn stem_of_root_index_keeps_own_stem() {
    // A root index.ts is an entry, not a barrel — keeps stem "index".
    assert_eq!(stem_of(Path::new("index.ts")), "index");
}

#[test]
fn translate_project_emits_barrel_module() {
    // entry imports "./foo" → foo/index.ts → src/foo/mod.rs (a subdirectory
    // barrel becomes the directory's mod), so `mod foo;` resolves. The barrel
    // must not emit src/index.rs.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("foo")).unwrap();
    write(
        root,
        "package.json",
        r#"{ "name": "app", "bin": "main.ts" }"#,
    );
    write(
        root,
        "main.ts",
        "import { foo } from \"./foo\";\nfunction main() { foo(); }",
    );
    write(&root.join("foo"), "index.ts", "export function foo() {}");
    let out = tmp.path().join("out");
    translate_project(root, &package_at(root), &out, None).unwrap();
    assert!(
        out.join("src")
            .join("app")
            .join("foo")
            .join("mod.rs")
            .exists(),
        "barrel src/app/foo/mod.rs missing"
    );
    assert!(
        !out.join("src").join("index.rs").exists(),
        "barrel should not emit src/index.rs"
    );
}

#[test]
fn translate_project_detects_circular_import() {
    // a → b → a: Rust forbids circular modules; the guard surfaces the cycle
    // with the files involved instead of letting cargo report a vague error.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(
        root,
        "package.json",
        r#"{ "name": "app", "bin": "main.ts" }"#,
    );
    write(
        root,
        "main.ts",
        "import { a } from \"./a\";\nfunction main() { a(); }",
    );
    write(
        root,
        "a.ts",
        "import { b } from \"./b\";\nexport function a() { b(); }",
    );
    write(
        root,
        "b.ts",
        "import { a } from \"./a\";\nexport function b() { a(); }",
    );
    let out = tmp.path().join("out");
    // A TS import cycle lowers to a `use`-cycle cargo accepts, so the guard
    // reports it as a warning (stderr) and emit proceeds — both files still
    // land. Asserting an error would lock in the old reject behavior.
    translate_project(root, &package_at(root), &out, None).unwrap();
    assert!(
        out.join("src").join("app").join("a.rs").exists(),
        "a.rs missing"
    );
    assert!(
        out.join("src").join("app").join("b.rs").exists(),
        "b.rs missing"
    );
}

#[test]
fn translate_project_module_file_has_no_main() {
    // File role (arch decision point 8): a bin entry collects top-level
    // statements into `fn main`; an imported module file only declares,
    // never executes → no `fn main` (a crate-internal module, brought in
    // by the entry via `mod`). A bin importing one helper module:
    //   main.ts (bin)    → src/main.rs has `fn main`
    //   util.ts  (module) → src/util.rs  has no `fn main`
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(
        root,
        "package.json",
        r#"{ "name": "app", "bin": "main.ts" }"#,
    );
    write(
        root,
        "main.ts",
        "import { helper } from \"./util\";\nfunction main(): void { helper(); }",
    );
    write(root, "util.ts", "export function helper(): void {}");
    let out = tmp.path().join("out");
    translate_project(root, &package_at(root), &out, None).unwrap();
    let main_rs = fs::read_to_string(out.join("src").join("main.rs")).unwrap();
    let util_rs = fs::read_to_string(out.join("src").join("app").join("util.rs")).unwrap();
    assert!(
        main_rs.contains("fn main"),
        "bin entry missing fn main: {main_rs}"
    );
    assert!(
        !util_rs.contains("fn main"),
        "module file should not have fn main: {util_rs}"
    );
}

#[test]
fn translate_project_nested_import_uses_tree_path() {
    // The source tree is preserved, so a relative import across a directory
    // resolves to `crate::<dir>::<mod>` (not the flat `crate::<mod>`):
    // `./sub/util` → `crate::sub::util`, with `src/sub/mod.rs` declaring it.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("sub")).unwrap();
    write(
        root,
        "package.json",
        r#"{ "name": "app", "bin": "main.ts" }"#,
    );
    write(
        root,
        "main.ts",
        "import { helper } from \"./sub/util\";\nfunction main() { helper(); }",
    );
    write(&root.join("sub"), "util.ts", "export function helper() {}");
    let out = tmp.path().join("out");
    translate_project(root, &package_at(root), &out, None).unwrap();
    let main_rs = fs::read_to_string(out.join("src").join("main.rs")).unwrap();
    assert!(
        main_rs.contains("crate::app::sub::util"),
        "nested import should resolve to crate::app::sub::util, got: {main_rs}"
    );
    assert!(
        out.join("src")
            .join("app")
            .join("sub")
            .join("util.rs")
            .exists(),
        "src/app/sub/util.rs missing"
    );
    assert!(
        out.join("src")
            .join("app")
            .join("sub")
            .join("mod.rs")
            .exists(),
        "src/app/sub/mod.rs missing"
    );
}
