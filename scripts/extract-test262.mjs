#!/usr/bin/env node
// Extracts DashScript differential fixtures from tc39/test262 into per-category
// files under `tests/conformance/data/test262/<category>.json`. Each test262
// file's asserts are rewritten (`assert.sameValue(a,b)` → `console.log(a)`,
// `assert.throws(C,fn)` → try/catch + console.log) and wrapped in
// `function main(): void { … }`, preserving the file's setup. The conformance
// harness then runs Node (oracle) vs `ds` (actual) on the same source and
// diffs stdout — a differential test.
//
// Per-category files (not one giant test262.json) so the harness can run a
// single builtin end-to-end (`DASH_TEST262_CATEGORIES=math`) and write a
// per-category matrix — incremental work, one builtin at a time. "Add a
// builtin" = run this with `--category <name>` once; the harness discovers the
// new file automatically.
//
// test262 is the tc39 official ECMAScript conformance suite (the one
// Node/Bun/Deno/V8/Boa all run); bcd/runtime-compat only test API *existence*,
// not *semantics*, so they are not used here.
//
// Requires the repo cloned beside the project:
//   git clone https://github.com/tc39/test262 .temp/test262
// Then:
//   node scripts/extract-test262.mjs --category math,number   # a subset
//   node scripts/extract-test262.mjs                          # all builtins
//   node scripts/extract-test262.mjs --probe                  # Math/round smoke

import { readFileSync, writeFileSync, readdirSync, statSync, existsSync, mkdirSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, resolve, relative, join } from "node:path";
import { fileURLToPath } from "node:url";

// acorn ships as CJS without an ESM default export — load it via createRequire
// (same pattern scripts/sync-bcd.mjs uses for @mdn/browser-compat-data).
const require = createRequire(import.meta.url);
const acorn = require("acorn");

const __dirname = dirname(fileURLToPath(import.meta.url));
const TEST262 = resolve(__dirname, "..", ".temp", "test262");
// One `<category>.json` per builtin. The conformance harness globs this dir,
// so a new category file is automatically included — no Rust edit needed.
const OUT_DIR = resolve(
  __dirname,
  "..",
  "crates",
  "dashscript",
  "tests",
  "conformance",
  "data",
  "test262",
);

