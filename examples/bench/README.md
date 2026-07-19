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
| array-ops           |  168.8 |  218.7 |  169.8 | 5000000000     | ✓   |
| array-read          |  608.4 | 1106.6 |  690.0 | 499999500000   | ✓   |
| array-write         | 1193.6 | 1048.8 |  706.9 | 999999         | ✓   |
| binary-trees        |   30.0 |  124.6 |  120.1 | 1500001500000  | ✓   |
| closure             |   62.5 |  152.5 |  155.8 | 25000000000000 | ✓   |
| factorial           |  321.9 |  206.0 |  197.6 | 49950000000    | ✓   |
| fib                 |   62.3 |  194.1 |  157.3 | 9227465        | ✓   |
| int-add             |  691.6 |  776.1 |  755.9 | 49999999906710 | ✓   |
| levenshtein         |  159.6 |  138.9 |  123.0 | 600000         | ✓   |
| loop-data-dependent | 3466.8 | 1478.0 | 1466.2 | 2.550796048282 | ✓   |
| mandelbrot          |   45.9 |  134.2 |  126.2 | 8011148        | ✓   |
| matrix-multiply     |   69.6 |  154.4 |  137.0 | 0              | ✗   |
| method-calls        |   34.1 |  127.3 |  114.4 | 10000000       | ✓   |
| nested-loops        |  627.3 | 1074.7 |  708.2 | 499999500000   | ✓   |
| object-create       |  165.0 |  269.9 |  198.7 | 1499998500000  | ✓   |
| primes              |   36.2 |  139.7 |  136.0 | 78498          | ✓   |
| str-concat          |   31.5 |  121.3 |  145.1 | 100000         | ✓   |
| string-ops          |  176.2 |  170.7 |  173.2 | 129991         | ✓   |

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
  `binary-trees`, `closure`)** — `ds` leads 2.4–4×: zero-overhead native code,
  no JIT warmup, no boxing. `fib` 3.1×, `primes` 3.8×, `binary-trees` 4.0×,
  `method-calls` 3.7× faster than node. The `&mut self` receiver in
  `method-calls` is marked `let mut` by the translator (task #134); without
  that inference the bench would not compile under `ds`.
- **`factorial`** — the exception in the numeric group: `ds` is _slower_
  (322 vs node 206). The kernel is `sum += i % 1000` over 1e8 iterations; the
  `f64` modulo + loop overhead is a real optimization target — DashScript's
  emitted loop has not yet had the index/induction-variable cleanup V8 applies.
- **Array-read kernels (`array-read`, `nested-loops`, `object-create`)** — `ds`
  leads 1.6–1.8×: Rust's bounds-check elimination handles the sequential read
  pattern, and there's no GC pause. Arrays here are built by indexed assignment
  (`arr[i] = v` → `__ds::array_set`, ES auto-grow).
- **Array-write kernels (`array-write`)** — `ds` trails bun (1194 vs 707). Each
  `arr[i] = v` routes through the `__ds::array_set` helper (bounds check + grow
  branch) rather than an inlined store; over 1e6 × 2 × 100 calls that helper
  call dominates. bun's JIT inlines the grow. An indexed-store fast path when
  the index is already in range is the obvious win.
- **`loop-data-dependent`** — `ds` is 2.3× _slower_ (3467 vs node 1478). The
  `sum = sum*x[i&63] + x[(i*7)&63]` recurrence is a sequential dependency chain
  the optimizer cannot reorder, and the array reads keep their bounds check.
  This is the clearest perf gap in the suite and a flagged optimization target.
- **`str-concat`** — `ds` leads (31.5 vs node 121). `s = s + "x"` lowers to
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
