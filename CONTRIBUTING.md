# Contributing to DashScript

Thanks for contributing! This guide covers the **workflow** for contributing and the **coding standards** that keep DashScript consistent.

> DashScript is a TS → Rust transpiler: a Rust core (reusing oxc for parse/lint/format) plus a thin TypeScript CLI/npm surface.

## Development Setup

```bash
pnpm install          # install JS workspace dependencies
pnpm build            # vp pack — build all workspace packages
vp check              # lint + format + type-check (Oxlint + Oxfmt)
vp test run           # run tests (Vitest)
```

For the Rust core, `cargo build` / `cargo test` / `cargo clippy` apply to `crates/dashscript`.

Prerequisites: **Node.js 18+**, **pnpm 9+**, **Rust stable** (to build DashScript itself — the toolchain it ships to end users is separate and DashScript-managed).

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
  dashscript/        the only core crate, three modules:
                     translator/ (oxc AST → Rust), manifest/ (manifest.json → Cargo.toml), bindgen/ (Rust → .ds)
apps/
  ds/                standalone `ds` binary
packages/
  dashscript/        the single npm package: bin `ds` + editor types
```

## Coding Standards

### Rust — core crate (`crates/dashscript`)

- **Functions / variables**: `snake_case`. **Types / traits / enums**: `PascalCase`. **Constants**: `SCREAMING_SNAKE_CASE`. **Modules / files**: `snake_case`.
- **Reuse oxc for parsing, build check/fmt on the AST** — consume `oxc_parser` / `oxc_ast` / `oxc_allocator` as given. `oxc_linter` / `oxc_formatter` are `publish = false` (not on crates.io), so `ds check` and `ds fmt` are built in-process on the parsed AST; do not shell out to external oxlint/oxfmt.
- **One mapping rule per AST node kind** in `translator/`. Unmapped nodes must raise a diagnostic — never silently emit broken Rust.
- **Diagnostics over panics** — collect errors, recover, and report as many as possible. Reserve `unwrap`/`panic!` for true invariants in tests.
- **No logic in bindings** — `apps/ds` and the npm package are thin. If you are writing translation logic there, it belongs in the core crate.
- Run `cargo fmt` and `cargo clippy -- -D warnings` before committing.

### TypeScript — CLI / npm surface (`packages/dashscript`)

- **Functions**: `camelCase`. **Files & directories**: `kebab-case`. **Interfaces / types**: `PascalCase`, no `I` prefix, `Options` suffix, `readonly` properties.
- **Constants**: `as const` objects (not `enum`), `SCREAMING_SNAKE_CASE` keys, lowercase values.
- ESM (`"type": "module"`), `strict` mode, no implicit `any`.
- `vp check` (Oxlint + Oxfmt) is the source of truth for TS style.

### DashScript source (`.ds`)

TypeScript-flavored surface. The mapping table is still growing — when adding `.ds` fixtures, follow TS conventions and keep samples minimal. Do not invent syntax the translator cannot yet map.

### DashScript manifest (`manifest.json`)

- Use **target-prefixed** dependency keys (`rust:serde`, reserved `go:` / `zig:`) — they mirror `ds add <target>:<crate>` exactly.
- Prefer a `target` field for the project's primary backend so `ds build` has a default.
- `manifest` must round-trip cleanly: every target-prefixed dependency maps to one `Cargo.toml` entry (version reqs pass through to Cargo today).

## Adding a Translation Rule, Manifest Field, or Bindgen Target

Most changes fall into one of three shapes:

| Change                     | Where                   | Pattern                                                                                                                                                         |
| -------------------------- | ----------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **New AST → Rust mapping** | `translator/`           | Add one rule for the AST node kind; add a `.ds` fixture; run `ds build` and `cargo check` the emitted Rust. Unmapped nodes must error, not silently miscompile. |
| **New manifest field**     | `manifest/`             | Extend the `manifest.json` reader and the `Cargo.toml` emitter together; keep target-prefixed dependency keys and normalize versions.                           |
| **New bindgen target**     | `bindgen/`              | Map a Rust construct (e.g. `struct`, `enum`, `trait`) to its `.ds` declaration so editor types stay correct.                                                    |
| **New `ds` subcommand**    | `apps/ds` + npm package | Wire a thin command to an existing core module; no logic in the CLI layer.                                                                                      |

Rule of thumb: **a new front-end construct must be mappable end-to-end** — a `.ds` feature that the translator cannot yet lower should fail loudly with a diagnostic, not produce Rust that won't compile.

## Pull Request Checklist

- [ ] `vp check` passes
- [ ] `pnpm build` succeeds for the changed package
- [ ] `cargo test` + `cargo clippy` pass for the core crate
- [ ] Any new AST mapping has a `.ds` fixture whose emitted Rust passes `cargo check`
- [ ] Naming & patterns follow the standards above
- [ ] Changes are minimal and focused — match existing style
- [ ] No translation logic added to `apps/` or the npm package (it belongs in `crates/`)
