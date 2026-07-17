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
  "target": "bin",
  "dependencies": {
    "rust:adler": "1.0.2"
  }
}
```

## The source: `main.ds`

[`main.ds`](./main.ds) imports the added crate's type and uses it — the same
`import { X } from "crate"` syntax as a local module import, lowered to
`use crate::X`:

```ds
import { Adler32 } from "adler";

function emptySlot(): Adler32 | null {
  return null;
}

function main(): void {
  const slot: Adler32 | null = emptySlot();
  if (slot === null) {
    console.log("adler crate linked; no hash computed yet");
  }
}
```

`ds check main.ds` reports no issues (the crate import is translatable), and
`ds build main.ds` compiles `adler` (resolved from `manifest.json`) into a
native binary in `dist/` — reusing the source `ds add` already fetched.

## Build reuses cargo's cache (no re-download)

`ds build` turns `manifest.json` into a `Cargo.toml` and compiles in
`.cache/dash/<name>/`. cargo reuses the `~/.cargo` source that `ds add`
already fetched — nothing is downloaded twice, and repeat builds are
incremental. (Running `ds add` and `ds build` separately is the intended
flow, mirroring `npm install` then `vp pack`.)

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
