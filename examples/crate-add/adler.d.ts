// Type stub for the `adler` cargo crate (`ds add cargo:adler` records it under
// `dashscript.cargo.dependencies` in package.json). In batch 4d, `ds add
// cargo:<crate>` will bindgen this from the crate's own source in ~/.cargo —
// the way rust-analyzer reads its deps — so this hand-written stub is a
// placeholder until that lands. Adler32 is adler's rolling-hash state type.
declare module "adler" {
  export class Adler32 {
    constructor();
    update(data: ArrayLike<number>): void;
    checksum(): number;
  }
}
