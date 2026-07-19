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
| array-ops           |  114.5 |  229.5 |  172.2 | 2053.1 | 5000000000     | ✓   |
| array-read          |  502.6 | 1140.2 |  740.6 | 3045.3 | 499999500000   | ✓   |
| array-write         |  533.2 | 1105.1 |  721.1 | 4172.0 | 999999         | ✓   |
| binary-trees        |   32.8 |  130.6 |  125.5 |  139.9 | 1500001500000  | ✓   |
| closure             |   63.0 |  153.1 |  140.6 |  242.7 | 25000000000000 | ✓   |
| factorial           |   72.8 |  216.8 |  201.5 |  590.8 | 49950000000    | ✓   |
| fib                 |   67.9 |  211.3 |  182.5 |  148.6 | 9227465        | ✓   |
| int-add             |  657.0 |  757.4 |  736.2 | 2317.8 | 49999999906710 | ✗   |
| levenshtein         |  153.4 |  151.7 |  118.1 | 1191.0 | 600000         | ✓   |
| loop-data-dependent | 2264.8 | 1512.0 | 1446.0 |    T/O | 2.550796048282 | ✓   |
| mandelbrot          |   41.4 |  162.2 |  118.0 |  140.5 | 8011148        | ✓   |
| matrix-multiply     |   67.9 |  154.9 |  139.3 | 1949.2 | 41079519680    | ✓   |
| method-calls        |   35.9 |  137.4 |  120.3 | 2715.0 | 10000000       | ✓   |
| nested-loops        |  514.7 | 1168.3 |  754.9 | 7401.9 | 499999500000   | ✓   |
| object-create       |  152.2 |  255.6 |  177.2 | 1166.0 | 1499998500000  | ✓   |
| primes              |   32.1 |  125.2 |  122.1 |  311.3 | 78498          | ✓   |
| str-concat          |   25.0 |  119.3 |  100.5 |  130.4 | 100000         | ✓   |
| string-ops          |  155.7 |  157.9 |  176.0 |  241.0 | 129991         | ✓   |

_All times wall-clock ms per process launch, median of 5 samples. Measured
2026-07-20, Windows 11, ds 0.0.0 / node v24.18.0 / bun 1.3.6 / perry (native).
`results.json` holds the raw per-sample numbers._

_The single `✗` is **`int-add`**, and it is perry's deviation, not
DashScript's: the 1e9-iteration sum reaches ~5×10¹⁷, past 2⁵³ where f64 loses
integer precision. `ds` / `node` / `bun` all return the ES-correct
`499999999067109000` (f64); `perry` computes the sum as **i64** and returns
`499999999067108992`. DashScript matches node/bun — the row is flagged only
because the cross-runtime checksum gate refuses to silently endorse a
divergence. (`int-add` annotates the accumulator `let sum: number = 0` so
DashScript's number-flavor inference keeps it `f64`; without the annotation
Phase 1 would promote it to `i64` and return the exact — but non-ES, since an
ES `number` is `f64` — `499999999500000000`.)_

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

- **Numeric / allocation-free (`fib`, `factorial`, `mandelbrot`,
  `method-calls`, `primes`, `binary-trees`, `closure`)** — `ds` leads 2.4–4.4×:
  zero-overhead native code, no JIT warmup, no boxing. `factorial` joins this
  group after number-flavor inference (Phase 1) promoted its counter and
  accumulator to `i64` — `sum += i % 1000` is now pure integer arithmetic (no
  `f64` modulo); the sum stays under 2⁵³, so `i64` matches ES `f64` exactly.
- **`loop-data-dependent`** — `ds` is 1.5× slower (2265 vs node 1512), down
  from 2.3× before Phase 1 promoted the loop counter to `i64` (eliminating the
  per-iteration `f64→i64→i32→f64→usize` cast chain). The remaining gap is the
  `sum = sum*x[i&63] + x[(i*7)&63]` recurrence — a sequential dependency chain
  the optimizer cannot reorder (a fundamental data hazard), not counter flavor.
- **`levenshtein`** — `ds` now matches node (153 vs 152). The Myers bit-vector
  inner loop is dominated by the `as i64 as i32` cast chain each bitwise op
  emits (JS `ToInt32` wrap), which a future Phase 2 may shorten by promoting
  the bit vectors to `i64`.
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
