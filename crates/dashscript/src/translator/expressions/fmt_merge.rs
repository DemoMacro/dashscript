//! Inlining nested `format!` calls into an outer `format!`/`println!`.
//!
//! A translated argument that is itself a `format!(…)` call must not become a
//! `{}` placeholder wrapped in another `format!`/`println!` — clippy's
//! `format_in_format_args` flags the redundancy. Instead the inner format
//! string and its arguments are spliced into the outer one, with positional
//! placeholders renumbered so they keep pointing at the right argument slot.
//!
//! Likewise a `.to_string()` used only as a `{}` argument is redundant — `{}`
//! already renders via `Display`, and `.to_string()` itself requires `Display`
//! — so it is stripped.

use proc_macro2::TokenStream;
use syn::{parse_quote, Expr};

/// One translated argument, classified for splicing into an outer format string.
pub(super) enum Inlined {
    /// The argument is a `format!(fmt, args…)` call: splice `fmt` (renumbered)
    /// into the outer string and append `args` to the outer argument list.
    Format { fmt: String, args: Vec<Expr> },
    /// A plain `{}` argument. Redundant `.to_string()` has been stripped.
    Display(Expr),
}

/// Classify a translated argument: a `format!` call is inlined; otherwise any
/// redundant trailing `.to_string()` is stripped and the value is shown via `{}`.
pub(super) fn inline_arg(expr: Expr) -> Inlined {
    if let Some((fmt, args)) = extract_format_call(&expr) {
        Inlined::Format { fmt, args }
    } else {
        Inlined::Display(strip_redundant_to_string(expr))
    }
}

/// If `expr` is `format!(fmt_lit, args…)`, return its format string and args.
fn extract_format_call(expr: &Expr) -> Option<(String, Vec<Expr>)> {
    let Expr::Macro(em) = expr else {
        return None;
    };
    // The translator emits `format!(…)` (single-segment path).
    let last = em.mac.path.segments.last()?;
    if last.ident != "format" {
        return None;
    }
    let mut tokens = em.mac.tokens.clone().into_iter();
    // The first token must be the format-string literal.
    let lit = match tokens.next()? {
        proc_macro2::TokenTree::Literal(l) => l,
        _ => return None,
    };
    let lit_str =
        syn::parse2::<syn::LitStr>(proc_macro2::TokenTree::Literal(lit).into()).ok()?;
    let fmt = lit_str.value();
    // Everything after the literal, split on top-level commas. `Group` tokens
    // are atomic under `into_iter`, so commas inside `()`/`[]`/`{}` stay nested.
    let rest: TokenStream = tokens.collect();
    Some((fmt, split_args(rest)))
}

fn split_args(tokens: TokenStream) -> Vec<Expr> {
    let mut args = Vec::new();
    let mut current = TokenStream::new();
    for tt in tokens.into_iter() {
        if let proc_macro2::TokenTree::Punct(p) = &tt {
            if p.as_char() == ',' {
                if let Ok(e) = syn::parse2::<Expr>(std::mem::take(&mut current)) {
                    args.push(e);
                }
                continue;
            }
        }
        current.extend([tt]);
    }
    if let Ok(e) = syn::parse2::<Expr>(current) {
        args.push(e);
    }
    args
}

