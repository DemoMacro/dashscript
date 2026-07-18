# dashscript

![npm version](https://img.shields.io/npm/v/dashscript)
![npm downloads](https://img.shields.io/npm/dw/dashscript)
![npm license](https://img.shields.io/npm/l/dashscript)

> **TypeScript ergonomics, Rust performance, compiled to native.** A typed, TypeScript-flavored language (`.ds`) that compiles to native binaries via idiomatic Rust — one package providing the `ds` CLI, the translation core, and editor types.

## Features

- 🦀 **TypeScript → Rust → native binary** — write TypeScript-flavored `.ds`, compile to a native binary (or a Rust crate with `--target rust`)
- ⚡ **Powered by oxc** — reuses [oxc](https://oxc.rs/) for parsing, lint, and format; no reimplementation
- 📦 **One package** — `dashscript` provides the `ds` CLI, the core, and types
- 🗂️ **`manifest.json` → `Cargo.toml`** — target-prefixed dependencies (`rust:serde`) compile straight to Cargo
- 🔌 **Auto type hints** — `ds add rust:<crate>` generates `.ds` declarations for any Rust crate
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

### Write `.ds`, compile to a native binary

```typescript
// main.ds — TypeScript-flavored source
function greet(name: string): string {
  return `Hello, ${name}!`;
}

const message: string = greet("DashScript");
```

```bash
$ ds main.ds                      # run a file directly (like `node a.js`)
$ ds build main.ds                # → dist/<name> — a native binary (default)
$ ds build main.ds --target rust  # → dist/<name>/ — the translated Rust crate
$ ds run <script>                 # run a manifest.json script (like `pnpm run`)
```

`ds main.ds` runs a file directly (translate → compile cached → run). `ds build` parses with oxc, translates the AST to idiomatic Rust, and compiles a **native binary** into `dist/<name>` (the way `vp pack` ships a runnable artifact); `--target rust` stops at the Rust crate. Both reuse the in-project cache (`.cache/dash/<name>/`, or `~/.cache/dash/` for a lone file). `ds run <script>` runs a shell command from `manifest.json` `scripts` (like `pnpm run`).

### Declare dependencies — `manifest.json` → `Cargo.toml`

`manifest.json` is the **package.json ∩ Cargo.toml intersection** — `bin` declares a project's executables (package.json `bin` → cargo `[[bin]]`), so one project compiles to several binaries; `lib`/`devDependencies` map to `[lib]`/`[dev-dependencies]`. Dependencies carry a `rust:` target prefix. On `ds build`, the manifest is translated into a `Cargo.toml`:

```json
{
  "name": "my-app",
  "target": "bin",
  "bin": {
    "serve": "serve.ds",
    "migrate": "migrate.ds"
  },
  "dependencies": {
    "rust:serde": "1.0",
    "rust:tokio": "1.0"
  }
}
```

### Use a Rust crate with type hints

```bash
$ ds add rust:serde
```

`ds add` runs **bindgen** — it reads a Rust crate and emits a `.ds` declaration, so importing the crate gives you editor completion and type checking.

### Check & format (powered by oxc)

```bash
$ ds lint <file>   # translatability check (in-process)
$ ds check <file>  # lint + format check, like `vp check` (in-process)
$ ds fmt <file>    # format .ds in place (in-process)
```

## CLI

| Command                                   | Description                                                                                                                                      |
| ----------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------ |
| `ds <file.ds>`                            | Run a file directly — translate → compile (cached) → run (like `node a.js`)                                                                      |
| `ds run <script>`                         | Run a `manifest.json` script (like `pnpm run`)                                                                                                   |
| `ds build [<file>] [--target] [--filter]` | Compile a native binary in `dist/<name>` (at a workspace root, builds all members; `--filter <name>` picks one; `--target rust` emits the crate) |
| `ds lint <file>`                          | Translatability check (in-process, on the oxc AST)                                                                                               |
| `ds check <file>`                         | Lint + format check, like `vp check` (in-process)                                                                                                |
| `ds fmt <file>`                           | Format `.ds` in place (in-process)                                                                                                               |
| `ds install`                              | Fetch manifest deps via cargo + write `Cargo.lock` (like `pnpm install`)                                                                         |
| `ds add rust:<crate>`                     | Fetch crate via cargo + record `rust:<crate>` in `manifest.json`                                                                                 |
| `ds add <file>.rs`                        | Bindgen a local Rust file → `<stem>.ds` declaration                                                                                              |
| `ds cache clean`                          | Remove the in-project `.cache/`                                                                                                                  |

## Under the Hood

`dashscript` is a TS → Rust transpiler. It reuses oxc for the TypeScript-flavored front end and owns only the AST → Rust mapping table, the `manifest.json` → `Cargo.toml` translation, and Rust-crate → `.ds` bindgen. Correctness of generated Rust is delegated to `cargo check` / `cargo clippy`.

## License

MIT © [Demo Macro](https://www.demomacro.com/)
