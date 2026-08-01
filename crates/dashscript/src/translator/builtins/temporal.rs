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

use oxc_ast::ast::{Argument, StaticMemberExpression};
use proc_macro2::Span;
use syn::{parse_quote, Expr};

use super::super::bindings;
use super::super::context::Ctx;
use super::super::expressions::{temporal_type_of_local, translate_argument, translate_expr};

/// All Temporal types DashScript lowers to a `temporal_rs::<Type>`: the five
/// date/time types plus `Duration`, `Instant`, and `ZonedDateTime`. The single
/// recognition set behind `temporal_type_of_local` (instance-method dispatch)
/// and `temporal_from_type` (binding inference) — a local or inline
/// `Temporal.<Type>.from(…)` resolving to any of these routes its `.method()`
/// through `temporal_method`. The date/time types additionally share the
/// calendar/time accessor shape; the others' fields simply miss the accessor
/// table and fall through.
pub(in crate::translator) const TEMPORAL_TYPES: &[&str] = &[
    "PlainDate",
    "PlainDateTime",
    "PlainTime",
    "PlainYearMonth",
    "PlainMonthDay",
    "Duration",
    "Instant",
    "ZonedDateTime",
];

/// The date/time types that share an infallible single-arg `from_utf8`
/// constructor and the same calendar/time accessor shape (`year`/`hour`/…).
/// `Temporal.<Type>.from(s)` lowers to `temporal_rs::<Type>` for each. Single
/// source of truth — `infer.rs` (the binding's inferred type) and
/// `member.rs::is_temporal_local` (accessor dispatch) read `TEMPORAL_TYPES`,
/// while this list marks the accessor-bearing subset.
pub(in crate::translator) const TEMPORAL_DATE_TIME_TYPES: &[&str] = &[
    "PlainDate",
    "PlainDateTime",
    "PlainTime",
    "PlainYearMonth",
    "PlainMonthDay",
];

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
        // Single-arg `from_utf8`: the five date/time types + Duration + Instant
        // all share `from_utf8(&[u8]) -> TemporalResult<Self>` + the same
        // Err→DsError shape.
        ("Duration" | "Instant", "from") => temporal_from(ty, args, ctx),
        // `Temporal.ZonedDateTime.from(s)` — `from_utf8` takes the ES default
        // disambiguation (`Compatible`) and offset (`Reject`) options boa/perry
        // also default to; no options-bag overload is lowered statically.
        ("ZonedDateTime", "from") => zoned_date_time_from(args, ctx),
        ("PlainDate", "compare") => plain_date_compare(args, ctx),
        // `Temporal.Instant.compare(a, b)` — Instant derives `Ord`, so
        // `a.cmp(&b)` gives the ES -1/0/1 (boa does `one.cmp(&two) as i8`).
        ("Instant", "compare") => instant_compare(args, ctx),
        // `Temporal.Instant.fromEpochMilliseconds(n)` →
        // `Instant::from_epoch_milliseconds(i64)` (a `.ts` `number` is `f64`,
        // cast to `i64` — ES truncates the fractional part).
        ("Instant", "fromEpochMilliseconds") => instant_from_epoch_millis(args, ctx),
        _ if method == "from" && TEMPORAL_DATE_TIME_TYPES.contains(&ty) => {
            temporal_from(ty, args, ctx)
        }
        _ => None,
    }
}

