# dashscript

![npm version](https://img.shields.io/npm/v/dashscript)
![npm downloads](https://img.shields.io/npm/dw/dashscript)
![npm license](https://img.shields.io/npm/l/dashscript)

> A TypeScript-frontend language (`.ds`) that transpiles to idiomatic Rust — one package providing the `ds` CLI, the transpiler core, and editor types.

## Features

- 🦀 **TypeScript → Rust → native binary** — write TypeScript-flavored `.ds`, compile to a native binary (or a Rust crate with `--emit rust`)
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
$ ds build main.ds              # → dist/<name> — a native binary (default)
$ ds build main.ds --emit rust  # → dist/<name>/ — the translated Rust crate
$ ds run main.ds                # translate → compile (cached) → run
```

`ds build` parses with oxc, translates the AST to idiomatic Rust, and compiles a **native binary** into `dist/<name>` (the way `vp pack` ships a runnable artifact). `--emit rust` stops at the Rust crate; `ds run` compiles and runs in one step, reusing the in-project cache (`.cache/build/<name>/`, or `~/.cache/dash/` for a lone file).

### Declare dependencies — `manifest.json` → `Cargo.toml`

```json
{
  "name": "my-app",
  "target": "rust",
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
$ ds check   # verify .ds is translatable to valid Rust (in-process)
$ ds fmt     # format .ds in place (in-process)
```

## CLI

| Command                       | Description                                                                  |
| ----------------------------- | ---------------------------------------------------------------------------- |
| `ds build <file.ds>`          | Parse → translate → compile to a native binary in `dist/<name>`              |
| `ds build --emit rust <file>` | Parse → translate → emit a Rust crate (`Cargo.toml` + src) in `dist/<name>/` |
| `ds run <file.ds>`            | Translate → compile (cached) → run                                           |
| `ds check`                    | Verify `.ds` is translatable to valid Rust (in-process, on the oxc AST)      |
| `ds fmt`                      | Format `.ds` in place (in-process)                                           |
| `ds add rust:<crate>`         | Fetch crate via cargo + record `rust:<crate>` in `manifest.json`             |
| `ds add <file>.rs`            | Bindgen a local Rust file → `<stem>.ds` declaration                          |

## Under the Hood

`dashscript` is a TS → Rust transpiler. It reuses oxc for the TypeScript-flavored front end and owns only the AST → Rust mapping table, the `manifest.json` → `Cargo.toml` translation, and Rust-crate → `.ds` bindgen. Correctness of generated Rust is delegated to `cargo check` / `cargo clippy`.

## License

MIT © [Demo Macro](https://www.demomacro.com/)
