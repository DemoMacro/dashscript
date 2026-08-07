# DashScript

Language support for [DashScript](https://github.com/DemoMacro/dashscript) (`.ts`/`.js`) — JavaScript/TypeScript ergonomics, Rust performance, native + wasm + napi outputs.

This extension is a thin bridge over VS Code's built-in TypeScript server. It adds only what the native TS server cannot provide: crate-level go-to-definition and translatability diagnostics from the shared `ds lsp`, plus the `@dashscript/typescript-plugin` for `cargo:` import handling. Syntax highlighting, completions, hover, signature help, document symbols, find references, and rename all come from VS Code's native TypeScript — nothing is duplicated.

## Features

- **Crate go-to-definition** — `Cmd+click` a name from `cargo:<crate>` to jump into the crate's source under `~/.cargo`, resolved at the symbol level through a `rust-analyzer` backend (served by `ds lsp`).
- **Translatability diagnostics** — live `ds check` feedback marking constructs that cannot lower to valid Rust (served by `ds lsp`).
- **`cargo:` import handling** — the `@dashscript/typescript-plugin` suppresses the TS2307 that `cargo:` imports would otherwise raise (their types live in `~/.cargo`, not a `.d.ts`), and auto-generates a `.d.ts` beside any local `.rs` you `import "./x"`.

## Requirements

- `ds` on your PATH — build it with `cargo install --path crates/dashscript`.
- `rust-analyzer` on your PATH (for crate go-to-definition).

## TypeScript plugin setup

The `@dashscript/typescript-plugin` is **not** bundled in this vsix — it loads from your project's own `node_modules` through `tsconfig.json`, so add it as a dev dependency and reference it there:

```jsonc
// tsconfig.json — load the plugin
{
  "compilerOptions": {
    "plugins": [{ "name": "@dashscript/typescript-plugin" }],
  },
}
```

VS Code's bundled TypeScript cannot resolve a plugin that lives in the workspace `node_modules` (see [microsoft/vscode#232406](https://github.com/microsoft/vscode/issues/232406)), so point VS Code at the workspace TypeScript once via `.vscode/settings.json`:

```json
{
  "js/ts.tsdk.path": "node_modules/typescript/lib",
  "js/ts.tsdk.promptToUseWorkspaceVersion": true
}
```

Accept the "Use Workspace Version" prompt once and the plugin loads — after that, `cargo:` imports lose their red squiggle and gain go-to-definition.
