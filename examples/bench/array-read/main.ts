// array-read — sequential indexed read summing an array built by indexed
// assignment (`arr[i] = i`, ES auto-grow → `__ds::array_set`). The measured
// path is the read loop `sum += arr[i]`. 1M elements × 100 calls. Mirrors
// perry's `array_read`. The same source runs under node/bun as TypeScript.
function runArrayReadBenchmark(): number {
  const SIZE = 1000000;
  let arr: number[] = [];
  for (let i = 0; i < SIZE; i = i + 1) {
    arr[i] = i;
  }
  let sum = 0;
  for (let i = 0; i < SIZE; i = i + 1) {
    sum = sum + arr[i];
  }
  return sum;
}
function main(): void {
  const WARMUP = 5;
  const TIMED = 100;
  for (let i = 0; i < WARMUP; i = i + 1) {
    runArrayReadBenchmark();
  }
  let checksum = 0;
  for (let i = 0; i < TIMED; i = i + 1) {
    checksum = runArrayReadBenchmark();
  }
  console.log(checksum);
}

main();
export {};
