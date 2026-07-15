# CLAUDE.md

You are a senior developer working on **DashScript** — a TypeScript-frontend language (`.ds`) that **transpiles to idiomatic Rust**, with Go and Zig backends planned. DashScript does **not** implement its own parser, linter, or formatter: it reuses [`oxc`](https://oxc.rs/) (`oxc_parser` + `oxc_ast` + `oxc_allocator`, plus `oxlint`/`oxfmt`) for the TypeScript-flavored front end, then translates the resulting AST into Rust source and a `Cargo.toml`. The core is Rust; the `ds` CLI ships as a single `dashscript` package (npm + standalone binary).

> Coding standards, design patterns, and the contribution workflow live in [CONTRIBUTING.md](./CONTRIBUTING.md). This file is the architectural context an agent must understand before changing code. Read both.

## Project

**DashScript** is a TS → Rust transpiler. Three jobs, no more:

1. **Translate** — oxc AST → idiomatic Rust source.
2. **Manifest** — a `manifest.json` project manifest → `Cargo.toml`.
3. **Bindgen** — a Rust crate → a `.ds` type declaration, for editor type hints.

| Aspect               | Value                               |
| -------------------- | ----------------------------------- |
| Language name        | DashScript                          |
| File extension       | `.ds`                               |
| npm package / binary | `dashscript` (binary command: `ds`) |
| Repo                 | `DemoMacro/dashscript` (MIT)        |

**Core philosophy**

- **Dash** — fast. Reuse oxc (one of the fastest TS toolchains) for parse/lint/fmt, emit native Rust, and validate the output with `cargo check` / `cargo clippy`.
- **Script** — a typed, TypeScript-flavored surface. Developers write what they know; DashScript maps it to Rust.
- **Bridge** — the AST-to-Rust translation table, plus manifest and bindgen, carry TS-front semantics into the Rust world safely.

## Tech Stack

| Layer                | Technology                                               | Role                                                                          |
| -------------------- | -------------------------------------------------------- | ----------------------------------------------------------------------------- |
| Parsing              | `oxc_parser` + `oxc_ast` + `oxc_allocator` (Rust crates) | `.ds` → AST. **Reused, not reimplemented.**                                   |
| Lint & format        | oxc (`oxlint` / `oxfmt`)                                 | `ds check` / `ds fmt` on `.ds`. **Reused.**                                   |
| Translation core     | Rust                                                     | AST → Rust source (the only logic DashScript owns)                            |
| Rust emission        | `syn` AST construction + `prettyplease` printer          | idiomatic, `cargo fmt`-clean output                                           |
| Manifest             | `manifest.json` → `Cargo.toml`                           | dependency resolution; never `package.json`                                   |
| Bindgen              | Rust (`syn`-style crate metadata) → `.ds` declaration    | type hints for Rust crates                                                    |
| Rust toolchain       | pinned standalone build, DashScript-managed              | downloaded on demand like an npm dependency; no system `rustup` for end users |
| JS surface           | TypeScript (ESM, strict)                                 | single `dashscript` npm package (CLI wrapper, types)                          |
| Build / check / test | vite-plus (`vp pack` / `vp check` / `vp test`), `cargo`  | unified toolchain                                                             |

## Compilation Pipeline

```
.ds source
  → oxc parser (reused)          .ds → oxc AST
  → translator (DashScript)      oxc AST → Rust source
  → manifest (DashScript)        manifest.json → Cargo.toml
  → output                       a buildable Cargo project, then cargo check / clippy

.ds check / ds fmt               reuse oxc directly (oxlint / oxfmt) — no own checker/formatter
```

There is no separate semantic-analysis, type-checking, or IR stage of our own. oxc resolves the front-end structure and validates `.ds` syntax; DashScript's job is the mapping table from oxc AST nodes to Rust constructs. Target-language correctness is delegated to `cargo check` / `cargo clippy` on the generated project.

## Architecture: Translation Model

The central mental model — a **mapping table**, not a multi-stage compiler:

| Front (`.ds`, via oxc AST)  | Bridge rule  | Back (Rust)                          |
| --------------------------- | ------------ | ------------------------------------ |
| `number`                    | scalar       | `f64` (or `i64`/`u64` by annotation) |
| `string`                    | scalar       | `&str` param / `String` return       |
| `boolean`                   | scalar       | `bool`                               |
| `T[]` / `Array<T>`          | collection   | `Vec<T>`                             |
| `interface` / `type` object | record       | `struct`                             |
| `function`                  | callable     | `fn`                                 |
| union `A \| B`              | tagged union | `enum`                               |

Three sub-systems share this table:

- **translator** — walks the oxc AST and emits Rust. Each AST node kind has one mapping rule; unmapped nodes raise a clear diagnostic rather than silently producing broken Rust.
- **manifest** — reads the project's `manifest.json` and emits a `Cargo.toml`. Dependencies are keyed by **target prefix** (`rust:serde`) so multiple backends can coexist; version reqs pass through to Cargo today (npm-style normalization is planned).
- **bindgen** — reads a Rust crate's public surface and emits a `.ds` declaration so that importing the crate in `.ds` yields editor completion and types. This is what `ds add rust:<crate>` runs.

## Architecture: Distribution

Hybrid cargo + pnpm workspace. One product name, two reach paths. **Core logic lives only in `crates/`.** The CLI and npm package are thin wrappers; never put translation logic there — it would then exist in only one distribution path.

| Path                   | Contains                                             | Consumed by                             |
| ---------------------- | ---------------------------------------------------- | --------------------------------------- |
| `crates/dashscript/`   | Pure Rust core (translator + manifest + bindgen)     | Rust via cargo                          |
| `apps/ds/`             | Standalone `ds` binary                               | `cargo install dashscript`, brew, scoop |
| `packages/dashscript/` | Single npm package — `ds` CLI wrapper + editor types | `pnpm add dashscript`, `npx ds`         |

## Package Layout

```
crates/
  dashscript/            the only core crate (modular)
    src/
      translator/        oxc AST → Rust source (a directory: one file per node category)
      manifest.rs        manifest.json → Cargo.toml
      bindgen.rs         Rust crate → .ds type declaration

apps/
  ds/                    standalone `ds` binary

packages/
  dashscript/            the single npm package: bin `ds` + editor types
```

One core crate, three modules, three responsibilities. Split into more crates only when a module grows its own release cadence — not before.

## CLI

Unified entry `ds`, subcommand style:

```
ds build <file.ds>            # parse with oxc → translate → emit Rust project + Cargo.toml
ds check                      # lint & type-check .ds (oxc — oxlint)
ds fmt                        # format .ds (oxc — oxfmt)
ds add rust:<crate>           # fetch crate + generate .ds declaration (bindgen)
ds run <file.ds>              # translate → emit a Cargo project → cargo run
ds test                       # run .ds tests (planned)
```

`ds add` is the single entry for bringing a Rust crate into a DashScript project — it generates the declaration inline; there is **no separate `ds gen` step**.

## Design Decisions

Each decision states its trade-off so contributors know what _not_ to "fix".

**Reuse oxc for parsing/lint/format (vs a hand-written `.ds` toolchain).**
DashScript's surface is TypeScript-flavored; oxc is already the fastest, spec-compliant TS toolchain. Reusing it for parse, lint, and format avoids re-deriving TS grammar and checker logic. ✅ speed + correctness for free · ❌ coupled to oxc's AST shape; a breaking oxc change is a translator change.

**Transpiler, not a full language (vs own type-checker / IR).**
A TS-front → Rust mapping table plus `cargo check` on the output covers the goal with a fraction of the surface area. oxc gives structure and lint; `cargo` gives correctness. ✅ small scope, fast to ship · ❌ no cross-target IR — adding Go/Zig backends later means a new mapping table each, not a shared lowering.

**`manifest.json` (vs `package.json`).**
A `package.json` at the project root is claimed by npm/pnpm and would mislead JS tooling. A dedicated `manifest.json` avoids the collision and carries DashScript-specific fields (`target`, prefixed dependencies). ✅ no ecosystem clash · ❌ one more file format to document.

**Target-prefixed dependencies (`rust:serde`) (vs bare names).**
A prefix records which backend a dependency belongs to, so a project can mix targets (e.g. a Rust app with a Zig FFI) and so future `go:` / `zig:` backends slot in without schema changes. It also mirrors the `ds add rust:<crate>` command verbatim. ✅ multi-target ready · ❌ slightly more to type for the common single-target case.

**One `dashscript` package (vs a separate `@dashscript/cli`).**
The CLI is the product; splitting it into a sub-package adds an install step with no benefit. One package, one binary name (`ds`). ✅ simplest install (`pnpm add dashscript`) · ❌ coarser release granularity.

**DashScript-managed Rust toolchain (vs depend on a system `rustup`).**
DashScript pins a specific Rust version and downloads its standalone build on demand — like an npm dependency — into its own cache, so end users never install Rust separately. ✅ zero-setup install, reproducible builds · ❌ large first-run download and toolchain-management code. (Contributors building DashScript itself still need a system Rust toolchain.)

**One core crate, modular (vs many crates).**
The three responsibilities are small and share the translation table; a single `dashscript` crate with `translator` / `manifest` / `bindgen` modules is enough until a module needs independent versioning. ✅ low overhead · ❌ coarser release granularity.

## Roadmap

- **Initial scope** — `translator` (a core subset of oxc AST → Rust), `manifest` (`manifest.json` → `Cargo.toml`), a DashScript-managed Rust toolchain (pinned, downloaded on demand), `ds build` / `ds check` / `ds fmt`, `bindgen` + `ds add`. One `.ds` file compiles to a buildable Cargo project, checked by `cargo`.
- **More backends** — `go:` and `zig:` mapping tables.
- **Developer experience** — `ds test`, editor/LSP integration, conformance fixtures. (`ds run` already builds and runs a Cargo project.)
- **Self-hosting (north star)** — rewrite the toolchain in `.ds` itself: the Rust bootstrap compiler compiles a `.ds` compiler, which then compiles itself. Viable because `.ds` reaches `oxc` (and any Rust crate) through bindgen — no need to reimplement oxc.

## Performance

- Inherit oxc's parsing/lint/format speed; no duplicate front-end work.
- Emit `cargo fmt`-clean Rust so the output needs no reformatting.
- Delegate correctness to `cargo check` / `cargo clippy` rather than reimplementing a Rust type-checker.

## Behavioral Guidelines

- State assumptions explicitly. If a mapping or crate does not exist yet, say so before implementing against it.
- No features beyond what was asked. No speculative abstractions. (Core logic lives in `crates/` only; mappings live in the `translator` table; do not reimplement what oxc already provides.)
- Touch only what you must. Match existing style — Rust follows Rust idioms, JS surfaces follow the existing TS conventions.
- Transform tasks into verifiable goals: "add a mapping" → "write a `.ds` fixture, run `ds build`, compile the emitted Rust with `cargo check`, assert it builds."
