#!/usr/bin/env node
/**
 * DashScript benchmark harness.
 *
 * Each bench is a subdirectory of examples/bench holding `main.ds` +
 * `main.ts` + `manifest.json` — one algorithm written once; `main.ds` is
 * DashScript's entry (it lowers to Rust `fn main`), `main.ts` runs unchanged
 * under node / bun / perry with a trailing `main()`.
 *
 * Every available runtime runs each bench. We report the median wall-clock of
 * `BENCH_SAMPLES` (env, default 5) process launches and gate every result on
 * the stdout checksum matching across runtimes — a "fast" number from a build
 * that returned the wrong answer is flagged, not silently reported.
 *
 * ds is timed against its prebuilt `dist/<name>(.exe)` (from `ds build`) and
 * perry against its compiled native binary; both are pure native execution
 * with no compiler on the hot path. node and bun launch their own VM (V8 /
 * JSC), so their wall-clock includes runtime startup — exactly what any
 * `node script.ts` invocation pays. See README.md for what this does and does
 * not measure, and why the cross-runtime checksum is the load-bearing check.
 *
 * Per-sample timeout. The first available runtime (ds) sets the reference
 * median for a bench; every later runtime gets `max(BENCH_TIMEOUT_MS,
 * ds_median × BENCH_RATIO)` per sample, and a sample over that is killed and
 * the runtime marked `T/O` for that bench. This stops a pathologically slow
 * runtime from blocking the whole suite — perry on an f64 recurrence loop its
 * optimizer cannot fold ran ~100s/sample vs ds's ~3s, and timing it 5× added
 * minutes for a number that is not informative. ds itself uses the absolute
 * `BENCH_TIMEOUT_MS` floor, so a genuinely hung bench still terminates.
 *
 * Usage:
 *   node run.mjs                  # all benches, all available runtimes
 *   node run.mjs fib array-ops    # a subset
 *   BENCH_SAMPLES=11 node run.mjs fib
 *   BENCH_TIMEOUT_MS=30000 node run.mjs   # tighten the per-sample ceiling
 */
