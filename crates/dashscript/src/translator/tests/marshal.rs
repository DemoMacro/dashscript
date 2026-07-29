//! Empirical probe: `serde_json::Value` <-> rquickjs `Value` marshal and the
//! `call_fn` shape — eval a function body, call it with marshaled args, marshal
//! the return. Establishes the rquickjs 0.12 API truth for `__ds_engine::call_fn`
//! (B6b). The marshal/transcode logic proven here is ported verbatim into the
//! emitted `__ds_engine.rs` helper module.

use rquickjs::{Array, Context, Ctx, FromJs, IntoJs, Object, Runtime, Type, Value};

/// `serde_json::Value` -> rquickjs `Value` (recursive). Numbers fall back to
/// `NaN` (ES `Number` cannot losslessly hold an out-of-range integer; the engine
/// re-stringifies it, matching Node for the cases that reach this path).
fn json_to_js<'js>(ctx: &Ctx<'js>, v: &serde_json::Value) -> rquickjs::Result<Value<'js>> {
    match v {
        serde_json::Value::Null => Ok(Value::new_null(ctx.clone())),
        serde_json::Value::Bool(b) => Ok(Value::new_bool(ctx.clone(), *b)),
        serde_json::Value::Number(n) => Ok(Value::new_float(
            ctx.clone(),
            n.as_f64().unwrap_or(f64::NAN),
        )),
        serde_json::Value::String(s) => s.as_str().into_js(ctx),
        serde_json::Value::Array(arr) => {
            let js_arr = Array::new(ctx.clone())?;
            for (i, e) in arr.iter().enumerate() {
                js_arr.set(i, json_to_js(ctx, e)?)?;
            }
            js_arr.into_js(ctx)
        }
        serde_json::Value::Object(obj) => {
            let o = Object::new(ctx.clone())?;
            for (k, val) in obj.iter() {
                o.set(k.as_str(), json_to_js(ctx, val)?)?;
            }
            o.into_js(ctx)
        }
    }
}

/// rquickjs `Value` -> `serde_json::Value` (recursive). Symbols, BigInts, and the
/// void types collapse to `null` (the closest JSON representation).
fn js_to_json<'js>(ctx: &Ctx<'js>, v: Value<'js>) -> rquickjs::Result<serde_json::Value> {
    match v.type_of() {
        Type::Uninitialized | Type::Undefined | Type::Null => Ok(serde_json::Value::Null),
        Type::Bool => Ok(serde_json::Value::Bool(v.as_bool().unwrap())),
        Type::Int | Type::Float => Ok(serde_json::json!(v.as_number().unwrap())),
        Type::String => {
            let s: String = FromJs::from_js(ctx, v)?;
            Ok(serde_json::Value::String(s))
        }
        Type::Array => {
            let arr: Array = Array::from_js(ctx, v)?;
            let mut out = Vec::with_capacity(arr.len());
            for i in 0..arr.len() {
                let elem: Value = arr.get(i)?;
                out.push(js_to_json(ctx, elem)?);
            }
            Ok(serde_json::Value::Array(out))
        }
        Type::Object
        | Type::Function
        | Type::Constructor
        | Type::Promise
        | Type::Exception
        | Type::Proxy => {
            let obj: Object = Object::from_js(ctx, v)?;
            let mut map = serde_json::Map::new();
            for kv in obj.props::<String, Value>() {
                let (k, val) = kv?;
                map.insert(k, js_to_json(ctx, val)?);
            }
            Ok(serde_json::Value::Object(map))
        }
        Type::Symbol | Type::BigInt | Type::Module | Type::Unknown => Ok(serde_json::Value::Null),
    }
}

#[test]
fn marshal_roundtrips_primitives_and_collections() {
    let runtime = Runtime::new().expect("rquickjs Runtime");
    let ctx = Context::full(&runtime).expect("rquickjs Context");
    let original = serde_json::json!({
        "n": 42.5,
        "s": "hi",
        "b": true,
        "nil": null,
        "arr": [1.0, "two", false, null],
        "obj": { "nested": { "deep": 7.0 } }
    });
    let round = ctx.with(|ctx: Ctx<'_>| {
        let js = json_to_js(&ctx, &original).expect("json_to_js");
        js_to_json(&ctx, js).expect("js_to_json")
    });
    assert_eq!(round, original);
}

#[test]
fn call_fn_invokes_named_function_with_marshaled_args() {
    let runtime = Runtime::new().expect("rquickjs Runtime");
    let ctx = Context::full(&runtime).expect("rquickjs Context");
    let ret = ctx.with(|ctx: Ctx<'_>| -> serde_json::Value {
        ctx.eval::<(), _>("function __ds_probe_add(a, b) { return a + b; }")
            .expect("eval body");
        let args = [serde_json::json!(3.0), serde_json::json!(4.0)];
        let js_args = Array::new(ctx.clone()).expect("Array::new");
        for (i, a) in args.iter().enumerate() {
            js_args
                .set(i, json_to_js(&ctx, a).unwrap())
                .expect("set arg");
        }
        ctx.globals()
            .set("__ds_call_args", js_args)
            .expect("set args");
        let r: Value = ctx.eval("__ds_probe_add(...__ds_call_args)").expect("call");
        let _ = ctx.globals().remove("__ds_call_args");
        js_to_json(&ctx, r).expect("js_to_json")
    });
    assert_eq!(ret, serde_json::json!(7.0));
}
