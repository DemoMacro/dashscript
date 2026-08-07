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

| bench               |     ds |   node |    bun |  perry |    ant | checksum       |     |
| ------------------- | -----: | -----: | -----: | -----: | -----: | -------------- | --- |
| array-ops           |   80.3 |  143.9 |  132.2 | 1923.4 | 1899.2 | 5000000000     | ✓   |
| array-read          |  324.1 |  567.9 |  674.7 | 2848.6 |    T/O | 499999500000   | ✓   |
| array-write         |  329.8 |  613.9 |  652.5 | 3223.4 |    T/O | 999999         | ✓   |
| binary-trees        |   23.5 |  100.3 |   90.4 |  119.1 |  486.3 | 1500001500000  | ✓   |
| closure             |   53.4 |  246.8 |  117.7 |  216.0 |  252.2 | 25000000000000 | ✓   |
| factorial           |   66.1 |  380.3 |  155.5 |  546.6 |  762.5 | 49950000000    | ✓   |
| fib                 |   56.7 |  155.2 |  119.4 |  123.0 |  222.3 | 9227465        | ✓   |
| int-add             |  622.8 |  946.1 |  676.3 | 2217.4 | 3770.4 | 49999999906710 | ✗   |
| levenshtein         |   54.8 |  106.5 |   90.0 | 1023.7 | 4333.1 | 600000         | ✓   |
| loop-data-dependent | 1297.0 | 1334.4 | 1331.2 |    T/O |    T/O | 2.550796048282 | ✓   |
| mandelbrot          |   36.4 |  112.0 |   96.4 |  131.9 |  201.4 | 8011148        | ✓   |
| matrix-multiply     |   56.7 |  114.6 |  108.6 | 1762.9 |  547.4 | 41079519680    | ✓   |
| method-calls        |   29.4 |   99.7 |   89.8 | 2688.9 |  801.5 | 10000000       | ✓   |
| nested-loops        |  431.4 |  597.4 |  740.9 | 9112.2 |    T/O | 499999500000   | ✓   |
| object-create       |  146.5 |  220.1 |  173.1 | 1150.3 | 9288.5 | 1499998500000  | ✓   |
| primes              |   24.3 |  114.1 |   90.8 |  290.2 |  344.2 | 78498          | ✓   |
| str-concat          |   21.0 |   95.3 |   78.2 |  112.6 |   66.4 | 100000         | ✓   |
| string-ops          |   65.4 |  132.8 |  134.7 |  206.5 |  734.6 | 129991         | ✓   |

_All times wall-clock ms per process launch, median of 7 samples. Measured
2026-08-07 (clean machine, no background load), Windows 11, ds 0.0.0 /
node v26.5.0 / bun 1.3.6 / perry 0.5.1220 / ant 12.3.
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
allocator-heavy kernels (`array-read` / `array-write` / `nested-loops` `T/O`,
`object-create` 9 s) — interpreter dispatch and GC dominate there; it is
uncompetitive on anything numeric or allocation-bound, and only approaches the
pack on `str-concat` (66 ms, second to `ds`)._

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
  `method-calls`, `primes`, `binary-trees`, `closure`)** — `ds` leads 2.1–3.8×:
  zero-overhead native code, no JIT warmup, no boxing. `factorial` joins this
  group after number-flavor inference (Phase 1) promoted its counter and
  accumulator to `i64` — `sum += i % 1000` is now pure integer arithmetic (no
  `f64` modulo); the sum stays under 2⁵³, so `i64` matches ES `f64` exactly.
- **`loop-data-dependent`** — `ds` leads (1297 vs node 1334, vs bun 1331). The
  bitwise **index** `x[i & 63]` emits its masked result straight to `usize`
  (not via `f64`), which both saves a conversion per access and keeps the
  `& 63` range visible to LLVM so the `Vec` bounds check is elided (V8 elides
  it too). The LCG seed multiplies as `f64`, not `i64`: `seed * 1103515245`
  reaches ~2.4e18, past 2⁵³ where an exact `i64` product would diverge from the
  rounded ES `number` result. The `sum = sum*x[i&63] + …` recurrence stays a
  sequential hazard either way.
- **`levenshtein`** — `ds` leads ~1.6× (55 vs node 107, vs bun 90). The Myers
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
  `array-ops`)** — `ds` leads 1.2–2× across reads and writes:
  Rust's bounds-check elimination handles the sequential pattern, and the
  indexed-assignment fast path (`__ds::array_set_index` for integer indices,
  `#[inline]`) lets the optimizer fold the ES auto-grow path.
- **`matrix-multiply`** — `ds` leads (57 vs node 115). The kernel writes its
  result through a `&mut Vec` reference parameter (`matmul(a, b, &mut c)`), so
  the caller sees the mutation with no clone — ES reference semantics lowered
  correctly.
- **`str-concat`** — `ds` leads (21 vs node 95). `s = s + "x"` lowers to Rust
  `String + &str`, whose growth is amortized-O(1) doubling.
- **`string-ops`** — `ds` leads ~2× (65 vs node 133, vs bun 135). The workload
  is dominated by `slice` reallocation, where V8/JSC and Rust are all
  allocator-bound.
