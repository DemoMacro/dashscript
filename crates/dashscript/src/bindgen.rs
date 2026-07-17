//! Rust source → `.ds` type declaration.
//!
//! Powers `ds add`: inspect a crate's public surface and emit a `.ds`
//! declaration so importing it yields editor completion and types — the
//! cross-language analogue of `@types` / DefinitelyTyped. The reverse of the
//! [`translator`](crate::translator): Rust (`syn`) → `.ds`.

use syn::{
    FnArg, GenericArgument, ItemFn, ItemStruct, PathArguments, ReturnType, Type, Visibility,
};

/// Generates a `.ds` type declaration from Rust source.
#[derive(Default)]
pub struct Bindgen {
    // Options land here: visibility filters, rename rules, feature flags, ...
}

impl Bindgen {
    /// Create a bindgen with default options.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Generate a `.ds` declaration from Rust source text.
    ///
    /// Only `pub` structs and functions are mapped today; `enum`, `trait`, and
    /// non-public items are skipped.
    ///
    /// # Errors
    /// Returns an error string if the source is not valid Rust (`syn` parse fail).
    pub fn generate(&self, rust_source: &str) -> Result<String, String> {
        let file = syn::parse_file(rust_source).map_err(|e| format!("parse rust: {e}"))?;
        let mut out = String::new();
        for item in file.items {
            match item {
                syn::Item::Struct(s) => {
                    if let Some(decl) = struct_decl(&s) {
                        out.push_str(&decl);
                    }
                }
                syn::Item::Fn(f) => {
                    if let Some(decl) = fn_decl(&f) {
                        out.push_str(&decl);
                    }
                }
                _ => {}
            }
        }
        Ok(out)
    }
}

/// `pub struct Point { pub x: f64 }` → `interface Point { x: number; }`.
/// Non-public structs are skipped.
fn struct_decl(s: &ItemStruct) -> Option<String> {
    if !matches!(s.vis, Visibility::Public(_)) {
        return None;
    }
    let fields: Vec<String> = s
        .fields
        .iter()
        .filter_map(|f| {
            let name = f.ident.as_ref()?;
            Some(format!("  {name}: {};", reverse_type(&f.ty)))
        })
        .collect();
    if fields.is_empty() {
        return Some(format!("interface {} {{}}\n\n", s.ident));
    }
    Some(format!(
        "interface {} {{\n{}\n}}\n\n",
        s.ident,
        fields.join("\n")
    ))
}

/// `pub fn add(a: f64, b: f64) -> f64` → `declare function add(a: number, b: number): number;`.
/// Non-public functions are skipped.
fn fn_decl(f: &ItemFn) -> Option<String> {
    if !matches!(f.vis, Visibility::Public(_)) {
        return None;
    }
    let params: Vec<String> = f
        .sig
        .inputs
        .iter()
        .filter_map(|arg| {
            let FnArg::Typed(pt) = arg else { return None };
            let name = ident_of_pat(&pt.pat)?;
            Some(format!("{name}: {}", reverse_type(&pt.ty)))
        })
        .collect();
    let ret = reverse_return(&f.sig.output);
    Some(format!(
        "declare function {}({}): {};\n",
        f.sig.ident,
        params.join(", "),
        ret
    ))
}

/// Map a Rust type to its `.ds` spelling.
fn reverse_type(ty: &Type) -> String {
    match ty {
        Type::Path(tp) => reverse_path(tp),
        Type::Reference(r) => reverse_type(&r.elem),
        _ => "any".to_string(),
    }
}

fn reverse_path(tp: &syn::TypePath) -> String {
    let Some(seg) = tp.path.segments.last() else {
        return "any".to_string();
    };
    let name = seg.ident.to_string();
    match name.as_str() {
        "f64" | "f32" | "i8" | "i16" | "i32" | "i64" | "i128" | "u8" | "u16" | "u32" | "u64"
        | "u128" | "usize" | "isize" => "number".to_string(),
        "String" | "str" => "string".to_string(),
        "bool" => "boolean".to_string(),
        "Vec" => generic_arg(tp, 0).map_or_else(
            || "any[]".to_string(),
            |t| format!("{}[]", reverse_type(&t)),
        ),
        "Option" => generic_arg(tp, 0).map_or_else(
            || "any".to_string(),
            |t| format!("{} | null", reverse_type(&t)),
        ),
        _ => name,
    }
}

/// The `n`-th type argument of a generic path, if present.
fn generic_arg(tp: &syn::TypePath, n: usize) -> Option<Type> {
    let seg = tp.path.segments.last()?;
    let PathArguments::AngleBracketed(args) = &seg.arguments else {
        return None;
    };
    let Some(GenericArgument::Type(t)) = args.args.iter().nth(n) else {
        return None;
    };
    Some(t.clone())
}

fn reverse_return(output: &ReturnType) -> String {
    match output {
        ReturnType::Default => "void".to_string(),
        ReturnType::Type(_, ty) => reverse_type(ty),
    }
}

fn ident_of_pat(pat: &syn::Pat) -> Option<String> {
    if let syn::Pat::Ident(pi) = pat {
        return Some(pi.ident.to_string());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::Bindgen;

    #[test]
    fn generates_interface_from_struct() {
        let rust = "pub struct Point { pub x: f64, pub name: String }";
        let ds = Bindgen::new().generate(rust).expect("should bindgen");
        assert!(ds.contains("interface Point"), "got:\n{ds}");
        assert!(ds.contains("x: number"), "got:\n{ds}");
        assert!(ds.contains("name: string"), "got:\n{ds}");
    }

    #[test]
    fn generates_function_declaration() {
        let rust = "pub fn add(a: f64, b: f64) -> f64 { a + b }";
        let ds = Bindgen::new().generate(rust).expect("should bindgen");
        assert!(
            ds.contains("declare function add(a: number, b: number): number"),
            "got:\n{ds}"
        );
    }

    #[test]
    fn skips_non_public_items() {
        let rust = "struct Hidden { x: f64 } fn private() {}";
        let ds = Bindgen::new().generate(rust).expect("should bindgen");
        assert!(ds.trim().is_empty(), "got:\n{ds}");
    }

    #[test]
    fn maps_vec_and_option() {
        let rust = "pub struct Bag { pub items: Vec<f64>, pub note: Option<String> }";
        let ds = Bindgen::new().generate(rust).expect("should bindgen");
        assert!(ds.contains("items: number[]"), "got:\n{ds}");
        assert!(ds.contains("note: string | null"), "got:\n{ds}");
    }
}
