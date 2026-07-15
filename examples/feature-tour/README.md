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
2^10: 1024
kinds: linear rotational
magnitude 2.23606797749979 => small
hello, world
true
ababab
square: 81
sum 1..5: 15
counted: 8
empty
set: 42
```

`ds run` delegates execution to `cargo`, so a Rust toolchain must be on `PATH`.

## What it demonstrates

| `.ds`                          | Rust                                        |
| ------------------------------ | ------------------------------------------- |
| `interface Vector { … }`       | `struct Vector { … }`                       |
| `type Kind = "a" \| "b"`       | `enum Kind { A, B }`                        |
| `switch (kind) { case … }`     | `match kind { Kind::A => … }`               |
| `Math.sqrt(…)`, `x ** 2`       | `….sqrt()`, `x.powf(2.0)`                   |
| `a + b + " => " + …`           | `format!("{}{}{}{}", …)` (string concat)    |
| `cond ? "a" : "b"`             | `if cond { … } else { … }`                  |
| `s.toLowerCase()`, `.includes` | `.to_lowercase()`, `.contains(…)`           |
| `"ab".repeat(3)`               | `….repeat(3.0 as usize)`                    |
| `(n) => n * n`                 | `\|n\| n * n`                               |
| `for (let i = …; …; i++)`      | `{ let mut i …; while … { …; i += 1.0; } }` |
| `for (const n of xs)`          | `for &n in &xs`                             |
| `continue` / `break`           | `continue` / `break`                        |
| `number \| null`, `…!`         | `Option<f64>`, `….unwrap()`                 |
| `if (items)` / `if (maybe)`    | `!items.is_empty()` / `maybe.is_some()`     |

## License

[MIT](../../LICENSE)
