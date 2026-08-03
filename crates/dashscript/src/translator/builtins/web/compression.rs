//! WHATWG `CompressionStream` API (a WinterTC Web API) — the compression side
//! of the Streams standard. `new CompressionStream(format)` builds a byte
//! transform whose `writable`/`readable` sides share one state: `writer.write(
//! bytes)` buffers, `writer.close()` runs the one-shot `flate2` compression,
//! and `reader.read()` yields the single compressed chunk. The transform is
//! **internal** (`flate2`, never a user closure), so this sidesteps the
//! `'static`-capture blocker that gates a general user-sink `WritableStream`.
//! The instance methods (`getWriter`/`write`/`close`/`getReader`/`read`) lower
//! as plain method calls on the handle types — only the constructor needs a
//! dispatch arm (`expressions/new`), and the `writable`/`readable` fields are
//! plain `pub` field access. Backed by `flate2` via the `Compression` runtime
//! dep; pure-Rust static track, never degraded. `DecompressionStream` shares
//! this model (same `DsCompressionStream` type, a `Decompress` direction);
//! `brotli`/true streaming remain honest partials.

use oxc_ast::ast::{Argument, Expression, StaticMemberExpression};
use syn::{parse_quote, Expr, Type};

use super::super::super::bindings;
use super::super::super::context::Ctx;
use super::super::super::expressions::{translate_argument, translate_expr};

/// The Rust type a `CompressionStream` constructor builds: `CompressionStream`
/// → `crate::__ds::DsCompressionStream`. `None` for any other name.
pub(in crate::translator) fn compression_ctor_type(name: &str) -> Option<Type> {
    match name {
        // Both lower to `DsCompressionStream` — the writable/readable/writer/
        // reader containers are direction-agnostic; only the constructor's
        // direction arg (and so `close()`'s codec run) differs.
        "CompressionStream" | "DecompressionStream" => {
            Some(parse_quote!(crate::__ds::DsCompressionStream))
        }
        _ => None,
    }
}

/// `new CompressionStream(format)` → `DsCompressionStream::new(variant, Compress)`,
/// `new DecompressionStream(format)` → `DsCompressionStream::new(variant, Decompress)`.
/// The format must be a string literal `gzip`/`deflate`/`deflate-raw` (the
/// three `flate2` mappings); `brotli` (no `flate2` backend) and a non-literal
/// format have no static form → `None`, so `expressions/new` falls through to
/// the generic `Foo::new` path (E0433 — an honest unsupported).
pub(in crate::translator) fn compression_stream_ctor(
    name: &str,
    args: &[Argument],
) -> Option<Expr> {
    let fmt = format_variant(args.first()?.as_expression()?)?;
    let dir: Expr = match name {
        "CompressionStream" => parse_quote!(crate::__ds::DsCodecDir::Compress),
        "DecompressionStream" => parse_quote!(crate::__ds::DsCodecDir::Decompress),
        _ => return None,
    };
    Some(parse_quote!(crate::__ds::DsCompressionStream::new(#fmt, #dir)))
}

/// Map a `gzip`/`deflate`/`deflate-raw` string literal to its
/// `DsCompressionFormat` variant. `None` for any other value (or a non-literal).
fn format_variant(expr: &Expression) -> Option<Expr> {
    let Expression::StringLiteral(s) = expr else {
        return None;
    };
    match s.value.as_str() {
        "gzip" => Some(parse_quote!(crate::__ds::DsCompressionFormat::Gzip)),
        "deflate" => Some(parse_quote!(crate::__ds::DsCompressionFormat::Deflate)),
        "deflate-raw" => Some(parse_quote!(crate::__ds::DsCompressionFormat::DeflateRaw)),
        _ => None,
    }
}

/// A `CompressionStream` instance method, dispatched on the receiver's resolved
/// shape. Returns `None` for an unmapped receiver or name, so the call falls
/// through to a plain method call (cargo check rejects it honestly). Covers the
/// one-shot read/write loop: `cs.writable.getWriter()`, `cs.readable.getReader()`
/// (receiver is the `<DsCompressionStream>.writable`/`.readable` field), and
/// `writer.write(chunk)`/`writer.close()`/`reader.read()` (receiver is the
/// writer/reader local whose type `callee_return_path` registered).
pub(in crate::translator) fn compression_method(
    sm: &StaticMemberExpression,
    args: &[Argument],
    ctx: &Ctx<'_>,
) -> Option<Expr> {
    let obj = translate_expr(&sm.object, ctx);
    let name = sm.property.name.as_str();
    // `cs.writable.getWriter()` — receiver is the `writable` field of a
    // `DsCompressionStream` local (plain pub field access).
    if name == "getWriter" && args.is_empty() && is_compression_side(&sm.object, ctx, "writable") {
        return Some(parse_quote!(#obj.get_writer()));
    }
    // `cs.readable.getReader()` — receiver is the `readable` field.
    if name == "getReader" && args.is_empty() && is_compression_side(&sm.object, ctx, "readable") {
        return Some(parse_quote!(#obj.get_reader()));
    }
    // `writer.write(chunk)` on a `DsCompressionWriter` local.
    if name == "write" && is_compression_local(&sm.object, ctx, "DsCompressionWriter") {
        let chunk = translate_argument(args.first()?, ctx);
        return Some(parse_quote!(#obj.write(#chunk)));
    }
    // `writer.close()` on a `DsCompressionWriter` local (self-consuming — the
    // helper's `close(self)` runs the one-shot `flate2` compression).
    if name == "close"
        && args.is_empty()
        && is_compression_local(&sm.object, ctx, "DsCompressionWriter")
    {
        return Some(parse_quote!(#obj.close()));
    }
    // `reader.read()` on a `DsCompressionReader` local.
    if name == "read"
        && args.is_empty()
        && is_compression_local(&sm.object, ctx, "DsCompressionReader")
    {
        return Some(parse_quote!(#obj.read()));
    }
    None
}

/// True when `expr` is `<DsCompressionStream local>.<side>` (the `writable`/
/// `readable` pub field), so `expr.getWriter()`/`.getReader()` dispatch on the
/// field's handle type.
fn is_compression_side(expr: &Expression, ctx: &Ctx<'_>, side: &str) -> bool {
    let Expression::StaticMemberExpression(sm) = expr else {
        return false;
    };
    if sm.property.name.as_str() != side {
        return false;
    }
    let Expression::Identifier(id) = &sm.object else {
        return false;
    };
    let name = bindings::snake(id.name.as_str()).to_string();
    ctx.local_type(&name).is_some_and(|p| {
        p.segments
            .last()
            .is_some_and(|s| s.ident == "DsCompressionStream")
    })
}

/// True when `expr` is a local whose recorded type's last segment is `type_`
/// (a `DsCompressionWriter`/`DsCompressionReader` binding, registered by
/// `callee_return_path` from `getWriter()`/`getReader()`).
fn is_compression_local(expr: &Expression, ctx: &Ctx<'_>, type_: &str) -> bool {
    let Expression::Identifier(id) = expr else {
        return false;
    };
    let name = bindings::snake(id.name.as_str()).to_string();
    ctx.local_type(&name)
        .is_some_and(|p| p.segments.last().is_some_and(|s| s.ident == type_))
}
