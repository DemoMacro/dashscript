# hello-world

A minimal [DashScript](https://github.com/DemoMacro/dashscript) project —
data modeling and control flow end to end: an `interface` becomes a Rust
`struct`, object literals become struct literals, and `if` / `while` /
`for…of`, arithmetic and comparison operators, and template literals each map
to their idiomatic Rust counterpart. Function names convert from TypeScript
`camelCase` to Rust `snake_case`; type names keep their `PascalCase`.

## Files

| File            | Purpose                                          |
| --------------- | ------------------------------------------------ |
| `main.ds`       | The program source.                              |
| `manifest.json` | Project manifest: target backend + dependencies. |

## Run

From this directory:

```sh
ds run main.ds       # parse → translate → emit a Cargo project → cargo run
```

Output:

```
y is bigger
magnitude squared: 25
0
1
2
10
20
```

`ds run` delegates execution to `cargo`, so a Rust toolchain must be on
`PATH`. (A DashScript-managed toolchain will replace this later.)

## What it translates to

`main.ds` maps to roughly:

```rust
struct Point {
    pub x: f64,
    pub y: f64,
}

fn magnitude_squared(p: Point) -> f64 {
    p.x * p.x + p.y * p.y
}

fn main() {
    let p = Point { x: 3.0, y: 4.0 };
    if p.x >= p.y {
        println!("{}", "x is bigger".to_string());
    } else {
        println!("{}", "y is bigger".to_string());
    }
    println!("{}", format!("magnitude squared: {}", magnitude_squared(p)));
    let mut i = 0.0;
    while i < 3.0 {
        println!("{}", i);
        i += 1.0;
    }
    let xs = vec![10.0, 20.0, 30.0];
    for &v in &xs {
        if v > 5.0 && v < 25.0 {
            println!("{}", v);
        }
    }
}
```

DashScript exposes Rust ownership: `magnitude_squared(p)` moves `p`, so the
call is placed where `p` is last used.

## License

[MIT](../../LICENSE)
