#!/usr/bin/env node
// Extracts WinterTC (Ecma TC55) conformance fixtures from
// web-platform-tests/wpt into per-dir files under
// `tests/conformance/data/wpt/<dir>.json`. Each WPT `.any.js` fixture's body
// is wrapped verbatim in `function main(): void { … }` — `test()`/
// `assert_equals` stay as-is (DashScript lowers them to `__ds::wpt_*` static
// helpers). The conformance harness runs the **static path only** (translate →
// cargo → run) and judges by exit code + `AssertionError` detection: a
// fixture whose asserts all hold passes (supported); a thrown `AssertionError`
// fails it (partial); a build failure / timeout / rejected construct is
// unsupported. WinterTC is pure-Rust: there is NO engine fallback — the
// testharness builtin + every Web API mapping are static.
//
// Dir scope = the WinterTC ECMA-429 §5 minimum common web API
// (url/urlpattern/encoding/hr-time/html/dom/WebCryptoAPI/console/fetch/
// streams/FileAPI/compression/wasm/xhr), NOT an ad-hoc pick. WinterTC55 has no
// separate test repo — "the WinterTC test suite is a subset of Web platform
// Tests" (W3C 2025-11-12 minutes), and the subset *is* the §5 API list. So a
// differential failure points at the API to implement next.
//
// Per-dir files (not one giant wpt.json) so the harness can run a single API
// end-to-end (`DASH_WPT_CATEGORIES=url`) and write a per-dir matrix —
// incremental work, one Web API at a time. "Add a Web API" = implement the
// static mapping, re-run this with `--dirs <api>`, watch the dir's matrix rise.
//
// Requires the repo cloned beside the project:
//   git clone --depth 1 https://github.com/web-platform-tests/wpt .temp/wpt
// Then:
//   node scripts/extract-wpt.mjs --dirs url,encoding   # a subset
//   node scripts/extract-wpt.mjs                        # all WinterTC §5 dirs

import { readFileSync, writeFileSync, readdirSync, statSync, existsSync, mkdirSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, resolve, relative, join } from "node:path";
import { fileURLToPath } from "node:url";

// acorn ships as CJS without an ESM default export — load it via createRequire
// (same pattern scripts/extract-test262.mjs uses).
const require = createRequire(import.meta.url);
const acorn = require("acorn");

const __dirname = dirname(fileURLToPath(import.meta.url));
const WPT = resolve(__dirname, "..", ".temp", "wpt");
// One `<dir>.json` per WinterTC API. The conformance harness globs this dir,
// so a new dir file is automatically included — no Rust edit needed.
const OUT_DIR = resolve(
  __dirname,
  "..",
  "crates",
  "dashscript",
  "tests",
  "conformance",
  "data",
  "wpt",
);

// WinterTC ECMA-429 §5 minimum common web API → WPT top-level dirs. NOT an
// ad-hoc pick: this is the authoritative scope of "the WinterTC test suite"
// (a WPT subset, per the W3C 2025-11-12 minutes). `Translator::check` marks
// constructs it cannot lower as `unsupported` (the honest signal — a gap you
// can see) rather than hiding them with a whitelist, so every §5 dir is
// carried up front.
const WINTERTC_DIRS = [
  "url",
  "urlpattern",
  "encoding",
  "hr-time",
  "html",
  "dom",
  "WebCryptoAPI",
  "console",
  "fetch",
  "streams",
  "FileAPI",
  "compression",
  "wasm",
  "xhr",
];

// `--dirs url,encoding` → only those WPT top-level dirs (case-insensitive
// match against the WPT dir name). Omitted → every WinterTC §5 dir.
function requestedDirs() {
  const i = process.argv.indexOf("--dirs");
  if (i !== -1 && process.argv[i + 1]) {
    return process.argv[i + 1]
      .split(",")
      .map((s) => s.trim())
      .filter(Boolean);
  }
  return WINTERTC_DIRS;
}

function walk(dir, out) {
  for (const e of readdirSync(dir)) {
    const p = join(dir, e);
    if (statSync(p).isDirectory()) walk(p, out);
    else if (e.endsWith(".any.js")) out.push(p);
  }
}

// WPT fixtures carry metadata as leading `// META: key=value` lines (no
// frontmatter block, unlike test262's `/*--- … ---*/`). Returns the parsed
// META + the body (everything after the META block). `script=/foo.js` →
// includes (harness scripts the fixture depends on); `variant=` → per-variant
// include sets (carried as metadata; the harness runs the full fixture body);
// `global=`/`timeout=`/`title=` → metadata.
function parseMeta(src) {
  const includes = [];
  const variants = [];
  const meta = {};
  const lines = src.split(/\r?\n/);
  let bodyStart = 0;
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    if (line.trim() === "") {
      bodyStart = i + 1;
      continue;
    }
    const m = line.match(/^\/\/\s*META:\s*(\w+)=(.*)$/);
    if (!m) break; // first non-META, non-blank line → body starts here
    const [, key, value] = m;
    const v = value.trim();
    if (key === "script") includes.push(v);
    else if (key === "variant") variants.push(v);
    else meta[key] = v;
    bodyStart = i + 1;
  }
  return { includes, variants, meta, body: lines.slice(bodyStart).join("\n") };
}

