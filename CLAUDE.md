# CLAUDE.md

You are a senior developer working on **DashScript** — JavaScript/TypeScript ergonomics, Rust performance, native + wasm + napi outputs. It compiles **JavaScript/TypeScript** to idiomatic **Rust** shipped as a **native binary** (with **WebAssembly** and **napi** targets on the same mapping table), pursuing maximum **test262** conformance, the **WinterTC** Minimum Common Web API, and a bridge between the **npm** and **cargo** ecosystems. oxc parses `.ts`/`.tsx`/`.js`/`.jsx`/`.mjs`/`.cjs` alike; the static translator maps the ESM surface (`import`/`export`), so `.ts` and ESM `.js`/`.mjs` lower statically, while a CommonJS module (`require`/`module.exports`) degrades to the embedded QuickJS engine. DashScript does **not** implement its own parser: it reuses [`oxc`](https://oxc.rs/) (`oxc_parser` + `oxc_ast` + `oxc_allocator`) for the TypeScript-flavored front end, then translates the resulting AST into Rust source and a `Cargo.toml`. `check` and `fmt` are built in-process on that same parsed AST — `oxc_linter` and `oxc_formatter` are `publish = false` in oxc's workspace (not on crates.io), so DashScript reuses oxc as a _capability_ (AST + diagnostics + codegen) rather than depending on those crates. The core is Rust; the `ds` CLI ships as a single `dashscript` package (npm + standalone binary).

> Coding standards, design patterns, and the contribution workflow live in [CONTRIBUTING.md](./CONTRIBUTING.md). This file is the architectural context an agent must understand before changing code. Read both.

## Project

**DashScript** is a TS → Rust transpiler. Three jobs, no more:

1. **Translate** — oxc AST → idiomatic Rust source.
2. **Package** — a `package.json` project package → `Cargo.toml`.
3. **Bindgen** — a local Rust source file → a `.d.ts` type declaration, for editor type hints.

| Aspect               | Value                               |
| -------------------- | ----------------------------------- |
| Language name        | DashScript                          |
| File extension       | `.ts` (+`.js`/`.mjs`/`.cjs`)        |
| npm package / binary | `dashscript` (binary command: `ds`) |
| Repo                 | `DemoMacro/dashscript` (MIT)        |

**Core philosophy**

- **Dash** — fast. Reuse oxc (one of the fastest TS parsers) for the front end, build `check`/`fmt` on the same parsed AST in-process, emit native Rust, and validate the output with `cargo check` / `cargo clippy`.
- **Script** — a typed, TypeScript-flavored surface. Developers write what they know; DashScript maps it to Rust.
- **Bridge** — the AST-to-Rust translation table, plus package and bindgen, carry JavaScript/TypeScript semantics into the Rust world safely.

## Tech Stack

| Layer                | Technology                                               | Role                                                                                    |
| -------------------- | -------------------------------------------------------- | --------------------------------------------------------------------------------------- |
| Parsing              | `oxc_parser` + `oxc_ast` + `oxc_allocator` (Rust crates) | `.ts`/`.js` → AST. **Reused, not reimplemented.**                                       |
| Check & format       | `oxc_parser` AST + `oxc_diagnostics` + `oxc_codegen`     | `ds check` (translatability) / `ds fmt` (pretty-print); not a shell-out to oxlint/oxfmt |
| Translation core     | Rust                                                     | AST → Rust source (the only logic DashScript owns)                                      |
| Rust emission        | `syn` AST construction + `prettyplease` printer          | idiomatic, `cargo fmt`-clean output                                                     |
| Package              | `package.json` → `Cargo.toml`                            | npm fields reused; `dashscript.cargo` for Rust crate deps + target                      |
| Bindgen              | Rust (`syn`-style crate metadata) → `.d.ts` declaration  | type hints for Rust crates                                                              |
| Rust toolchain       | pinned standalone build, DashScript-managed              | downloaded on demand like an npm dependency; no system `rustup` for end users           |
| JS surface           | TypeScript (ESM, strict)                                 | single `dashscript` npm package (CLI wrapper, types)                                    |
| Build / check / test | vite-plus (`vp pack` / `vp check` / `vp test`), `cargo`  | unified toolchain                                                                       |

## Compilation Pipeline

```
.ts/.js source
  → oxc parser (reused)          .ts/.js → oxc AST
  → translator (DashScript)      oxc AST → Rust source
  → package (DashScript)        package.json → Cargo.toml
  → cached cargo project         .cache/dash/<name>/ in-project, or ~/.cache/dash/<hash>/ for a lone file
  → output                       dist/<name> (native binary, default) or dist/<name>/ (Rust crate, --target rust)

ds lint / ds check / ds fmt     built in-process on the oxc_parser AST (oxc_linter/oxc_formatter are publish=false)
```

Correctness is a three-layer chain: (1) **structure** — `oxc_parser` parses `.ts` and reports syntax errors; (2) **translatability** — DashScript's own `lint` walks the AST and flags any construct the translator cannot lower to valid Rust (the translator is the single source of truth for "what maps"); (3) **target** — `cargo check` / `cargo clippy` on the emitted project is the final arbiter. There is no cross-target IR: oxc gives structure, the translator is the mapping table, `cargo` gives Rust correctness. Constructs the static translator cannot lower do not stop the build — they degrade to the embedded QuickJS engine per-function (the compatibility path; see Design Decisions).

## Architecture: Translation Model

The central mental model — a **mapping table**, not a multi-stage compiler:

| Front (`.ts`, via oxc AST)  | Bridge rule  | Back (Rust)                          |
| --------------------------- | ------------ | ------------------------------------ |
| `number`                    | scalar       | `f64` (or `i64`/`u64` by annotation) |
| `string`                    | scalar       | `&str` param / `String` return       |
| `boolean`                   | scalar       | `bool`                               |
| `T[]` / `Array<T>`          | collection   | `Vec<T>`                             |
| `interface` / `type` object | record       | `struct`                             |
| `class`                     | record       | `struct` + `impl`                    |
| `function`                  | callable     | `fn`                                 |
| union `A \| B`              | tagged union | `enum`                               |

Three sub-systems share this table:

- **translator** — walks the oxc AST and emits Rust. Mappings are organized to mirror their source of truth: `expressions/` is one file per AST node family (`mod.rs` is dispatch + shared helpers only); `builtins/` is one file per ES built-in mirroring tc39 test262's `test/built-ins/`, so a differential failure points straight at the file to fix. Each AST node kind has one mapping rule; unmapped nodes raise a clear diagnostic rather than silently producing broken Rust.
- **package** — reads the project's `package.json` and emits a `Cargo.toml`. Standard npm fields (`name`/`version`/`bin`/`main`/`scripts`/`workspaces`/`dependencies`/`devDependencies`) are reused verbatim; Rust crate deps live under `dashscript.cargo.dependencies` (bare crate names, Cargo.toml-style values — `"serde": "1.0"` or `{ "version": "1.0", "features": [...] }`), and `dashscript.target` sets the output shape. npm `dependencies` resolve to `node_modules` and never reach Cargo.toml.
- **bindgen** — reads a local Rust source file's public surface and emits a `.d.ts` declaration beside it, so importing it in `.ts` yields editor completion and types. This is what `ds add <file>.rs` runs. A crate added via `ds add cargo:<crate>` needs no `.d.ts` stub — its types come from the crate's own source in `~/.cargo`, read directly by the language server (the way rust-analyzer reads its deps).

## Architecture: Distribution

Hybrid cargo + pnpm workspace. One product name, two reach paths. **Core logic lives only in `crates/dashscript/` (the library).** The `ds` binary is a thin target on that same crate (`bin/`); the npm package is a thin wrapper. Never put translation logic in `bin/` or the npm package — it would then exist in only one distribution path.

| Path                   | Contains                                                                     | Consumed by                                                                                  |
| ---------------------- | ---------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `crates/dashscript/`   | Rust library (translator + package + bindgen) **+ the `ds` binary** (`bin/`) | `cargo install dashscript` (the `ds` CLI), `cargo add dashscript` (the library), brew, scoop |
| `packages/dashscript/` | Single npm package — `ds` CLI wrapper + editor types                         | `pnpm add dashscript`, `npx ds`                                                              |

## Package Layout

```
crates/
  dashscript/            the only crate — library + the `ds` binary
    src/                 the library: translator + package + bindgen
      translator/        oxc AST → Rust source
        expressions/     one file per AST node family (literals, binary, …, call);
                         mod.rs is the dispatch table + shared helpers only
        builtins/        ES built-in libraries — one file per built-in, mirroring
                         tc39 test262's test/built-ins/ (math, array, string,
                         number, object, global, console); node/ and web/ reserved
                         for future Node stdlib (node:crypto…) and Web APIs (fetch)
        functions/       statement-level translation, one file per kind
        …                declarations, types, bindings, context, check, registry
      package.rs        package.json → Cargo.toml
      bindgen.rs         Rust crate → .d.ts type declaration
    bin/                 the `ds` binary — thin, no translation logic
      main.rs            dispatch + help
      commands/          one module per command group
      lsp.rs             the language server

packages/
  dashscript/            the single npm package: bin `ds` + editor types
```

One core crate, three modules, three responsibilities. Split into more crates only when a module grows its own release cadence — not before.

## CLI

Unified entry `ds`, subcommand style:

```
ds <file.ts>                  # run a file directly (like `node a.js`)
ds run <script>               # run a package.json script (like `pnpm run`)
ds build [<file>] [--target]  # parse → translate → compile a native binary in dist/<name>
                              #   --target rust → emit the Rust crate in dist/<name>/ instead
ds lint <file>                # translatability check (parser + translator rules)
ds check <file>               # lint + format check, like `vp check`
ds fmt <file>                 # format .ts in place (built-in formatter)
ds install                    # fetch package deps via cargo + write Cargo.lock (like `pnpm install`)
ds add cargo:<crate>           # fetch crate via cargo + record cargo:<crate> in package.json
ds add <file>.rs              # bindgen a local Rust file → <stem>.d.ts type declaration
ds cache clean                # remove the in-project .cache/
ds test                       # run .ts tests (planned)
```

`ds <file.ts>` runs a file directly (like `node a.js`); `ds run <script>` runs a `package.json` script (like `pnpm run` — `run` is always explicit, so it never collides with `ds <file.ts>`). `ds build` defaults to a **native binary**: translate → a cached Cargo project (`.cache/dash/<name>/` in-project, keyed by package name so two projects' `main.ts` don't collide; or `~/.cache/dash/<hash>/` for a lone file, found by walking up for a `package.json`) → `cargo build --release` → copy each declared `bin` to `dist/<bin-name>`. A project is one crate — every `.ts` translates into `src/<stem>.rs`, and only `bin`/`main` become cargo targets. `--target rust` stops at the translated crate (`dist/<name>/`, no `target/`); `--target` overrides `dashscript.target` (default `bin`). `<name>` is the package.json `name` (fallback: parent dir, then file stem). `target/` never lands in `dist/`.

`ds build` at a **workspace root** (a `package.json` with `workspaces` globs, e.g. `["apps/*", "packages/*"]`) builds every member under **one cargo workspace**: members at `.cache/dash/<name>/` beneath a root `[workspace]` Cargo.toml that owns a shared `target/` and `Cargo.lock`, so a dependency two members share compiles once. Each member's binary lands in its **own** `<member>/dist/<name>` (not the workspace root), so every package stays independently publishable. `--filter <name>` builds one member. (`--target rust` emits each member's crate to its own `<member>/dist/<name>/`; inter-member path dependencies and task caching are not yet done.)

