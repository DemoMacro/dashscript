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
  { dir: "test/built-ins/String/prototype", category: "string" },
  { dir: "test/built-ins/Array/prototype", category: "array" },
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
  if (!m) return { flags: [], includes: [], bodyStart: 0 };
  const flags = [];
  const includes = [];
  const list = (block, key, out) => {
    const mm = block.match(new RegExp(`${key}:\\s*\\[([^\\]]*)\\]`));
    if (!mm) return;
    for (const f of mm[1]
      .split(",")
      .map((s) => s.trim())
      .filter(Boolean)) {
      out.push(f.replace(/['"]/g, ""));
    }
  };
  list(m[1], "flags", flags);
  list(m[1], "includes", includes);
  return { flags, includes, bodyStart: m.index + m[0].length };
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
  // DashScript has Rust semantics — no `[[Construct]]`, no `Reflect`, no
  // prototype chain. `new X(...)` / `new.target` / `Reflect.*` are JS
  // object-model reflection that never maps to Rust; flag them so extract()
  // skips the fixture instead of recording a fake "unsupported translator TODO".
  let inapplicable = false;
  const visit = (node) => {
    if (!node || typeof node.type !== "string") return;
    if (
      node.type === "NewExpression" ||
      node.type === "MetaProperty" ||
      (node.type === "Identifier" && node.name === "Reflect")
    ) {
      inapplicable = true;
    }
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
  return { ok: true, body: out, n, inapplicable };
}

function extract() {
  if (!existsSync(TEST262)) {
    console.error(`test262 not found at ${TEST262}`);
    console.error(`Run: git clone https://github.com/tc39/test262 .temp/test262`);
    process.exit(1);
  }
  const features = [];
  const seen = new Set();
  let skipParse = 0;
  let skipNoAssert = 0;
  // frontmatter `includes:` lists $INCLUDE harness files the extractor does
  // not inline (isConstructor.js, byteConversionValues.js, …); their
  // identifiers are undefined in the fixture, so skip rather than translate.
  let skipHarness = 0;
  // `new` / `Reflect` / `new.target` — JS object-model reflection; DashScript
  // has Rust semantics, so these never apply (see rewrite()'s `inapplicable`).
  let skipReflect = 0;
  for (const { dir, category } of SCOPE) {
    const root = resolve(TEST262, dir);
    if (!existsSync(root)) continue;
    const files = [];
    walk(root, files);
    for (const f of files) {
      const rel = relative(TEST262, f).replace(/\\/g, "/");
      if (!FILTER(rel)) continue;
      const src = readFileSync(f, "utf8");
      const { flags, includes, bodyStart } = frontmatter(src);
      const r = rewrite(src.slice(bodyStart));
      if (!r.ok) {
        if (r.reason === "parse error") skipParse++;
        else skipNoAssert++;
        continue;
      }
      if (includes.length > 0) {
        skipHarness++;
        continue;
      }
      if (r.inapplicable) {
        skipReflect++;
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
  console.log(
    `extract-test262: wrote ${features.length} fixtures to ${OUT}\n` +
      `  skipped: parse=${skipParse} noassert=${skipNoAssert} harness($INCLUDE)=${skipHarness} reflect(new/Reflect)=${skipReflect}`,
  );
}

extract();