/// `Temporal.<Type>.from(item)` for a single-arg `from_utf8` type (the five
/// date/time types + Duration + Instant).
///
/// ES semantics: a string parses via `from_utf8`; a non-string non-object
/// (number/boolean) is a `TypeError` (`ToTemporalDate`/`ToTemporalTime`/…
/// reject it); an object goes through the property-bag coercion. The static
/// path covers the string + throws cases — a string literal or a known-string
/// local parses (a malformed ISO string lowers to a `DsError`, the ES error
/// class); anything else lowers to a `TypeError` so `assert.throws(TypeError,…)`
/// matches. Full property-bag coercion is a larger batch.
fn temporal_from(ty: &str, args: &[Argument], ctx: &Ctx<'_>) -> Option<Expr> {
    let a = args.first()?;
    let ty = syn::Ident::new(ty, Span::call_site());
    if !is_string_arg(a, ctx) {
        return Some(parse_quote! {
            ::std::panic::panic_any(crate::__ds::DsError::new(
                "TypeError",
                "Temporal.from requires a string or Temporal object",
            ))
        });
    }
    let e = if let Argument::StringLiteral(s) = a {
        let lit = syn::LitStr::new(s.value.as_str(), Span::call_site());
        parse_quote!(#lit)
    } else {
        translate_argument(a, ctx)
    };
    let err_rhs = ds_panic_temporal_err();
    Some(parse_quote!({
        // A malformed ISO string is an ES `RangeError`/`SyntaxError`, not a
        // panic — lower the `TemporalResult::Err` to a `DsError` so `catch (e)`
        // recovers `e.constructor.name`/`e.name`/`e.message` (the temporal-rs
        // `ErrorKind` is the ES error class; `into_message` is the text).
        match temporal_rs::#ty::from_utf8((#e).as_bytes()) {
            Ok(__d) => __d,
            Err(__err) => #err_rhs,
        }
    }))
}

