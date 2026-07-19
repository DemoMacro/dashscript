# DashScript benchmarks

Microbenchmarks comparing **DashScript** (`ds`, TypeScript → native Rust) against
runtime peers running the identical TypeScript source — **node** (V8) and **bun**
(JSC), with **perry** (also TypeScript → native) where its toolchain is
available. Each bench is one algorithm written once: `main.ds` is DashScript's
entry (it lowers to Rust `fn main`), `main.ts` runs unchanged under node / bun /
perry with a trailing `main()`.

The suite mirrors the shape of perry's `benchmarks/` — the polyglot
single-language kernels (`array-write`, `array-read`, `object-create`,
`nested-loops`, `loop-data-dependent`) and the Node/Bun compute kernels
(`factorial`, `closure`, `binary-trees`, `mandelbrot`, `matrix-multiply`,
`method-calls`) — so the same algorithms perry compares against Node/Bun are
compared against DashScript here.

## Run

```bash
# all benches, every available runtime (ds / node / bun / perry)
node examples/bench/run.mjs

# a subset
node examples/bench/run.mjs fib array-ops

# more samples, or pin specific runtimes
BENCH_SAMPLES=11 BENCH_RUNTIMES=ds,node,bun node examples/bench/run.mjs
```

The harness writes `results.json` (median + raw samples, machine-readable) and
prints a table. **Every row is gated on the stdout checksum matching across
runtimes** — a fast time from a build that returned the wrong answer is flagged
`✗ MISMATCH`, never reported as a win.

## Methodology

**What is timed.** Wall-clock per process launch, median of `BENCH_SAMPLES`
(default 5) runs.

- **ds** — `ds build` produces `dist/<name>(.exe)`; the timed process is the
  prebuilt native binary — pure native execution, no `cargo` on the hot path.
- **node** / **bun** — `node main.ts` / `bun main.ts`; the timed process
  includes VM startup (V8 / JSC init), exactly what any `node script.ts`
  invocation pays.
- **perry** — `perry compile` produces a native binary, timed the same way as
  ds. This needs `perry_runtime.lib` on the link path (`PERRY_RUNTIME_DIR` or a
  workspace install); a winget / CLI-only install fails at link, in which case
  the harness records the error and reports `—`.

**Why external wall-clock, not internal `Date.now()`.** DashScript's `main.ds`
deliberately has no `Date.now()` — the bench output is a pure checksum — so all
runtimes are measured by the same external yardstick: the time a real
`<runtime> script` invocation takes end to end. Two bench shapes coexist, both
externally timed:

- **single large call** (`fib`, `factorial`, `closure`, `mandelbrot`,
  `matrix-multiply`, `binary-trees`, `method-calls`) — `main()` runs the kernel
  once and prints the checksum; the 5 samples are 5 process launches.
- **accumulated calls** (`array-write`, `array-read`, `object-create`,
  `nested-loops`, `loop-data-dependent`, `array-ops`, `levenshtein`,
  `string-ops`, `str-concat`, `int-add`, `primes`) — `main()` runs a short
  warmup then a `TIMED` loop of the kernel, printing the accumulated checksum;
  the 5 samples are 5 process launches of that loop.

Either way the cross-runtime checksum is the load-bearing check, and the median
smooths one-off OS noise.

**What the checksum guards.** Every bench `console.log`s a single value that
depends on the full computation. If a runtime returns a different value the row
is `✗` — a perf number without correctness is worthless. (This caught the
`slice` borrow-move bug in `string-ops`, the `arr[i] = arr[j]` borrow conflict
in `array-ops`, and the `&mut self` receiver inference in `method-calls`.)

## Results

<!-- Updated by `node run.mjs` — re-run to refresh. Lower wall-clock median is better. -->

| bench               |     ds |   node |    bun | checksum       |     |
| ------------------- | -----: | -----: | -----: | -------------- | --- |
| array-ops           |  140.6 |  215.8 |  159.7 | 5000000000     | ✓   |
| array-read          |  583.6 | 1000.2 |  666.4 | 499999500000   | ✓   |
| array-write         |  646.8 | 1002.7 |  635.5 | 999999         | ✓   |
| binary-trees        |   30.6 |  122.1 |  113.7 | 1500001500000  | ✓   |
| closure             |   61.4 |  141.1 |  143.0 | 25000000000000 | ✓   |
| factorial           |  306.2 |  201.4 |  190.8 | 49950000000    | ✓   |
| fib                 |   66.4 |  179.2 |  148.9 | 9227465        | ✓   |
| int-add             |  683.6 |  762.5 |  740.8 | 49999999906710 | ✓   |
| levenshtein         |  162.0 |  139.8 |  120.7 | 600000         | ✓   |
| loop-data-dependent | 3457.3 | 1450.8 | 1440.9 | 2.550796048282 | ✓   |
| mandelbrot          |   48.0 |  125.9 |  119.2 | 8011148        | ✓   |
| matrix-multiply     |   71.9 |  156.5 |  142.7 | 0              | ✗   |
| method-calls        |   32.1 |  119.9 |  108.7 | 10000000       | ✓   |
| nested-loops        |  586.9 | 1025.7 |  728.8 | 499999500000   | ✓   |
| object-create       |  161.2 |  266.7 |  181.7 | 1499998500000  | ✓   |
| primes              |   35.2 |  140.8 |  131.9 | 78498          | ✓   |
| str-concat          |   28.5 |  121.6 |  107.2 | 100000         | ✓   |
| string-ops          |  166.5 |  166.0 |  166.6 | 129991         | ✓   |

