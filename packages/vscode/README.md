# DashScript

Language support for [DashScript](https://github.com/DemoMacro/dashscript) (`.ds`) — TypeScript ergonomics, Rust performance, compiled to native.

## Features

- **Syntax highlighting** — reuses the built-in TypeScript grammar.
- **Diagnostics** — real-time `ds check` feedback as you type.
- **Completions** — `.ds` built-ins (`console`, `Math`, `Number`, …) and their members (`console.log`, `Math.PI`), plus your own declarations. Type `.` to trigger member completion.
- **Go-to-definition** — `.ds` symbols and imported crate names, resolved through a `rust-analyzer` backend.

## Requirements

- `ds` on your PATH — build it with `cargo install --path crates/dashscript`.
- `rust-analyzer` on your PATH (for crate go-to-definition).
