//! `Temporal.*` API → the `temporal_rs` crate (boa-dev/temporal-rs — the Rust
//! implementation of ECMAScript Temporal). One file per built-in mirroring
//! test262's `test/built-ins/Temporal/` (reserved for future fixtures).
//!
//! `temporal_rs` is statically typed and its calendar accessors are infallible
//! (`.year()` returns `i32`, not `TemporalResult<i32>`), so a
//! `Temporal.PlainDate` maps directly to `temporal_rs::PlainDate` — no `__ds`
//! helper slice. The `from` constructor returns `TemporalResult`; its `Err` is
//! lowered by `panic_any`-ing a `DsError` whose `name` is the temporal-rs
//! `ErrorKind` mapped to its ES error class (a malformed ISO string is an ES
//! `RangeError`/`SyntaxError`, not a bare unwrap panic) — see
//! `RuntimeDep::Error` + `try`/`catch` in `functions/try_throw.rs`.

use oxc_ast::ast::Argument;
use proc_macro2::Span;
use syn::{parse_quote, Expr};

use super::super::context::Ctx;
use super::super::expressions::translate_argument;

/// `Temporal.<Type>.<method>(…)` static calls. The caller (`translate_call`)
/// has already split the nested callee (`Temporal.PlainDate.from`) into its
/// type and method names. Returns `None` for any unrecognized pair (an unknown
/// `Temporal.X.Y` surfaces as E0425 honestly).
pub(in crate::translator) fn temporal_static(
    ty: &str,
    method: &str,
    args: &[Argument],
    ctx: &Ctx<'_>,
) -> Option<Expr> {
    match (ty, method) {
        ("PlainDate", "from") => plain_date_from(args, ctx),
        ("PlainDate", "compare") => plain_date_compare(args, ctx),
        _ => None,
    }
}

/// `Temporal.PlainDate.from(s)` → `temporal_rs::PlainDate::from_utf8(s.as_bytes()).unwrap()`.
/// `from_utf8` is an inherent constructor (no `FromStr` trait import needed);
/// a string literal stays a bare `&str` so `.as_bytes()` yields a `&'static [u8]`.
fn plain_date_from(args: &[Argument], ctx: &Ctx<'_>) -> Option<Expr> {
    let a = args.first()?;
    let e = if let Argument::StringLiteral(s) = a {
        let lit = syn::LitStr::new(s.value.as_str(), Span::call_site());
        parse_quote!(#lit)
    } else {
        translate_argument(a, ctx)
    };
    Some(parse_quote!({
        // A malformed ISO string is an ES `RangeError`/`SyntaxError`, not a
        // panic — lower the `TemporalResult::Err` to a `DsError` so `catch (e)`
        // recovers `e.constructor.name`/`e.name`/`e.message` (the temporal-rs
        // `ErrorKind` is the ES error class; `into_message` is the text).
        match temporal_rs::PlainDate::from_utf8((#e).as_bytes()) {
            Ok(__d) => __d,
            Err(__err) => ::std::panic::panic_any(crate::__ds::DsError::new(
                match __err.kind() {
                    temporal_rs::error::ErrorKind::Generic => "Error",
                    temporal_rs::error::ErrorKind::Type => "TypeError",
                    temporal_rs::error::ErrorKind::Range => "RangeError",
                    temporal_rs::error::ErrorKind::Syntax => "SyntaxError",
                    temporal_rs::error::ErrorKind::Assert => "ImplementationError",
                },
                __err.into_message(),
            )),
        }
    }))
}

/// `Temporal.PlainDate.compare(a, b)` → -1/0/1 (ES Temporal's
/// `Temporal.CompareResult`). `temporal_rs::PlainDate::compare_iso` returns
/// `Ordering`; the two args are bound first so a plain `&__a`/`&__b` borrow
/// works whether they are locals or inline `Temporal.PlainDate.from(…)` calls.
/// The result is an ES `number` (`f64`).
fn plain_date_compare(args: &[Argument], ctx: &Ctx<'_>) -> Option<Expr> {
    let a = translate_argument(args.first()?, ctx);
    let b = translate_argument(args.get(1)?, ctx);
    Some(parse_quote!({
        let __a = #a;
        let __b = #b;
        match temporal_rs::PlainDate::compare_iso(&__a, &__b) {
            ::core::cmp::Ordering::Less => -1_f64,
            ::core::cmp::Ordering::Equal => 0_f64,
            ::core::cmp::Ordering::Greater => 1_f64,
        }
    }))
}
