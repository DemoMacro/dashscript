# crate-add

Demonstrates `ds add` / `ds remove` — DashScript's package management, which
reuses **cargo as the global store and resolver** (the Rust analogue of how
pnpm reuses its content-addressable store). DashScript keeps no store of its
own and resolves no version conflicts — cargo does, exactly as rust-analyzer
relies on cargo.

## Add a crate

```sh
ds add adler
```

This:

- downloads `adler` (and all transitive deps) into cargo's global registry
  (`~/.cargo`) via `cargo add` — there is no second store;
- records `rust:adler = "<version>"` in `manifest.json`.

`manifest.json` afterwards:

```json
{
  "name": "crate-add",
  "target": "rust",
  "dependencies": {
    "rust:adler": "1.0.2"
  }
}
```

## Build reuses cargo's cache (no re-download)

`ds build` turns `manifest.json` into a `Cargo.toml` and runs `cargo check`.
cargo reuses the `~/.cargo` source that `ds add` already fetched — nothing is
downloaded twice. (Running `ds add` and `ds build` separately is the intended
flow, mirroring `npm install` then `tsc`.)

## Type information comes from source, not from generated stubs

No `.ds` declaration files are generated. Type information for an added crate
(hover, jump-to-definition, completion) comes from the crate's own source in
`~/.cargo`, read directly by the DashScript language server — the same way
rust-analyzer reads its dependencies rather than maintaining a parallel set of
type stubs. Rust is statically typed, so the source is the complete truth.

## Remove a crate

```sh
ds remove adler
```

Removes `rust:adler` from `manifest.json`.

## License

[MIT](../../LICENSE)
