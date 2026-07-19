// int-add — tight arithmetic loop, 1e9 integer additions. Allocation-free,
// numeric-only: a direct measure of raw numeric throughput and loop
// machinery (no GC, no allocation, no boxing). The same source runs under
// node/bun as TypeScript.
function main(): void {
  const N = 1e9;
  let sum: number = 0;
  for (let i = 0; i < N; i = i + 1) {
    sum = sum + i;
  }
  console.log(sum);
}
main();
export {};
