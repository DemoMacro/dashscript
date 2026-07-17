# hello-world

A minimal [DashScript](https://github.com/DemoMacro/dashscript) project — data
modeling, control flow, nullable types, and enums end to end: an `interface`
becomes a Rust `struct`, object literals become struct literals, `if` / `while`
/ `for…of`, operators, and template literals map to their idiomatic Rust
counterpart, `T | null` becomes `Option<T>`, and `type T = "a" | "b"` becomes an
`enum`. Function names convert from TypeScript `camelCase` to Rust `snake_case`;
type names keep their `PascalCase`.

## Files

| File            | Purpose                                          |
| --------------- | ------------------------------------------------ |
| `main.ds`       | The program source.                              |
| `manifest.json` | Project manifest: target backend + dependencies. |

## Run

From this directory:

```sh
ds main.ds           # run the file directly (translate → compile cached → run)
```

Output:

```
y is bigger
magnitude squared: 25
36
waiting
finished
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

enum Status {
    Pending,
    Done,
}

fn magnitude_squared(p: Point) -> f64 {
    p.x * p.x + p.y * p.y
}

fn describe(s: Status) -> String {
    match s {
        Status::Pending => return "waiting".to_string(),
        Status::Done => return "finished".to_string(),
    }
}

fn main() {
    let p = Point { x: 3.0, y: 4.0 };
    if p.x >= p.y {
        println!("{}", "x is bigger".to_string());
    } else {
        println!("{}", "y is bigger".to_string());
    }
    println!("{}", format!("magnitude squared: {}", magnitude_squared(p)));
    let explicit: Option<f64> = Some(36.0);
    println!("{}", explicit.unwrap());
    let pending = Status::Pending;
    println!("{}", describe(pending));
    let done = Status::Done;
    println!("{}", describe(done));
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
call is placed where `p` is last used. Nullable types map to `Option<T>`; a
string-literal union maps to an `enum`, with a `switch` becoming a `match`.

## License

[MIT](../../LICENSE)
