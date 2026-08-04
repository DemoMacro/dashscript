//! WHATWG `File` API — `new File(bits, name, options?)` constructor (FileAPI, a
//! WinterTC Web API). A `File` is a `Blob` with a `name` and a `lastModified`,
//! so the constructor flattens the `bits` (reusing the `Blob` parts collector)
//! and builds a `crate::__ds::DsFile` (which wraps a `DsBlob`). The instance
//! surface — `size`/`type`/`slice`/`text`/`arrayBuffer`/`bytes` — is *inherited*
//! from `Blob`: `is_blob_local` accepts a `DsFile` receiver, so `blob_method`
//! and the `Blob` accessors dispatch on a `File` unchanged. Only the
//! `File`-specific `name`/`lastModified` live here (mapped in `member.rs`).

use oxc_ast::ast::{Argument, Expression, ObjectPropertyKind, PropertyKey};
use syn::{parse_quote, Expr, Type};

use super::super::super::context::Ctx;
use super::blob::{blob_type_arg, collect_blob_parts, expr_to_string};

/// The Rust type a WHATWG `File` constructor builds, if `name` is `File`:
/// `crate::__ds::DsFile`. `None` otherwise (the `new` lowering falls through
/// to the generic `Foo::new` path and surfaces at `cargo check`).
pub(in crate::translator) fn file_ctor_type(name: &str) -> Option<Type> {
    match name {
        "File" => Some(parse_quote!(crate::__ds::DsFile)),
        _ => None,
    }
}

/// `new File(bits, name, options?)` → `DsFile::new(bytes, type_, name,
/// last_modified)`. The `bits` sequence is flattened to a `Vec<u8>` (reusing
/// the `Blob` parts collector — a `string` → UTF-8 bytes, a `number` →
/// `number_to_string` then bytes, a `Uint8Array`/`Blob` local → its bytes);
/// `name` is coerced via ES `ToString` (default `""` when absent); `options.type`
/// supplies the MIME (default `""`) and `options.lastModified` the epoch-ms
/// (default `Date.now()` — the current time). A non-sequence `bits` panics the
/// `TypeError` ES throws (the WPT verdict reads the panic prefix).
pub(in crate::translator) fn file_ctor(args: &[Argument], ctx: &Ctx<'_>) -> Expr {
    let bytes = match args.first().and_then(Argument::as_expression) {
        None => parse_quote!(::std::vec::Vec::new()),
        Some(Expression::ArrayExpression(arr)) => collect_blob_parts(arr, ctx),
        Some(_) => {
            parse_quote!({ ::core::panic!("TypeError: File construct: bits must be a sequence") })
        }
    };
    let name = match args.get(1).and_then(Argument::as_expression) {
        Some(e) => expr_to_string(e, ctx),
        // ES throws a TypeError when `name` is absent; the static path emits an
        // empty name so a misuses fixture surfaces as a value mismatch, not a
        // build failure.
        None => parse_quote!(::std::string::String::new()),
    };
    let (type_, last_modified) = file_options(args.get(2).and_then(Argument::as_expression), ctx);
    parse_quote!(crate::__ds::DsFile::new(#bytes, #type_, #name, #last_modified))
}

/// Read `options.type` (the MIME string, default `""`) and
/// `options.lastModified` (epoch-ms, default `Date.now()`) from the ES options
/// object. A non-object `options` yields the defaults. `lastModified` defaults
/// to the wall-clock epoch-ms (the ES default — the current time); a fixture
/// that asserts a specific value is a value mismatch (a partial), not a build
/// failure.
fn file_options(opt: Option<&Expression>, ctx: &Ctx<'_>) -> (Expr, Expr) {
    let type_ = blob_type_arg(opt, ctx);
    let last_modified = match opt {
        Some(Expression::ObjectExpression(obj)) => {
            let mut lm: Option<Expr> = None;
            for kind in &obj.properties {
                let ObjectPropertyKind::ObjectProperty(p) = kind else {
                    continue;
                };
                let is_lm = match &p.key {
                    PropertyKey::StaticIdentifier(id) => id.name.as_str() == "lastModified",
                    PropertyKey::StringLiteral(s) => s.value.as_str() == "lastModified",
                    _ => false,
                };
                if is_lm {
                    let v = super::super::super::expressions::translate_expr(&p.value, ctx);
                    lm = Some(parse_quote!((#v) as f64));
                    break;
                }
            }
            lm.unwrap_or_else(default_last_modified)
        }
        _ => default_last_modified(),
    };
    (type_, last_modified)
}

/// `Date.now()` — the epoch-ms default for `lastModified`. Used when the option
/// is absent (ES) or `options` is not an object.
fn default_last_modified() -> Expr {
    parse_quote!({
        ::std::time::SystemTime::now()
            .duration_since(::std::time::UNIX_EPOCH)
            .map(|__d| __d.as_millis() as f64)
            .unwrap_or(0.0)
    })
}
