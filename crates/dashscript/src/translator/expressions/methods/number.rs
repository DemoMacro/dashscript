//! Methods on a `.ds` `number` (`f64`).

use oxc_ast::ast::{Argument, StaticMemberExpression};
use proc_macro2::Span;
use syn::{parse_quote, Expr};

use super::super::super::context::Ctx;
use super::super::translate_expr;
use super::usize_arg;

/// Methods on a `.ds` `number` (`f64`). `.toFixed(n)` → a formatted string
/// with `n` decimal places. Returns `None` for an unmapped name.
pub(in crate::translator::expressions) fn number_method(
    sm: &StaticMemberExpression,
    args: &[Argument],
    ctx: &Ctx<'_>,
) -> Option<Expr> {
    let recv = translate_expr(&sm.object, ctx);
    Some(match sm.property.name.as_str() {
        // `(3.14).toFixed(2)` → `format!("{:.*}", n, …)`. In Rust the `*`
        // precision argument comes first, the value second.
        "toFixed" => {
            let prec = usize_arg(args.first()?, ctx);
            parse_quote!(format!("{:.*}", #prec, #recv))
        }
        // `(255).toString(radix)` → a format with the matching base. Only a
        // literal radix of 2/8/16/10 maps; the f64 is cast to `u32` first (TS
        // truncates the fractional part). A non-literal radix is unmapped.
        "toString" if !args.is_empty() => {
            let radix = match args.first()? {
                Argument::NumericLiteral(n) => n.value,
                _ => return None,
            };
            let fmt = match radix as u64 {
                16 => "{:x}",
                2 => "{:b}",
                8 => "{:o}",
                10 => "{}",
                _ => return None,
            };
            let fmt_lit = syn::LitStr::new(fmt, Span::call_site());
            parse_quote!(format!(#fmt_lit, (#recv) as u32))
        }
        _ => return None,
    })
}
