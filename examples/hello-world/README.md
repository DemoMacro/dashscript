# hello-world

A minimal [DashScript](https://github.com/DemoMacro/dashscript) project —
the smallest program that exercises the translator today.

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
Hello, world!
```

`ds run` delegates execution to `cargo`, so a Rust toolchain must be on
`PATH`. (A DashScript-managed toolchain will replace this later.)

## What it translates to

`main.ds` maps to roughly:

```rust
fn main() {
    println!("{}", "Hello, world!".to_string());
}
```

## License

[MIT](../../LICENSE)
