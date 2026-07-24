# Feature Tour

A [DashScript](https://github.com/DemoMacro/dashscript) (`.ts`) example per
translator capability. Each file is a self-contained program with a `main()`
entry point, organized to mirror the module layout in
`crates/dashscript/src/translator/` — so a feature and the file that lowers it
sit one glance apart.

## Examples

| File               | Translator module                              | Demonstrates                                                      |
| ------------------ | ---------------------------------------------- | ----------------------------------------------------------------- |
| `declarations.ts`  | `declarations.rs`                              | `interface` / `type` / union → `struct` / `enum`; optional fields |
| `classes.ts`       | `class.rs` (+ `expressions/new.rs`)            | `class` → `struct` + `impl`; `new Foo()` → `Foo::new()`; `this`   |
| `functions.ts`     | `functions/mod.rs`                             | functions, generics, default params, arrow functions              |
| `operators.ts`     | `expressions/{binary,logical,unary}.rs`        | arithmetic, comparison, logical, bitwise, ternary, `typeof`       |
| `control-flow.ts`  | `functions/{control_flow,switch}.rs`           | `if` / `while` / `for` / `for-of` / `switch` (enum and `string`)  |
| `destructuring.ts` | `functions/destructure.rs`                     | object/array patterns, rename, rest                               |
| `arrays.ts`        | `expressions/array.rs` + `builtins/array.rs`   | literals, indexing, `map` / `filter` / `reduce`, `push`, slice    |
| `strings.ts`       | `builtins/string.rs`                           | `trim` / `upper` / `lower` / `repeat` / `split` / `slice`         |
| `math.ts`          | `builtins/math.rs`                             | `Math.*` functions and constants                                  |
| `numbers.ts`       | `builtins/number.rs`                           | `toFixed`, `toString(radix)`, `Number.*`                          |
| `records.ts`       | `expressions/object.rs` + `builtins/object.rs` | object literals, `Record<K,V>`, `Object.keys` / `values`, spread  |
| `globals.ts`       | `builtins/global.rs`                           | `parseInt`, `parseFloat`, `isNaN`, value casts                    |

## Running

Each file is a lone `.ts` program — run it directly (translated, then compiled
in the global cache, Deno-style), or build a native binary:

```sh
ds run declarations.ts      # translate → compile (cached) → run
ds build declarations.ts    # → dist/declarations (native binary)
```

`ds run` delegates execution to `cargo`, so a Rust toolchain must be on `PATH`.

## Scope

These cover the TypeScript subset that maps to idiomatic Rust today. Two gaps
are called out in the source rather than papered over:

- **`&mut self` method calls** — a `&mut self` method translates correctly, but
  calling one needs a mutable binding at the call site, which the translator
  does not yet infer from a method's signature. See the `classes.ts` header.
- **`type X = Record<…>` alias** — the alias is not yet resolved to `HashMap`
  at use sites, so `records.ts` uses an inline `Record<K, V>` annotation.
