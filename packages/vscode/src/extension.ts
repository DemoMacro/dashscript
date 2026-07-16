//! DashScript VS Code extension entry.
//!
//! Stage 1 ships syntax highlighting only — declared statically in
//! `package.json` (language id + TextMate grammar + language configuration),
//! so no activation logic is needed yet. Stage 2 wires `activate` /
//! `deactivate` to a `vscode-languageclient` that spawns the `ds lsp` server.

export function activate(): void {
  // Stage 2: start the `ds` language server client.
}

export function deactivate(): void {}
