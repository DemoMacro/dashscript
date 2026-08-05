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

// Inline a fixture's `includes:` harness files verbatim before its body, so
// the translator sees the same helpers the test262 runner injects via
// $INCLUDE (testWithTypedArrayConstructors, compareArray, … defined in
// harness/<file>.js). A fixture whose body references one of these degrades
// per-function to the engine; `__ds_engine::call_fn` evals the whole module's
// JS (`__DS_MODULE_JS`, including these inlined helpers) before invoking the
// degraded function, so the helper is in scope. Recursive depth-first: a
// harness file's own `includes:` are inlined before its body. A `seen` set
// breaks cycles; a missing file is skipped. The frontmatter block is stripped
// — only the JS body is inlined.
function inlineIncludes(includes, seen = new Set()) {
  let out = "";
  for (const inc of includes) {
    const incPath = resolve(TEST262, "harness", inc);
    const norm = inc.replace(/\\/g, "/");
    if (seen.has(norm) || !existsSync(incPath)) continue;
    seen.add(norm);
    const src = readFileSync(incPath, "utf8");
    const { includes: nested, bodyStart } = frontmatter(src);
    out += inlineIncludes(nested, seen);
    out += src.slice(bodyStart).trim() + "\n";
  }
  return out;
}

// test262's runner injects `sta.js` + `assert.js` into *every* fixture by
// default (they are NOT listed in frontmatter `includes:`), so harness helpers
// freely call their globals — e.g. `testTypedArray.js`'s
// `testWithTypedArrayConstructors` calls `isPrimitive` from `assert.js`.
// Inline `assert.js` verbatim before any explicit includes so a degraded
// fixture finds `isPrimitive`/`assert` in `__DS_MODULE_JS`. `sta.js` is not
// inlined: its `Test262Error` is provided (more completely) by the production
// `register_assert` engine builtin, `$DONOTEVALUATE` is almost never used, and
// `$262` comes from the test262 host — DashScript's engine supplies the
// stronger real-thread `$262` (AGENT_262_ENGINE_BUILTIN), not `sta.js`.
const DEFAULT_INCLUDES = ["assert.js"];

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
      // frontmatter `includes:` lists $INCLUDE harness files (testTypedArray.js,
      // atomicsHelper.js, …). These are inlined verbatim before the body so the
      // translator sees the same helpers the test262 runner injects: a fixture
      // referencing one (e.g. testWithTypedArrayConstructors) degrades to the
      // engine, where `__DS_MODULE_JS` (the whole module's JS) carries the
      // helper into scope. `new`/`Reflect`/`new.target` likewise route to engine.
      const inlined = inlineIncludes([...DEFAULT_INCLUDES, ...includes]);
      const fixture = inlined
        ? `${inlined}\nfunction main(): void {\n${r.body.trim()}\n}\nmain();\n`
        : `function main(): void {\n${r.body.trim()}\n}\nmain();\n`;
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
    "the engine). The frontmatter `includes:` ($INCLUDE harness files like " +
    "testTypedArray.js) are inlined verbatim before the body, so a fixture referencing " +
    "a harness helper degrades to the engine and finds the helper in `__DS_MODULE_JS`. " +
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
      `  skipped: parse=${tally.parse} noassert=${tally.noassert}`,
  );
}

extract();
