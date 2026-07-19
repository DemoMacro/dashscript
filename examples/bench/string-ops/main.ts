// string-ops — identical algorithm to main.ds (see there for rationale).
// Runs under node/bun/perry as TypeScript with a trailing `main()` call.
function runStringBenchmark(): number {
  const SIZE = 10000;
  let str = "";
  for (let i = 0; i < SIZE; i = i + 1) {
    str = str + "a";
  }
  let foundCount = 0;
  const patterns: string[] = ["aaa", "aaaa", "aaaaa"];
  for (let p = 0; p < patterns.length; p = p + 1) {
    let idx = 0;
    while (idx < str.length) {
      const found = str.indexOf(patterns[p], idx);
      if (found === -1) {
        break;
      }
      foundCount = foundCount + 1;
      idx = found + 1;
    }
  }
  let sliceSum = 0;
  for (let i = 0; i < 1000; i = i + 1) {
    const start = i % (SIZE - 100);
    const slice = str.slice(start, start + 100);
    sliceSum = sliceSum + slice.length;
  }
  return foundCount + sliceSum;
}
function main(): void {
  const WARMUP = 5;
  const TIMED = 100;
  for (let i = 0; i < WARMUP; i = i + 1) {
    runStringBenchmark();
  }
  let checksum = 0;
  for (let i = 0; i < TIMED; i = i + 1) {
    checksum = runStringBenchmark();
  }
  console.log(checksum);
}
main();
export {};
