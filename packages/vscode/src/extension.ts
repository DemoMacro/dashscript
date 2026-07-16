//! DashScript VS Code extension entry.
//!
//! Wires the `.ds` language (declared statically in `package.json`) to the
//! `ds lsp` language server over stdio. Stage 3 extends the server with crate
//! go-to-definition via a rust-analyzer backend.

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
    documentSelector: [{ scheme: "file", language: "dashscript" }],
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