_All times wall-clock ms per process launch, median of 5 samples. Measured
2026-07-19, Windows 11, ds 0.0.0 / node v24.18.0 / bun 1.3.6. `results.json`
holds the raw per-sample numbers. `perry` and `quickjs` are skipped here — perry
needs `perry_runtime.lib` on the link path (a workspace install or
`PERRY_RUNTIME_DIR`), and quickjs was not on PATH; set `BENCH_RUNTIMES` to add
them once available._

## Benches

| bench                   | what it tests                                                                         |
| ----------------------- | ------------------------------------------------------------------------------------- |
| **fib**                 | recursive `fib(35)` — numeric recursion, allocation-free (the classic transpiler win) |
| **factorial**           | 1e8 `sum += i % 1000` — integer accumulation, modulo, tight loop                      |
| **closure**             | 5e7 calls to `compute(x) { return x*2+1 }` — function-invocation overhead             |
| **mandelbrot**          | 800×800 Mandelbrot escape iteration — FP math, data-dependent inner loop              |
| **method-calls**        | 1e7 `counter.increment()` — `&mut self` dispatch, receiver mutation                   |
| **binary-trees**        | 1e6 `new Point3D(...)` + field sum — short-lived allocation, scalar replacement       |
| **matrix-multiply**     | 256³ ijk matmul on flat arrays — computed-index access, array write-back              |
| **int-add**             | 1e9 integer additions — raw numeric throughput, loop machinery                        |
| **primes**              | Sieve of Eratosthenes to 1e6 — `Vec` indexing, indexed assignment, nested loops       |
| **str-concat**          | 1e5 string appends — heap growth, copy-on-grow                                        |
| **levenshtein**         | Myers bit-vector edit distance — bit manipulation, tight inner loop                   |
| **array-ops**           | 100k array build / sum / in-place reverse / even-count — `Vec` ops, indexed assign    |
| **string-ops**          | 10k string build + `indexOf` scan + 1000 `slice`s — allocator, `indexOf`, `slice`     |
| **array-read**          | 1e6 sequential indexed read sum — read loop over a `__ds::array_set`-grown array      |
| **array-write**         | 1e6 indexed assignment × 2 passes — `__ds::array_set` write path (ES auto-grow)       |
| **object-create**       | 1e6 `Point { x, y }` struct build + field sum — allocator, scalar replacement         |
| **nested-loops**        | 1000×1000 indexed matrix scan — cache-bound nested read                               |
| **loop-data-dependent** | 1e7 `sum = sum*x[i&63] + x[(i*7)&63]` — sequential dependency, defeats vectorization  |

## Interpretation

- **Numeric / allocation-free (`fib`, `mandelbrot`, `method-calls`, `primes`,
  `binary-trees`, `closure`)** — `ds` leads 2.7–4×: zero-overhead native code,
  no JIT warmup, no boxing. `fib` 2.7×, `primes` 4.0×, `binary-trees` 4.0×,
  `method-calls` 3.7× faster than node. The `&mut self` receiver in
  `method-calls` is marked `let mut` by the translator (task #134); without
  that inference the bench would not compile under `ds`.
- **`factorial`** — the exception in the numeric group: `ds` is _slower_
  (306 vs node 201). The kernel is `sum += i % 1000` over 1e8 iterations; the
  `f64` modulo + loop overhead is the real cost — the emitted loop keeps `sum`
  as `f64` rather than an integer induction variable. Inferring an integer
  number flavor (i64) here is the P0 translator fix.
- **Array-read kernels (`array-read`, `nested-loops`, `object-create`)** — `ds`
  leads 1.6–1.8×: Rust's bounds-check elimination handles the sequential read
  pattern, and there's no GC pause. Arrays here are built by indexed assignment
  (`arr[i] = v` → `__ds::array_set`, ES auto-grow).
- **Array-write kernels (`array-write`)** — `ds` now matches bun (647 vs 636),
  recovered from a 1194 ms regression, after `#[inline]` on `__ds::array_set`
  plus the release profile (`opt-level = 3`, `lto = "thin"`,
  `codegen-units = 1`). Each `arr[i] = v` still routes through the helper
  (bounds check + ES auto-grow branch), but inlining lets the optimizer fold
  the grow path, closing the gap to bun's JIT-inlined store.
- **`loop-data-dependent`** — `ds` is 2.4× _slower_ (3457 vs node 1451). The
  `sum = sum*x[i&63] + x[(i*7)&63]` recurrence is a sequential dependency chain
  the optimizer cannot reorder, and the array reads keep their bounds check.
  This is the clearest perf gap in the suite and a flagged optimization target.
- **`str-concat`** — `ds` leads (28.5 vs node 122). `s = s + "x"` lowers to
  Rust `String + &str`, whose growth is amortized-O(1) doubling, _not_ naive
  per-append reallocation.
- **`string-ops`** — the three runtimes are within ~6 ms (170–176): the
  workload is dominated by `slice` reallocation and `indexOf` scanning, where
  V8/JSC and Rust are all allocator-bound. This row surfaced the `slice`
  receiver-borrow bug (fixed — the receiver is borrowed, not moved).
- **`matrix-multiply`** — `✗ MISMATCH`. The kernel writes the result back
  through a function parameter: `matmul(a, b, c)` mutates `c`, which ES passes
  by reference. DashScript lowers an array parameter to an owned `Vec<T>` and
  (because the caller reads `c` afterward) clones it at the call site, so the
  mutation is lost and the checksum reads 0. Fixing this needs parameter-
  reference analysis (a `&mut Vec<T>` parameter when the body mutates it) — a
  translator TODO, recorded honestly rather than papered over by editing the
  bench. node and bun return the correct `41079519680`.
