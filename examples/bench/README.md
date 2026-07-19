# DashScript benchmarks

Microbenchmarks comparing **DashScript** (`ds`, TypeScript → native Rust) against
**node** (V8), **bun** (JSC), and **perry** (also TypeScript → native), all
running the identical TypeScript source. Each bench is one algorithm written
once: `main.ds` is DashScript's entry (it lowers to Rust `fn main`), `main.ts`
runs unchanged under node / bun / perry with a trailing `main()`.

The kernel selection mirrors perry's `benchmarks/` — the polyglot single-
language kernels and the Node/Bun compute kernels — so the same algorithms
perry compares against Node/Bun are compared against DashScript here.

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
`✗ MISMATCH`, never reported as a win. A runtime slower than `ds_median + 30s`
per sample is killed and marked `T/O`, so one pathologically slow runtime
cannot block the suite.

## Methodology

**What is timed.** Wall-clock per process launch, median of `BENCH_SAMPLES`
(default 5) runs.

- **ds** — `ds build` produces `dist/<name>(.exe)`; the timed process is the
  prebuilt native binary — pure native execution, no `cargo` on the hot path.
- **node** / **bun** — `node main.ts` / `bun main.ts`; the timed process
  includes VM startup (V8 / JSC init), exactly what any `node script.ts`
  invocation pays.
- **perry** — `perry compile` produces a native binary, timed the same way as
  ds.

`main.ds` deliberately has no `Date.now()` — the bench output is a pure
checksum — so all runtimes are measured by the same external yardstick: the
time a real `<runtime> script` invocation takes end to end. Every bench
`console.log`s a single value that depends on the full computation; a runtime
that returns a different value flags the row `✗`, because a perf number
without correctness is worthless.

## Results

<!-- Updated by `node run.mjs` — re-run to refresh. Lower wall-clock median is better. -->

| bench               |     ds |   node |    bun |  perry | checksum       |     |
| ------------------- | -----: | -----: | -----: | -----: | -------------- | --- |
| array-ops           |  140.8 |  212.2 |  167.9 | 2147.3 | 5000000000     | ✓   |
| array-read          |  652.5 | 1148.9 |  749.6 | 3028.8 | 499999500000   | ✓   |
| array-write         |  757.0 | 1130.7 |  710.6 | 3564.4 | 999999         | ✓   |
| binary-trees        |   29.5 |  119.1 |  116.9 |  145.7 | 1500001500000  | ✓   |
| closure             |   64.1 |  154.0 |  146.5 |  245.2 | 25000000000000 | ✓   |
| factorial           |  313.0 |  209.1 |  187.2 |  591.3 | 49950000000    | ✓   |
| fib                 |   69.4 |  190.2 |  147.4 |  139.4 | 9227465        | ✓   |
| int-add             |  689.9 |  771.4 |  744.4 | 2333.8 | 49999999906710 | ✗   |
| levenshtein         |  164.1 |  142.5 |  118.8 | 1127.7 | 600000         | ✓   |
| loop-data-dependent | 3347.3 | 1471.7 | 1457.3 |    T/O | 2.550796048282 | ✓   |
| mandelbrot          |   43.4 |  144.7 |  127.1 |  143.3 | 8011148        | ✓   |
| matrix-multiply     |   71.7 |  148.9 |  143.5 | 2136.8 | 41079519680    | ✓   |
| method-calls        |   31.5 |  138.4 |  109.2 | 3004.1 | 10000000       | ✓   |
| nested-loops        |  690.3 | 1197.5 |  790.0 | 7252.0 | 499999500000   | ✓   |
| object-create       |  155.6 |  253.0 |  190.9 | 1126.2 | 1499998500000  | ✓   |
| primes              |   35.9 |  116.5 |  116.0 |  314.7 | 78498          | ✓   |
| str-concat          |   21.9 |  115.7 |  102.7 |  132.1 | 100000         | ✓   |
| string-ops          |  151.6 |  160.5 |  159.9 |  235.2 | 129991         | ✓   |

_All times wall-clock ms per process launch, median of 5 samples. Measured
2026-07-19, Windows 11, ds 0.0.0 / node v24.18.0 / bun 1.3.6 / perry (native).
`results.json` holds the raw per-sample numbers._

_The single `✗` is **`int-add`**, and it is perry's deviation, not
DashScript's: the 1e9-iteration sum reaches ~5×10¹⁷, past 2⁵³ where f64 loses
integer precision. `ds` / `node` / `bun` all return the ES-correct
`499999999067109000` (f64); `perry` computes the sum as **i64** and returns
`499999999067108992`. DashScript matches node/bun — the row is flagged only
because the cross-runtime checksum gate refuses to silently endorse a
divergence._

_`perry` on `loop-data-dependent` is `T/O`: its optimizer cannot fold the f64
recurrence, and a single sample runs past the `ds_median + 30s` ceiling._

## Benches

| bench                   | what it tests                                                                         |
| ----------------------- | ------------------------------------------------------------------------------------- |
| **fib**                 | recursive `fib(35)` — numeric recursion, allocation-free (the classic transpiler win) |
| **factorial**           | 1e8 `sum += i % 1000` — integer accumulation, modulo, tight loop                      |
| **closure**             | 5e7 calls to `compute(x) { return x*2+1 }` — function-invocation overhead             |
| **mandelbrot**          | 800×800 Mandelbrot escape iteration — FP math, data-dependent inner loop              |
| **method-calls**        | 1e7 `counter.increment()` — `&mut self` dispatch, receiver mutation                   |
| **binary-trees**        | 1e6 `new Point3D(...)` + field sum — short-lived allocation, scalar replacement       |
| **matrix-multiply**     | 256³ ijk matmul on flat arrays — computed-index access, write-back via `&mut` param   |
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
  `binary-trees`, `closure`)** — `ds` leads 2.4–4.4×: zero-overhead native
  code, no JIT warmup, no boxing.
- **`factorial`** — the exception in the numeric group: `ds` is _slower_
  (313 vs node 209). The kernel is `sum += i % 1000` over 1e8 iterations; the
  emitted loop keeps `sum` as `f64` rather than an integer induction variable.
  Inferring an integer number flavor (i64) is the P0 translator fix.
- **`loop-data-dependent`** — `ds` is 2.3× _slower_ (3347 vs node 1472). The
  `sum = sum*x[i&63] + x[(i*7)&63]` recurrence is a sequential dependency chain
  the optimizer cannot reorder, and the loop counter is emitted as `f64`. The
  clearest perf gap in the suite — same number-flavor fix as `factorial`.
- **Array kernels (`array-read`, `array-write`, `nested-loops`, `object-create`,
  `array-ops`)** — `ds` leads 1.5–1.8× on reads and matches bun on writes:
  Rust's bounds-check elimination handles the sequential pattern, and
  `__ds::array_set` is `#[inline]`, so the optimizer folds the ES auto-grow
  path.
- **`matrix-multiply`** — `ds` leads (72 vs node 149). The kernel writes its
  result through a `&mut Vec` reference parameter (`matmul(a, b, &mut c)`), so
  the caller sees the mutation with no clone — ES reference semantics lowered
  correctly.
- **`str-concat`** — `ds` leads (22 vs node 116). `s = s + "x"` lowers to Rust
  `String + &str`, whose growth is amortized-O(1) doubling.
- **`string-ops`** — the three runtimes are within ~10 ms (152–161): the
  workload is dominated by `slice` reallocation and `indexOf` scanning, where
  V8/JSC and Rust are all allocator-bound.
