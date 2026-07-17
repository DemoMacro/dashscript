# Contributing to DashScript

Thanks for contributing! This guide covers the **workflow** for contributing and the **coding standards** that keep DashScript consistent.

> DashScript compiles a TypeScript-flavored language (`.ds`) to native binaries via Rust: a Rust core (reusing oxc for parse/lint/format) plus a thin TypeScript CLI/npm surface.

## Development Setup

```bash
pnpm install          # install JS workspace dependencies
pnpm build            # vp pack — build all workspace packages
vp check              # lint + format + type-check (Oxlint + Oxfmt)
vp test run           # run tests (Vitest)
```

For the Rust core, `cargo build` / `cargo test` / `cargo clippy` apply to `crates/dashscript`.

Prerequisites: **Node.js 18+**, **pnpm 9+**, **Rust stable** (to build DashScript itself — the toolchain it ships to end users is separate and DashScript-managed).

### Editor support — `.ds` in VS Code

Syntax highlight, live diagnostics, and go-to-definition ship as a VS Code extension (`packages/vscode`) backed by the `ds lsp` server. After `pnpm install`:

1. Put `ds` on your PATH: `cargo install --path crates/dashscript`.
2. (Optional, for crate go-to-definition) Put `rust-analyzer` on your PATH: `rustup component add rust-analyzer`.
3. Build and install the extension:
   ```bash
   pnpm --filter dashscript-vscode package
   code --install-extension packages/vscode/dashscript-vscode-*.vsix
   ```

After install, opening any `.ds` file gives TS-based syntax highlight, real-time `ds check` diagnostics, and go-to-definition — in-file symbols and imported crate names (resolved via the rust-analyzer backend).

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
                     library (src/): translator/, manifest.rs, bindgen.rs
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
- **Diagnostics over panics** — collect errors, recover, and report as many as possible. Reserve `unwrap`/`panic!` for true invariants in tests.
- **No logic in bindings** — the `ds` binary (`bin/` on the `dashscript` crate) and the npm package are thin. If you are writing translation logic there, it belongs in the library (`src/`).
- Run `cargo fmt` and `cargo clippy -- -D warnings` before committing.

### TypeScript — CLI / npm surface (`packages/dashscript`)

- **Functions**: `camelCase`. **Files & directories**: `kebab-case`. **Interfaces / types**: `PascalCase`, no `I` prefix, `Options` suffix, `readonly` properties.
- **Constants**: `as const` objects (not `enum`), `SCREAMING_SNAKE_CASE` keys, lowercase values.
- ESM (`"type": "module"`), `strict` mode, no implicit `any`.
- `vp check` (Oxlint + Oxfmt) is the source of truth for TS style.

### DashScript source (`.ds`)

TypeScript-flavored surface. The mapping table is still growing — when adding `.ds` fixtures, follow TS conventions and keep samples minimal. Do not invent syntax the translator cannot yet map.

### DashScript manifest (`manifest.json`)

- Use **target-prefixed** dependency keys (`rust:serde`) — they mirror `ds add <target>:<crate>` exactly.
- Set `target` to the output shape (`bin` default — native binary; `rust` — translated crate; `wasm`/`napi` planned); `--target` overrides it on `ds build`.
- `manifest` must round-trip cleanly: every target-prefixed dependency maps to one `Cargo.toml` entry (version reqs pass through to Cargo today).

## Conformance / Support Matrix

`crates/dashscript/tests/conformance.rs` answers a question the per-node translation tests (`translator/tests/`) do not: **does the translated Rust actually compile?** Those tests assert the output _contains_ a substring; they never run `cargo check`. Conformance runs the full three-layer chain per fixture — `Translator::check` (translatability), then `translate` + `cargo check` (the emitted Rust must compile) — and records `supported` | `partial` (translates but won't compile) | `unsupported` (`check` flags it). A `partial` here is a real translator gap the substring tests missed.

Feature data lives in `crates/dashscript/tests/conformance/data/`:

- `tests-fixtures.json` — **auto-extracted** from `translator/tests/*.rs` by `scripts/extract-tests.mjs`. Every `let src = "..."` in a `translates_*` `#[test]` becomes a fixture (**zero hand-written**). These are recorded informationally — no `expect`, so the run reports the current state and surfaces its partials without asserting them.
- `test262.json` — **auto-extracted** from tc39 test262 by `scripts/extract-test262.mjs`. Each test is rewritten to a `main()` that logs its assertions; the differential layer (in progress) diffs `ds` output against Node's — the oracle, so there are no hand-written expectations (mechanism detailed in `CLAUDE.md`). Whitelists `test/built-ins/{Math,String,Array,Object,Number}/`; descriptor/Symbol/async tests are `unsupported`.
- `correctness.json` — the **only** hand-written fixtures. Each carries `expect` + `expect_output`; the runner `cargo run`s the emitted program and compares stdout. These are asserted (regression guard).

Regenerate the auto-derived lists (from the repo root, after `pnpm install`):

```bash
node scripts/extract-tests.mjs     # translator/tests → tests-fixtures.json
node scripts/extract-test262.mjs   # tc39 test262 → test262.json (after `git clone https://github.com/tc39/test262 .temp/test262`)
```

Run the harness:

```bash
cargo test -p dashscript --test conformance
```

Each run rewrites `tests/conformance/matrix.md` (human-readable) and `matrix.json` (machine-readable) beside the source. Only `correctness.json` entries are asserted; `tests-fixtures.json` entries are recorded informationally — the partials they surface are the actionable output.

**Adding a correctness case** — append to `data/correctness.json` (the fixture must declare `function main()`):

```json
{
  "id": "correctness.array_join",
  "category": "correctness",
  "source": "manual",
  "fixture": "function main(): void { const xs: number[] = [1, 2, 3]; console.log(xs.join(\"-\")); }",
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
| **New AST → Rust mapping** | `translator/`                            | Add one rule for the AST node kind; add a `.ds` fixture; run `ds build --target rust` and `cargo check` the emitted Rust. Unmapped nodes must error, not silently miscompile. |
| **New manifest field**     | `manifest/`                              | Extend the `manifest.json` reader and the `Cargo.toml` emitter together; keep target-prefixed dependency keys and normalize versions.                                         |
| **New bindgen target**     | `bindgen/`                               | Map a Rust construct (e.g. `struct`, `enum`, `trait`) to its `.ds` declaration so editor types stay correct.                                                                  |
| **New `ds` subcommand**    | `crates/dashscript` `bin/` + npm package | Wire a thin command to an existing core module; no logic in the CLI layer.                                                                                                    |

Rule of thumb: **a new front-end construct must be mappable end-to-end** — a `.ds` feature that the translator cannot yet lower should fail loudly with a diagnostic, not produce Rust that won't compile.

## Pull Request Checklist

- [ ] `vp check` passes
- [ ] `pnpm build` succeeds for the changed package
- [ ] `cargo test` + `cargo clippy` pass for the core crate
- [ ] Any new AST mapping has a `.ds` fixture whose emitted Rust (`ds build --target rust`) passes `cargo check`
- [ ] Naming & patterns follow the standards above
- [ ] Changes are minimal and focused — match existing style
- [ ] No translation logic added to `bin/` or the npm package (it belongs in the library, `src/`)
