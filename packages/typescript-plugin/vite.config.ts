import { defineConfig } from "vite-plus";

export default defineConfig({
  pack: {
    entry: ["src/index.ts"],
    // tsserver loads language-service plugins via `require()`, which can't
    // load an ESM module on VS Code's bundled Node (< 22). Emit CommonJS so
    // the plugin actually loads and getSemanticDiagnostics runs.
    format: "cjs",
    copy: [
      {
        // The single-source stdlib declaration lives in the translator crate
        // (its drift-guard test asserts every declared symbol translates to
        // Rust). Copy it into the package output as `global.d.ts` so a
        // DashScript TS project can `include` it for the built-in globals
        // (`console`, `Math`, `parseInt`, …) — the `lib.d.ts` analogue.
        from: "../../crates/dashscript/src/translator/builtins/dashscript.d.ts",
        rename: "global.d.ts",
      },
    ],
  },
});
