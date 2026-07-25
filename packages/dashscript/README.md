# dashscript

![npm version](https://img.shields.io/npm/v/dashscript)
![npm downloads](https://img.shields.io/npm/dw/dashscript)
![npm license](https://img.shields.io/npm/l/dashscript)

> **TypeScript ergonomics, Rust performance, compiled to native.** A typed, TypeScript-flavored language (`.ts`) that compiles to native binaries via idiomatic Rust — one package providing the `ds` CLI, the translation core, and editor types.

## Features

- 🦀 **TypeScript → Rust → native binary** — write TypeScript-flavored `.ts`, compile to a native binary (or a Rust crate with `--target rust`)
- ⚡ **Powered by oxc** — reuses [oxc](https://oxc.rs/) for parsing, lint, and format; no reimplementation
- 📦 **One package** — `dashscript` provides the `ds` CLI, the core, and types
- 🗂️ **`package.json` → `Cargo.toml`** — Rust crate deps under `dashscript.cargo` compile straight to Cargo
- 🔌 **Auto type hints** — types for any Rust crate come straight from its source (zero stubs); `ds add <file>.rs` bindgens a local Rust file to a `.d.ts` beside it
- 🛠️ **Bundled toolchain** — DashScript manages its own pinned Rust toolchain; no separate `rustup` install

## Installation

```bash
# Install with npm
$ npm install -g dashscript

# Install with yarn
$ yarn global add dashscript

# Install with pnpm
$ pnpm add -g dashscript
```

## Quick Start

### Write `.ts`, compile to a native binary

```typescript
// main.ts — TypeScript-flavored source
function greet(name: string): string {
  return `Hello, ${name}!`;
}

const message: string = greet("DashScript");
```

```bash
$ ds main.ts                      # run a file directly (like `node a.js`)
$ ds build main.ts                # → dist/<name> — a native binary (default)
$ ds build main.ts --target rust  # → dist/<name>/ — the translated Rust crate
$ ds run <script>                 # run a package.json script (like `pnpm run`)
```

`ds main.ts` runs a file directly (translate → compile cached → run). `ds build` parses with oxc, translates the AST to idiomatic Rust, and compiles a **native binary** into `dist/<name>` (the way `vp pack` ships a runnable artifact); `--target rust` stops at the Rust crate. Both reuse the in-project cache (`.cache/dash/<name>/`, or `~/.cache/dash/` for a lone file). `ds run <script>` runs a shell command from `package.json` `scripts` (like `pnpm run`).

### Declare dependencies — `package.json` → `Cargo.toml`

`package.json` is the one manifest every JS tool already reads. Standard npm fields map straight to cargo: `bin` declares a project's executables (package.json `bin` → cargo `[[bin]]`), so one project compiles to several binaries; `main` → `[lib]`; Rust crate deps under `dashscript.cargo.dependencies` → `[dependencies]` (npm `dependencies` stay JS deps, never reaching Cargo.toml). On `ds build`, the package is translated into a `Cargo.toml`:

```json
{
  "name": "my-app",
  "bin": {
    "serve": "serve.ts",
    "migrate": "migrate.ts"
  },
  "dashscript": {
    "target": "bin",
    "cargo": {
      "dependencies": {
        "serde": "1.0",
        "tokio": "1.0"
      }
    }
  }
}
```

### Use a Rust crate with type hints

```bash
$ ds add cargo:serde
```

`ds add cargo:<crate>` fetches the crate via cargo and records it in `package.json` — **no `.d.ts` stub is generated**. Rust is statically typed, so the crate's own source (in `~/.cargo`) is the complete type truth, read directly by the editor the way rust-analyzer reads its deps. For a local Rust file, `ds add <file>.rs` runs bindgen to emit a `.d.ts` declaration beside it.

### Check & format (powered by oxc)

```bash
$ ds lint <file>   # translatability check (in-process)
$ ds check <file>  # lint + format check, like `vp check` (in-process)
$ ds fmt <file>    # format .ts in place (in-process)
```

## CLI

| Command                                   | Description                                                                                                                                      |
| ----------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------ |
| `ds <file.ts>`                            | Run a file directly — translate → compile (cached) → run (like `node a.js`)                                                                      |
| `ds run <script>`                         | Run a `package.json` script (like `pnpm run`)                                                                                                    |
| `ds build [<file>] [--target] [--filter]` | Compile a native binary in `dist/<name>` (at a workspace root, builds all members; `--filter <name>` picks one; `--target rust` emits the crate) |
| `ds lint <file>`                          | Translatability check (in-process, on the oxc AST)                                                                                               |
| `ds check <file>`                         | Lint + format check, like `vp check` (in-process)                                                                                                |
| `ds fmt <file>`                           | Format `.ts` in place (in-process)                                                                                                               |
| `ds install`                              | Fetch manifest deps via cargo + write `Cargo.lock` (like `pnpm install`)                                                                         |
| `ds add cargo:<crate>`                    | Fetch crate via cargo + record under `dashscript.cargo.dependencies` (prefix optional)                                                           |
| `ds add <file>.rs`                        | Bindgen a local Rust file → `<stem>.d.ts` declaration                                                                                            |
| `ds cache clean`                          | Remove the in-project `.cache/`                                                                                                                  |

## Under the Hood

`dashscript` is a TS → Rust transpiler. It reuses oxc for the TypeScript-flavored front end and owns only the AST → Rust mapping table, the `package.json` → `Cargo.toml` translation, and Rust-crate → `.d.ts` bindgen. Correctness of generated Rust is delegated to `cargo check` / `cargo clippy`.

## License

MIT © [Demo Macro](https://www.demomacro.com/)
