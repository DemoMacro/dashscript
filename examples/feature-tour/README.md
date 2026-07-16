# feature-tour

A single [DashScript](https://github.com/DemoMacro/dashscript) program that
exercises the subset which transpiles to idiomatic Rust: interfaces and enums,
arrows, string methods and concatenation, `Math`, every loop form, `switch`,
nullable `Option`, and truthiness.

## Run

From this directory:

```sh
ds run main.ds       # parse → translate → emit a Cargo project → cargo run
```

Output:

```
magnitude: 5
makeVector: 5 6
destructured: 7 8
sum: 7
record size: 2
alice: 90
prices size: 2
2^10: 1024
kinds: linear rotational
magnitude 2.23606797749979 => small
updated: 99 5
circle area: 9
square area: 16
hello, world
true
ababab
split: 3
str indexOf: 2
square: 81
doubled: 2 4 6
evens: 2
slice: 2 3
indexOf: 1
includes 2: true
find 2: 2
some >2: true
every >0: true
join: 1-2-3
reduce: 6
ys[0]: 9
wv.x: 10
sum 1..5: 15
counted: 8
empty
set: 42
```

`ds run` delegates execution to `cargo`, so a Rust toolchain must be on `PATH`.

## What it demonstrates

| `.ds`                                  | Rust                                            |
| -------------------------------------- | ----------------------------------------------- |
| `interface Vector { … }`               | `struct Vector { … }`                           |
| `Record<string, number>`               | `HashMap<String, f64>`                          |
| `{ alice: 90, … }` (Record)            | `HashMap::from([(…, …)])`                       |
| `m["k"]` (HashMap)                     | `m.get("k").copied().unwrap()`                  |
| `m["k"] = v` (HashMap)                 | `m.insert("k".to_string(), v)`                  |
| `xs[i] = v`                            | `xs[i as usize] = v`                            |
| `v.x = v`                              | `v.x = v`                                       |
| `return { x, y }` (typed return)       | `Vector { x: …, y: … }`                         |
| `{ ...base, x: 99 }` (typed)           | `Vector { x: 99.0, ..base }`                    |
| `const { x, y } = v` (typed source)    | `let Vector { x, y } = v;`                      |
| `f({ x, y })` (typed param)            | `f(Vector { x: …, y: … })`                      |
| `type Kind = "a" \| "b"`               | `enum Kind { A, B }`                            |
| `switch (kind) { case … }`             | `match kind { Kind::A => … }`                   |
| `{ kind: "c"; r } \| { kind: "s"; s }` | `enum Shape { Circle { r }, Square { s } }`     |
| `switch (s.kind) { case …: s.r }`      | `match s { Shape::Circle { r } => r }`          |
| `Math.sqrt(…)`, `x ** 2`               | `….sqrt()`, `x.powf(2.0)`                       |
| `a + b + " => " + …`                   | `format!("{}{}{}{}", …)` (string concat)        |
| `cond ? "a" : "b"`                     | `if cond { … } else { … }`                      |
| `s.toLowerCase()`, `.includes`         | `.to_lowercase()`, `.contains(…)`               |
| `"ab".repeat(3)`                       | `….repeat(3.0 as usize)`                        |
| `"a,b,c".split(",")`                   | `….split(…).map(to_string).collect()`           |
| `"hello".indexOf("ll")`                | `….find(…).map(\|b\| b as f64).unwrap_or(-1.0)` |
| `(n) => n * n`                         | `\|n\| n * n`                                   |
| `xs.map((n) => n * 2)`                 | `xs.iter().copied().map(\|n\| …).collect()`     |
| `xs.filter((n) => n > 1)`              | `xs.iter().copied().filter(\|&n\| …).collect()` |
| `xs.slice(1, 3)`                       | `xs[1.0 as usize..3.0 as usize].to_vec()`       |
| `xs.indexOf(2)`                        | `….position(\|y\| y == 2.0).unwrap_or(-1.0)`    |
| `xs.includes(2)`                       | `….contains(&2.0)`                              |
| `xs.find((n) => …)`                    | `….iter().copied().find(\|&n\| …)`              |
| `xs.some` / `.every`                   | `….any(\|n\| …)` / `….all(\|n\| …)`             |
| `xs.join("-")`                         | `….map(to_string).collect().join("-")`          |
| `xs.reduce((a, b) => …, 0)`            | `….fold(0.0, \|a, b\| …)`                       |
| `for (let i = …; …; i++)`              | `{ let mut i …; while … { …; i += 1.0; } }`     |
| `for (const n of xs)`                  | `for &n in &xs`                                 |
| `continue` / `break`                   | `continue` / `break`                            |
| `number \| null`, `…!`                 | `Option<f64>`, `….unwrap()`                     |
| `if (items)` / `if (maybe)`            | `!items.is_empty()` / `maybe.is_some()`         |

## License

[MIT](../../LICENSE)
