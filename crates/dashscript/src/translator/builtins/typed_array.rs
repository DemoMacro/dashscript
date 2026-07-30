//! ES `Uint8Array` / typed-array instance methods on a DashScript `Vec<u8>`.
//!
//! `Uint8Array` → `Vec<u8>` (see `expressions/new`); these methods dispatch on
//! the receiver's resolved type being `Vec<u8>`. Only `set` is mapped today:
//! ES `TypedArray.prototype.set(source, offset)` copies `source`'s bytes into
//! the receiver starting at `offset`. The receiver is pre-sized at the call
//! site (`new Uint8Array(h.length + 4)` then `buf.set(h, 0)`), so a Rust
//! `copy_from_slice` matches ES when the caller sized the buffer to fit — ES
//! would throw `RangeError` on overflow, but the buffer is sized exactly to
//! the source, so the slice copy never runs past the end.

use oxc_ast::ast::{Argument, StaticMemberExpression};
use syn::{parse_quote, Expr};

use super::super::context::Ctx;
use super::super::expressions::{is_vec_u8_local, translate_argument, translate_expr};

/// A `Uint8Array` instance method, dispatched on the receiver's type being
/// `Vec<u8>`. Returns `None` for a non-byte-buffer receiver or an unmapped
/// name (falls through to a plain call → `cargo check` rejects it honestly).
pub(in crate::translator) fn typed_array_method(
    sm: &StaticMemberExpression,
    args: &[Argument],
    ctx: &Ctx<'_>,
) -> Option<Expr> {
    if !is_vec_u8_local(&sm.object, ctx) {
        return None;
    }
    let name = sm.property.name.as_str();
    let obj = translate_expr(&sm.object, ctx);
    Some(match name {
        // `buf.set(source, offset = 0)` → copy `source`'s bytes into `buf`
        // starting at `offset`. The source is bound first so a source that
        // reads the same buffer (`buf.set(buf, …)`) cannot alias the `&mut`
        // borrow the slice copy takes — the immutable borrow ends at the `let`
        // before the mutable one starts.
        "set" => {
            let src = translate_argument(args.first()?, ctx);
            let off: Expr = match args.get(1) {
                Some(a) => {
                    let e = translate_argument(a, ctx);
                    parse_quote!((#e) as usize)
                }
                None => parse_quote!(0_usize),
            };
            parse_quote!({
                let __ds_src = #src;
                let __ds_off = #off;
                #obj[__ds_off..__ds_off + __ds_src.len()].copy_from_slice(&__ds_src);
            })
        }
        _ => return None,
    })
}
