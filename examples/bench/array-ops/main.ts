// array-ops — identical algorithm to main.ds (see there for rationale).
// Runs under node/bun/perry as TypeScript with a trailing `main()` call.
function runArrayBenchmark(): number {
  const SIZE = 100000;
  const arr: number[] = [];
  for (let i = 0; i < SIZE; i = i + 1) {
    arr.push(i);
  }
  let sum = 0;
  for (let i = 0; i < SIZE; i = i + 1) {
    sum = sum + arr[i];
  }
  let left = 0;
  let right = SIZE - 1;
  while (left < right) {
    const temp = arr[left];
    arr[left] = arr[right];
    arr[right] = temp;
    left = left + 1;
    right = right - 1;
  }
  let evenCount = 0;
  for (let i = 0; i < SIZE; i = i + 1) {
    if (arr[i] % 2 === 0) {
      evenCount = evenCount + 1;
    }
  }
  return sum + evenCount;
}
function main(): void {
  const WARMUP = 5;
  const TIMED = 100;
  for (let i = 0; i < WARMUP; i = i + 1) {
    runArrayBenchmark();
  }
  let checksum = 0;
  for (let i = 0; i < TIMED; i = i + 1) {
    checksum = runArrayBenchmark();
  }
  console.log(checksum);
}
main();
export {};