// `--category math,number` → only those builtins (lowercase, matching the
// `test/built-ins/` dir name). Omitted → every top-level dir (the full ~68
// builtins). Nothing is excluded up front: `Translator::check` marks
// constructs it cannot lower as `unsupported` (the honest signal — a gap you
// can see) rather than hiding them with a whitelist.
function requestedCategories() {
  const i = process.argv.indexOf("--category");
  if (i !== -1 && process.argv[i + 1]) {
    return process.argv[i + 1]
      .split(",")
      .map((s) => s.trim().toLowerCase())
      .filter(Boolean);
  }
  return null;
}

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
          // Wrap the callable in parens: an anonymous `function () { … }`
          // emitted bare in statement position is parsed as a
          // `FunctionDeclaration` (which requires a name) by oxc's TS
          // SourceType → "Expected function name". `(fn)()` forces the
          // expression form so it parses as a call.
          repl: `try { (${fn})(); console.log("__OK__"); } catch (e) { console.log(e.constructor.name); }`,
        });
      } else if (args[0]) {
        // sameValue / notSameValue: log the actual (left) operand.
        const actual = body.slice(args[0].start, args[0].end);
        edits.push({ start: node.start, end: node.end, repl: `console.log(${actual})` });
      }
      if (args[0] || (kind === "throws" && args[1])) n++;
    }
    // `assert(x)` (a direct call, not `assert.sameValue`) is test262's shorthand
    // for `assert.sameValue(x, true)` — rewrite it the same way (log the
    // operand) so ds and Node emit identical stdout. Unrewritten, the bare
    // `assert(...)` lowers to Rust's `assert` macro (E0423 expected function).
    if (
      node.type === "CallExpression" &&
      node.callee?.type === "Identifier" &&
      node.callee.name === "assert" &&
      node.arguments[0]
    ) {
      const actual = body.slice(node.arguments[0].start, node.arguments[0].end);
      edits.push({ start: node.start, end: node.end, repl: `console.log(${actual})` });
      n++;
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
  const requested = requestedCategories();
  const builtInsDir = resolve(TEST262, "test", "built-ins");
  const SCOPE = readdirSync(builtInsDir)
    .filter((e) => statSync(join(builtInsDir, e)).isDirectory())
    .map((e) => ({ dir: `test/built-ins/${e}`, category: e.toLowerCase() }))
    .filter((s) => requested === null || requested.includes(s.category));
  if (requested && SCOPE.length === 0) {
    console.error(`no test/built-ins/ dir matches --category=${requested.join(",")}`);
    process.exit(1);
  }

  mkdirSync(OUT_DIR, { recursive: true });
  // Per-category feature lists + global skip tallies.
  const byCat = new Map();
  const tally = { parse: 0, noassert: 0, harness: 0, reflect: 0 };
  for (const { dir, category } of SCOPE) {
    const root = resolve(TEST262, dir);
    if (!existsSync(root)) continue;
    const files = [];
    walk(root, files);
    const feats = [];
    const seen = new Set();
    for (const f of files) {
      const rel = relative(TEST262, f).replace(/\\/g, "/");
      if (!FILTER(rel)) continue;
      const src = readFileSync(f, "utf8");
      const { flags, includes, bodyStart } = frontmatter(src);
      const r = rewrite(src.slice(bodyStart));
      if (!r.ok) {
        if (r.reason === "parse error") tally.parse++;
        else tally.noassert++;
        continue;
      }
      // frontmatter `includes:` lists $INCLUDE harness files the extractor
      // does not inline (isConstructor.js, byteConversionValues.js, …); their
      // identifiers are undefined in the fixture, so skip rather than translate.
      if (includes.length > 0) {
        tally.harness++;
        continue;
      }
      // `new` / `Reflect` / `new.target` — JS object-model reflection;
      // DashScript has Rust semantics, so these never apply.
      if (r.inapplicable) {
        tally.reflect++;
        continue;
      }
      const fixture = `function main(): void {\n${r.body.trim()}\n}\n`;
      const id = "test262." + rel.replace(/\.js$/, "").replace(/[/.]/g, ".").toLowerCase();
      if (seen.has(id)) continue;
      seen.add(id);
      feats.push({
        id,
        category,
        source: "test262",
        fixture,
        origin: rel,
        n_asserts: r.n,
        flags,
      });
    }
    if (feats.length > 0) byCat.set(category, feats);
  }

  const comment =
    "Auto-extracted from tc39/test262 by scripts/extract-test262.mjs (category scope). " +
    "Each fixture wraps a test262 file's asserts in `function main(): void { … }` with " +
    "assert.sameValue(a,b)→console.log(a) / assert.throws→try-catch. The conformance " +
    "harness runs Node (oracle) vs ds (actual) and diffs stdout (differential test). " +
    "DO NOT edit by hand.";
  let total = 0;
  const summary = [];
  for (const [cat, feats] of [...byCat.entries()].sort((a, b) => a[0].localeCompare(b[0]))) {
    writeFileSync(
      join(OUT_DIR, `${cat}.json`),
      `${JSON.stringify({ _comment: comment, features: feats }, null, 2)}\n`,
    );
    total += feats.length;
    summary.push(`${cat}=${feats.length}`);
  }
  console.log(
    `extract-test262: wrote ${total} fixtures across ${byCat.size} categories to ${OUT_DIR}\n` +
      `  ${summary.join("  ")}\n` +
      `  skipped: parse=${tally.parse} noassert=${tally.noassert} harness($INCLUDE)=${tally.harness} reflect(new/Reflect)=${tally.reflect}`,
  );
}

extract();
