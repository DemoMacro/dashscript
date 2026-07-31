# Contributing to DashScript

Thanks for contributing! This guide covers the **workflow** for contributing and the **coding standards** that keep DashScript consistent.

> DashScript compiles a TypeScript-flavored language (`.ts`) to native binaries via Rust: a Rust core (reusing oxc for parse/lint/format) plus a thin TypeScript CLI/npm surface.

## Development Setup

```bash
pnpm install          # install JS workspace dependencies
pnpm build            # vp pack — build all workspace packages
vp check              # lint + format + type-check (Oxlint + Oxfmt)
vp test run           # run tests (Vitest)
```

For the Rust core, `cargo build` / `cargo test` / `cargo clippy` apply to `crates/dashscript`.

Prerequisites: **Node.js 18+**, **pnpm 9+**, **Rust stable** (to build DashScript itself — the toolchain it ships to end users is separate and DashScript-managed).

### Editor support — `.ts` in VS Code

Editor support is a thin bridge over VS Code's built-in TypeScript server. The `packages/vscode` extension starts the shared `ds lsp` for what the native TS server cannot do (crate go-to-definition + translatability diagnostics), and the `@dashscript/typescript-plugin` handles `cargo:` imports. Syntax highlighting, completions, hover, signature help, document symbols, find references, and rename all come from VS Code's native TypeScript. After `pnpm install`:

1. Put `ds` on your PATH: `cargo install --path crates/dashscript`.
2. (Required for crate go-to-definition) Put `rust-analyzer` on your PATH: `rustup component add rust-analyzer`.
3. Build and install the extension:
   ```bash
   pnpm --filter dashscript-vscode package
   code --install-extension packages/vscode/dashscript-vscode-*.vsix
   ```
