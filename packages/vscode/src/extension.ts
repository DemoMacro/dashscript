//! DashScript VS Code extension entry.
//!
//! Connects the shared `ds lsp` core (crate go-to-definition via rust-analyzer
//! + translatability diagnostics) to `.ts` files over stdio. The
//! `@dashscript/typescript-plugin` (local `.rs` bindgen + `.d.ts` → `.rs`
//! jump + `cargo:` TS2307 suppression) is loaded by tsserver via the project's
//! own tsconfig `compilerOptions.plugins` — never bundled in this vsix, so it
//! resolves from the workspace node_modules where the project installed it.
//! `.ts` uses VS Code's native TypeScript language — no custom grammar.

import { ExtensionContext, workspace } from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
} from "vscode-languageclient/node";

let client: LanguageClient | undefined;

export function activate(context: ExtensionContext): void {
  const config = workspace.getConfiguration("dashscript");
  const dsPath = config.get<string>("dsPath") ?? "ds";
  const rustAnalyzerPath = config.get<string>("rustAnalyzerPath") ?? "rust-analyzer";
  const serverOptions: ServerOptions = {
    run: { command: dsPath, args: ["lsp"], transport: TransportKind.stdio },
    debug: { command: dsPath, args: ["lsp"], transport: TransportKind.stdio },
  };
  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: "file", language: "typescript" }],
    // Forwarded to `ds lsp` so it can spawn the rust-analyzer backend.
    initializationOptions: { rustAnalyzerPath },
  };
  client = new LanguageClient(
    "dashscriptLsp",
    "DashScript Language Server",
    serverOptions,
    clientOptions,
  );
  // `LanguageClient` carries its own `dispose()` (stops the server), so it
  // satisfies VS Code's `Disposable` shape. `start()` returns `Promise<void>`.
  context.subscriptions.push(client);
  void client.start();
}

export function deactivate(): Promise<void> | undefined {
  return client?.stop();
}
