// operators — arithmetic, comparison, logical, bitwise, ternary, typeof.
// Mirrors crates/dashscript/src/translator/expressions/{binary,logical,unary}.rs.
function main(): void {
  console.log("arith:", 1 + 2 * 3);
  const cmpA = 4;
  const cmpB = 2;
  console.log("compare:", cmpA >= cmpB, cmpB < 5);
  // `&&` / `!` on comparison results (not on bare literals — a constant
  // `true && false` trips oxlint's no-constant-binary-expression).
  console.log("logical:", cmpA >= cmpB && cmpB > 0, !(cmpA >= cmpB));
  // Bitwise `&`/`|`/`^` operate on 32-bit ints (`as i32`), like JS.
  console.log("bitwise:", 6 & 3, 6 | 1, 6 ^ 3);
  console.log("shift:", 1 << 3, 256 >> 2);
  console.log("bitnot:", ~0);
  // A ternary → an `if` expression.
  const mag = 7;
  console.log(mag > 5 ? "large" : "small");
  // A template literal → `format!`.
  const name = "ada";
  console.log(`Hello, ${name}!`);
  // `typeof` is a compile-time type query (DashScript is statically typed).
  console.log(typeof 1, typeof "x", typeof true, typeof null);
}

main();
export {};
