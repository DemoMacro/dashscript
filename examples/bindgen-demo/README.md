# bindgen-demo

Demonstrates `ds add` — turning a Rust source file's public surface into a
`.ds` type declaration (the reverse of translation), so importing Rust from
DashScript yields editor completion and types. This is the cross-language
analogue of `@types` / DefinitelyTyped.

## Run

```sh
ds add examples/bindgen-demo/geometry.rs
```

This writes `geometry.ds` beside the source:

```
interface Point {
  x: number;
  y: number;
}

interface Polyline {
  points: Point[];
  label: string | null;
}

declare function distance(a: number, b: number): number;
```

`ds add` re-runs bindgen on every invocation, so the declaration is always
in sync with the Rust source — there is no separate generation step.

## License

[MIT](../../LICENSE)
