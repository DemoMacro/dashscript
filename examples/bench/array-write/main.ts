// array-write — sequential indexed assignment to an array built by indexed
// assignment itself (`arr[i] = …`), which ES auto-grows. DashScript lowers
// `arr[i] = v` to `__ds::array_set` (append/grow), matching node/bun rather
// than panicking like a bare Rust `Vec[i] = v`. Two passes per call: fill with
// zeros, then overwrite with the index — 1M × 100 calls. Mirrors perry's
// `array_write` (10M once → here 1M × 100, fit to DashScript's multi-call
// bench shape). The same source runs under node/bun as TypeScript.
function runArrayWriteBenchmark(): number {
  const SIZE = 1000000;
  let arr: number[] = [];
  for (let i = 0; i < SIZE; i = i + 1) {
    arr[i] = 0;
  }
  for (let i = 0; i < SIZE; i = i + 1) {
    arr[i] = i;
  }
  return arr[SIZE - 1];
}
function main(): void {
  const WARMUP = 5;
  const TIMED = 100;
  for (let i = 0; i < WARMUP; i = i + 1) {
    runArrayWriteBenchmark();
  }
  let checksum = 0;
  for (let i = 0; i < TIMED; i = i + 1) {
    checksum = runArrayWriteBenchmark();
  }
  console.log(checksum);
}

main();
export {};
