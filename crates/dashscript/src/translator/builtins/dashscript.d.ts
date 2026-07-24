// DashScript standard library — the `lib.d.ts` analogue.
//
// Ambient declarations for DashScript's ES built-ins (`console`, `Math`,
// `Number`, `String`, `Array`, `Object`, and the global functions/constants).
// Hand-written, like TypeScript's `lib.es5.d.ts`: each member is implemented by
// a matching arm in `crates/dashscript/src/translator/builtins/<name>.rs`, and
// the drift-guard test asserts every declared symbol actually translates to
// Rust.
//
// The `@dashscript/typescript-plugin` injects this file so `.ts` sources get
// completion, hover, and types for the built-ins (the way `lib.d.ts` ships with
// TypeScript). `interface`s declare namespace members (`console`, `Math`, …);
// `declare function`/`declare var` declare globals (`parseInt`, `NaN`, …). A
// trailing `// doc` on a line is that symbol's hover text.

// console — the global logging object (`console.rs`: `log` → `println!`,
// `warn`/`error` → `eprintln!`).
interface console {
  log(...data: any[]): void; // Print to stdout (lowers to `println!`).
  warn(...data: any[]): void; // Print to stderr (lowers to `eprintln!`).
  error(...data: any[]): void; // Print to stderr (lowers to `eprintln!`).
}

// Math — the global math namespace (`math.rs`).
interface Math {
  PI: number; // The ratio π ≈ 3.14159.
  E: number; // Euler's number e ≈ 2.71828.
  LN10: number; // Natural log of 10 ≈ 2.30259.
  LN2: number; // Natural log of 2 ≈ 0.69315.
  LOG10E: number; // Base-10 log of e ≈ 0.43429.
  LOG2E: number; // Base-2 log of e ≈ 1.44270.
  SQRT2: number; // Square root of 2 ≈ 1.41421.
  SQRT1_2: number; // Square root of 1/2 ≈ 0.70711.
  abs(x: number): number; // Absolute value.
  round(x: number): number; // Round to the nearest integer (halves toward +∞).
  floor(x: number): number; // Round toward −∞.
  ceil(x: number): number; // Round toward +∞.
  trunc(x: number): number; // Drop the fractional part (round toward 0).
  sqrt(x: number): number; // Square root.
  cbrt(x: number): number; // Cube root.
  exp(x: number): number; // e raised to the power x.
  expm1(x: number): number; // e^x − 1 (accurate near 0).
  log(x: number): number; // Natural logarithm.
  log2(x: number): number; // Base-2 logarithm.
  log10(x: number): number; // Base-10 logarithm.
  log1p(x: number): number; // Natural log of (1 + x), accurate near 0.
  pow(base: number, exp: number): number; // `base` raised to the power `exp`.
  sign(x: number): number; // −1, 0, or +1 indicating the sign.
  sin(x: number): number; // Sine (radians).
  cos(x: number): number; // Cosine (radians).
  tan(x: number): number; // Tangent (radians).
  asin(x: number): number; // Arc sine (radians).
  acos(x: number): number; // Arc cosine (radians).
  atan(x: number): number; // Arc tangent (radians).
  atan2(y: number, x: number): number; // Angle of the point (x, y), in radians.
  sinh(x: number): number; // Hyperbolic sine.
  cosh(x: number): number; // Hyperbolic cosine.
  tanh(x: number): number; // Hyperbolic tangent.
  asinh(x: number): number; // Inverse hyperbolic sine.
  acosh(x: number): number; // Inverse hyperbolic cosine.
  atanh(x: number): number; // Inverse hyperbolic tangent.
  hypot(...values: number[]): number; // Square root of the sum of squares (+∞ if any arg is ±∞).
  max(...values: number[]): number; // The largest argument (−∞ if empty).
  min(...values: number[]): number; // The smallest argument (+∞ if empty).
  clz32(x: number): number; // Leading zero bits of the uint32 value of `x`.
  fround(x: number): number; // Round `x` to the nearest f32 and back to f64.
  imul(a: number, b: number): number; // 32-bit integer multiply of `a` and `b`.
  sumPrecise(values: number[]): number; // Exact sum (ES2026): NaN propagates, mixed ±∞ yield NaN.
}

