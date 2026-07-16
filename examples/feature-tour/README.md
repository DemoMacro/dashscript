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
trig: 0 1
bitwise: 2
shift: 8
bitnot: -1
kinds: linear rotational
magnitude 2.23606797749979 => small
updated: 99 5
circle area: 9
square area: 16
item name: widget
item tag: new
identity: 42
default: hello, world
override: hi, world
chain none: -1
hello, world
true
ababab
split: 3
str indexOf: 2
as string: 42
parsed: 100
sliced str: ell
trimmed: ab
charAt: e
fixed: 3.14
square: 81
doubled: 2 4 6
evens: 2
slice: 2 3
indexOf: 1
findIndex >1: 1
includes 2: true
find 2: 2
some >2: true
every >0: true
join: 1-2-3
reduce: 6
each: 1
each: 2
each: 3
tail: 4
array destructure: 10 20
sorted: 1 2 3
reversed: 3 2 1
concat size: 5
ys[0]: 9
wv.x: 10
sum 1..5: 15
counted: 8
empty
set: 42
```

`ds run` delegates execution to `cargo`, so a Rust toolchain must be on `PATH`.

## What it demonstrates

| `.ds`                                  | Rust                                                       |
| -------------------------------------- | ---------------------------------------------------------- |
| `interface Vector { … }`               | `struct Vector { … }`                                      |
| `Record<string, number>`               | `HashMap<String, f64>`                                     |
| `{ alice: 90, … }` (Record)            | `HashMap::from([(…, …)])`                                  |
| `m["k"]` (HashMap)                     | `m.get("k").copied().unwrap()`                             |
| `m["k"] = v` (HashMap)                 | `m.insert("k".to_string(), v)`                             |
| `xs[i] = v`                            | `xs[i as usize] = v`                                       |
| `v.x = v`                              | `v.x = v`                                                  |
| `return { x, y }` (typed return)       | `Vector { x: …, y: … }`                                    |
| `{ ...base, x: 99 }` (typed)           | `Vector { x: 99.0, ..base }`                               |
| `interface V { y?: T }`                | `pub y: Option<T>`                                         |
| `{ x: 1 }` (omit optional `y?`)        | `V { x: …, y: None }`                                      |
| `const { x, y } = v` (typed source)    | `let Vector { x, y } = v;`                                 |
| `f({ x, y })` (typed param)            | `f(Vector { x: …, y: … })`                                 |
| `type Kind = "a" \| "b"`               | `enum Kind { A, B }`                                       |
| `switch (kind) { case … }`             | `match kind { Kind::A => … }`                              |
| `{ kind: "c"; r } \| { kind: "s"; s }` | `enum Shape { Circle { r }, Square { s } }`                |
| `switch (s.kind) { case …: s.r }`      | `match s { Shape::Circle { r } => r }`                     |
| `Math.sqrt(…)`, `x ** 2`               | `….sqrt()`, `x.powf(2.0)`                                  |
| `Math.sin(…)`, `Math.atan2(y, x)`      | `….sin()`, `….atan2(…)`                                    |
| `a & b` / `a \| b` / `a ^ b`           | `((a as i32) & (b as i32)) as f64`                         |
| `a << b` / `a >> b` / `a >>> b`        | `(a as i32).wrapping_shl(b as u32) as f64`                 |
| `~a`                                   | `!(a as i32) as f64`                                       |
| `String(42)`                           | `format!("{}", 42.0)`                                      |
| `parseInt("100")`                      | `….trim().parse::<f64>().unwrap()`                         |
| `a + b + " => " + …`                   | `format!("{}{}{}{}", …)` (string concat)                   |
| `cond ? "a" : "b"`                     | `if cond { … } else { … }`                                 |
| `s.toLowerCase()`, `.includes`         | `.to_lowercase()`, `.contains(…)`                          |
| `"ab".repeat(3)`                       | `….repeat(3.0 as usize)`                                   |
| `"a,b,c".split(",")`                   | `….split(…).map(to_string).collect()`                      |
| `"hello".indexOf("ll")`                | `….find(…).map(\|b\| b as f64).unwrap_or(-1.0)`            |
| `"hello".slice(1, 4)`                  | `….to_string()[1..4].to_string()`                          |
| `"  ab".trimStart()`                   | `….trim_start()`                                           |
| `pi.toFixed(2)`                        | `format!("{:.*}", 2.0 as usize, pi)`                       |
| `"hello".charAt(1)`                    | `….chars().nth(…).map(to_string).unwrap_or_default()`      |
| `function id<T>(x: T): T`              | `fn id<T>(x: T) -> T`                                      |
| `function f(x = 5)`                    | `fn f(x: Option<T>); let x = x.unwrap_or(5)`               |
| `(n) => n * n`                         | `\|n\| n * n`                                              |
| `xs.map((n) => n * 2)`                 | `xs.iter().copied().map(\|n\| …).collect()`                |
| `xs.filter((n) => n > 1)`              | `xs.iter().copied().filter(\|&n\| …).collect()`            |
| `xs.slice(1, 3)`                       | `xs[1.0 as usize..3.0 as usize].to_vec()`                  |
| `xs.indexOf(2)`                        | `….position(\|y\| y == 2.0).unwrap_or(-1.0)`               |
| `xs.findIndex((n) => …)`               | `….position(\|&n\| …).map(\|i\| i as f64).unwrap_or(-1.0)` |
| `xs.includes(2)`                       | `….contains(&2.0)`                                         |
| `xs.find((n) => …)`                    | `….iter().copied().find(\|&n\| …)`                         |
| `xs.some` / `.every`                   | `….any(\|n\| …)` / `….all(\|n\| …)`                        |
| `xs.join("-")`                         | `….map(to_string).collect().join("-")`                     |
| `xs.reduce((a, b) => …, 0)`            | `….fold(0.0, \|a, b\| …)`                                  |
| `xs.forEach((n) => …)`                 | `….iter().copied().for_each(\|n\| …)`                      |
| `[...xs, 4]`                           | `[xs.as_slice(), &[4][..]].concat()`                       |
| `const [a, b] = xs`                    | `let a = xs[0]; let b = xs[1];`                            |
| `xs.concat(ys)`                        | `[xs.as_slice(), ys.as_slice()].concat()`                  |
| `xs.reverse()`                         | `xs.reverse()`                                             |
| `xs.sort()`                            | `xs.sort_by(\|a, b\| a.partial_cmp(&b).unwrap())`          |
| `for (let i = …; …; i++)`              | `{ let mut i …; while … { …; i += 1.0; } }`                |
| `for (const n of xs)`                  | `for &n in &xs`                                            |
| `continue` / `break`                   | `continue` / `break`                                       |
| `number \| null`, `…!`                 | `Option<f64>`, `….unwrap()`                                |
| `if (items)` / `if (maybe)`            | `!items.is_empty()` / `maybe.is_some()`                    |
| `v?.x` (Option receiver)               | `v.as_ref().map(\|__c\| __c.x)`                            |

## License

[MIT](../../LICENSE)