// Parse the body for test counts + flag async/fetch use (matrix diagnostics,
// not filtering). The body is returned verbatim — DashScript lowers `test()`/
// `assert_equals` to static `__ds::wpt_*` helpers; `async_test`/`promise_test`
// classify `Reject` (no static lowering, no engine fallback — WinterTC is
// static-only); `fetch`/other Tier-3 Web APIs leave the fixture honestly
// `unsupported` until mapped. Returns { ok, body, nTests, hasAsync, hasFetch }
// or { ok: false, reason }.
function rewrite(body) {
  let ast;
  try {
    ast = acorn.parse(body, {
      ecmaVersion: "latest",
      sourceType: "script",
      allowReturnOutsideFunction: true,
    });
  } catch (e) {
    return { ok: false, reason: `parse error: ${e.message}` };
  }
  let nTests = 0;
  let hasAsync = false;
  let hasFetch = false;
  const visit = (node) => {
    if (!node || typeof node.type !== "string") return;
    if (node.type === "CallExpression") {
      const callee = node.callee;
      const name =
        callee.type === "Identifier"
          ? callee.name
          : callee.type === "MemberExpression" && !callee.computed
            ? callee.property?.name
            : undefined;
      if (name === "test") nTests++;
      else if (name === "async_test" || name === "promise_test") {
        nTests++;
        hasAsync = true;
      } else if (name === "step_timeout" || name === "setTimeout" || name === "setInterval") {
        hasAsync = true;
      } else if (name === "fetch") hasFetch = true;
    }
    for (const k in node) {
      const v = node[k];
      if (Array.isArray(v)) v.forEach(visit);
      else if (v && typeof v.type === "string") visit(v);
    }
  };
  visit(ast);
  return { ok: true, body, nTests, hasAsync, hasFetch };
}

function extract() {
  if (!existsSync(WPT)) {
    console.error(`wpt not found at ${WPT}`);
    console.error(`Run: git clone --depth 1 https://github.com/web-platform-tests/wpt .temp/wpt`);
    process.exit(1);
  }
  const dirs = requestedDirs();
  mkdirSync(OUT_DIR, { recursive: true });
  const byDir = new Map();
  const tally = { parse: 0 };
  let total = 0;
  for (const dir of dirs) {
    const root = resolve(WPT, dir);
    if (!existsSync(root)) {
      console.error(`wpt dir not found: ${dir} (skipping)`);
      continue;
    }
    const files = [];
    walk(root, files);
    const feats = [];
    const seen = new Set();
    for (const f of files) {
      const rel = relative(WPT, f).replace(/\\/g, "/");
      const src = readFileSync(f, "utf8");
      const { includes, variants, meta, body } = parseMeta(src);
      const r = rewrite(body);
      if (!r.ok) {
        tally.parse++;
        continue;
      }
      // Wrap the body verbatim — the implicit `fn main` collector gathers the
      // top-level `test()` calls (pure-TS execution semantics, same as test262).
      const fixture = `function main(): void {\n${r.body.trim()}\n}\nmain();\n`;
      const id =
        "wpt." +
        rel
          .replace(/\.any\.js$/, "")
          .replace(/[/.]/g, ".")
          .toLowerCase();
      if (seen.has(id)) continue;
      seen.add(id);
      feats.push({
        id,
        category: dir.toLowerCase(),
        source: "wpt",
        fixture,
        origin: rel,
        n_tests: r.nTests,
        async: r.hasAsync,
        fetch: r.hasFetch,
        includes,
        variants,
        meta,
      });
    }
    if (feats.length > 0) byDir.set(dir.toLowerCase(), feats);
    total += feats.length;
  }

  const comment =
    "Auto-extracted from web-platform-tests/wpt by scripts/extract-wpt.mjs " +
    "(WinterTC ECMA-429 §5 minimum common API scope). Each fixture wraps a WPT " +
    ".any.js body verbatim in `function main(): void { … }` — test()/" +
    "assert_equals stay as-is (DashScript lowers them to __ds::wpt_* static " +
    "helpers). WinterTC is pure-Rust: the conformance harness runs the static " +
    "path only (no engine fallback). `includes` lists the WPT harness scripts " +
    "(// META: script=) the fixture depends on. DO NOT edit by hand.";
  const summary = [];
  for (const [cat, feats] of [...byDir.entries()].sort((a, b) => a[0].localeCompare(b[0]))) {
    writeFileSync(
      join(OUT_DIR, `${cat}.json`),
      `${JSON.stringify({ _comment: comment, features: feats }, null, 2)}\n`,
    );
    summary.push(`${cat}=${feats.length}`);
  }
  console.log(
    `extract-wpt: wrote ${total} fixtures across ${byDir.size} dirs to ${OUT_DIR}\n` +
      `  ${summary.join("  ")}\n` +
      `  skipped: parse=${tally.parse}`,
  );
}

extract();
