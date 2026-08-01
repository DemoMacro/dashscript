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

use oxc_ast::ast::{Argument, CallExpression, Expression, StaticMemberExpression};
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
        // `Temporal.<Type>.compare(a, b)` → -1/0/1. `compare_iso` for the
        // ISO-field types (date/time/year-month), `compare_instant` for
        // ZonedDateTime (compares the exact instant), `a.cmp(&b)` for the
        // `Ord`-deriving scalar types (Instant, PlainTime). See
        // [`temporal_compare`].
        ("PlainDate" | "PlainDateTime" | "PlainYearMonth", "compare") => {
            let ty = syn::Ident::new(ty, Span::call_site());
            temporal_compare(
                args,
                ctx,
                |a: Expr, b: Expr| parse_quote!(temporal_rs::#ty::compare_iso(&(#a), &(#b))),
            )
        }
        ("ZonedDateTime", "compare") => temporal_compare(
            args,
            ctx,
            |a: Expr, b: Expr| parse_quote!(temporal_rs::ZonedDateTime::compare_instant(&(#a), &(#b))),
        ),
        ("Instant" | "PlainTime", "compare") => {
            temporal_compare(args, ctx, |a: Expr, b: Expr| parse_quote!((#a).cmp(&(#b))))
        }
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

/// Classify-time mirror of [`temporal_static`]'s match — true when
/// `Temporal.<ty>.<method>` has a static lowering. The classify table routes a
/// mapped pair to the static path (zero-cost `temporal_rs`) and an unmapped
/// pair to the engine (polyfill), so this must stay in lockstep with
/// `temporal_static`'s arms (pinned by the `drift` module's
/// `static_maps_match_emit_arms` test). Argument compatibility — a property-bag coercion like
/// `from({year,month})` — is decided separately in `classify`; a mapped pair
/// whose args force a coercion still degrades.
pub(in crate::translator) fn temporal_static_maps(ty: &str, method: &str) -> bool {
    matches!(
        (ty, method),
        ("Duration" | "Instant", "from")
            | ("ZonedDateTime", "from")
            | ("PlainDate" | "PlainDateTime" | "PlainYearMonth", "compare")
            | ("ZonedDateTime", "compare")
            | ("Instant" | "PlainTime", "compare")
            | ("Instant", "fromEpochMilliseconds")
    ) || (method == "from" && TEMPORAL_DATE_TIME_TYPES.contains(&ty))
}

/// Classify-time mirror of [`temporal_new`]'s match — true when
/// `new Temporal.<ty>(…)` has a static ISO-field lowering (the four date/time
/// types whose constructors take integer ISO fields). Drift-pinned against
/// `temporal_new` (the `drift` module's `new_maps_match_emit_arms` test).
pub(in crate::translator) fn temporal_new_maps(ty: &str) -> bool {
    matches!(
        ty,
        "PlainDate" | "PlainDateTime" | "PlainTime" | "PlainYearMonth"
    )
}

/// `(ty, method)` for a `Temporal.<ty>.<method>(…)` call callee, or `None` if
/// `expr` is not that two-level member shape. Used by `classify` to look up
/// [`temporal_static_maps`] without re-walking the chain. The returned strings
/// borrow the oxc arena (the property `Atom`s), not `expr` itself.
pub(in crate::translator) fn temporal_callee_split<'a>(
    expr: &Expression<'a>,
) -> Option<(&'a str, &'a str)> {
    let Expression::StaticMemberExpression(method_sm) = expr else {
        return None;
    };
    let Expression::StaticMemberExpression(type_sm) = &method_sm.object else {
        return None;
    };
    let Expression::Identifier(id) = &type_sm.object else {
        return None;
    };
    (id.name.as_str() == "Temporal").then(|| {
        (
            type_sm.property.name.as_str(),
            method_sm.property.name.as_str(),
        )
    })
}

/// The Temporal type a binding's initializer resolves to, when it is a
/// `Temporal.<Type>.from(…)` (the only static call that yields a Temporal
/// value — `compare` yields a number) or a `new Temporal.<Type>(…)`. Used by
/// `check` to record the local's type so `classify` routes a later
/// `Temporal.X.compare(a, b)` / `from(arg)` through the static `temporal_rs`
/// mapping only when the arg genuinely is that Temporal type, degrading an
/// unknown local to the polyfill rather than risking a cargo type error.
pub(in crate::translator) fn temporal_init_type(init: &Expression) -> Option<String> {
    match init {
        Expression::CallExpression(c) => temporal_call_result_type(c),
        Expression::NewExpression(n) => temporal_type_of_callee(&n.callee),
        _ => None,
    }
}

/// The Temporal type yielded by a `Temporal.<Type>.<method>(…)` static call —
/// `from` for any `<Type>` (the constructor); other mapped calls (`compare`,
/// `fromEpochMilliseconds`) yield a number, so they return `None`.
fn temporal_call_result_type(c: &CallExpression) -> Option<String> {
    let (ty, method) = temporal_callee_split(&c.callee)?;
    (method == "from").then(|| ty.to_string())
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

/// `Temporal.<Type>.compare(a, b)` → ES -1/0/1 (`f64`). `cmp` builds the
/// `core::cmp::Ordering` expression from the two operands — the receiver
/// type's `compare_iso` (ISO-field types: PlainDate/PlainDateTime/
/// PlainYearMonth), `compare_instant` (ZonedDateTime), or `.cmp(…)` for an
/// `Ord`-deriving scalar type (Instant, PlainTime). The operands are passed
/// by value and lowered inline (the closure wraps each in `&(…)`, so a
/// `compare(a, a)` self-comparison borrows twice rather than moving —
/// `temporal_rs` Temporal values are not `Copy`). Returns `f64` (the ES
/// `number`).
fn temporal_compare(
    args: &[Argument],
    ctx: &Ctx<'_>,
    cmp: impl Fn(Expr, Expr) -> Expr,
) -> Option<Expr> {
    let a = translate_argument(args.first()?, ctx);
    let b = translate_argument(args.get(1)?, ctx);
    let cmp_expr = cmp(a, b);
    Some(parse_quote!({
        match #cmp_expr {
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

/// `new Temporal.<Type>(isoFields…)` → `temporal_rs::<Type>::new(…)`. The four
/// date/time types whose constructors take ISO integer fields lower here; each
/// field casts from a `.ts` `number` (`f64`), and a trailing-missing arg pads
/// to `0` (ES `ToInteger(undefined) = 0`). PlainDate/PlainDateTime/
/// PlainYearMonth pass `Calendar::ISO` (the ES iso8601 default); PlainYearMonth
/// passes `None` for the optional `referenceDay` when only year/month are
/// given. Returns `None` for any other callee/type — `Instant` is epoch-only,
/// `ZonedDateTime` needs a time zone, `Duration` takes 10 components, and
/// `PlainMonthDay`'s constructor needs a reference year — so they fall through
/// to the generic path honestly.
pub(in crate::translator) fn temporal_new(
    callee: &Expression,
    args: &[Argument],
    ctx: &Ctx<'_>,
) -> Option<Expr> {
    let ty = temporal_type_of_callee(callee)?;
    let err_rhs = ds_panic_temporal_err();
    Some(match ty.as_str() {
        "PlainDate" => {
            let y = iso_field(args, 0, "i32", ctx);
            let m = iso_field(args, 1, "u8", ctx);
            let d = iso_field(args, 2, "u8", ctx);
            temporal_unwrap(
                parse_quote!(temporal_rs::PlainDate::new(#y, #m, #d, temporal_rs::Calendar::ISO)),
                err_rhs,
            )
        }
        "PlainDateTime" => {
            let y = iso_field(args, 0, "i32", ctx);
            let mo = iso_field(args, 1, "u8", ctx);
            let d = iso_field(args, 2, "u8", ctx);
            let h = iso_field(args, 3, "u8", ctx);
            let mi = iso_field(args, 4, "u8", ctx);
            let s = iso_field(args, 5, "u8", ctx);
            let ms = iso_field(args, 6, "u16", ctx);
            let us = iso_field(args, 7, "u16", ctx);
            let ns = iso_field(args, 8, "u16", ctx);
            temporal_unwrap(
                parse_quote!(temporal_rs::PlainDateTime::new(
                    #y, #mo, #d, #h, #mi, #s, #ms, #us, #ns,
                    temporal_rs::Calendar::ISO,
                )),
                err_rhs,
            )
        }
        "PlainTime" => {
            let h = iso_field(args, 0, "u8", ctx);
            let mi = iso_field(args, 1, "u8", ctx);
            let s = iso_field(args, 2, "u8", ctx);
            let ms = iso_field(args, 3, "u16", ctx);
            let us = iso_field(args, 4, "u16", ctx);
            let ns = iso_field(args, 5, "u16", ctx);
            temporal_unwrap(
                parse_quote!(temporal_rs::PlainTime::new(#h, #mi, #s, #ms, #us, #ns)),
                err_rhs,
            )
        }
        "PlainYearMonth" => {
            let y = iso_field(args, 0, "i32", ctx);
            let m = iso_field(args, 1, "u8", ctx);
            temporal_unwrap(
                parse_quote!(temporal_rs::PlainYearMonth::new(
                    #y,
                    #m,
                    ::core::option::Option::None,
                    temporal_rs::Calendar::ISO,
                )),
                err_rhs,
            )
        }
        _ => return None,
    })
}

/// The `<Type>` name of a `Temporal.<Type>` callee, or `None` if `callee` is
/// not that shape. Used by [`temporal_new`] to dispatch `new
/// Temporal.<Type>(…)`. Returns an owned `String` so the oxc AST's arena
/// lifetimes stay inside this function. Exposed for `infer.rs` to give a `new
/// Temporal.<Type>(…)` binding the matching `temporal_rs::<Type>` so accessors
/// (`dt.year`/…) dispatch.
pub(in crate::translator) fn temporal_type_of_callee(callee: &Expression) -> Option<String> {
    let Expression::StaticMemberExpression(sm) = callee else {
        return None;
    };
    let Expression::Identifier(id) = &sm.object else {
        return None;
    };
    (id.name.as_str() == "Temporal").then(|| sm.property.name.to_string())
}

/// One ISO integer field for a `new Temporal.<Type>(…)` constructor: the arg
/// at `idx` cast to its target integer type (`(arg) as i32`/`u8`/`u16`), or a
/// typed `0` literal when absent — ES `ToInteger(undefined) = 0`, and the
/// `temporal_rs` constructors require every field. The cast from a `.ts`
/// `number` (`f64`) truncates toward zero, matching ES `ToInt32`/`ToUint8`/
/// `ToUint16` for the non-fractional fields Temporal uses.
fn iso_field(args: &[Argument], idx: usize, ty: &str, ctx: &Ctx<'_>) -> Expr {
    let ty_id = syn::Ident::new(ty, Span::call_site());
    match args.get(idx) {
        Some(a) => {
            let v = translate_argument(a, ctx);
            parse_quote!((#v) as #ty_id)
        }
        None => parse_quote!(0 as #ty_id),
    }
}

/// Wrap a `temporal_rs` `TemporalResult<Self>` constructor so its `Err` lowers
/// to a `DsError` (the `ErrorKind` is the ES error class; `into_message` the
/// text) — the shared Ok/Err match used by `temporal_new`. `err_rhs` is from
/// [`ds_panic_temporal_err`]; `call` is the constructor expression.
fn temporal_unwrap(call: Expr, err_rhs: Expr) -> Expr {
    parse_quote!({
        match #call {
            Ok(__d) => __d,
            Err(__err) => #err_rhs,
        }
    })
}

#[cfg(test)]
mod drift {
    //! Classify↔emit drift guard for the Temporal static mappings. The classify
    //! mirrors ([`super::temporal_static_maps`] / [`super::temporal_new_maps`])
    //! must report `true` for exactly the pairs the emit arms
    //! ([`super::temporal_static`] / [`super::temporal_new`]) lower. Each
    //! contract below is the emit side hand-transcribed; the tests pin that the
    //! mirror neither lags (a mapped pair needlessly routed to the engine) nor
    //! leads (a claimed static lowering with no emit arm). Adding an emit arm
    //! means adding its pair here AND in the matching `*_maps` function — both
    //! tests fail on drift.

    use super::{temporal_new_maps, temporal_static_maps};

    /// Every `(ty, method)` pair the `temporal_static` emit match lowers.
    const STATIC_CONTRACT: &[(&str, &str)] = &[
        // `from` (single-arg `from_utf8`): date/time types + Duration + Instant.
        ("PlainDate", "from"),
        ("PlainDateTime", "from"),
        ("PlainTime", "from"),
        ("PlainYearMonth", "from"),
        ("PlainMonthDay", "from"),
        ("Duration", "from"),
        ("Instant", "from"),
        // `from` (ZonedDateTime takes disambiguation options).
        ("ZonedDateTime", "from"),
        // `compare` (ISO-field `compare_iso`).
        ("PlainDate", "compare"),
        ("PlainDateTime", "compare"),
        ("PlainYearMonth", "compare"),
        // `compare` (`compare_instant`).
        ("ZonedDateTime", "compare"),
        // `compare` (`Ord`-deriving scalar `.cmp`).
        ("Instant", "compare"),
        ("PlainTime", "compare"),
        // `Instant::from_epoch_milliseconds`.
        ("Instant", "fromEpochMilliseconds"),
    ];

    /// Pairs with no `temporal_static` arm — the mirror must not claim one.
    const STATIC_NEGATIVE: &[(&str, &str)] = &[
        ("Duration", "compare"),
        ("PlainMonthDay", "compare"),
        ("ZonedDateTime", "fromEpochMilliseconds"),
        ("PlainDate", "toJSON"),
        ("PlainDate", "fromEpochSeconds"),
    ];

    /// The `ty` values the `temporal_new` emit match lowers (ISO-field ctors).
    const NEW_CONTRACT: &[&str] = &["PlainDate", "PlainDateTime", "PlainTime", "PlainYearMonth"];

    /// Types with no `temporal_new` arm (epoch/time-zone/10-arity/ref-year only).
    const NEW_NEGATIVE: &[&str] = &["Duration", "Instant", "ZonedDateTime", "PlainMonthDay"];

    #[test]
    fn static_maps_match_emit_arms() {
        for &(ty, method) in STATIC_CONTRACT {
            assert!(
                temporal_static_maps(ty, method),
                "temporal_static_maps({ty:?}, {method:?}) = false, but `temporal_static` \
                 lowers it — add the pair to the classify mirror so the static path is \
                 not needlessly sent to the engine"
            );
        }
        for &(ty, method) in STATIC_NEGATIVE {
            assert!(
                !temporal_static_maps(ty, method),
                "temporal_static_maps({ty:?}, {method:?}) = true, but `temporal_static` \
                 has no arm for it — the mirror would route to a non-existent lowering"
            );
        }
    }

    #[test]
    fn new_maps_match_emit_arms() {
        for &ty in NEW_CONTRACT {
            assert!(
                temporal_new_maps(ty),
                "temporal_new_maps({ty:?}) = false, but `temporal_new` lowers it — add \
                 the type to the classify mirror"
            );
        }
        for &ty in NEW_NEGATIVE {
            assert!(
                !temporal_new_maps(ty),
                "temporal_new_maps({ty:?}) = true, but `temporal_new` has no arm for it"
            );
        }
    }
}