/// `Temporal.ZonedDateTime.from(s)` → `ZonedDateTime::from_utf8` with the ES
/// default disambiguation options (`Compatible` + `Reject` — the same defaults
/// boa/perry resolve when no options bag is passed). A non-string arg lowers to
/// a `TypeError`, matching `from`'s shape; a malformed string lowers to a
/// `DsError` (the `ErrorKind` is the ES error class).
fn zoned_date_time_from(args: &[Argument], ctx: &Ctx<'_>) -> Option<Expr> {
    let a = args.first()?;
    if !is_string_arg(a, ctx) {
        return Some(parse_quote! {
            ::std::panic::panic_any(crate::__ds::DsError::new(
                "TypeError",
                "Temporal.from requires a string or Temporal object",
            ))
        });
    }
    let e = if let Argument::StringLiteral(s) = a {
        let lit = syn::LitStr::new(s.value.as_str(), Span::call_site());
        parse_quote!(#lit)
    } else {
        translate_argument(a, ctx)
    };
    let err_rhs = ds_panic_temporal_err();
    Some(parse_quote!({
        match temporal_rs::ZonedDateTime::from_utf8(
            (#e).as_bytes(),
            temporal_rs::options::Disambiguation::Compatible,
            temporal_rs::options::OffsetDisambiguation::Reject,
        ) {
            Ok(__d) => __d,
            Err(__err) => #err_rhs,
        }
    }))
}

/// `Temporal.Instant.fromEpochMilliseconds(n)` →
/// `Instant::from_epoch_milliseconds(n as i64)`. Returns `TemporalResult`, so a
/// malformed value (e.g. non-finite, out of range) lowers to a `DsError` like
/// `from`'s Err; ES truncates a fractional `number` via the `as i64` cast.
fn instant_from_epoch_millis(args: &[Argument], ctx: &Ctx<'_>) -> Option<Expr> {
    let n = translate_argument(args.first()?, ctx);
    let err_rhs = ds_panic_temporal_err();
    Some(parse_quote!({
        match temporal_rs::Instant::from_epoch_milliseconds((#n) as i64) {
            Ok(__d) => __d,
            Err(__err) => #err_rhs,
        }
    }))
}

/// Whether `arg` is a string for `Temporal.<Type>.from` — a string literal or a
/// local inferred to be `String` (a string variable parses; a non-string local
/// or any other expression lowers to the `TypeError` path above).
fn is_string_arg(a: &Argument, ctx: &Ctx<'_>) -> bool {
    match a {
        Argument::StringLiteral(_) => true,
        Argument::Identifier(id) => ctx
            .local_type(&bindings::snake(&id.name).to_string())
            .is_some_and(|p| p.is_ident("String")),
        _ => false,
    }
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

/// `Temporal.Instant.compare(a, b)` → -1/0/1. `Instant` derives `Ord`, so
/// `__a.cmp(&__b)` is the ES `CompareEpochNanoseconds` ordering. The args are
/// bound first (locals or inline `Temporal.Instant.from(…)`); the result is an
/// ES `number` (`f64`).
fn instant_compare(args: &[Argument], ctx: &Ctx<'_>) -> Option<Expr> {
    let a = translate_argument(args.first()?, ctx);
    let b = translate_argument(args.get(1)?, ctx);
    Some(parse_quote!({
        let __a = #a;
        let __b = #b;
        match __a.cmp(&__b) {
            ::core::cmp::Ordering::Less => -1_f64,
            ::core::cmp::Ordering::Equal => 0_f64,
            ::core::cmp::Ordering::Greater => 1_f64,
        }
    }))
}

/// The Temporal types whose `toString`/`toJSON` map to `temporal_rs`'s
/// `Display` impl. The five date/time types carry Display except `PlainTime`
/// (no Display impl in `temporal_rs`); `Duration` has Display. `Instant` and
/// `ZonedDateTime` do not — their string forms need a time zone
/// (`to_ixdtf_string`), so they fall through (no static mapping).
fn temporal_has_display(ty: &str) -> bool {
    matches!(
        ty,
        "PlainDate" | "PlainDateTime" | "PlainYearMonth" | "PlainMonthDay" | "Duration"
    )
}

/// `panic_any(DsError::new(<ErrorKind→ES class>, msg))` — the shared right-hand
/// side of every `Err(__err)` arm that lowers a `temporal_rs::TemporalResult`
/// error to a `DsError` (so `catch (e)` recovers `e.constructor.name` — the
/// `ErrorKind` is the ES error class; `into_message` is the text). Used by
/// `from`/`zoned_date_time_from`/`ZonedDateTime.equals`.
fn ds_panic_temporal_err() -> Expr {
    parse_quote!(::std::panic::panic_any(crate::__ds::DsError::new(
        match __err.kind() {
            temporal_rs::error::ErrorKind::Generic => "Error",
            temporal_rs::error::ErrorKind::Type => "TypeError",
            temporal_rs::error::ErrorKind::Range => "RangeError",
            temporal_rs::error::ErrorKind::Syntax => "SyntaxError",
            temporal_rs::error::ErrorKind::Assert => "ImplementationError",
        },
        __err.into_message(),
    )))
}

/// A `Temporal.<Type>` instance method on a `temporal_rs::<Type>` receiver —
/// `d.toString()` / `d.toJSON()` / `d.equals(other)`. Dispatched on the
/// receiver resolving to a Temporal type; returns `None` for a non-Temporal
/// receiver or an unmapped name (falls through to a plain call → `cargo check`
/// rejects it honestly). `toString`/`toJSON` lower to `Display` for the types
/// that have it (see `temporal_has_display`); `equals` to `PartialEq` for the
/// date/time + Duration + Instant types (all derive it), and to the inherent
/// `equals` method for `ZonedDateTime` (no `PartialEq` derive).
pub(in crate::translator) fn temporal_method(
    sm: &StaticMemberExpression,
    args: &[Argument],
    ctx: &Ctx<'_>,
) -> Option<Expr> {
    let ty = temporal_type_of_local(&sm.object, ctx)?;
    let obj = translate_expr(&sm.object, ctx);
    Some(match sm.property.name.as_str() {
        // `d.toString()` / `d.toJSON()` → ISO string via `Display`. Only the
        // types with a Display impl; Instant/ZonedDateTime fall through (their
        // string forms need a time zone — no static mapping yet).
        "toString" | "toJSON" if temporal_has_display(&ty) => {
            parse_quote!(::std::string::ToString::to_string(&(#obj)))
        }
        // `z.equals(other)` → bool via the inherent `equals(&self, &Self)`
        // (ZonedDateTime derives no PartialEq). Its `TemporalResult` Err is
        // lowered to a DsError like `from`'s.
        "equals" if ty == "ZonedDateTime" => {
            let other = translate_argument(args.first()?, ctx);
            let err_rhs = ds_panic_temporal_err();
            parse_quote!({
                let __a = #obj;
                let __b = #other;
                match __a.equals(&__b) {
                    Ok(__eq) => __eq,
                    Err(__err) => #err_rhs,
                }
            })
        }
        // `d.equals(other)` → bool (`PartialEq`, derived on the date/time types
        // + Duration + Instant).
        "equals" => {
            let other = translate_argument(args.first()?, ctx);
            parse_quote!((#obj) == (#other))
        }
        _ => return None,
    })
}
