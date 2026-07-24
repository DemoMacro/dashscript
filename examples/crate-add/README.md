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
- records `adler = "<version>"` under `dashscript.cargo.dependencies` in
  `package.json`.

`package.json` afterwards:

```json
{
  "name": "crate-add",
  "dashscript": {
    "cargo": {
      "dependencies": {
        "adler": "1.0.2"
      }
    }
  }
}
```

## The source: `main.ts`

[`main.ts`](./main.ts) imports the added crate's type and uses it — the
`cargo:` prefix (aligned with Deno's `npm:`/`jsr:`/`node:` family markers)
marks a Cargo crate, lowered to `use crate::X`:

```ts
import { Adler32 } from "cargo:adler";

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

`ds lint main.ts` reports no issues (the crate import is translatable), and
`ds build main.ts` compiles `adler` (resolved from `package.json`) into a
native binary in `dist/` — reusing the source `ds add` already fetched.

## Build reuses cargo's cache (no re-download)

`ds build` turns `package.json` into a `Cargo.toml` and compiles in
`.cache/dash/<name>/`. cargo reuses the `~/.cargo` source that `ds add`
already fetched — nothing is downloaded twice, and repeat builds are
incremental. (Running `ds add` and `ds build` separately is the intended
flow, mirroring `npm install` then `vp pack`.)

## Type information comes from source, not from generated stubs

No `.ts` declaration files are generated. Type information for an added crate
(hover, jump-to-definition, completion) comes from the crate's own source in
`~/.cargo`, read directly by the DashScript language server — the same way
rust-analyzer reads its dependencies rather than maintaining a parallel set of
type stubs. Rust is statically typed, so the source is the complete truth.

## Remove a crate

```sh
ds remove adler
```

Removes `adler` from `package.json`.

## License

[MIT](../../LICENSE)
