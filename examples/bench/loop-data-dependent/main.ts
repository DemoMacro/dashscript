// loop-data-dependent — a genuinely-non-foldable f64 loop:
// `sum = sum * x[i & 63] + x[(i * 7) & 63]` over 10M iterations. The sequential
// dependency on `sum` defeats reassoc + the vectorizer; the array reads defeat
// constant propagation. Bit-ops (`&`) and a linear-congruential seed fill the
// 64-entry `x` table (every value in [0.5, 1.0) so the chain contracts to a
// fixed point, not overflow). Mirrors perry's polyglot `loop_data_dependent`
// (100M once → here 10M × 100, fit to DashScript's multi-call shape). The same
// source runs under node/bun as TypeScript.
function runLoopDataDependentBenchmark(): number {
  const N = 64;
  const ITERATIONS = 10000000;
  let seed = 42;
  let x: number[] = [];
  for (let i = 0; i < N; i = i + 1) {
    seed = (seed * 1103515245 + 12345) & 0x7fffffff;
    x[i] = 0.5 + (seed / 2147483647.0) * 0.5;
  }
  let sum = 1.0;
  for (let i = 0; i < ITERATIONS; i = i + 1) {
    sum = sum * x[i & (N - 1)] + x[(i * 7) & (N - 1)];
  }
  return sum;
}
function main(): void {
  const WARMUP = 5;
  const TIMED = 100;
  for (let i = 0; i < WARMUP; i = i + 1) {
    runLoopDataDependentBenchmark();
  }
  let checksum = 0;
  for (let i = 0; i < TIMED; i = i + 1) {
    checksum = runLoopDataDependentBenchmark();
  }
  console.log(checksum);
}

main();
export {};
