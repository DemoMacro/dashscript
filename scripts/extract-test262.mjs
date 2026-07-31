#!/usr/bin/env node
// Extracts DashScript differential fixtures from tc39/test262 into per-category
// files under `tests/conformance/data/test262/<category>.json`. Each test262
// Each test262 file's body is wrapped verbatim in `function main(): void { … }`
// — assert.sameValue/throws stay as-is (DashScript lowers them to a SameValue
// check / the engine). The conformance harness runs `ds` and judges by exit
// code + Test262Error detection (no Node oracle): a fixture whose asserts all
// hold passes (supported); a thrown Test262Error fails it (partial).
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
  if (!m) return { flags: [], includes: [], features: [], bodyStart: 0 };
  const flags = [];
  const includes = [];
  const features = [];
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
  list(m[1], "features", features);
  return { flags, includes, features, bodyStart: m.index + m[0].length };
}

const ASSERTS = new Set(["sameValue", "notSameValue", "throws"]);

// Parse `body`, count its asserts, and detect reflection. The body is returned
// verbatim — DashScript lowers `assert.sameValue`/`notSameValue` to a static
// SameValue check, while `assert.throws`, reflection, and composite operands
// route to the embedded engine, where the test262 harness's reference semantics
// run natively. Returns { ok, body, n } or { ok: false, reason }.
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
  let n = 0;
  // `new X(...)` / `new.target` / `Reflect.*` are JS object-model reflection.
  // Recorded for the tally but no longer excluded — such a fixture routes to
  // the engine, where the reflection runs natively under the test262 harness.
  let inapplicable = false;
  const visit = (node) => {
    if (!node || typeof node.type !== "string") return;
    const isAssertMember =
      node.type === "CallExpression" &&
      node.callee?.type === "MemberExpression" &&
      !node.callee.computed &&
      node.callee.object?.type === "Identifier" &&
      node.callee.object.name === "assert" &&
      ASSERTS.has(node.callee.property.name);
    const isBareAssert =
      node.type === "CallExpression" &&
      node.callee?.type === "Identifier" &&
      node.callee.name === "assert" &&
      node.arguments[0];
    if (isAssertMember || isBareAssert) n++;
    if (
      node.type === "NewExpression" ||
      node.type === "MetaProperty" ||
      (node.type === "Identifier" && node.name === "Reflect")
    ) {
      inapplicable = true;
    }
    for (const k in node) {
      const v = node[k];
      if (Array.isArray(v)) v.forEach(visit);
      else if (v && typeof v.type === "string") visit(v);
    }
  };
  visit(ast);
  if (n === 0) return { ok: false, reason: "no asserts" };
  return { ok: true, body, n, inapplicable };
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
  const tally = { parse: 0, noassert: 0 };
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
      const { flags, includes, features, bodyStart } = frontmatter(src);
      const r = rewrite(src.slice(bodyStart));
      if (!r.ok) {
        if (r.reason === "parse error") tally.parse++;
        else tally.noassert++;
        continue;
      }
      // frontmatter `includes:` lists $INCLUDE harness files (isConstructor.js,
      // propertyHelper.js, …) the extractor does not inline; the conformance
      // harness injects them on the engine path, so carry them through rather
      // than skip. `new`/`Reflect`/`new.target` likewise route to the engine.
      const fixture = `function main(): void {\n${r.body.trim()}\n}\nmain();\n`;
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
        features,
        includes,
      });
    }
    if (feats.length > 0) byCat.set(category, feats);
  }

  const comment =
    "Auto-extracted from tc39/test262 by scripts/extract-test262.mjs (category scope). " +
    "Each fixture wraps a test262 file's body verbatim in `function main(): void { … }` " +
    "— assert.sameValue/throws stay as-is (DashScript lowers them to a SameValue check / " +
    "the engine). `includes` lists the test262 harness files ($INCLUDE) the conformance " +
    "harness injects on the engine path. DO NOT edit by hand.";
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
      `  skipped: parse=${tally.parse} noassert=${tally.noassert}`,
  );
}

extract();
