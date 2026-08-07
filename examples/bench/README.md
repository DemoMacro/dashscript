# DashScript benchmarks

Microbenchmarks comparing **DashScript** (`ds`, TypeScript → native Rust) against
**node** (V8), **bun** (JSC), **perry** (also TypeScript → native), and **ant**
(a JavaScript runtime), all running the identical TypeScript source. Each bench
is one algorithm written once: `main.ts` is DashScript's entry (it lowers to
Rust `fn main`); the same file runs unchanged under node / bun / ant with a
trailing `main()`, and `perry compile` lowers it to native.

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

# ant is enabled by pointing ANT_BIN at its binary (the harness hardcodes no
# install path); unset ANT_BIN to skip it
ANT_BIN=/path/to/ant node examples/bench/run.mjs
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
- **ant** — `ant main.ts`; a JavaScript runtime, timed like node/bun (process
  launch incl. runtime startup). It is not assumed to be on `PATH` — the
  harness reads its location from `ANT_BIN` and skips it when unset.

`main.ts` deliberately has no `Date.now()` — the bench output is a pure
checksum — so all runtimes are measured by the same external yardstick: the
time a real `<runtime> script` invocation takes end to end. Every bench
`console.log`s a single value that depends on the full computation; a runtime
that returns a different value flags the row `✗`, because a perf number
without correctness is worthless.

## Results

<!-- Updated by `node run.mjs` — re-run to refresh. Lower wall-clock median is better. -->

| bench               |     ds |   node |    bun |  perry |     ant | checksum       |     |
| ------------------- | -----: | -----: | -----: | -----: | ------: | -------------- | --- |
| array-ops           |   86.8 |  159.6 |  158.0 | 2146.1 |  2087.1 | 5000000000     | ✓   |
| array-read          |  447.5 |  646.9 |  699.2 | 3133.9 | 15900.6 | 499999500000   | ✓   |
| array-write         |  503.6 |  713.6 |  696.2 | 3498.6 | 26031.6 | 999999         | ✓   |
| binary-trees        |   29.9 |  122.7 |  127.9 |  132.9 |   501.0 | 1500001500000  | ✓   |
| closure             |   62.3 |  274.3 |  146.9 |  261.9 |   280.5 | 25000000000000 | ✓   |
| factorial           |   82.9 |  425.8 |  186.5 |  601.7 |   832.5 | 49950000000    | ✓   |
| fib                 |   80.6 |  195.6 |  159.6 |  150.1 |   245.7 | 9227465        | ✓   |
| int-add             |  675.8 | 1045.2 |  751.5 | 2386.2 |  4054.6 | 49999999906710 | ✗   |
| levenshtein         |   59.9 |  129.1 |  122.9 | 1135.6 |  5073.0 | 600000         | ✓   |
| loop-data-dependent | 1407.7 | 1484.1 | 1478.3 |    T/O |     T/O | 2.550796048282 | ✓   |
| mandelbrot          |   42.1 |  131.0 |  131.1 |  147.4 |   215.1 | 8011148        | ✓   |
| matrix-multiply     |   85.0 |  139.9 |  137.4 | 2085.7 |   620.3 | 41079519680    | ✓   |
| method-calls        |   36.2 |  120.9 |  120.1 | 2826.1 |   844.5 | 10000000       | ✓   |
| nested-loops        |  463.6 |  683.0 |  727.8 | 7373.2 | 16677.3 | 499999500000   | ✓   |
| object-create       |  162.1 |  252.6 |  193.2 | 1262.8 |  8917.3 | 1499998500000  | ✓   |
| primes              |   41.8 |  179.1 |  121.8 |  312.1 |   375.4 | 78498          | ✓   |
| str-concat          |   27.4 |  119.2 |  104.2 |  127.6 |    75.6 | 100000         | ✓   |
| string-ops          |   83.4 |  177.3 |  181.6 |  236.1 |   839.7 | 129991         | ✓   |

_All times wall-clock ms per process launch, median of 5 samples. Measured
2026-07-31, Windows 11, ds 0.0.0 / node v26.5.0 / bun 1.3.6 / perry 0.5.1220 /
ant 12.3; `levenshtein` and `loop-data-dependent` re-measured 2026-08-07 (9
samples) after the bit-vector `i64` / `.length` `i64` / multiplication-`f64`
flavor changes; `string-ops` and `array-ops` re-measured 2026-08-07 (11
samples).
`results.json` holds the raw per-sample numbers. A runtime slower than
`ds_median + 10s` per sample is killed and shown as `T/O`._

_The single `✗` is **`int-add`**, and it is perry's deviation, not
DashScript's: the 1e9-iteration sum reaches ~5×10¹⁷, past 2⁵³ where f64 loses
integer precision. `ds` / `node` / `bun` / `ant` all return the ES-correct
`499999999067109000` (f64); `perry` computes the sum as **i64** and returns
`499999999067108992`. DashScript matches node/bun/ant — the row is flagged only
because the cross-runtime checksum gate refuses to silently endorse a
divergence. (`int-add` annotates the accumulator `let sum: number = 0` so
DashScript's number-flavor inference keeps it `f64`; without the annotation
Phase 1 would promote it to `i64` and return the exact — but non-ES, since an
ES `number` is `f64` — `499999999500000000`.)_

_`perry` and `ant` on `loop-data-dependent` are `T/O`: perry's optimizer cannot
fold the f64 recurrence, and ant's interpreter cannot finish 1e7 dependent
iterations within the `ds_median + 10s` ceiling. `ant` is also slow on the
allocator-heavy kernels (`array-write` 26 s, `nested-loops` 17 s, `object-create`
9 s) — interpreter dispatch and GC dominate there; it is uncompetitive on
anything numeric or allocation-bound, and only approaches the pack on
`str-concat` (76 ms, second to `ds`)._

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
- **`loop-data-dependent`** — `ds` leads (1408 vs node 1484, vs bun 1478). The
  bitwise **index** `x[i & 63]` emits its masked result straight to `usize`
  (not via `f64`), which both saves a conversion per access and keeps the
  `& 63` range visible to LLVM so the `Vec` bounds check is elided (V8 elides
  it too). The LCG seed multiplies as `f64`, not `i64`: `seed * 1103515245`
  reaches ~2.4e18, past 2⁵³ where an exact `i64` product would diverge from the
  rounded ES `number` result. The `sum = sum*x[i&63] + …` recurrence stays a
  sequential hazard either way.
- **`levenshtein`** — `ds` leads ~2.2× (60 vs node 129, vs bun 123). The Myers
  bit-vector inner loop keeps its accumulators (`pv`/`mv`/`eq`) and the string
  lengths (`n`/`m` from `a.length`/`b.length`) in `i64`: each bitwise op yields
  a `ToInt32` result sign-extended to `i64`, and `.length` is a non-negative
  integer < 2⁵³, so the bit vectors and the score/loop counters no longer
  round-trip through `f64`. That cut the runtime 162 ms → 78 ms (bit-vector
  `i64`) → 60 ms (`.length` `i64`) — LLVM does not fold the inner-loop
  `f64`↔`i32` cast chain on its own. The values stay under 2³¹, so `i64` matches
  ES `number` exactly; `*`, which can overshoot 2⁵³, stays `f64` (see
  `loop-data-dependent`).
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
- **`string-ops`** — `ds` leads ~2.1× (83 vs node 177, vs bun 182). The workload
  is dominated by `slice` reallocation, where V8/JSC and Rust are all
  allocator-bound.
