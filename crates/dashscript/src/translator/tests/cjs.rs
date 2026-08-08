//! Empirical probe: a CommonJS wrapper loads under rquickjs. QuickJS has no
//! built-in CommonJS (it is ESM-only, like txiki.js), so a `.js` package is
//! run by wrapping it `(function (module, exports, require) { <src> })`,
//! calling that with a fresh `module`, and reading `module.exports[fn]`. This
//! test is the answer to "can QuickJS do CommonJS?" — yes, at the user-land
//! wrapper level; the open hard part is `require` (recursive resolution), not
//! the wrapper itself.

#[test]
fn commonjs_wrapper_loads_export_and_calls() {
    use rquickjs::{Context, Ctx, Runtime};
    let runtime = Runtime::new().expect("rquickjs Runtime");
    let ctx = Context::full(&runtime).expect("rquickjs Context");
    // Wrap a CommonJS package and call its export in one sloppy-mode eval —
    // the shape `__ds::engine::call` will lower to. `require` is a throwing
    // stub here (zero-dep probe); a real `require` is a separate concern.
    let program = r#"
        var __ds_module = { exports: {} };
        (function (module, exports, require) {
            module.exports.add = function (a, b) { return a + b; };
        })(__ds_module, __ds_module.exports, function () {
            throw new Error("require: zero-dep probe");
        });
        __ds_module.exports.add(3, 4)
    "#;
    let sum: f64 = ctx
        .with(|ctx: Ctx<'_>| ctx.eval(program))
        .expect("rquickjs CommonJS eval");
    assert_eq!(sum, 7.0);
}