import { execSync, spawnSync } from "node:child_process";
import { readdirSync, existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const ROOT = import.meta.dirname;
const SAMPLES = +(process.env.BENCH_SAMPLES ?? 5);
const IS_WIN = process.platform === "win32";
const EXE = IS_WIN ? ".exe" : "";
// Per-sample wall-clock budget. The first runtime (ds) is the reference:
// later runtimes get max(TIMEOUT_MS, ds_median × RATIO).
const TIMEOUT_MS = +(process.env.BENCH_TIMEOUT_MS ?? 60_000);
const RATIO = +(process.env.BENCH_RATIO ?? 20);
// Builds (ds build / perry compile) get a separate, roomier ceiling — a cold
// first compile is legitimately slower than any single run.
const BUILD_TIMEOUT_MS = +(process.env.BENCH_BUILD_TIMEOUT_MS ?? 300_000);

const sh = (cmd, opts = {}) => execSync(cmd, { stdio: "pipe", timeout: BUILD_TIMEOUT_MS, ...opts });
const shOut = (cmd, opts = {}) =>
  execSync(cmd, { encoding: "utf8", stdio: ["pipe", "pipe", "pipe"], ...opts });

const errShort = (e) =>
  String(e?.message ?? e)
    .split("\n")[0]
    .slice(0, 100);

/// Run `cmd` once via the shell, killing it past `timeoutMs`. Returns
/// `{ ok, stdout, timedOut, status }` so the caller can tell a timeout (killed
/// mid-run) from a real non-zero exit, without try/parse on Node error strings.
function runOnce(cmd, opts, timeoutMs) {
  const r = spawnSync(cmd, {
    shell: true,
    encoding: "utf8",
    stdio: ["pipe", "pipe", "pipe"],
    timeout: timeoutMs,
    ...opts,
  });
  // status === null with a signal means the process was killed (the timeout
  // path); a real crash still produces a numeric status.
  const timedOut = r.status === null && r.signal !== null;
  return {
    ok: r.status === 0,
    stdout: r.stdout ?? "",
    timedOut,
    status: r.status,
  };
}

// A runtime is reachable if `<cmd> --version` (or `-v`) exits 0.
function available(cmd) {
  try {
    shOut(`${cmd} --version`);
    return true;
  } catch {
    try {
      shOut(`${cmd} -v`);
      return true;
    } catch {
      return false;
    }
  }
}

function median(samples) {
  const s = [...samples].sort((a, b) => a - b);
  const m = s.length >> 1;
  return s.length % 2 ? s[m] : (s[m - 1] + s[m]) / 2;
}

const benches = readdirSync(ROOT)
  .filter((d) => existsSync(join(ROOT, d, "main.ds")))
  .filter((d) => process.argv.slice(2).length === 0 || process.argv.slice(2).includes(d))
  .sort();

const RUNTIMES = [
  {
    name: "ds",
    vcmd: "ds",
    build: (dir) => sh("ds build", { cwd: dir }),
    runCmd: (dir, name) => `"${join(dir, "dist", name + EXE)}"`,
  },
  {
    name: "node",
    vcmd: "node",
    build: () => {},
    runCmd: () => `node main.ts`,
  },
  {
    name: "bun",
    vcmd: "bun",
    build: () => {},
    runCmd: () => `bun main.ts`,
  },
  {
    name: "perry",
    vcmd: "perry",
    build: (dir, name) => sh(`perry compile main.ts -o ${name}-perry${EXE}`, { cwd: dir }),
    runCmd: (dir, name) => `"${join(dir, name + "-perry" + EXE)}"`,
  },
]
  .filter((r) => available(r.vcmd))
  .filter((r) => {
    const want = process.env.BENCH_RUNTIMES?.split(",").map((s) => s.trim());
    return !want || want.includes(r.name);
  });

if (RUNTIMES.length === 0) {
  console.error("no runtimes available (need at least one of: ds, node, bun, perry)");
  process.exit(1);
}

const fmt = (ms) => (ms == null ? "    —" : ms.toFixed(1).padStart(7) + " ms");

const results = [];
const header = ["bench", ...RUNTIMES.map((r) => r.name), "checksum", "ok"];
console.log(header.join("   "));
console.log("-".repeat(16 + RUNTIMES.length * 10 + 24));

for (const bench of benches) {
  const dir = join(ROOT, bench);
  const name = JSON.parse(readFileSync(join(dir, "manifest.json"), "utf8")).name;
  const row = { bench, name, times: {}, checksums: {}, error: {}, timedOut: {} };
  // The first runtime's median is the reference for relative timeouts on
  // subsequent runtimes.
  let refMed = null;

  for (let ri = 0; ri < RUNTIMES.length; ri++) {
    const rt = RUNTIMES[ri];
    try {
      rt.build(dir, name);
    } catch (e) {
      row.times[rt.name] = null;
      row.error[rt.name] = "build: " + errShort(e);
      continue;
    }
    const cmd = rt.runCmd(dir, name);
    const budget = refMed == null ? TIMEOUT_MS : Math.max(TIMEOUT_MS, refMed * RATIO);
    const samples = [];
    let checksum = null;
    let timedOut = false;
    let runErr = null;
    for (let i = 0; i < SAMPLES; i++) {
      const t = process.hrtime.bigint();
      const r = runOnce(cmd, { cwd: dir }, budget);
      if (r.timedOut) {
        timedOut = true;
        break;
      }
      if (!r.ok) {
        runErr = `exit ${r.status}`;
        break;
      }
      if (checksum == null) checksum = r.stdout.trim().split(/\r?\n/).pop().trim();
      samples.push(Number(process.hrtime.bigint() - t) / 1e6);
    }
    if (checksum != null) row.checksums[rt.name] = checksum;
    if (timedOut) row.timedOut[rt.name] = true;
    else if (runErr != null) row.error[rt.name] = runErr;
    row.times[rt.name] = samples.length > 0 && !timedOut ? median(samples) : null;
    if (ri === 0 && row.times[rt.name] != null) refMed = row.times[rt.name];
  }

  const okVals = Object.values(row.checksums);
  // A bench is consistent only when every runtime that produced a checksum
  // agrees; a runtime that timed out (no checksum) does not count against it.
  row.consistent = okVals.length > 0 && okVals.every((v) => v === okVals[0]);
  const ref = okVals[0] ?? "(none)";

  console.log(
    [
      bench.padEnd(12),
      ...RUNTIMES.map((r) => (row.timedOut[r.name] ? "    T/O" : fmt(row.times[r.name]))),
      String(ref).slice(0, 14).padEnd(14),
      row.consistent ? "✓" : "✗ MISMATCH",
    ].join("   "),
  );
  for (const [k, v] of Object.entries(row.error)) {
    console.log(`    ${k}: ${v}`);
  }
  results.push(row);
}

writeFileSync(
  join(ROOT, "results.json"),
  JSON.stringify({ samples: SAMPLES, runtimes: RUNTIMES.map((r) => r.name), results }, null, 2) +
    "\n",
);
console.log(`\nwrote results.json (${SAMPLES} samples each, median reported)`);
