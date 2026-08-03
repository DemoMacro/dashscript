# fetch-smoke

A [DashScript](https://github.com/DemoMacro/dashscript) project exercising the
WinterTC `fetch` Web API — a **native** async mapping backed by `reqwest`
(never degraded to the embedded engine, per the WinterTC policy). `fetch(url)`
lowers to `__ds::ds_fetch`; `await fetch(url)` records `r: DsResponse`, so
`r.status` / `.ok` / `.headers` rewrite to the wrapper's zero-arg accessors and
`await r.text()` to the async body drain.

## Files

| File           | Purpose                                             |
| -------------- | --------------------------------------------------- |
| `main.ts`      | The program source.                                 |
| `package.json` | Project manifest: npm fields + `dashscript` config. |

## Run

From this directory:

```sh
ds main.ts           # run the file directly (translate → compile cached → run)
```

This example targets `https://example.com`, so a run needs network access.
`ds run` delegates execution to `cargo`, so a Rust toolchain must be on `PATH`.

## What it translates to

`main.ts` maps to roughly:

```rust
async fn probe(url: String) {
    let r = crate::__ds::ds_fetch(url).await;
    println!("{}", r.status());
    println!("{}", r.ok());
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    probe("https://example.com".to_string()).await;
}
```

The top-level `await probe(...)` is collected into the implicit `fn main`,
which DashScript emits as a single-thread `#[tokio::main]` async entry (no
`Send` bounds — matching JavaScript's single-thread semantics). `fetch`,
`Response.status/.ok/.headers`, and the `reqwest` dependency are injected by
the `Fetch` runtime dep.

## License

[MIT](../../LICENSE)
