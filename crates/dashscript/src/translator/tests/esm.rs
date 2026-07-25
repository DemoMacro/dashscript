//! Empirical probe: a `.js` ESM module loads under rquickjs via QuickJS's
//! native module system (no CommonJS wrapper). The modern npm ecosystem ships
//! ESM + `.d.ts`, and QuickJS supports `import`/`export` natively
//! (`JS_EVAL_TYPE_MODULE`), so an ESM entry — `export function add(...)` — is
//! eval'd as a module and its named export read directly. This is the shape
//! `__ds_engine::call` will lower ESM `.js` packages to.

#[test]
fn esm_module_evaluates_and_exports_callable() {
    use rquickjs::{Context, Ctx, Module, Runtime};
    let runtime = Runtime::new().expect("rquickjs Runtime");
    let ctx = Context::full(&runtime).expect("rquickjs Context");
    // A single ESM source: declares `add`, then stashes the call result on
    // globalThis (a module body cannot `return`).
    let module_src = r#"
        export function add(a, b) { return a + b; }
        globalThis.__ds_result = add(3, 4);
    "#;
    let result = ctx.with(|ctx: Ctx<'_>| -> rquickjs::Result<f64> {
        let module = Module::evaluate(ctx.clone(), "probe", module_src)?;
        module.finish::<()>()?;
        ctx.globals().get::<_, f64>("__ds_result")
    });
    assert_eq!(result.expect("rquickjs ESM eval"), 7.0);
}
