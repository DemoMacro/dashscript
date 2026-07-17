#!/usr/bin/env node
// Extracts every `.ds` fixture from `crates/dashscript/src/translator/tests/*.rs`
// into `tests/conformance/data/tests-fixtures.json` — zero hand-written fixtures.
//
// Each `#[test]` there has a `let src = "..."` that is a verified-translatable
// `.ds` snippet. The conformance runner cargo-checks each one (translator/tests
// only asserts the translated Rust *contains* a substring — it never compiles
// it), recording supported/partial/unsupported informationally. No `expect` is
// emitted, so the run reports the current state without asserting it.
//
// Run: node scripts/extract-tests.mjs

import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const dir = resolve(__dirname, "..", "crates", "dashscript", "src", "translator", "tests");
const out = resolve(
  __dirname,
  "..",
  "crates",
  "dashscript",
  "tests",
  "conformance",
  "data",
  "tests-fixtures.json",
);

const files = readdirSync(dir).filter((f) => f.endsWith(".rs") && f !== "mod.rs");
const fixtures = [];
const seen = new Set();
for (const f of files) {
  const category = f.replace(/\.rs$/, "");
  const src = readFileSync(resolve(dir, f), "utf8");
  let curFn = null;
  let pendingSrc = false; // `let src =` on its own line — the string follows
  const emit = (raw) => {
    if (!curFn) return;
    const ds = raw.replace(/\\"/g, '"').replace(/\\n/g, " ");
    if (ds.includes("function")) {
      const id = `${category}.${curFn}`;
      if (!seen.has(id)) {
        seen.add(id);
        fixtures.push({ id, category, source: "translator-tests", fixture: ds });
      }
    }
    curFn = null; // one fixture per test (the first `let src`)
    pendingSrc = false;
  };
  for (const line of src.split("\n")) {
    const fnM = line.match(/fn\s+(translates_\w+)\s*\(/);
    if (fnM) {
      curFn = fnM[1];
      pendingSrc = false;
      continue;
    }
    const srcM = line.match(/let\s+src\s*=\s*"((?:[^"\\]|\\.)*)"/);
    if (srcM) {
      emit(srcM[1]);
      continue;
    }
    // `let src =` on its own line: the string literal is on the next line.
    if (pendingSrc) {
      const contM = line.match(/^\s*"((?:[^"\\]|\\.)*)"/);
      if (contM) {
        emit(contM[1]);
        continue;
      }
    }
    if (/let\s+src\s*=\s*$/.test(line)) pendingSrc = true;
  }
}

const doc = {
  _comment:
    "Auto-extracted from translator/tests/*.rs by scripts/extract-tests.mjs. DO NOT edit by hand. Each entry is a verified-translatable .ds snippet; the conformance runner cargo-checks it. No `expect` — status is recorded informationally.",
  features: fixtures,
};
writeFileSync(out, `${JSON.stringify(doc, null, 2)}\n`);
console.log(`extract-tests: wrote ${fixtures.length} fixtures to ${out}`);