/// Drop a redundant trailing `.to_string()`: `{}` renders via `Display`, and
/// `.to_string()` requires `Display`, so the receiver is always safe to show.
/// Indexing a `String`/`&str` yields an unsized `str`, so such a receiver is
/// borrowed (`&recv`) to stay sized; other receivers are shown directly.
fn strip_redundant_to_string(expr: Expr) -> Expr {
    if let Expr::MethodCall(mc) = &expr {
        if mc.method == "to_string" && mc.args.is_empty() {
            let recv = &*mc.receiver;
            return if matches!(recv, Expr::Index(_)) {
                parse_quote!(&#recv)
            } else {
                (*mc.receiver).clone()
            };
        }
    }
    expr
}

/// Renumber every positional argument in a format string by `offset`, so it can
/// be spliced into an outer `format!` whose argument list already holds `offset`
/// items. Named arguments are left untouched.
///
/// Implicit `{}` placeholders and spec-level `*` (width/precision) become
/// explicit `N$` references in source order; explicit `N`/`N$` gain the offset.
/// Covers the shapes the translator emits: `{}`, `{:.*}`, `{:x}`, `{:>1$}`.
pub(super) fn renumber_format(fmt: &str, offset: usize) -> String {
    let mut out = String::with_capacity(fmt.len());
    let mut chars = fmt.chars().peekable();
    // Next implicit argument index, local to this format string.
    let mut implicit = 0usize;
    while let Some(c) = chars.next() {
        if (c == '{' || c == '}') && chars.peek() == Some(&c) {
            // Escaped `{{` / `}}`.
            out.push(c);
            out.push(c);
            chars.next();
            continue;
        }
        if c == '{' {
            // Argument field: up to ':' or '}'.
            let mut arg = String::new();
            while let Some(&p) = chars.peek() {
                if p == ':' || p == '}' {
                    break;
                }
                arg.push(p);
                chars.next();
            }
            let has_spec = chars.peek() == Some(&':');
            let mut spec = String::new();
            if has_spec {
                chars.next();
                spec = renumber_spec(&mut chars, offset, &mut implicit);
            }
            out.push('{');
            out.push_str(&render_arg(&arg, offset, &mut implicit));
            if has_spec {
                out.push(':');
                out.push_str(&spec);
            }
            out.push('}');
            // Consume the original closing brace.
            if chars.peek() == Some(&'}') {
                chars.next();
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Render a placeholder's argument field (`arg` is the text before `:`/`}`).
fn render_arg(arg: &str, offset: usize, implicit: &mut usize) -> String {
    if arg.is_empty() {
        let n = *implicit + offset;
        *implicit += 1;
        n.to_string()
    } else if let Ok(n) = arg.parse::<usize>() {
        (n + offset).to_string()
    } else {
        // Named argument — keep as-is.
        arg.to_string()
    }
}

/// Rewrite a format spec (`:`…`}`): `*` becomes an explicit `(implicit)$`
/// reference; `N$` becomes `(N+offset)$`; everything else is copied verbatim.
fn renumber_spec(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    offset: usize,
    implicit: &mut usize,
) -> String {
    let mut out = String::new();
    while let Some(&c) = chars.peek() {
        if c == '}' {
            break;
        }
        chars.next();
        if c == '*' {
            let n = *implicit + offset;
            *implicit += 1;
            out.push_str(&n.to_string());
            out.push('$');
        } else if c.is_ascii_digit() {
            let mut num = String::new();
            num.push(c);
            while let Some(&d) = chars.peek() {
                if d.is_ascii_digit() {
                    num.push(d);
                    chars.next();
                } else {
                    break;
                }
            }
            if chars.peek() == Some(&'$') {
                chars.next();
                let n: usize = num.parse().unwrap_or(0);
                out.push_str(&(n + offset).to_string());
                out.push('$');
            } else {
                // Literal width digit — not an argument reference.
                out.push_str(&num);
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::renumber_format;

    #[test]
    fn no_placeholders() {
        assert_eq!(renumber_format("hi", 0), "hi");
        assert_eq!(renumber_format("hi", 5), "hi");
    }

    #[test]
    fn implicit_placeholder() {
        assert_eq!(renumber_format("{}", 0), "{0}");
        assert_eq!(renumber_format("{}", 3), "{3}");
    }

    #[test]
    fn two_implicit_placeholders() {
        assert_eq!(renumber_format("{} {}", 0), "{0} {1}");
        assert_eq!(renumber_format("{} {}", 2), "{2} {3}");
    }

    #[test]
    fn escaped_braces() {
        assert_eq!(renumber_format("{{}}", 0), "{{}}");
        assert_eq!(renumber_format("[{{{}}}]", 1), "[{{{1}}}]");
    }

    #[test]
    fn explicit_positional() {
        assert_eq!(renumber_format("{0}", 2), "{2}");
        assert_eq!(renumber_format("{1} {0}", 1), "{2} {1}");
    }

    #[test]
    fn type_spec_to_string_radix() {
        // `{:x}` — value is the single implicit arg.
        assert_eq!(renumber_format("{:x}", 0), "{0:x}");
        assert_eq!(renumber_format("{:x}", 1), "{1:x}");
    }

    #[test]
    fn precision_star_to_fixed() {
        // `{:.*}` — precision `*` is arg0, value is arg1.
        assert_eq!(renumber_format("{:.*}", 0), "{1:.0$}");
        assert_eq!(renumber_format("{:.*}", 2), "{3:.2$}");
    }

    #[test]
    fn width_ref_pad_start() {
        // `{:>1$}` — value is arg0, width references arg1.
        assert_eq!(renumber_format("{:>1$}", 0), "{0:>1$}");
        assert_eq!(renumber_format("{:>1$}", 2), "{2:>3$}");
    }

    #[test]
    fn splice_concat_wrapper() {
        assert_eq!(renumber_format("[{}]", 0), "[{0}]");
        assert_eq!(renumber_format("[{}]", 2), "[{2}]");
    }
}
