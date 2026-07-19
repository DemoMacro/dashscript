// binary-trees — short-lived object allocation: 1M `Point3D` instances built
// and field-summed per call, single call. Measures the allocator + field-load
// path; the optimizer can scalar-replace an object that never escapes. Mirrors
// perry's suite `12_binary_trees` (named for the allocation pattern, not the
// classic binary-trees GC bench). The same source runs under node/bun as TS.
class Point3D {
  x: number;
  y: number;
  z: number;
  constructor(x: number, y: number, z: number) {
    this.x = x;
    this.y = y;
    this.z = z;
  }
}
function runBinaryTrees(): number {
  const ITERATIONS = 1000000;
  let sum = 0;
  for (let i = 0; i < ITERATIONS; i = i + 1) {
    const p = new Point3D(i, i + 1, i + 2);
    sum = sum + p.x + p.y + p.z;
  }
  return sum;
}
function main(): void {
  console.log(runBinaryTrees());
}

main();
export {};
