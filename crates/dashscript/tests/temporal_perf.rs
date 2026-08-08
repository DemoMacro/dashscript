//! Temporal 静态路径 (`temporal_rs`) vs 引擎路径 (QuickJS + @js-temporal/polyfill)
//! 性能对比 —— 量化"静态优先 + polyfill 兜底"策略里静态那一步的零成本收益。
//!
//! 跑: `cargo test -p dashscript --test temporal_perf -- --ignored --nocapture`
//!
//! 三条曲线:
//! - **静态** (`temporal_rs`): DashScript 静态映射 emit 出的 `temporal_rs::PlainDate::from_utf8`
//!   + 访问器,原生 Rust。
//! - **引擎冷启动**: 每次 op 新建 `Runtime` + 注入 242KB polyfill + 1 次 eval —— 模拟
//!   conformance harness 每 fixture 一个 ctx 的最坏情况。
//! - **引擎稳态**: 1 次注入 + N 次 eval (共享 thread_local ctx) —— 模拟生产降级
//!   `__ds::engine` 的持久 runtime。

use rquickjs::{context::EvalOptions, Context, Ctx, Runtime};
use std::time::Instant;

const INTL_STUB: &str = r#"
if (!globalThis.Intl) {
  function __ds_dtf() {
    return {
      format: function (date) {
        var d = typeof date === 'number' ? new Date(date) : date instanceof Date ? date : new Date(+date || Date.now());
        var y = d.getUTCFullYear(); var era = y < 1 ? 'BC' : 'AD'; var ay = y < 1 ? 1 - y : y;
        function p(x) { return x < 10 ? '0' + x : '' + x; }
        return d.getUTCMonth() + 1 + '/' + d.getUTCDate() + '/' + ay + ' ' + era + ' ' + p(d.getUTCHours()) + ':' + p(d.getUTCMinutes()) + ':' + p(d.getUTCSeconds());
      },
      formatToParts: function () { return []; }, formatRange: function () { return ''; },
      formatRangeToParts: function () { return []; },
      resolvedOptions: function () { return { calendar: 'iso8601', locale: 'en-US', numberingSystem: 'latn', timeZone: 'UTC' }; },
    };
  }
  __ds_dtf.supportedLocalesOf = function () { return []; };
  globalThis.Intl = { DateTimeFormat: __ds_dtf, getCanonicalLocales: function (x) { return Array.isArray(x) ? x : [x]; } };
}
"#;

const TEMPORAL_POLYFILL: &str = include_str!("conformance/data/vendor/temporal-polyfill.umd.js");

const TEMPORAL_EXPOSE: &str = "\
globalThis.Temporal = globalThis.temporal.Temporal;
try { Date.prototype.toTemporalInstant = globalThis.temporal.toTemporalInstant; } catch (e) {}
";

fn sloppy() -> EvalOptions {
    let mut o = EvalOptions::default();
    o.strict = false;
    o
}

fn inject(ctx: &Ctx<'_>) {
    ctx.eval_with_options::<(), _>(INTL_STUB, sloppy()).unwrap();
    ctx.eval_with_options::<(), _>(TEMPORAL_POLYFILL, sloppy())
        .unwrap();
    ctx.eval_with_options::<(), _>(TEMPORAL_EXPOSE, sloppy())
        .unwrap();
}

/// `Temporal.PlainDate.from('2024-03-15')` + `d.year + d.month + d.day` ——
/// 静态映射覆盖的核心操作 (from 构造 + 访问器)。一次性 eval (含 JS parse),
/// 模拟 conformance harness 每 fixture 一个 eval。
const WORK_JS: &str =
    "(() => { const d = Temporal.PlainDate.from('2024-03-15'); return d.year + d.month + d.day; })()";

/// 预编译函数体 (eval 一次定义到 globalThis), 之后仅 eval 调用 —— 模拟生产降级
/// `__ds::engine.call_fn(name, args)` (函数已编译, 主循环不 re-parse 函数体)。
const DEFINE_WORK_JS: &str =
    "globalThis.__ds_work = () => { const d = Temporal.PlainDate.from('2024-03-15'); return d.year + d.month + d.day; };";
