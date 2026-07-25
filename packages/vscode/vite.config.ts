import { defineConfig } from "vite-plus";

export default defineConfig({
  pack: {
    entry: ["src/extension.ts"],
    // `vscode` is injected by the extension host at runtime (it is never in
    // node_modules). neverBundle it so rolldown doesn't warn about every
    // `require("vscode")` reaching through vscode-languageclient.
    deps: { neverBundle: ["vscode"] },
  },
});
