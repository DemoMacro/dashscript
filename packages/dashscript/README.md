# dashscript

![npm version](https://img.shields.io/npm/v/dashscript)
![npm downloads](https://img.shields.io/npm/dw/dashscript)
![npm license](https://img.shields.io/npm/l/dashscript)

> A TypeScript-frontend language (`.ds`) that transpiles to idiomatic Rust — one package providing the `ds` CLI, the transpiler core, and editor types.

## Features

- 🦀 **TypeScript → Rust** — write TypeScript-flavored `.ds`, ship native Rust
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

### Write `.ds`, compile to Rust

```typescript
// main.ds — TypeScript-flavored source
function greet(name: string): string {
  return `Hello, ${name}!`;
}

const message: string = greet("DashScript");
```

```bash
$ ds build main.ds
```

DashScript parses with oxc, translates the AST to idiomatic Rust, and emits a buildable Cargo project.

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
$ ds check   # lint & type-check .ds (oxc — oxlint)
$ ds fmt     # format .ds (oxc — oxfmt)
```

## CLI

| Command               | Description                                                   |
| --------------------- | ------------------------------------------------------------- |
| `ds build <file.ds>`  | Parse with oxc → translate → emit Rust project + `Cargo.toml` |
| `ds check`            | Lint & type-check `.ds` (oxlint)                              |
| `ds fmt`              | Format `.ds` (oxfmt)                                          |
| `ds add rust:<crate>` | Fetch crate + generate `.ds` declaration (bindgen)            |
| `ds run <file.ds>`    | Build then run the generated Rust (planned)                   |

## Under the Hood

`dashscript` is a TS → Rust transpiler. It reuses oxc for the TypeScript-flavored front end and owns only the AST → Rust mapping table, the `manifest.json` → `Cargo.toml` translation, and Rust-crate → `.ds` bindgen. Correctness of generated Rust is delegated to `cargo check` / `cargo clippy`.

## License

MIT © [Demo Macro](https://www.demomacro.com/)
