import { Adler32 } from "cargo:adler";

// `Adler32` is adler's rolling-hash state type. Importing a crate's public
// type into `.ts` lowers to `use adler::Adler32;` — exactly like importing a
// type from a local module (`import { Point } from "./geom"`). The `cargo:`
// prefix (aligned with Deno's `npm:`/`jsr:`/`node:` family markers) marks this
// as a Cargo crate, brought in by `ds add cargo:adler`, which records `adler`
// under `dashscript.cargo.dependencies` in `package.json`.

function emptySlot(): Adler32 | null {
  return null;
}

function main(): void {
  const slot: Adler32 | null = emptySlot();
  if (slot === null) {
    console.log("adler crate linked; no hash computed yet");
  }
}

main();
