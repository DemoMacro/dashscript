// closure — function-invocation overhead: a `compute(x) { return x*2+1 }`
// free function called 50M times in a tight accumulation loop, single call.
// Despite the name, this is a direct call benchmark (no captured env). Mirrors
// perry's suite `14_closure`. The same source runs under node/bun as TypeScript.
function compute(x: number): number {
  return x * 2 + 1;
}
function runClosureBenchmark(): number {
  const ITERATIONS = 50000000;
  let sum = 0;
  for (let i = 0; i < ITERATIONS; i = i + 1) {
    sum = sum + compute(i);
  }
  return sum;
}
function main(): void {
  console.log(runClosureBenchmark());
}

main();
export {};
