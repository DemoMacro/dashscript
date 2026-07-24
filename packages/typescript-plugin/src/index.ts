//! @dashscript/typescript-plugin — a TypeScript language service plugin.
//!
//! Two editor features the shared `ds lsp` core cannot provide from inside the
//! TS server:
//! - local Rust bindgen: a relative `import "./x"` whose sibling `x.rs` exists
//!   gets a fresh `x.d.ts` generated beside it (`ds add <x>.rs`), zero-friction.
//! - local `.d.ts` → `.rs` go-to-definition: jumping to a bindgen declaration
//!   redirects to the real Rust source, where rust-analyzer takes over.
//!
//! Translatability diagnostics, crate go-to-definition, and crate hover all
//! come from the shared `ds lsp` core (connected via the editor's LSP client),
//! not this plugin — so every editor (VS Code / Zed / JetBrains) gets them
//! uniformly. This plugin only suppresses the TS2307 that TS would otherwise
//! raise on `cargo:` imports (types live in `~/.cargo`, not a `.d.ts`).
//!
//! Enable in `tsconfig.json`:
//!   "compilerOptions": { "plugins": [{ "name": "@dashscript/typescript-plugin" }] }
//!
//! `ds` is resolved from the configured `dsPath` (default `"ds"` on PATH), so
//! `cargo install dashscript` (or `pnpm add -g dashscript`) is the only setup.
//!
//! Standard-library globals (`console`, `Math`, `parseInt`, …) are NOT injected
//! at runtime: the package ships `dist/global.d.ts` (copied from the translator
//! crate's single-source declaration at build time via `pack.copy`), which a
//! DashScript project `include`s in its tsconfig for ambient types — the
//! `lib.d.ts` analogue.

import { spawnSync } from "node:child_process";
import { existsSync, statSync } from "node:fs";
import { dirname, resolve } from "node:path";

import type * as ts from "typescript/lib/tsserverlibrary";

/** The plugin factory, in the shape TypeScript loads via tsconfig `plugins`. */
function init(modules: { typescript: typeof ts }) {
  const ts = modules.typescript;

  function create(info: ts.server.PluginCreateInfo): ts.LanguageService {
    const logger = info.project.projectService.logger;
    logger.info("[dashscript] plugin create");

    const dsPath = (info.config?.dsPath as string) ?? "ds";

    /** Relative `import "./x"` whose sibling `<x>.rs` exists — the local Rust
     * modules a DashScript project binds via bindgen. Resolved against the
     * importing file's directory; a `.ts`/`.tsx` extension on the specifier is
     * stripped so `./geometry` and `./geometry.ts` both map to `geometry.rs`. */
    function* localRsImports(fileName: string): Generator<{ rs: string; dts: string }> {
      const dir = dirname(fileName);
      const src = info.languageService.getProgram()?.getSourceFile(fileName);
      if (!src) return;
      for (const stmt of src.statements) {
        if (!ts.isImportDeclaration(stmt)) continue;
        const spec = stmt.moduleSpecifier;
        if (!ts.isStringLiteral(spec)) continue;
        const mod = spec.text;
        if (!mod.startsWith(".")) continue;
        const stem = mod.replace(/\.[tj]sx?$/, "");
        const rs = resolve(dir, `${stem}.rs`);
        if (!existsSync(rs)) continue;
        yield { rs, dts: resolve(dir, `${stem}.d.ts`) };
      }
    }

    /** Ensure each local `.rs` import has a fresh `.d.ts` beside it: if the
     * declaration is missing or older than the `.rs`, run `ds add <x>.rs`
     * (bindgen) to (re)generate it. Zero-friction path — the developer writes
     * `geometry.rs` + `import "./geometry"` and never runs `ds add` by hand.
     * Best-effort: a bindgen failure is logged, not raised. */
    function ensureLocalRsTypes(fileName: string) {
      for (const { rs, dts } of localRsImports(fileName)) {
        const stale = !existsSync(dts) || statSync(rs).mtimeMs > statSync(dts).mtimeMs;
        if (!stale) continue;
        const res = spawnSync(dsPath, ["add", rs], { encoding: "utf8", windowsHide: true });
        if (res.status === 0) {
          logger.info(`[dashscript] bindgen ${rs}`);
        } else {
          logger.info(`[dashscript] bindgen failed (${res.status}): ${res.stderr ?? ""}`);
        }
      }
    }

    // Pass-through proxy over the real language service.
    const proxy: ts.LanguageService = Object.create(null);
    for (const key of Object.keys(info.languageService) as Array<keyof ts.LanguageService>) {
      const original = info.languageService[key] as unknown as (...args: unknown[]) => unknown;
      // @ts-expect-error — proxy assignment across the whole LanguageService surface
      proxy[key] = (...args: unknown[]) => original.apply(info.languageService, args);
    }

    // Refresh local `.rs` bindgen declarations at diagnostic time (a frequent
    // hook) so the `.d.ts` is fresh when go-to-definition needs it. The
    // translatability diagnostics themselves come from `ds lsp` (shared core),
    // not this plugin.
    //
    // Suppress TS2307 ("Cannot find module") for `cargo:` imports: their
    // types come from rust-analyzer via `ds lsp` hover (zero-stub — no `.d.ts`),
    // so TS cannot resolve the `cargo:` module specifier. Every other
    // diagnostic passes through unchanged.
    proxy.getSemanticDiagnostics = (fileName: string) => {
      const prior = info.languageService.getSemanticDiagnostics(fileName);
      if (!fileName.endsWith(".ts") || fileName.endsWith(".d.ts")) return prior;
      ensureLocalRsTypes(fileName);
      return prior.filter((d) => {
        if (d.code !== 2307) return true;
        const msg = typeof d.messageText === "string" ? d.messageText : "";
        return !msg.includes("cargo:");
      });
    };

    /** Go-to-definition: when the target is a local `<x>.d.ts` with a sibling
     * `<x>.rs`, redirect to the `.rs` source — the developer reads the real
     * Rust, and rust-analyzer then handles symbol-level navigation inside it.
     * Best-effort position is the file head (the exact column is found by
     * rust-analyzer once the file is open). Crate go-to-definition comes from
     * `ds lsp` (shared core), not this plugin. */
    proxy.getDefinitionAtPosition = (fileName: string, position: number) => {
      const prior = info.languageService.getDefinitionAtPosition(fileName, position);
      if (!prior) return prior;
      return prior.map((d) => {
        if (!d.fileName.endsWith(".d.ts")) return d;
        const rs = d.fileName.replace(/\.d\.ts$/, ".rs");
        if (!existsSync(rs)) return d;
        return { ...d, fileName: rs, textSpan: { start: 0, length: 0 } };
      });
    };

    return proxy;
  }

  return { create };
}

export default init;
