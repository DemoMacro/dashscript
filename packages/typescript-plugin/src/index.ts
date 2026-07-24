//! @dashscript/typescript-plugin — a TypeScript language service plugin.
//!
//! Surfaces DashScript translatability diagnostics inline in the editor by
//! decorating `getSemanticDiagnostics`: each `.ts` file is run through
//! `ds lint --json` (cached per file version, so an unchanged file is not
//! re-linted) and the structured diagnostics are merged into the TypeScript
//! list with `source: "dashscript"`.
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

/** One entry from `ds lint --json` (1-based line/column, LSP convention). */
interface DsLintDiagnostic {
  file: string;
  line: number;
  column: number;
  endLine: number;
  endColumn: number;
  message: string;
  severity: "error" | "warning";
}

/** The plugin factory, in the shape TypeScript loads via tsconfig `plugins`. */
function init(modules: { typescript: typeof ts }) {
  const ts = modules.typescript;

  function create(info: ts.server.PluginCreateInfo): ts.LanguageService {
    const logger = info.project.projectService.logger;
    logger.info("[dashscript] plugin create");

    const dsPath = (info.config?.dsPath as string) ?? "ds";
    const host = info.languageServiceHost;

    /** Per-file diagnostic cache keyed by script version — avoids re-linting
     * an unchanged file on every `getSemanticDiagnostics` call. */
    const cache = new Map<string, { version: string; diags: ts.Diagnostic[] }>();

    /** Run `ds lint --json <fileName>` and convert its diagnostics to TS. */
    function runLint(fileName: string): ts.Diagnostic[] {
      let stdout = "";
      try {
        const res = spawnSync(dsPath, ["lint", "--json", fileName], {
          encoding: "utf8",
          windowsHide: true,
        });
        if (res.error) {
          logger.info(`[dashscript] spawn failed: ${res.error.message}`);
          return [];
        }
        stdout = res.stdout ?? "";
      } catch (e) {
        logger.info(`[dashscript] lint threw: ${(e as Error).message}`);
        return [];
      }
      let parsed: DsLintDiagnostic[];
      try {
        parsed = JSON.parse(stdout || "[]");
      } catch {
        logger.info("[dashscript] non-JSON lint output (is 'ds' on PATH?)");
        return [];
      }
      const sourceFile = info.languageService.getProgram()?.getSourceFile(fileName);
      if (!sourceFile) return [];
      return parsed
        .map((d) => toDiagnostic(d, sourceFile))
        .filter((d): d is ts.Diagnostic => d !== undefined);
    }

    /** Convert a 1-based line/column `ds` diagnostic to a 0-based `ts.Diagnostic`. */
    function toDiagnostic(d: DsLintDiagnostic, file: ts.SourceFile): ts.Diagnostic | undefined {
      try {
        const start = file.getPositionOfLineAndCharacter(d.line - 1, d.column - 1);
        const end = file.getPositionOfLineAndCharacter(d.endLine - 1, d.endColumn - 1);
        return {
          file,
          start,
          length: Math.max(0, end - start),
          messageText: d.message,
          category:
            d.severity === "warning" ? ts.DiagnosticCategory.Warning : ts.DiagnosticCategory.Error,
          code: 0,
          source: "dashscript",
        };
      } catch {
        return undefined;
      }
    }

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

    // Pass-through proxy over the real language service, then override
    // `getSemanticDiagnostics` to append DashScript translatability diagnostics.
    const proxy: ts.LanguageService = Object.create(null);
    for (const key of Object.keys(info.languageService) as Array<keyof ts.LanguageService>) {
      const original = info.languageService[key] as unknown as (...args: unknown[]) => unknown;
      // @ts-expect-error — proxy assignment across the whole LanguageService surface
      proxy[key] = (...args: unknown[]) => original.apply(info.languageService, args);
    }

    proxy.getSemanticDiagnostics = (fileName: string) => {
      const prior = info.languageService.getSemanticDiagnostics(fileName);
      if (!fileName.endsWith(".ts") || fileName.endsWith(".d.ts")) return prior;
      ensureLocalRsTypes(fileName); // refresh `<x>.d.ts` for local `.rs` imports
      const version = host.getScriptVersion(fileName);
      const cached = cache.get(fileName);
      let diags: ts.Diagnostic[];
      if (cached && cached.version === version) {
        diags = cached.diags;
      } else {
        diags = runLint(fileName);
        cache.set(fileName, { version, diags });
      }
      return [...prior, ...diags];
    };

    /** Go-to-definition: when the target is a local `<x>.d.ts` with a sibling
     * `<x>.rs`, redirect to the `.rs` source — the developer reads the real
     * Rust, and rust-analyzer then handles symbol-level navigation inside it.
     * Best-effort position is the file head (the exact column is found by
     * rust-analyzer once the file is open). Redirects for cargo crates land
     * later — they need a crate `.d.ts` source first. */
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
