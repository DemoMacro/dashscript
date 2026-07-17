#!/usr/bin/env node
// Extracts DashScript differential fixtures from tc39/test262 into
// `tests/conformance/data/test262.json`. Each test262 file's asserts are
// rewritten (`assert.sameValue(a,b)` → `console.log(a)`, `assert.throws(C,fn)`
// → try/catch + console.log) and wrapped in `function main(): void { … }`,
// preserving the file's setup. The conformance harness then runs Node (oracle)
// vs `ds` (actual) on the same source and diffs stdout — a differential test.
//
// test262 is the tc39 official ECMAScript conformance suite (the one
// Node/Bun/Deno/V8/Boa all run); bcd/runtime-compat only test API *existence*,
// not *semantics*, so they are not used here.
//
// Requires the repo cloned beside the project:
//   git clone https://github.com/tc39/test262 .temp/test262
// Then:
//   node scripts/extract-test262.mjs            # full whitelisted scope
//   node scripts/extract-test262.mjs --probe    # Math/round only (smoke)

import { readFileSync, writeFileSync, readdirSync, statSync, existsSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, resolve, relative, join } from "node:path";
import { fileURLToPath } from "node:url";

// acorn ships as CJS without an ESM default export — load it via createRequire
// (same pattern scripts/sync-bcd.mjs uses for @mdn/browser-compat-data).
const require = createRequire(import.meta.url);
const acorn = require("acorn");

const __dirname = dirname(fileURLToPath(import.meta.url));
const TEST262 = resolve(__dirname, "..", ".temp", "test262");
const OUT = resolve(
  __dirname,
  "..",
  "crates",
  "dashscript",
  "tests",
  "conformance",
  "data",
  "test262.json",
);

// Whitelisted test262 dirs — DashScript scope is pure language builtins
// (Math/String/Array/Object/Number methods). Descriptor/proto/Symbol/async
// tests live elsewhere and are naturally filtered by `Translator::check`.
const SCOPE = [
  { dir: "test/built-ins/Math", category: "math" },
  // Phase 2 extends to String/prototype, Array/prototype, Object, Number.
];

// `--probe` restricts to Math/round for the stage-1 smoke test.
const PROBE = process.argv.includes("--probe");
const FILTER = PROBE ? (p) => p.includes("/Math/round/") : () => true;

function walk(dir, out) {
  for (const e of readdirSync(dir)) {
    const p = join(dir, e);
    if (statSync(p).isDirectory()) walk(p, out);
    else if (e.endsWith(".js")) out.push(p);
  }
}

// test262 frontmatter is a `/*--- … ---*/` block (after a copyright comment).
// Returns its flags + the offset where the body begins.
function frontmatter(src) {
  const m = src.match(/\/\*---([\s\S]*?)---\*\//);
  if (!m) return { flags: [], bodyStart: 0 };
  const flags = [];
  const fm = m[1].match(/flags:\s*\[([^\]]*)\]/);
  if (fm) {
    for (const f of fm[1]
      .split(",")
      .map((s) => s.trim())
      .filter(Boolean)) {
      flags.push(f.replace(/['"]/g, ""));
    }
  }
  return { flags, bodyStart: m.index + m[0].length };
}

const ASSERTS = new Set(["sameValue", "notSameValue", "throws"]);

// Rewrite every assert.{sameValue,notSameValue,throws} call in `body` to a
// console.log / try-catch, using source slices (no AST generator needed).
// Returns { ok, body, n } or { ok: false, reason }.
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
  const edits = [];
  let n = 0;
  const visit = (node) => {
    if (!node || typeof node.type !== "string") return;
    if (
      node.type === "CallExpression" &&
      node.callee?.type === "MemberExpression" &&
      !node.callee.computed &&
      node.callee.object?.type === "Identifier" &&
      node.callee.object.name === "assert" &&
      ASSERTS.has(node.callee.property.name)
    ) {
      const kind = node.callee.property.name;
      const args = node.arguments;
      if (kind === "throws" && args[1]) {
        const fn = body.slice(args[1].start, args[1].end);
        edits.push({
          start: node.start,
          end: node.end,
          repl: `try { ${fn}(); console.log("__OK__"); } catch (e) { console.log(e.constructor.name); }`,
        });
      } else if (args[0]) {
        // sameValue / notSameValue: log the actual (left) operand.
        const actual = body.slice(args[0].start, args[0].end);
        edits.push({ start: node.start, end: node.end, repl: `console.log(${actual})` });
      }
      if (args[0] || (kind === "throws" && args[1])) n++;
    }
    for (const k in node) {
      const v = node[k];
      if (Array.isArray(v)) v.forEach(visit);
      else if (v && typeof v.type === "string") visit(v);
    }
  };
  visit(ast);
  if (n === 0) return { ok: false, reason: "no asserts" };
  edits.sort((a, b) => b.start - a.start);
  let out = body;
  for (const e of edits) out = out.slice(0, e.start) + e.repl + out.slice(e.end);
  return { ok: true, body: out, n };
}

function extract() {
  if (!existsSync(TEST262)) {
    console.error(`test262 not found at ${TEST262}`);
    console.error(`Run: git clone https://github.com/tc39/test262 .temp/test262`);
    process.exit(1);
  }
  const features = [];
  const seen = new Set();
  let skipped = 0;
  for (const { dir, category } of SCOPE) {
    const root = resolve(TEST262, dir);
    if (!existsSync(root)) continue;
    const files = [];
    walk(root, files);
    for (const f of files) {
      const rel = relative(TEST262, f).replace(/\\/g, "/");
      if (!FILTER(rel)) continue;
      const src = readFileSync(f, "utf8");
      const { flags, bodyStart } = frontmatter(src);
      const r = rewrite(src.slice(bodyStart));
      if (!r.ok) {
        skipped++;
        continue;
      }
      const fixture = `function main(): void {\n${r.body.trim()}\n}\n`;
      const id = "test262." + rel.replace(/\.js$/, "").replace(/[/.]/g, ".").toLowerCase();
      if (seen.has(id)) continue;
      seen.add(id);
      features.push({
        id,
        category,
        source: "test262",
        fixture,
        origin: rel,
        n_asserts: r.n,
        flags,
      });
    }
  }
  const doc = {
    _comment:
      "Auto-extracted from tc39/test262 by scripts/extract-test262.mjs. Each fixture wraps a test262 file's asserts in `function main(): void { … }` with assert.sameValue(a,b)→console.log(a) / assert.throws→try-catch. The conformance harness runs Node (oracle) vs ds (actual) and diffs stdout (differential test). DO NOT edit by hand.",
    features,
  };
  writeFileSync(OUT, `${JSON.stringify(doc, null, 2)}\n`);
  console.log(`extract-test262: wrote ${features.length} fixtures (${skipped} skipped) to ${OUT}`);
}

extract();
