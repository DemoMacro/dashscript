// levenshtein — Myers bit-vector edit distance, ported from
// ka-weihe/fastest-levenshtein (https://github.com/ka-weihe/fastest-levenshtein).
// The original keeps a module-global `Uint32Array` peck table across calls and
// ships a multi-block `myers_x` path for strings > 32 chars. DashScript's
// module scope allows only function/type declarations (no top-level let/const),
// has no TypedArray yet, and moves array arguments by value — so the peck
// table is a `number[]` allocated once in `main`, the bit-vector core is
// inlined into the hot loop to reuse it (cleared per call, as the original),
// and only the single-block `myers_32` path (strings ≤ 32) is ported. The
// bit-manipulation itself is byte-for-byte identical. The same source runs
// under node/bun as TypeScript — a fair, same-algorithm comparison.
function main(): void {
  const a = "abcdefghijklmnopqrstuvwxyzaabbc";
  const b = "abcdefghijklmnopqrstuvwxzz12345";
  const n = a.length;
  const m = b.length;
  const nm1 = n - 1;
  const lst = 1 << nm1;
  let peq: number[] = [];
  for (let i = 0; i < 128; i = i + 1) {
    peq.push(0);
  }
  const loops = 1e5;
  let total = 0;
  for (let iter = 0; iter < loops; iter = iter + 1) {
    let pv = -1;
    let mv = 0;
    let sc = n;
    for (let i = n - 1; i >= 0; i = i - 1) {
      const c = a.charCodeAt(i);
      peq[c] = peq[c] | (1 << i);
    }
    for (let i = 0; i < m; i = i + 1) {
      let eq = peq[b.charCodeAt(i)];
      const xv = eq | mv;
      const eqpv = eq & pv;
      const sum = eqpv + pv;
      eq = eq | (sum ^ pv);
      mv = mv | ~(eq | pv);
      pv = pv & eq;
      if ((mv & lst) !== 0) {
        sc = sc + 1;
      }
      if ((pv & lst) !== 0) {
        sc = sc - 1;
      }
      mv = (mv << 1) | 1;
      pv = (pv << 1) | ~(xv | mv);
      mv = mv & xv;
    }
    for (let i = n - 1; i >= 0; i = i - 1) {
      peq[a.charCodeAt(i)] = 0;
    }
    total = total + sc;
  }
  console.log(total);
}
main();
export {};
