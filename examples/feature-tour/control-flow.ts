// control-flow — if, while, for, for-of, switch, break/continue.
// Mirrors crates/dashscript/src/translator/functions/{control_flow,switch}.rs.
type Status = "idle" | "running" | "done";
// `switch` over a string-literal union (a union → Rust `enum` + `match`). It
// takes a `Status` parameter so the switch sees the full union — a `let
// status: Status = "running"` in `main` would narrow to the literal "running"
// and make the other cases incomparable (TS2678). Each arm `return`s (no
// `break`), and Rust's `match` enforces exhaustiveness at compile time, so the
// TS `never`-type exhaustiveness trick is unnecessary here: a missing arm is a
// cargo error, not a silent miss.
function statusLabel(status: Status): string {
  switch (status) {
    case "idle":
      return "waiting";
    case "running":
      return "in progress";
    case "done":
      return "complete";
  }
}
function main(): void {
  // if / else if / else
  const score = 85;
  let grade = "fail";
  if (score >= 90) {
    grade = "A";
  } else if (score >= 80) {
    grade = "B";
  } else {
    grade = "C";
  }
  console.log("grade:", grade);
  // while + break (a `let` mutated in the loop → `let mut`)
  let i = 0;
  let sum = 0;
  while (i < 10) {
    if (i === 5) break;
    sum = sum + i;
    i = i + 1;
  }
  console.log("while sum:", sum);
  // classic C-style for loop
  let product = 1;
  for (let j = 1; j <= 5; j = j + 1) {
    product = product * j;
  }
  console.log("factorial:", product);
  // for-of over an array
  const xs = [10, 20, 30];
  let total = 0;
  for (const x of xs) {
    total = total + x;
  }
  console.log("for-of total:", total);
  // switch over a string-literal union discriminant (a union → Rust `enum` +
  // `match`) — see `statusLabel` for why it is a function, not an inline `let`.
  console.log("status:", statusLabel("running"));
}

main();
export {};