const CALL_WORK_JS: &str = "__ds_work()";

#[test]
#[ignore]
fn compare_temporal_paths() {
    let n = 50000;

    // 静态: temporal_rs (DashScript 静态映射 emit 的目标)
    let mut acc_static: i64 = 0;
    let t = Instant::now();
    for _ in 0..n {
        let d = temporal_rs::PlainDate::from_utf8("2024-03-15".as_bytes()).unwrap();
        acc_static += d.year() as i64 + d.month() as i64 + d.day() as i64;
    }
    let static_ns = t.elapsed().as_nanos() as f64 / n as f64;

    // 引擎冷启动: 每次 op 新 Runtime + 注入 polyfill + 1 eval
    let ncold = 300;
    let mut acc_cold: f64 = 0.0;
    let t = Instant::now();
    for _ in 0..ncold {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        ctx.with(|cx: Ctx<'_>| {
            inject(&cx);
            acc_cold += cx.eval_with_options::<f64, _>(WORK_JS, sloppy()).unwrap();
        });
    }
    let cold_ns = t.elapsed().as_nanos() as f64 / ncold as f64;

    // 引擎稳态-eval: 1 注入 + N eval 完整 JS (含每次 parse, 模拟 harness)
    let rt = Runtime::new().unwrap();
    let ctx = Context::full(&rt).unwrap();
    ctx.with(|cx: Ctx<'_>| inject(&cx));
    let mut acc_steady: f64 = 0.0;
    let t = Instant::now();
    for _ in 0..n {
        ctx.with(|cx: Ctx<'_>| {
            acc_steady += cx.eval_with_options::<f64, _>(WORK_JS, sloppy()).unwrap();
        });
    }
    let steady_ns = t.elapsed().as_nanos() as f64 / n as f64;

    // 引擎稳态-call_fn: 1 注入 + 1 定义函数 + N 次仅调用 (生产降级模式)
    let rt = Runtime::new().unwrap();
    let ctx = Context::full(&rt).unwrap();
    ctx.with(|cx: Ctx<'_>| {
        inject(&cx);
        cx.eval_with_options::<(), _>(DEFINE_WORK_JS, sloppy())
            .unwrap();
    });
    let mut acc_fn: f64 = 0.0;
    let t = Instant::now();
    for _ in 0..n {
        ctx.with(|cx: Ctx<'_>| {
            acc_fn += cx
                .eval_with_options::<f64, _>(CALL_WORK_JS, sloppy())
                .unwrap();
        });
    }
    let fn_ns = t.elapsed().as_nanos() as f64 / n as f64;

    eprintln!(
        "acc = static {acc_static} / cold {} / steady {} / fn {}",
        acc_cold as i64, acc_steady as i64, acc_fn as i64
    );
    eprintln!();
    eprintln!("=== Temporal: PlainDate.from + year/month/day ===");
    eprintln!(
        "静态 temporal_rs (原生 Rust):        {:>10.1} ns/op",
        static_ns
    );
    eprintln!(
        "引擎冷启动  (每 op 新 ctx+polyfill): {:>10.1} ns/op",
        cold_ns
    );
    eprintln!(
        "引擎稳态-eval (共享 ctx, 含 parse):  {:>10.1} ns/op",
        steady_ns
    );
    eprintln!("引擎稳态-call_fn (预编译, 仅调用):   {:>10.1} ns/op", fn_ns);
    eprintln!();
    eprintln!("静态 vs 引擎稳态-eval:   慢 {:.0}x", steady_ns / static_ns);
    eprintln!("静态 vs 引擎稳态-call_fn: 慢 {:.0}x", fn_ns / static_ns);
    eprintln!("静态 vs 引擎冷启动:      慢 {:.0}x", cold_ns / static_ns);
}
