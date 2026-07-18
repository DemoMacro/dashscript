# DashScript

Language support for [DashScript](https://github.com/DemoMacro/dashscript) (`.ds`) — TypeScript ergonomics, Rust performance, compiled to native.

## Features

- **Syntax highlighting** — reuses the built-in TypeScript grammar.
- **Diagnostics** — real-time `ds check` feedback as you type.
- **Completions** — `.ds` built-ins (`console`, `Math`, `Number`, …) and their members (`console.log`, `Math.PI`), plus your own declarations. Type `.` to trigger member completion.
- **Go-to-definition** — `.ds` symbols and imported crate names, resolved through a `rust-analyzer` backend.
- **Document symbols** — the outline view lists every function, interface, type alias, and import.
- **Hover** — see a user function's signature (`function greet(name: string): string`), a builtin's type and doc (`Math.round`, `parseInt`), or a crate symbol (via rust-analyzer).
- **Signature help** — parameter hints while typing inside a call (`Math.round(`), with the active parameter highlighted; triggered on `(` and `,`.
- **Find references** — every read/write of a symbol, resolved at the symbol level (two same-named bindings in different scopes are never confused).
- **Rename** — F2 renames a symbol across its declaration and references in the file; symbol-level, so a parameter never renames a same-named top-level variable.

## Requirements

- `ds` on your PATH — build it with `cargo install --path crates/dashscript`.
- `rust-analyzer` on your PATH (for crate go-to-definition).