4. Load the TypeScript plugin from the workspace — VS Code's bundled TS can't resolve a workspace plugin ([microsoft/vscode#232406](https://github.com/microsoft/vscode/issues/232406)), so pin the workspace TS once in `.vscode/settings.json`:
   ```json
   {
     "js/ts.tsdk.path": "node_modules/typescript/lib",
     "js/ts.tsdk.promptToUseWorkspaceVersion": true
   }
   ```
   and declare the plugin in `tsconfig.json`: `"compilerOptions": { "plugins": [{ "name": "@dashscript/typescript-plugin" }] }`. Accept the "Use Workspace Version" prompt once.

After install, opening any `.ts` file gives native TS syntax highlight, completions, hover, references, and rename; `cargo:` imports resolve via the plugin (no TS2307, go-to-definition into `~/.cargo` source); and `ds check` translatability diagnostics surface inline.

## Contribution Workflow

1. **Fork & clone** — fork on GitHub, clone your fork, add `upstream` (`git remote add upstream https://github.com/DemoMacro/dashscript.git`).
2. **Branch** — branch off `main` (`feat/...`, `fix/...`, `docs/...`, …).
3. **Code** — follow the standards below; match existing style.
4. **Verify** — `vp check` passes; `pnpm build` succeeds for the changed package; `cargo test` + `cargo clippy` pass for the core crate.
5. **Commit** — use [conventional commits](https://www.conventionalcommits.org/): `feat:`, `fix:`, `docs:`, `refactor:`, `perf:`, `test:`, `build:`, `ci:`, `chore:`, `revert:`. Keep commit messages free of generator/AI attribution.
6. **Push & PR** — push to your fork and open a PR against `upstream/main`.

## Repository Layout

A hybrid cargo + pnpm workspace. Core logic lives only in `crates/`; everything else is a thin bridge.

```
crates/
  dashscript/        the only crate — library + the `ds` binary
                     library (src/): translator/, package.rs, bindgen.rs
                       translator/
                         expressions/   one file per AST node family (literals, binary, …, call);
                                        mod.rs is the dispatch table + shared helpers only
                         builtins/      ES built-ins, one file per built-in, mirroring tc39
                                        test262 test/built-ins/ (math, array, string, number,
                                        object, global, console)
                         functions/     statement translation, one file per kind
                     binary (bin/): the `ds` CLI + language server
packages/
  dashscript/        the single npm package: bin `ds` + editor types
```

## Coding Standards

### Rust — core crate (`crates/dashscript`)

- **Functions / variables**: `snake_case`. **Types / traits / enums**: `PascalCase`. **Constants**: `SCREAMING_SNAKE_CASE`. **Modules / files**: `snake_case`.
- **Reuse oxc for parsing, build lint/fmt on the AST** — consume `oxc_parser` / `oxc_ast` / `oxc_allocator` as given. `oxc_linter` / `oxc_formatter` are `publish = false` (not on crates.io), so `ds lint` and `ds fmt` are built in-process on the parsed AST; do not shell out to external oxlint/oxfmt.
- **One mapping rule per AST node kind** in `translator/`, slotted by what it maps: a new expression kind → `expressions/<family>.rs` (or a new family file); a new ES built-in → `builtins/<name>.rs` mirroring its tc39 test262 directory; a new statement kind → `functions/`. Unmapped nodes must raise a diagnostic — never silently emit broken Rust.
- **Type queries over type inference — `Ctx` is a reader, not a checker.** The translator's type knowledge lives in the `TypeRegistry`/`Locals`/`flavor`, surfaced as read-only queries on `Ctx` (`field_type`, `is_union`, …). When a lowering needs a type fact, add a `Ctx` query that reads existing registry data — do **not** write a `type_of_expr` inference pass. When a fact is genuinely unknown, emit `_` and let `cargo check` arbitrate (it is the final type authority). The translator is the single source of truth for "what maps" via its `classify` table (`Mapped`/`Reject`/`DegradeEngine`); `check` queries it rather than keeping a parallel rule tree.
- **Diagnostics over panics** — collect errors, recover, and report as many as possible. Reserve `unwrap`/`panic!` for true invariants in tests.
- **No logic in bindings** — the `ds` binary (`bin/` on the `dashscript` crate) and the npm package are thin. If you are writing translation logic there, it belongs in the library (`src/`).
- **Keep `translator/` files focused — one core crate, split by sub-responsibility.** One file per AST node family (`expressions/`), one per ES built-in (`builtins/`), statement translation under `functions/`. A file growing past ~1000 lines is a signal to split by sub-responsibility (`functions/mod.rs` splits into `entry`/`locals`/`escape`/`lazy_static`/`dispatch`). Do **not** split into separate crates — one core crate until a module needs its own release cadence.
- Run `cargo fmt` and `cargo clippy -- -D warnings` before committing.

### TypeScript — CLI / npm surface (`packages/dashscript`)

- **Functions**: `camelCase`. **Files & directories**: `kebab-case`. **Interfaces / types**: `PascalCase`, no `I` prefix, `Options` suffix, `readonly` properties.
- **Constants**: `as const` objects (not `enum`), `SCREAMING_SNAKE_CASE` keys, lowercase values.
- ESM (`"type": "module"`), `strict` mode, no implicit `any`.
- `vp check` (Oxlint + Oxfmt) is the source of truth for TS style.

### DashScript source (`.ts`)

TypeScript-flavored surface. The mapping table is still growing — when adding `.ts` fixtures, follow TS conventions and keep samples minimal. Do not invent syntax the translator cannot yet map.

**Execution model — pure-TS semantics.** A `.ts` file runs like a Node script: top-level declarations (`function`/`class`/`interface`/`type`/`import`/`export`) become Rust items and do **not** execute; top-level executable statements (`const`/`let`, expression statements, control flow, `throw`) run in source order, collected into an implicit `fn main` the translator emits. A file with only declarations emits an empty `fn main` (the way Node runs a script that defines functions but never calls them). `function main` is therefore an ordinary declaration — it is renamed `__ds_main` so it cannot collide with the cargo entry; to run it, call it explicitly at the top level (`main();`). A top-level binding referenced from inside a `function` would close over an `fn main` local (impossible for a Rust fn item), so it is hoisted to a module-global item — a const-expr literal to `pub const`, a runtime-immutable binding to a `static OnceLock<T>` + accessor, a mutable binding to `thread_local! { RefCell<T> }` (+ get/set accessors) — so no rewrite is needed. A module file initializes its globals eagerly (no `fn main`); an entry file seeds them in source order from `fn main`.\_

### DashScript package (`package.json`)

- Put Rust crate deps under **`dashscript.cargo.dependencies`** with bare crate names (`"serde": "1.0"` or `{ "version": "1.0", "features": [...] }`) — the same name `ds add cargo:<crate>` records (the `cargo:` prefix is optional). npm `dependencies` stay JS deps (→ `node_modules`) and never reach Cargo.toml.
- Set `dashscript.target` to the output shape (`bin` default — native binary; `rust` — translated crate; `wasm`/`napi` planned); `--target` overrides it on `ds build`.
- Declare executables under `bin` (package.json `bin` → cargo `[[bin]]`): a project is **one crate** — the whole directory's `.ts` files translate into `src/<stem>.rs`, and only the `bin`/`main` entries become cargo targets. `main` → `[lib]`; `dashscript.cargo.devDependencies` → `[dev-dependencies]`. A workspace root's shared metadata/deps inherit via `[workspace.package]`/`[workspace.dependencies]` (member `field.workspace = true`).
- The package must round-trip cleanly: every `dashscript.cargo` dependency maps to one `Cargo.toml` entry (version reqs pass through to Cargo today).

## Conformance / Support Matrix

`crates/dashscript/tests/conformance.rs` answers a question the per-node translation tests (`translator/tests/`) do not: **does the translated Rust actually compile?** Those tests assert the output _contains_ a substring; they never run `cargo check`. Conformance runs the full three-layer chain per fixture — `Translator::check` (translatability), then `translate` + `cargo check` (the emitted Rust must compile) — and records `supported` | `partial` (translates but won't compile) | `unsupported` (`check` flags it). A `partial` here is a real translator gap the substring tests missed.

Feature data lives in `crates/dashscript/tests/conformance/data/`:

- `tests-fixtures.json` — **auto-extracted** from `translator/tests/*.rs` by `scripts/extract-tests.mjs`. Every `let src = "..."` in a `translates_*` `#[test]` becomes a fixture (**zero hand-written**). These are recorded informationally — no `expect`, so the run reports the current state and surfaces its partials without asserting them.
- `test262/<cat>.json` (one file per builtin) — **auto-extracted** from tc39 test262 by `scripts/extract-test262.mjs`. Each test is rewritten to a `main()` that logs its assertions; the differential harness diffs `ds` output against Node's — the ground-truth oracle, so there are no hand-written expectations (mechanism detailed in `CLAUDE.md`). No whitelist: every `test/built-ins/` dir is one category; `new`/`Reflect`/`$INCLUDE`/descriptor/Symbol/async fixtures are filtered or marked `unsupported`. The test262 layer is **opt-in** via `DASH_TEST262_CATEGORIES` (unset → skipped, so a bare `cargo test` stays fast).
- `correctness.json` — the **only** hand-written fixtures. Each carries `expect` + `expect_output`; the runner `cargo run`s the emitted program and compares stdout. These are asserted (regression guard).

Regenerate the auto-derived lists (from the repo root, after `pnpm install`):

```bash
node scripts/extract-tests.mjs                       # translator/tests → tests-fixtures.json
node scripts/extract-test262.mjs --category math,number   # tc39 → data/test262/<cat>.json (after `git clone https://github.com/tc39/test262 .temp/test262`)
```

Run the harness (the test262 layer is opt-in — unset `DASH_TEST262_CATEGORIES` runs only correctness + translator-tests):

```bash
DASH_TEST262_CATEGORIES=math cargo test -p dashscript --test conformance                                          # one builtin
DASH_TEST262_CATEGORIES=math,number,string,array,object,json cargo test -p dashscript --test conformance          # the activated set
```

Each run rewrites `tests/conformance/matrix/` — one `test262-<cat>.{md,json}` per run category, plus `translator-tests.{md,json}`, `correctness.{md,json}`, and a `README.md` index (the project's ECMAScript-conformance scorecard). Only `correctness.json` entries are asserted; the others are recorded informationally — the partials they surface are the actionable output.

**Adding a correctness case** — append to `data/correctness.json`. The fixture must declare `function main()` **and call it** (`main();` at the end), because under pure-TS semantics a declaration alone does not run:

```json
{
  "id": "correctness.array_join",
  "category": "correctness",
  "source": "manual",
  "fixture": "function main(): void { const xs: number[] = [1, 2, 3]; console.log(xs.join(\"-\")); }\nmain();",
  "expect": "supported",
  "expect_output": "1-2-3",
  "note": "[1,2,3].join('-') prints 1-2-3 (f64 Display drops trailing .0)"
}
```

> `console.log(x)` lowers to `println!("{}", x)` — **Display**, not Debug. Correctness fixtures must log primitives or joined strings; never a bare `Vec`/`struct` (no `Display` ⇒ the emitted Rust won't compile). Verify a new mapping with `cargo run` before trusting a fixture.

**Adding a support-matrix fixture** — don't hand-write one. Add a `translates_*` `#[test]` to the relevant `translator/tests/*.rs` file; `extract-tests.mjs` picks up its `let src` on the next run. Support-matrix coverage grows from the translation tests that already exist.

**Fixture shape note:** bind array literals to a typed local first (`const xs: number[] = [1, 2, 3]; xs.map(...)`). An unannotated `[1, 2, 3]` lowers to `vec![1.0, ..]` whose element type is undecided, so chained methods fail trait resolution. This mirrors how `examples/` and `translator/tests/` write arrays.

## Adding a Translation Rule, Manifest Field, or Bindgen Target

Most changes fall into one of three shapes:

| Change                     | Where                                    | Pattern                                                                                                                                                                       |
| -------------------------- | ---------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **New AST → Rust mapping** | `translator/`                            | Add one rule for the AST node kind; add a `.ts` fixture; run `ds build --target rust` and `cargo check` the emitted Rust. Unmapped nodes must error, not silently miscompile. |
| **New package field**      | `package.rs`                             | Extend the `package.json` reader and the `Cargo.toml` emitter together; keep npm and `dashscript.cargo` deps separate; normalize versions.                                    |
| **New bindgen target**     | `bindgen/`                               | Map a Rust construct (e.g. `struct`, `enum`, `trait`) to its `.d.ts` declaration so editor types stay correct.                                                                |
| **New `ds` subcommand**    | `crates/dashscript` `bin/` + npm package | Wire a thin command to an existing core module; no logic in the CLI layer.                                                                                                    |

Rule of thumb: **a new front-end construct must be mappable end-to-end** — a `.ts` feature that the translator cannot yet lower should fail loudly with a diagnostic (not produce Rust that won't compile), or degrade to the embedded QuickJS engine per-function where no static mapping exists.

## Pull Request Checklist

- [ ] `vp check` passes
- [ ] `pnpm build` succeeds for the changed package
- [ ] `cargo test` + `cargo clippy` pass for the core crate
- [ ] Any new AST mapping has a `.ts` fixture whose emitted Rust (`ds build --target rust`) passes `cargo check`
- [ ] Naming & patterns follow the standards above
- [ ] Changes are minimal and focused — match existing style
- [ ] No translation logic added to `bin/` or the npm package (it belongs in the library, `src/`)