// Number — the global number namespace, static members only (`number.rs`).
// Instance methods (`toFixed`, `toString(radix)`, …) are not namespace members;
// completion surfaces them from the receiver's inferred type.
interface Number {
  EPSILON: number; // Difference between 1 and the next f64 (≈ 2.22e-16).
  MAX_SAFE_INTEGER: number; // Largest safe integer (2^53 − 1).
  MAX_VALUE: number; // Largest positive finite value.
  MIN_SAFE_INTEGER: number; // Smallest safe integer (−(2^53 − 1)).
  MIN_VALUE: number; // Smallest positive value (> 0).
  NaN: number; // Not-a-Number.
  NEGATIVE_INFINITY: number; // Negative infinity.
  POSITIVE_INFINITY: number; // Positive infinity.
  isNaN(x: number): boolean; // True when `x` is NaN.
  isFinite(x: number): boolean; // True when `x` is finite.
  isInteger(x: number): boolean; // True when `x` is an integer.
  isSafeInteger(x: number): boolean; // True when `x` is a safe integer (|x| ≤ 2^53 − 1).
  parseFloat(s: string): number; // Parse a float from a string (NaN if malformed).
  parseInt(s: string, radix?: number): number; // Parse an integer from a string.
}

// String — the global string namespace, static members only (`string.rs`).
interface String {
  fromCharCode(...codes: number[]): string; // String from UTF-16 char codes.
  fromCodePoint(...points: number[]): string; // String from Unicode code points.
}

// Array — the global array namespace, static members only (`array.rs`).
interface Array {
  from(src: any, mapFn?: (x: any) => any): any[]; // Shallow-copy an array, mapping each element when `mapFn` is given.
  of(...items: any[]): any[]; // Create an array from the arguments.
  isArray(x: unknown): boolean; // True when `x` is an array.
}

// Object — the global object namespace, static members only (`object.rs`).
interface Object {
  keys(o: any): string[]; // The record's own keys.
  values(o: any): any[]; // The record's own values.
  entries(o: any): [string, any][]; // The record's own [key, value] pairs.
  assign(target: any, ...srcs: any[]): any; // Merge each source into a clone of `target`.
  fromEntries(entries: any): any; // Build a record from key-value pairs.
  getOwnPropertyNames(o: any): string[]; // The record's own property names (≡ `Object.keys`).
  is(a: any, b: any): boolean; // Value identity — equal, or both NaN.
  freeze(o: any): any; // Return the record unchanged (Rust has no runtime freeze).
  seal(o: any): any; // Return the record unchanged (a no-op in Rust).
  preventExtensions(o: any): any; // Return the record unchanged (a no-op in Rust).
  isFrozen(o: any): boolean; // Always false — a Record is never frozen.
  isSealed(o: any): boolean; // Always false — a Record is never sealed.
  isExtensible(o: any): boolean; // Always true — a Record is always extensible.
}

// Global functions and constants — plain identifiers (`global.rs`).
// ES globals use `declare function`/`declare var`, exactly as `lib.es5.d.ts`
// does. A `declare var` is ambient (a type, no initializer); the translator
// maps each use site directly (`NaN` → `f64::NAN`, `Infinity` → `f64::INFINITY`).
declare function parseInt(s: string, radix?: number): number; // Parse an integer from a string.
declare function parseFloat(s: string): number; // Parse a floating-point number from a string.
declare function isNaN(x: number): boolean; // True when `x` is NaN.
declare function isFinite(x: number): boolean; // True when `x` is finite (not ±∞ or NaN).
declare function Boolean(x: unknown): boolean; // The truthiness of `x`.
declare function String(x: unknown): string; // The string form of `x`.
declare function Number(x: unknown): number; // The numeric form of `x`.
declare var undefined: any; // The absent value.
declare var Infinity: number; // Positive infinity.
declare var NaN: number; // Not-a-Number.
