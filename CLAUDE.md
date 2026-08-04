# CLAUDE.md

You are a senior developer working on **DashScript** — TypeScript ergonomics, Rust performance, compiled to native. It is a typed, TypeScript-flavored language (`.ts`) that compiles to native binaries via idiomatic Rust (`wasm` / `napi` outputs planned). DashScript does **not** implement its own parser: it reuses [`oxc`](https://oxc.rs/) (`oxc_parser` + `oxc_ast` + `oxc_allocator`) for the TypeScript-flavored front end, then translates the resulting AST into Rust source and a `Cargo.toml`. `check` and `fmt` are built in-process on that same parsed AST — `oxc_linter` and `oxc_formatter` are `publish = false` in oxc's workspace (not on crates.io), so DashScript reuses oxc as a _capability_ (AST + diagnostics + codegen) rather than depending on those crates. The core is Rust; the `ds` CLI ships as a single `dashscript` package (npm + standalone binary).

> Coding standards, design patterns, and the contribution workflow live in [CONTRIBUTING.md](./CONTRIBUTING.md). This file is the architectural context an agent must understand before changing code. Read both.

## Project

**DashScript** is a TS → Rust transpiler. Three jobs, no more:

1. **Translate** — oxc AST → idiomatic Rust source.
2. **Package** — a `package.json` project package → `Cargo.toml`.
3. **Bindgen** — a local Rust source file → a `.d.ts` type declaration, for editor type hints.

| Aspect               | Value                               |
| -------------------- | ----------------------------------- |
| Language name        | DashScript                          |
| File extension       | `.ts`                               |
| npm package / binary | `dashscript` (binary command: `ds`) |
| Repo                 | `DemoMacro/dashscript` (MIT)        |

**Core philosophy**

- **Dash** — fast. Reuse oxc (one of the fastest TS parsers) for the front end, build `check`/`fmt` on the same parsed AST in-process, emit native Rust, and validate the output with `cargo check` / `cargo clippy`.
- **Script** — a typed, TypeScript-flavored surface. Developers write what they know; DashScript maps it to Rust.
- **Bridge** — the AST-to-Rust translation table, plus package and bindgen, carry TS-front semantics into the Rust world safely.

## Tech Stack

| Layer                | Technology                                               | Role                                                                                    |
| -------------------- | -------------------------------------------------------- | --------------------------------------------------------------------------------------- |
| Parsing              | `oxc_parser` + `oxc_ast` + `oxc_allocator` (Rust crates) | `.ts` → AST. **Reused, not reimplemented.**                                             |
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
.ts source
  → oxc parser (reused)          .ts → oxc AST
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

Each decision states its trade-off so contributors know what _not_ to "fix".

**Reuse oxc for parsing, check, and format (vs depending on `oxc_linter`/`oxc_formatter`).**
DashScript's surface is TypeScript-flavored, so it reuses `oxc_parser`/`oxc_ast`/`oxc_allocator` (the _published_ part of oxc) rather than re-deriving TS grammar. `oxc_linter` and `oxc_formatter`, however, are `publish = false` in oxc's workspace — not on crates.io. So: `ds lint` reuses `oxc_parser` + `oxc_diagnostics` to report _translatability_ (does this `.ts` lower to valid Rust? — something eslint-style rules cannot express); `ds fmt` reuses `oxc_codegen` (published, pretty-print by default, not minified). `ds check` is the composite — lint plus a format check — matching `vp check`. ✅ no giant git dependency, keeps the "fast" promise · ❌ coupled to oxc's published API surface.

**Translatability, single source of truth = the translator's `classify` table (vs a parallel rule tree in `check`).**
What the translator can lower is what `ds lint` accepts. The contract is half-enforced already: `translate_type`/`translate_expr`/`translate_stmt` are exhaustive `match`es with no `_` fallback, so every unmapped AST kind is explicit in the translator — but its unsupported arm is an undifferentiated `todo!()`, so `check` keeps a hand-maintained `unsupported_pattern` tree that duplicates the translator's knowledge and drifts (a new mapping does not auto-relax a check rejection). The fix is not a full typed IR (oxc AST + `oxc_semantic` already give structure + semantics; scriptc needs an IR only because it has no oxc) but a **classify table**: the translator owns `Mapping { Mapped, Reject, DegradeEngine }` + `classify_expr`/`classify_stmt`, and `check`/`program_uses_engine` degrade to traverse-and-query. ✅ one place to change when a mapping is added; the Reject/DegradeEngine split also drives per-function engine fallback · ❌ the translator must keep the `classify` table in sync with its `match` arms (a drift metatest enforces it). _Today: the `classify` table is the single source of truth — `check`/`program_uses_engine` traverse and query it; a drift metatest keeps its verdicts in sync with the translator's exhaustive `match` arms._

**Transpiler, not a full language (vs own type-checker / IR).**
A TS-front → Rust mapping table plus `cargo check` on the output covers the goal with a fraction of the surface area. oxc gives structure; `cargo` gives correctness. ✅ small scope, fast to ship · ❌ no cross-target IR — the `wasm`/`napi` outputs are Rust target variants (same mapping table, a different `cargo --target`), not separate backends.

**Static-first, degrade over reject — one per-function QuickJS boundary for ES core + Web APIs (vs reject; vs per-expr engine; vs an npm whitelist; vs a separate "Web APIs never degrade" track).**
A construct the translator cannot lower to idiomatic Rust should still run, not stop the user. The fallback granularity is the **function** (and the **import** for npm packages), never the expression: a function body whose dynamic semantics (`typeof`/`in`/`delete`/computed keys/`Function` value/prototype reflection/`Reflect`/`Symbol`/`Proxy`) the static translator cannot express runs under embedded QuickJS (`rquickjs`, QuickJS-NG) — inheriting full ECMAScript semantics — while its Rust signature stays, with `serde_json` marshalling at the entry/exit boundary. This is a **single unified degrade boundary** covering both the ECMAScript core and the WinterTC Web APIs: each Web API maps to a Rust crate first (static, zero-cost), and a Web API edge the static translator cannot lower takes the same per-function engine path, where the Web API is registered as a builtin that delegates to the **same Rust impl** as the static path (`wire_web_apis`) — so degraded behavior is identical to static behavior, and conformance is preserved. (The prior stance — "Web APIs are a separate, static track, never degraded" — was premised on "QuickJS cannot target wasm"; Javy, which compiles `rquickjs` to `wasm32-wasip1` and registers Web APIs as builtins, disproves that premise, so the boundary unifies.) ✅ one degrade model not two, compatibility without rewriting source, the static path stays zero-cost and native · ❌ only that function pays the QuickJS cost; marshalling bounds the value types crossing the boundary (per-expr degradation is deliberately out of scope); the Web API engine builtin must stay in lockstep with its static impl. npm packages load via a QuickJS module loader (no compile whitelist); Node-API packages (`fs`/`process`) are marked, not silently miscompiled. _Today: per-function and transitive whole-module (npm `.js`) degradation are in via embedded QuickJS; `wire_web_apis` registers Web API builtins (Encoding/HrTime/Base64/Crypto) that share one Rust impl with the static path; the WPT conformance layer defaults to this degrade model. The degrade backend is target-pluggable by design — native embeds `rquickjs` in-process; a future `--target wasm` swaps in QuickJS-wasm (Javy proves `rquickjs` compiles to `wasm32-wasip1`) at the same per-function boundary, so the same degrade decision follows the code to the web._

**Reflection — compile-time evaluation, not a dynamic value model (vs NaN-boxing).**
A TS→native peer faces reflection (`typeof`/`instanceof`/`Object.keys`/`hasOwnProperty`) one of two ways. Perry and ant make every value a NaN-boxed 64-bit tag and every object a GC'd header + `class_id` + property table — reflection is then a tag read or a class-id chain walk (zero-cost), but every arithmetic op pays an unbox/box tax and every allocation pays a GC write barrier. DashScript keeps Rust's static types (`number`→`f64`, `class`→`struct`: no tag tax, no GC) and instead **evaluates reflection at compile time when the type is known**: `typeof x` → a string literal (already mapped); `x instanceof C` → a `bool` when `x`'s Rust type and `C`'s struct are both known (DashScript has no `extends`, so this is simpler than in a polymorphic engine); `Object.keys(obj)` → the struct's field names; `obj.hasOwnProperty("k")` → a compile-time `bool`. These lower to plain Rust, so they are zero-cost on native **and** on wasm — the static segment (including compile-time reflection) needs no engine, which is what makes the wasm output semantically complete for the common case. Only genuinely dynamic reflection (`for...in` over an arbitrary value, a runtime-computed key, `Reflect.ownKeys` on a dynamic value) degrades per-function — and on wasm that degraded minority pulls in QuickJS-wasm (the static segment stays engine-free). ✅ maximizes the Rust payoff (no NaN-boxing tax; reflection zero-cost where statically resolvable) and makes wasm portable for the common case without an engine · ❌ reflection on a value whose type is not statically known still degrades (per-function engine boundary cost).

**`.ts`: TypeScript surface, Rust semantics (vs a new surface syntax).**
`.ts` is written in a TypeScript-flavored syntax developers already know, but its semantics are Rust's — the goal is to express the full Rust type/memory-safety model (ownership, borrowing, lifetimes, traits), with TypeScript as the _presentation_ only. Today the translator covers a safe TS→Rust subset (auto clone/borrow/narrowing bridge the gaps); Rust-only constructs (explicit lifetimes, trait bounds, `unsafe`) are reached incrementally as real demand drives each, never speculatively. ✅ familiar to write, sound underneath · ❌ "covers full Rust" is a direction, not a present-tense claim.

**Implicit `fn main`, pure-TS execution semantics (vs special-casing `function main` as the cargo entry).**
A `.ts` file runs like a Node script: top-level declarations (`function`/`class`/`interface`/`type`/`import`/`export`) become Rust items and do **not** execute; top-level executable statements (`const`/`let`, expression statements, control flow, `throw`) run in source order, collected into an implicit `fn main` the translator always emits — empty for a declarations-only file (the way Node runs a script that defines functions but never calls them). `function main` is therefore an ordinary declaration: it is renamed `__ds_main` at the `NameTable` symbol level (so every call site follows automatically) and cannot collide with the cargo entry; a user runs it by calling `main();` explicitly at the top level. A top-level binding referenced from inside a `function` would close over an `fn main` local — impossible for a Rust fn item — so `check` flags it `unsupported` (move the binding into the function, or call the function from the top level). ✅ matches the TS/Node mental model, no "is this the entry?" special case · ❌ a top-level `const`/`let` referenced by a top-level `function` would close over an `fn main` local — now hoisted to a module-global item (see the next decision), so this no longer forces a rewrite.

**Module-global bindings for top-level `const`/`let` (vs rejecting the escape).**
A top-level `let` mutated and read across top-level `function`s is a single-threaded module global in TS; rejecting it (the current `check_escape`) forces the user to restructure. Rust has a clean model: a const-expr literal → `pub const`; a runtime-immutable binding referenced by a function → `static OnceLock<T>` + accessor; a mutable binding → `thread_local! { static RefCell<T> }` + get/set accessors (matching TS's single-threaded global semantics, lock-free). Initialization order stays source-order via the existing implicit-`fn main` collector — only the storage class changes. ✅ no user rewrite; matches TS global semantics · ❌ a `thread_local!` global is per-thread state, isolated from Worker threads (which carry their own). _Today: a const-expr literal hoists to `pub const`; a runtime-immutable binding to `static OnceLock<T>` + accessor; a mutable binding to `thread_local! { static RefCell<T> }` + accessors (`RefCell<Option<T>>` for a delayed `T | undefined`). A module file initializes eagerly; an entry file seeds in source order from `fn main`._

**Class — `struct` + `impl`, access control collapsed to `pub` (vs rejecting `class`; vs a `Box<dyn>` trait-object model).**
A `class C` lowers to `#[derive(Clone)] struct C { pub fields } impl C { fn new, pub fn methods }`: instance fields become `pub` struct fields, a `constructor` becomes `fn new`, methods become `pub fn method(&self | &mut self)` (`this` → `self`; `&mut self` when the body mutates a `this` member), and `new C(...)` → `C::new(...)`. Access control is the one TS notion that does not survive: a `private`/`protected` modifier is visibility only (no runtime name mangling), and Rust struct fields / impl methods are already `pub`, so it lowers as a normal member; only a `#private` identifier has no Rust analogue and stays `unsupported`. A `WeakMap`/`WeakSet` lowers to the same strong `HashMap`/`HashSet` backing as `Map`/`Set` — no GC-precise weak reference (a `WeakMap` keyed by `Uint8Array` is a `HashMap<Vec<u8>, V>`), but the value semantics are correct; TS→native peers split here (some keep strong refs and accept the leak, some lack `WeakMap` entirely), so this is a deliberate trade-off, not a defect. An initializer-only field (`m = new Map<K, V>()`) infers its type from the initializer, and collection methods (`.set`/`.get`/`.has`) plus `++`/`--` dispatch on a `this.<field>` receiver the same way as on a local. ✅ the common TS class shape (fields + ctor + methods + collection fields) translates statically, no rewrite · ❌ no GC-precise `WeakMap`; `extends`/`super`, `set` accessors, a class-level generic with an `extends` bound (`<T extends X>` — the bound is structural subtyping, so the subtype's fields are unreachable on a Rust type parameter), and `static` members are still `unsupported`. _Today: `private`/`protected` → `pub`, `WeakMap`/`WeakSet` → strong collection backing, initializer-only field type inference, and collection methods / `++`/`--` on a `this.<field>` receiver are mapped; `extends`/`set`/`<T extends X>`/`static` stay `unsupported`. A `get` accessor maps to a zero-arg method (`obj.x` → `obj.x()`); a class-level generic `<T>` (no bound) maps to a generic struct + `impl<T: Clone>`._

**`package.json` — reuse the npm manifest, don't invent a new one (vs a dedicated `manifest.json`).**
`package.json` is the one manifest JS developers already know and every editor reads, so DashScript reuses it directly rather than inventing a parallel format. The standard npm fields map to their cargo idioms: metadata (`name`/`version`/`description`/`license`/`repository`/`homepage`/`keywords`/`author` → `[package]`); `bin` (string or object → cargo `[[bin]]`); `main` → cargo `[lib]`; `scripts` (npm-only, no cargo analogue); `workspaces` member globs (plural, mirroring npm/yarn/bun; pnpm alone uses a separate `pnpm-workspace.yaml`). Cargo-only concerns that have no npm analogue live under a `dashscript` namespace: `dashscript.cargo.dependencies`/`dashscript.cargo.devDependencies` (Rust crate deps → `[dependencies]`/`[dev-dependencies]`) and `dashscript.target` (output shape: `bin` default / `rust` / `wasm` / `napi`). npm `dependencies`/`devDependencies` (no `cargo:` prefix) stay JS deps and resolve to `node_modules` — they never reach Cargo.toml. ✅ one manifest every tool already understands, zero new format · ❌ the `dashscript` namespace is DashScript-specific (but tiny: two keys).

**`dashscript.cargo` namespace + `cargo:` import prefix (vs a flat dependency list).**
Cargo crate deps are not npm deps — they have no `node_modules` entry, no semver range npm understands, and live in `~/.cargo`. So they sit in their own `dashscript.cargo.dependencies` map (bare crate names, Cargo.toml-style values), kept visually and semantically distinct from the npm `dependencies` that resolve to `node_modules`. In `.ts` source, importing one mirrors the crate name verbatim — `import { Adler32 } from "adler"` — and on the CLI `ds add cargo:<crate>` (the `cargo:` prefix is optional; `ds add <crate>` works too) records it. The `cargo:` prefix is the import-family marker in the Deno style (`npm:`/`jsr:`/`cargo:`), so a future genuinely-different backend slots in without renaming; `wasm`/`napi` are Rust target variants and reuse `cargo:` deps, not a new prefix. ✅ npm deps and cargo deps never confused, import prefix mirrors Deno's family convention · ❌ `cargo:` to type on `ds add` (optional — bare name works too).

**`ds build` ships a native binary by default (vs a Rust project).**
Like `vp pack` ships a runnable artifact in `dist/`, `ds build` translates → compiles (`cargo build --release`) → copies the binary to `dist/<name>`, so `dist/` holds a usable product, not an intermediate project. `--target rust` keeps the transpiler's first-class Rust output (`dist/<name>/`, a clean crate with no `target/`) for inspection or as the `wasm`/`napi` target starting point. ✅ `dist/` is a product; transpiler output still one `--target rust` away · ❌ a release compile is slower than emit-only — use `--target rust` when you only want the Rust.

**Cached build, Deno-style lookup (vs a fresh temp dir per run).**
`ds build`/`ds run` resolve the cache by walking up from the `.ts` file for a `package.json`: found → in-project `.cache/dash/<name>/` (dependencies live with the package); not found (a lone file) → global `~/.cache/dash/<hash>/`. cargo's own `target/` lives there, so repeat builds are incremental. This mirrors Deno (project → local `node_modules`, lone file → global cache) and reuses cargo's two-layer dependency model (`~/.cargo/registry` source + project `target/`) rather than adding a DashScript-owned store. ✅ fast repeats, deps follow the package, lone files still work · ❌ `.cache/` must be gitignored; first build still compiles std.

**`ds add` — two modes, crate vs local file (vs one-size-fits-all).**
`ds add cargo:<crate>` (the `cargo:` prefix is optional — `ds add <crate>` works too) fetches the crate via cargo (like `pnpm add`) and records the bare crate name under `dashscript.cargo.dependencies` in `package.json` — but generates **no `.d.ts` stub**: Rust is statically typed, so the crate's own source in `~/.cargo` is the complete type truth, read directly by the language server (exactly how rust-analyzer reads its deps — no parallel stub set to keep in sync). `ds add <file>.rs` runs **bindgen** on a local Rust source file, emitting `<stem>.d.ts` beside it for editor completion (the `@types`/DefinitelyTyped analogue). Bindgen therefore maps a file's public surface — `struct`/`enum`/`fn`/`trait`/`impl`. ✅ crates are zero-stub; local files get real declarations · ❌ bindgen coverage grows with the constructs local files expose.

**One `dashscript` package (vs a separate `@dashscript/cli`).**
The CLI is the product; splitting it into a sub-package adds an install step with no benefit. One package, one binary name (`ds`). ✅ simplest install (`pnpm add dashscript`) · ❌ coarser release granularity.

**DashScript-managed Rust toolchain (vs depend on a system `rustup`).**
DashScript pins a specific Rust version and downloads its standalone build on demand — like an npm dependency — into its own cache, so end users never install Rust separately. ✅ zero-setup install, reproducible builds · ❌ large first-run download and toolchain-management code. (Contributors building DashScript itself still need a system Rust toolchain.)

**One core crate, modular (vs many crates).**
The three responsibilities are small and share the translation table; a single `dashscript` crate with `translator` / `package` / `bindgen` modules is enough until a module needs independent versioning. ✅ low overhead · ❌ coarser release granularity.

**Workspace via package globs (vs a separate workspace file).**
A root `package.json` with a `workspaces` glob list (`["apps/*", "packages/*"]`) declares members — the same file already carries project metadata, so there is no separate `pnpm-workspace.yaml`. The plural `workspaces` mirrors npm/yarn/bun's `package.json` field (pnpm alone uses a separate file). `ds build` at the root emits **one cargo workspace** — members at `.cache/dash/<name>/` under a root `[workspace]` `Cargo.toml` — and compiles it once: members share `target/` and `Cargo.lock`, so a dependency two members use compiles once (cargo's hoisted-`node_modules`); `--filter <name>` picks one. Metadata and deps are inherited the cargo-native way: the root carries `[workspace.package]` and `[workspace.dependencies]` (the union of member deps), and each member's `[package]`/`[dependencies]` use `field.workspace = true` — so `version`/`edition`/`license` and shared deps are declared once at the root. ✅ one package format, monorepo from day one, shared compilation · ❌ no inter-member path dependencies or task caching (turbo/nx) yet — those land as real demand drives them.

**test262 as the conformance oracle, assert-driven (vs hand-written expectations; vs a Node stdout diff).**
The harness runs each tc39 test262 fixture — the official ECMAScript suite Node/Bun/Deno/V8 all run — and the verdict is **assert-driven**: the fixture's own `assert.sameValue(a, b)` / `assert.throws(…)` carry the expected values (the ECMAScript reference), so the result is whether every assert held — never a stdout diff against a Node oracle (Node ≠ spec; the `expected` arg _is_ the spec truth). Both the static path (`Translator::check` → `cargo build` → run) and the engine path (in-process QuickJS with the test262 harness injected) run the fixture verbatim, wrapped in `function main(): void { … }`: exit 0 = `supported`, a thrown `Test262Error` = `partial`, a build failure / timeout / engine `ReferenceError` (QuickJS lacks a built-in like Temporal or the `$262` agent API) = `unsupported`. The engine path also covers reflection without re-implementing it in Rust: a fixture using `verifyProperty` / property descriptors / prototype chains runs under QuickJS with `sta.js` + `assert.js` + the fixture's `$INCLUDE`s injected, so reflection runs with reference semantics — "degrade, don't reject" applied to conformance. The extractor (`scripts/extract-test262.mjs`) carries every `test/built-ins/<cat>/` fixture + its `includes:` list (no whitelist); constructs the static translator cannot lower degrade per-function to the engine rather than being filtered out. ✅ no hand-written oracle, the spec's own expected values, reflection covered via the engine · ❌ the TS→Rust subset means much of test262 is `partial` / `unsupported` today — that backlog is the output, surfaced honestly. (`bcd`/`runtime-compat` were dropped: they test API _existence_, not _semantics_; a Node oracle was dropped: render-parity with Node ≠ spec-conformance.)

## Roadmap

- **Initial scope** — `translator` (a core subset of oxc AST → Rust), `package` (`package.json` → `Cargo.toml`), a DashScript-managed Rust toolchain (pinned, downloaded on demand), `ds build` (native binary) / `ds run` / `ds check` / `ds fmt`, `bindgen` + `ds add`. One `.ts` file compiles to a native binary (or a Rust crate with `--target rust`), checked by `cargo`.
- **Compatibility — degrade, don't reject** — a construct the static translator cannot lower runs under an embedded QuickJS engine at the **function** granularity (inheriting full ECMAScript semantics) instead of failing, so existing TS/JS keeps working. _Today: per-function and transitive whole-module (npm `.js`) degradation are in via embedded QuickJS; the conformance harness runs engine-path fixtures in-process under the same QuickJS._
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
