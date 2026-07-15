# hello-world

A minimal [DashScript](https://github.com/DemoMacro/dashscript) project —
data modeling end to end: an `interface` becomes a Rust `struct`, an object
literal becomes a struct literal, and field access maps one-to-one.

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
3
4
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

fn main() {
    let p = Point { x: 3.0, y: 4.0 };
    println!("{}", p.x);
    println!("{}", p.y);
}
```

## License

[MIT](../../LICENSE)
