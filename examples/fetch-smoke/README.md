# fetch-smoke

A [DashScript](https://github.com/DemoMacro/dashscript) project exercising the
WinterTC `fetch` Web API — a **native** async mapping backed by `reqwest`
(a zero-cost static mapping: `fetch` lowers straight to a Rust crate). `fetch(url)`
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

async fn post(url: String) {
    let r = crate::__ds::ds_fetch_with(
        url,
        "POST".to_string(),
        ::std::option::Option::Some("hi".to_string()),
        ::std::vec![("Content-Type".to_string(), ("text/plain".to_string()).to_string())],
    )
    .await;
    println!("{}", r.status());
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    probe("https://example.com".to_string()).await;
    post("https://example.com".to_string()).await;
}
```

The top-level `await probe(...)` / `await post(...)` are collected into the
implicit `fn main`, which DashScript emits as a single-thread
`#[tokio::main]` async entry (no `Send` bounds — matching JavaScript's
single-thread semantics). `fetch(url)` → `ds_fetch`; `fetch(url, init)` with a
plain object `init` → `ds_fetch_with` (method/body/headers extracted,
ToString-coerced). `Response.status/.ok/.headers` are zero-arg accessors;
`await r.text()`/`.json()`/`.arrayBuffer()` are async body-draining fns
(`arrayBuffer` → `array_buffer`). `reqwest` (and `serde_json` for `json`) are
injected by the `Fetch` runtime dep.

## License

[MIT](../../LICENSE)
