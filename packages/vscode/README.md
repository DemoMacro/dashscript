# DashScript

Language support for [DashScript](https://github.com/DemoMacro/dashscript) (`.ds`) — a TypeScript-flavored syntax that transpiles to idiomatic Rust.

## Features

- **Syntax highlighting** — reuses the built-in TypeScript grammar.
- **Diagnostics** — real-time `ds check` feedback as you type.
- **Go-to-definition** — `.ds` symbols and imported crate names, resolved through a `rust-analyzer` backend.

## Requirements

- `ds` on your PATH — build it with `cargo install --path apps/ds`.
- `rust-analyzer` on your PATH (for crate go-to-definition).