`ds add` has two modes: `cargo:<crate>` records a crate in `package.json` (no `.d.ts` stub — types come from the crate's source via the language server); `<file>.rs` runs bindgen to emit `<stem>.d.ts`. There is **no separate `ds gen` step**.

## Design Decisions

Each decision states its core choice and the invariant an agent must respect — what _not_ to "fix".

**Reuse oxc for parsing, check, format (vs `oxc_linter`/`oxc_formatter`).** DashScript reuses the _published_ part of oxc (`oxc_parser`/`oxc_ast`/`oxc_allocator`); `oxc_linter`/`oxc_formatter` are `publish=false`, so `ds lint`/`ds fmt` are built in-process on the parsed AST (report _translatability_; pretty-print via `oxc_codegen`), never shell-out to oxlint/oxfmt. Invariant: reuse oxc's published crates for a new front-end capability — don't reimplement parsing/formatting.

**Translatability — single source of truth = the `classify` table (vs a parallel rule tree in `check`).** The translator owns `Mapping { Mapped, Reject, DegradeEngine }` + `classify_expr`/`classify_stmt`; `check`/`program_uses_engine` traverse and query it, not a hand-maintained rule tree. A drift metatest keeps `classify` verdicts in sync with the translator's exhaustive `match` arms. Invariant: add a mapping → change `classify` once; don't rebuild a rule tree in `check`.

**Transpiler, not a full language (vs own type-checker / IR).** A JS/TS→Rust mapping table plus `cargo check` on the output covers the goal. oxc gives structure, `cargo` gives correctness; there is no cross-target IR — `wasm`/`napi` are Rust target variants (same table, different `cargo --target`), not separate backends.

**Static-first, degrade over reject — one per-function QuickJS boundary for ES core + Web APIs (vs reject; vs per-expr engine; vs npm whitelist; vs a separate Web-API track).** A construct the translator cannot lower statically degrades at **function** granularity (and the **import** for npm packages) to embedded QuickJS (`rquickjs`, full ES semantics), keeping its Rust signature with `serde_json` marshalling at the boundary — never per-expression. This is **one unified boundary** for ES core and Web APIs: a Web API maps to a Rust crate first (zero-cost); a Web-API edge takes the same boundary with the API registered as a builtin delegating to the **same Rust impl** (`wire_web_apis`), so degraded behavior == static behavior. The trigger is `js_module_needs_engine` (a module the translator cannot lower), not an `is_npm_js` blanket rule — `.js`/`.mjs`/`.cjs` take the same static-first path as `.ts`. The degrade backend is target-pluggable (native embeds `rquickjs`; `--target wasm` swaps in QuickJS-wasm at the same boundary).

**Reflection — compile-time evaluation (vs NaN-boxing).** DashScript keeps Rust's static types (`number`→`f64`, `class`→`struct`: no tag tax, no GC) and evaluates reflection at compile time when the type is known: `typeof x`→a string literal; `x instanceof C`→a `bool`; `Object.keys(obj)`→the struct's field names; `obj.hasOwnProperty("k")`→a compile-time `bool`. These lower to plain Rust (zero-cost on native and wasm; the static segment needs no engine). Only reflection on a dynamically-typed value (`for...in` over an arbitrary value, a runtime key, `Reflect.ownKeys` on a dynamic value) degrades per-function. Invariant: prefer extending the compile-time path over reaching for the engine; never NaN-box.

**JavaScript/TypeScript surface, Rust semantics (vs a new surface syntax).** The surface is the syntax developers know — TS (`.ts`, typed) and ES (`.js`/`.mjs`/`.cjs`, untyped), all parsed into one AST; type annotations are _presentation_, not a prerequisite for translation. Static-first + graceful degradation (CJS, dynamic reflection, a Web-API edge degrade). The test262 suite (pure-`.js` fixtures, each through the compile path) verifies the ECMAScript language surface end-to-end, so `.js` and `.ts` share one mapping table and one correctness chain. The goal is to express the full Rust type/memory-safety model with TS as _presentation_ only; "covers full Rust" is a direction, not a present-tense claim — the residual tail is what degrades.

**Implicit `fn main`, pure-TS execution semantics (vs special-casing `function main`).** A `.ts` file runs like a Node script: top-level declarations (`function`/`class`/`interface`/`type`/`import`/`export`) become Rust items that do **not** execute; top-level executable statements run in source order, collected into an implicit `fn main` the translator always emits (empty for a declarations-only file). `function main` is an ordinary declaration renamed `__ds_main` at the `NameTable` symbol level (call sites follow automatically); a user runs it by calling `main();` at the top level.

**Module-global bindings for top-level `const`/`let` (vs rejecting the escape).** A top-level `let` mutated and read across top-level `function`s is a single-threaded module global. Const-expr literal → `pub const`; a runtime-immutable binding referenced by a function → `static OnceLock<T>` + accessor; a mutable binding → `thread_local! { static RefCell<T> }` + get/set accessors (`RefCell<Option<T>>` for a delayed `T | undefined`). _Today: a module file initializes eagerly; an entry file seeds in source order from `fn main`._

**Class — `struct` + `impl` (vs rejecting `class`; vs a `Box<dyn>` model).** `class C` → `#[derive(Clone)] struct C { pub fields } impl C { fn new, pub fn methods }`: fields → `pub` struct fields; `constructor` → `fn new`; methods → `pub fn method(&self | &mut self)` (`this`→`self`; `&mut self` when the body mutates a `this` member); `new C(...)` → `C::new(...)`. `private`/`protected` → `pub` (visibility only); `WeakMap`/`WeakSet` → strong `HashMap`/`HashSet` backing (no GC-precise weak ref, but correct value semantics). A `get` accessor → a zero-arg method; a class-level generic `<T>` → a generic struct + `impl<T: Clone>`. Invariant: `#private`, `extends`/`super`, `set` accessors, `<T extends X>`, and `static` members stay `unsupported`.

**`package.json` — reuse the npm manifest (vs a dedicated `manifest.json`).** DashScript reuses `package.json` directly. Standard npm fields map to cargo idioms (metadata → `[package]`; `bin` → `[[bin]]`; `main` → `[lib]`; `scripts` → npm-only). Cargo-only concerns live under a `dashscript` namespace: `dashscript.cargo.dependencies`/`devDependencies` (→ `[dependencies]`/`[dev-dependencies]`); `dashscript.target` (bin/rust/wasm/napi). npm `dependencies`/`devDependencies` stay JS deps → `node_modules`, never reach Cargo.toml.

**`dashscript.cargo` namespace + `cargo:` import prefix (vs a flat dependency list).** Cargo crate deps are not npm deps (no `node_modules`, no semver, live in `~/.cargo`), so they sit in their own `dashscript.cargo.dependencies` map. In `.ts` an import mirrors the crate name verbatim (`import { Adler32 } from "adler"`); on the CLI `ds add cargo:<crate>` records it (the `cargo:` prefix is optional). `cargo:` is the Deno-style import-family marker; `wasm`/`napi` reuse `cargo:`, not a new prefix.

**`ds build` ships a native binary by default (vs a Rust project).** `ds build` translates → compiles (`cargo build --release`) → copies the binary to `dist/<name>`, so `dist/` holds a product, not an intermediate project. `--target rust` keeps the transpiler's first-class Rust output (`dist/<name>/`, no `target/`) for inspection or as the `wasm`/`napi` starting point.

**Cached build, Deno-style lookup (vs a fresh temp dir per run).** `ds build`/`ds run` resolve the cache by walking up from the `.ts` file for a `package.json`: found → in-project `.cache/dash/<name>/`; not found (a lone file) → global `~/.cache/dash/<hash>/`. cargo's `target/` lives there, so repeat builds are incremental. This reuses cargo's two-layer dependency model rather than adding a DashScript-owned store.

**`ds add` — two modes, crate vs local file (vs one-size-fits-all).** `ds add cargo:<crate>` (prefix optional) fetches the crate and records the bare name under `dashscript.cargo.dependencies`, generating **no `.d.ts` stub** (the crate's source in `~/.cargo` is the complete type truth, read directly by the language server). `ds add <file>.rs` runs bindgen, emitting `<stem>.d.ts` for editor completion.

**One `dashscript` package (vs a separate `@dashscript/cli`).** The CLI is the product; a sub-package adds an install step with no benefit. One package, one binary name (`ds`).

**DashScript-managed Rust toolchain (vs a system `rustup`).** DashScript pins a Rust version and downloads its standalone build on demand into its own cache, so end users never install Rust separately. Contributors building DashScript itself still need a system Rust toolchain.

**One core crate, modular (vs many crates).** The three responsibilities are small and share the translation table; a single `dashscript` crate with `translator` / `package` / `bindgen` modules is enough until a module needs independent versioning.

**Workspace via package globs (vs a separate workspace file).** A root `package.json` with a `workspaces` glob list declares members (plural, mirroring npm/yarn/bun; pnpm uses a separate `pnpm-workspace.yaml`). With no `workspaces`, DashScript falls back to `pnpm-workspace.yaml`'s `packages:` or `deno.json`'s singular `workspace` field. `ds build` at the root emits **one cargo workspace** — members at `.cache/dash/<name>/` under a root `[workspace]` Cargo.toml sharing `target/` and `Cargo.lock` (a dependency two members use compiles once). Metadata/deps inherit cargo-native: the root carries `[workspace.package]`/`[workspace.dependencies]`, members use `field.workspace = true`. `--filter <name>` picks one.

**Package integration — every package is a crate (vs merge-into-consumer; vs a JS bundler).** A package — workspace member or npm dependency — is one cargo crate, referenced by a consumer through a cargo path dependency (`use office_open_xml::X`), exactly how `node_modules` layers packages. DashScript rejects the "merge" model (copying a dependency's source into the consumer's crate under a `member_crate_<stem>` prefix) — it is the root of a bug cluster (barrel-over-definition emit collisions, the `__ds_defn` suffix, cross-package type-intern complexity). Cargo's workspace + path dep is already Rust's `node_modules` (RFC 1525), so DashScript introduces **no JS bundler (no rolldown/esbuild)** — bundling re-parses, loses the 1:1 source→Rust map, and duplicates what cargo already does. A package in `.js`/`.mjs`/`.cjs` takes the same static-first path as `.ts`. _Today: workspace members build as independent crates with cross-member path deps; npm packages still largely use the merge model — the `.js` static-first path and npm independent-crate migration are the remaining direction._

**test262 as the conformance oracle, assert-driven (vs hand-written expectations; vs a Node stdout diff).** The harness runs each tc39 test262 fixture with an **assert-driven** verdict: the fixture's own `assert.sameValue`/`assert.throws` carry the expected values (the ECMAScript reference), so a fixture passes when its asserts hold — no Node oracle, no hand-written expectations. **Every fixture goes through the compile path** (`Translator::check` → `translate` + `cargo build` → run); there is no separate engine testbed. A fixture the translator cannot lower degrades per-function: the body is emitted into the production binary's embedded QuickJS (`__ds_engine` + `wire_web_apis`), so it runs inside the binary the user ships. Test compatibility (`assert`/`throws`/`Test262Error`, WPT `assert_equals`/`AssertionError`, `console`) is lifted into production as `register_*` engine builtins — the Javy register pattern, one Rust impl shared by the static `__ds::X` path and the engine builtin. Verdict: exit 0 = `supported`; a thrown `Test262Error`/`AssertionError` = `partial`; a build failure / timeout / `ReferenceError` (the binary's QuickJS lacks a host — `$262` for true threads, `$DONE` for an async loop, ShadowRealm reflection) = `unsupported`. The extractor carries every `test/built-ins/<cat>/` fixture + its `includes:` list (no whitelist).

## Roadmap

- **Initial scope** — `translator` (the core of the oxc AST → Rust), `package` (`package.json` → `Cargo.toml`), a DashScript-managed Rust toolchain (pinned, downloaded on demand), `ds build` (native binary) / `ds run` / `ds check` / `ds fmt`, `bindgen` + `ds add`. One `.ts`/`.js` file compiles to a native binary (or a Rust crate with `--target rust`), checked by `cargo`.
- **Compatibility — degrade, don't reject** — a construct the static translator cannot lower runs under an embedded QuickJS engine at the **function** granularity (inheriting full ECMAScript semantics) instead of failing, so existing TS/JS keeps working. _Today: per-function degradation is in via embedded QuickJS (a transitively-unmappable npm `.js` module pulls QuickJS for that module, but `.js`/`.ts` share one static-first path — see \_Package integration_); the conformance harness verifies degraded fixtures through the compile path (the production binary's embedded QuickJS, `__ds_engine` + `wire_web_apis`), not a separate in-process testbed.\_
- **WinterTC Minimum Common Web API** — the Ecma TC55 (formerly WinterCG) Minimum Common Web Platform API, on the **same static-first + per-function degrade model as the ECMAScript core** (one boundary, not two): each Web API maps to a Rust crate first (static, zero-cost), and a Web API edge the static translator cannot lower takes the per-function engine path with the Web API registered as a builtin delegating to the same Rust impl. Synchronous APIs map first; asynchronous ones are the point that introduces a tokio runtime and a thread model — at which point test262's true-thread `Atomics` unlocks naturally. Conformance is a WPT subset, run static-first with per-function engine degrade (mirroring test262). §6-exempt reflection (`instanceof`/`idlharness`/property descriptors) stays out-of-scope regardless. _Today: the WPT conformance layer is in and defaults to the degrade model; the synchronous set is largely mapped (URL/URLSearchParams/TextEncoder/Headers/Blob/FormData/SubtleCrypto/EventTarget/AbortController/DOMException/CompressionStream/ReadableStream…), and engine builtins (Encoding/HrTime/Base64/Crypto) share one Rust impl with the static path._
- **More outputs** — `wasm` and `napi` targets (Rust compiled to WebAssembly / napi-rs), so `.ts` ships to the web and Node ecosystems. The static segment (including compile-time-resolved reflection) lowers to plain wasm — semantically complete with no engine bundled for the common case; only the degraded minority needs a wasm JS engine (QuickJS-wasm — Javy proves `rquickjs` compiles to `wasm32-wasip1`), target-pluggable at the same per-function boundary as native.
- **Developer experience** — `ds test`, editor/LSP integration, conformance fixtures. (`ds run` already builds and runs a Cargo project.)
- **Self-hosting (north star)** — rewrite the toolchain in `.ts` itself: the Rust bootstrap compiler compiles a `.ts` compiler, which then compiles itself. Viable because `.ts` reaches `oxc` (and any Rust crate) through bindgen — no need to reimplement oxc.

## Performance

- Inherit oxc's parsing/lint/format speed; no duplicate front-end work.
- Emit `cargo fmt`-clean Rust so the output needs no reformatting.
- Delegate correctness to `cargo check` / `cargo clippy` rather than reimplementing a Rust type-checker.
- **Static segment is real Rust** — `number`→`f64`, `class`→`struct`, no NaN-boxing tag tax and no GC, so statically-mapped code (and statically-resolved reflection) runs at hand-written Rust speed; only the degraded minority pays the engine boundary cost. This is the fundamental edge over a dynamic-value-model peer, which tags every value and GCs every allocation.

## Behavioral Guidelines

- State assumptions explicitly. If a mapping or crate does not exist yet, say so before implementing against it.
- No features beyond what was asked. No speculative abstractions. (Core logic lives in `crates/` only; mappings live in the `translator` table; do not reimplement what oxc already provides.)
- Touch only what you must. Match existing style — Rust follows Rust idioms, JS surfaces follow the existing TS conventions.
- Transform tasks into verifiable goals: "add a mapping" → "write a `.ts` fixture, run `ds build`, compile the emitted Rust with `cargo check`, assert it builds."
- Verify before finishing a change: run `pnpm check` (`vp check` for TS lint/format/typecheck + `cargo clippy` on the core crate). After changing Rust translator logic, also run `cargo test --lib` (the 800+ translator tests). Never declare a task done with a check or test failing — report it.
