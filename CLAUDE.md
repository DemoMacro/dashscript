# CLAUDE.md

You are a senior developer working on **DashScript** — TypeScript ergonomics, Rust performance, compiled to native. It is a typed, TypeScript-flavored language (`.ds`) that compiles to native binaries via idiomatic Rust (`wasm` / `napi` outputs planned). DashScript does **not** implement its own parser: it reuses [`oxc`](https://oxc.rs/) (`oxc_parser` + `oxc_ast` + `oxc_allocator`) for the TypeScript-flavored front end, then translates the resulting AST into Rust source and a `Cargo.toml`. `check` and `fmt` are built in-process on that same parsed AST — `oxc_linter` and `oxc_formatter` are `publish = false` in oxc's workspace (not on crates.io), so DashScript reuses oxc as a _capability_ (AST + diagnostics + codegen) rather than depending on those crates. The core is Rust; the `ds` CLI ships as a single `dashscript` package (npm + standalone binary).

> Coding standards, design patterns, and the contribution workflow live in [CONTRIBUTING.md](./CONTRIBUTING.md). This file is the architectural context an agent must understand before changing code. Read both.

## Project

**DashScript** is a TS → Rust transpiler. Three jobs, no more:

1. **Translate** — oxc AST → idiomatic Rust source.
2. **Manifest** — a `manifest.json` project manifest → `Cargo.toml`.
3. **Bindgen** — a local Rust source file → a `.ds` type declaration, for editor type hints.

| Aspect               | Value                               |
| -------------------- | ----------------------------------- |
| Language name        | DashScript                          |
| File extension       | `.ds`                               |
| npm package / binary | `dashscript` (binary command: `ds`) |
| Repo                 | `DemoMacro/dashscript` (MIT)        |

**Core philosophy**

- **Dash** — fast. Reuse oxc (one of the fastest TS parsers) for the front end, build `check`/`fmt` on the same parsed AST in-process, emit native Rust, and validate the output with `cargo check` / `cargo clippy`.
- **Script** — a typed, TypeScript-flavored surface. Developers write what they know; DashScript maps it to Rust.
- **Bridge** — the AST-to-Rust translation table, plus manifest and bindgen, carry TS-front semantics into the Rust world safely.

## Tech Stack

| Layer                | Technology                                               | Role                                                                                    |
| -------------------- | -------------------------------------------------------- | --------------------------------------------------------------------------------------- |
| Parsing              | `oxc_parser` + `oxc_ast` + `oxc_allocator` (Rust crates) | `.ds` → AST. **Reused, not reimplemented.**                                             |
| Check & format       | `oxc_parser` AST + `oxc_diagnostics` + `oxc_codegen`     | `ds check` (translatability) / `ds fmt` (pretty-print); not a shell-out to oxlint/oxfmt |
| Translation core     | Rust                                                     | AST → Rust source (the only logic DashScript owns)                                      |
| Rust emission        | `syn` AST construction + `prettyplease` printer          | idiomatic, `cargo fmt`-clean output                                                     |
| Manifest             | `manifest.json` → `Cargo.toml`                           | dependency resolution; never `package.json`                                             |
| Bindgen              | Rust (`syn`-style crate metadata) → `.ds` declaration    | type hints for Rust crates                                                              |
| Rust toolchain       | pinned standalone build, DashScript-managed              | downloaded on demand like an npm dependency; no system `rustup` for end users           |
| JS surface           | TypeScript (ESM, strict)                                 | single `dashscript` npm package (CLI wrapper, types)                                    |
| Build / check / test | vite-plus (`vp pack` / `vp check` / `vp test`), `cargo`  | unified toolchain                                                                       |

## Compilation Pipeline

```
.ds source
  → oxc parser (reused)          .ds → oxc AST
  → translator (DashScript)      oxc AST → Rust source
  → manifest (DashScript)        manifest.json → Cargo.toml
  → cached cargo project         .cache/dash/<name>/ in-project, or ~/.cache/dash/<hash>/ for a lone file
  → output                       dist/<name> (native binary, default) or dist/<name>/ (Rust crate, --target rust)

.ds lint / ds check / ds fmt     built in-process on the oxc_parser AST (oxc_linter/oxc_formatter are publish=false)
```

Correctness is a three-layer chain: (1) **structure** — `oxc_parser` parses `.ds` and reports syntax errors; (2) **translatability** — DashScript's own `lint` walks the AST and flags any construct the translator cannot lower to valid Rust (the translator is the single source of truth for "what maps"); (3) **target** — `cargo check` / `cargo clippy` on the emitted project is the final arbiter. There is no cross-target IR: oxc gives structure, the translator is the mapping table, `cargo` gives Rust correctness.

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
- **bindgen** — reads a local Rust source file's public surface and emits a `.ds` declaration beside it, so importing it in `.ds` yields editor completion and types. This is what `ds add <file>.rs` runs. A crate added via `ds add rust:<crate>` needs no `.ds` stub — its types come from the crate's own source in `~/.cargo`, read directly by the language server (the way rust-analyzer reads its deps).

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
ds <file.ds>                  # run a file directly (like `node a.js`)
ds run <script>               # run a manifest.json script (like `pnpm run`)
ds build [<file>] [--target]  # parse → translate → compile a native binary in dist/<name>
                              #   --target rust → emit the Rust crate in dist/<name>/ instead
ds lint <file>                # translatability check (parser + translator rules)
ds check <file>               # lint + format check, like `vp check`
ds fmt <file>                 # format .ds in place (built-in formatter)
ds install                    # fetch manifest deps via cargo + write Cargo.lock (like `pnpm install`)
ds add rust:<crate>           # fetch crate via cargo + record rust:<crate> in manifest.json
ds add <file>.rs              # bindgen a local Rust file → <stem>.ds type declaration
ds cache clean                # remove the in-project .cache/
ds test                       # run .ds tests (planned)
```

`ds <file.ds>` runs a file directly (like `node a.js`); `ds run <script>` runs a `manifest.json` script (like `pnpm run` — `run` is always explicit so it never collides with `ds <file.ds>`). `ds build` defaults to a **native binary** — the way `vp pack` ships a runnable artifact, not an intermediate project. Translate → a cached Cargo project (`.cache/dash/<name>/` in-project — **one per project**, keyed by the manifest name so a project's entries share a cache and two `main.ds` files in different projects don't collide; or `~/.cache/dash/<hash>/` for a lone file, looked up by walking up from the `.ds` for a `manifest.json`) → `cargo build --release` → copy the binary to `dist/<name>`. `--target rust` stops at the translated Rust crate (`dist/<name>/`, no `target/`); `--target` overrides the manifest `target` (default `bin`). `<name>` is the `manifest.json` `name` (fallback: parent dir, then file stem). `target/` never lands in `dist/`.

`ds build` at a **workspace root** (a `manifest.json` with a `workspaces` glob list, e.g. `["apps/*", "packages/*"]`) builds every member under **one cargo workspace**: members are emitted at `.cache/dash/<name>/` beneath a root `Cargo.toml` (`[workspace] members`) that owns a shared `target/` and `Cargo.lock`, so a dependency two members use compiles once — cargo's native hoisted-`node_modules`, mirroring `pnpm-workspace.yaml` / cargo `[workspace]`. Each member's binary lands in its **own** `<member>/dist/<name>` (not the workspace root), so every package stays independently publishable — mirroring a pnpm workspace package's own `dist/`. `--filter <name>` (member manifest name or directory) builds one member. (`--target rust` emits each member's crate to its own `<member>/dist/<name>/`; inter-member path dependencies and task caching (turbo/nx) are not yet done.)

`ds add` has two modes: `rust:<crate>` records a crate in `manifest.json` (no `.ds` stub — types come from the crate's source via the language server); `<file>.rs` runs bindgen to emit `<stem>.ds`. There is **no separate `ds gen` step**.

## Design Decisions

Each decision states its trade-off so contributors know what _not_ to "fix".

**Reuse oxc for parsing, check, and format (vs depending on `oxc_linter`/`oxc_formatter`).**
DashScript's surface is TypeScript-flavored, so it reuses `oxc_parser`/`oxc_ast`/`oxc_allocator` (the _published_ part of oxc) rather than re-deriving TS grammar. `oxc_linter` and `oxc_formatter`, however, are `publish = false` in oxc's workspace — not on crates.io. So: `ds lint` reuses `oxc_parser` + `oxc_diagnostics` to report _translatability_ (does this `.ds` lower to valid Rust? — something eslint-style rules cannot express); `ds fmt` reuses `oxc_codegen` (published, pretty-print by default, not minified). `ds check` is the composite — lint plus a format check — matching `vp check`. ✅ no giant git dependency, keeps the "fast" promise · ❌ coupled to oxc's published API surface.

**Transpiler, not a full language (vs own type-checker / IR).**
A TS-front → Rust mapping table plus `cargo check` on the output covers the goal with a fraction of the surface area. oxc gives structure; `cargo` gives correctness. ✅ small scope, fast to ship · ❌ no cross-target IR — the `wasm`/`napi` outputs are Rust target variants (same mapping table, a different `cargo --target`), not separate backends.

**`.ds`: TypeScript surface, Rust semantics (vs a new surface syntax).**
`.ds` is written in a TypeScript-flavored syntax developers already know, but its semantics are Rust's — the goal is to express the full Rust type/memory-safety model (ownership, borrowing, lifetimes, traits), with TypeScript as the _presentation_ only. Today the translator covers a safe TS→Rust subset (auto clone/borrow/narrowing bridge the gaps); Rust-only constructs (explicit lifetimes, trait bounds, `unsafe`) are reached incrementally as real demand drives each, never speculatively. ✅ familiar to write, sound underneath · ❌ "covers full Rust" is a direction, not a present-tense claim.

**`manifest.json` (vs `package.json`).**
A `package.json` at the project root is claimed by npm/pnpm and would mislead JS tooling. A dedicated `manifest.json` avoids the collision and blends `Cargo.toml` `[package]` (metadata: `name`/`version`/`description`/`license`/`repository`/`homepage`/`keywords`/`authors`) with `package.json` (`entry`/`scripts`), plus DashScript-specific `target` (output shape: `bin` default / `rust` / `wasm` / `napi`) and `workspaces` member globs (plural, mirroring npm/yarn/bun's `package.json` field; pnpm alone uses a separate `pnpm-workspace.yaml`). ✅ no ecosystem clash, one file for project + metadata · ❌ one more file format to document.

**Target-prefixed dependencies (`rust:serde`) (vs bare names).**
Today DashScript targets Rust, so dependencies carry a `rust:` prefix that mirrors `ds add rust:<crate>` verbatim. The prefix is kept (not a bare name) so the schema stays forward-compatible if a genuinely different backend is ever added; `wasm`/`napi` are Rust target variants and reuse `rust:` deps, not new prefixes. ✅ forward-compatible schema · ❌ slightly more to type for the single-backend case.

**`ds build` ships a native binary by default (vs a Rust project).**
Like `vp pack` ships a runnable artifact in `dist/`, `ds build` translates → compiles (`cargo build --release`) → copies the binary to `dist/<name>`, so `dist/` holds a usable product, not an intermediate project. `--target rust` keeps the transpiler's first-class Rust output (`dist/<name>/`, a clean crate with no `target/`) for inspection or as the `wasm`/`napi` target starting point. ✅ `dist/` is a product; transpiler output still one `--target rust` away · ❌ a release compile is slower than emit-only — use `--target rust` when you only want the Rust.

**Cached build, Deno-style lookup (vs a fresh temp dir per run).**
`ds build`/`ds run` resolve the cache by walking up from the `.ds` file for a `manifest.json`: found → in-project `.cache/dash/<name>/` (dependencies live with the manifest); not found (a lone file) → global `~/.cache/dash/<hash>/`. cargo's own `target/` lives there, so repeat builds are incremental. This mirrors Deno (project → local `node_modules`, lone file → global cache) and reuses cargo's two-layer dependency model (`~/.cargo/registry` source + project `target/`) rather than adding a DashScript-owned store. ✅ fast repeats, deps follow the manifest, lone files still work · ❌ `.cache/` must be gitignored; first build still compiles std.

**`ds add` — two modes, crate vs local file (vs one-size-fits-all).**
`ds add rust:<crate>` fetches the crate via cargo (like `pnpm add`) and records `rust:<crate>` in `manifest.json` — but generates **no `.ds` stub**: Rust is statically typed, so the crate's own source in `~/.cargo` is the complete type truth, read directly by the language server (exactly how rust-analyzer reads its deps — no parallel stub set to keep in sync). `ds add <file>.rs` runs **bindgen** on a local Rust source file, emitting `<stem>.ds` beside it for editor completion (the `@types`/DefinitelyTyped analogue). Bindgen therefore maps a file's public surface — `struct`/`enum`/`fn`/`trait`/`impl`. ✅ crates are zero-stub; local files get real declarations · ❌ bindgen coverage grows with the constructs local files expose.

**One `dashscript` package (vs a separate `@dashscript/cli`).**
The CLI is the product; splitting it into a sub-package adds an install step with no benefit. One package, one binary name (`ds`). ✅ simplest install (`pnpm add dashscript`) · ❌ coarser release granularity.

**DashScript-managed Rust toolchain (vs depend on a system `rustup`).**
DashScript pins a specific Rust version and downloads its standalone build on demand — like an npm dependency — into its own cache, so end users never install Rust separately. ✅ zero-setup install, reproducible builds · ❌ large first-run download and toolchain-management code. (Contributors building DashScript itself still need a system Rust toolchain.)

**One core crate, modular (vs many crates).**
The three responsibilities are small and share the translation table; a single `dashscript` crate with `translator` / `manifest` / `bindgen` modules is enough until a module needs independent versioning. ✅ low overhead · ❌ coarser release granularity.

**Workspace via manifest globs (vs a separate workspace file).**
A root `manifest.json` with a `workspaces` glob list (`["apps/*", "packages/*"]`) declares members — the same file already carries project metadata, so there is no separate `pnpm-workspace.yaml`. The plural `workspaces` mirrors npm/yarn/bun's `package.json` field (pnpm alone uses a separate file). `ds build` at the root emits **one cargo workspace** — members at `.cache/dash/<name>/` under a root `[workspace]` `Cargo.toml` — and compiles it once: members share `target/` and `Cargo.lock`, so a dependency two members use compiles once (cargo's hoisted-`node_modules`); `--filter <name>` picks one. ✅ one manifest format, monorepo from day one, shared compilation · ❌ no inter-member path dependencies or task caching (turbo/nx) yet — those land as real demand drives them.

## Roadmap

- **Initial scope** — `translator` (a core subset of oxc AST → Rust), `manifest` (`manifest.json` → `Cargo.toml`), a DashScript-managed Rust toolchain (pinned, downloaded on demand), `ds build` (native binary) / `ds run` / `ds check` / `ds fmt`, `bindgen` + `ds add`. One `.ds` file compiles to a native binary (or a Rust crate with `--target rust`), checked by `cargo`.
- **More outputs** — `wasm` and `napi` targets (Rust compiled to WebAssembly / napi-rs), so `.ds` ships to the web and Node ecosystems.
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
