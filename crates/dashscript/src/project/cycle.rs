use super::*;

/// Guard: detect circular imports. Rust forbids circular module dependencies
/// (`mod a` → `mod b` → `mod a`), which cargo reports as a vague error; this
/// surfaces the cycle explicitly with the files involved. Each file's imports
/// are resolved to canonical paths so the graph holds regardless of how an
/// import is written.
pub(crate) fn detect_circular_imports(files: &[PathBuf]) -> Result<(), Box<dyn Error>> {
    let known: Vec<PathBuf> = files
        .iter()
        .map(|p| p.canonicalize().unwrap_or_else(|_| p.clone()))
        .collect();
    let mut graph: std::collections::HashMap<PathBuf, Vec<PathBuf>> =
        std::collections::HashMap::new();
    for f in files {
        let Ok(src) = fs::read_to_string(f) else {
            continue;
        };
        let base = f.parent().unwrap_or_else(|| Path::new(""));
        let key = f.canonicalize().unwrap_or_else(|_| f.clone());
        for imp in Translator::new().imports(&src) {
            // `import type` is erased at compile time — no Rust `use`, so no
            // runtime module dependency and no cycle edge.
            if imp.is_type_only {
                continue;
            }
            if let Ok((dep, _)) = resolve_local_module(base, &imp.source) {
                let dep = dep.canonicalize().unwrap_or(dep);
                if known.contains(&dep) {
                    graph.entry(key.clone()).or_default().push(dep);
                }
            }
        }
    }
    // DFS cycle detection (white=0 / gray=1 / black=2). A back edge to a gray
    // node closes a cycle.
    let mut color: std::collections::HashMap<PathBuf, u8> = std::collections::HashMap::new();
    for start in graph.keys() {
        if color.get(start).copied().unwrap_or(0) != 0 {
            continue;
        }
        let mut stack: Vec<PathBuf> = Vec::new();
        if let Some(cycle) = dfs_cycle(start, &graph, &mut color, &mut stack) {
            let names: Vec<String> = cycle
                .iter()
                .map(|p| {
                    p.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default()
                })
                .collect();
            return Err(format!(
                "dashscript: circular import detected — {}; refactor to break the cycle \
                 (Rust forbids circular module dependencies)",
                names.join(" → ")
            )
            .into());
        }
    }
    Ok(())
}

/// DFS helper for [`detect_circular_imports`]: returns the cycle path when a
/// back edge to a node already on the stack (gray) is found. Color: 0=white,
/// 1=gray (on stack), 2=black (fully explored).
pub(crate) fn dfs_cycle(
    node: &Path,
    graph: &std::collections::HashMap<PathBuf, Vec<PathBuf>>,
    color: &mut std::collections::HashMap<PathBuf, u8>,
    stack: &mut Vec<PathBuf>,
) -> Option<Vec<PathBuf>> {
    color.insert(node.to_path_buf(), 1);
    stack.push(node.to_path_buf());
    for dep in graph.get(node).into_iter().flatten() {
        match color.get(dep).copied().unwrap_or(0) {
            1 => {
                // Back edge → cycle. Slice from the dep's first occurrence and
                // close the loop for display.
                let start = stack.iter().position(|n| n == dep).unwrap();
                let mut cycle = stack[start..].to_vec();
                cycle.push(dep.clone());
                return Some(cycle);
            }
            0 => {
                if let Some(found) = dfs_cycle(dep, graph, color, stack) {
                    return Some(found);
                }
            }
            _ => {}
        }
    }
    stack.pop();
    color.insert(node.to_path_buf(), 2);
    None
}

/// Guard: no bin may import another bin. cargo forbids one `[[bin]]` from
/// `mod`-ing another, so shared code must live in a `[lib]` module. Compares
/// canonical file paths so the check holds regardless of how the import is
/// written.
pub(crate) fn detect_bin_imports_bin(
    root: &Path,
    bins: &[(String, String)],
) -> Result<(), Box<dyn Error>> {
    let mut bin_files: std::collections::HashMap<PathBuf, String> =
        std::collections::HashMap::new();
    for (bin_name, ds_path) in bins {
        if let Ok(canon) = root.join(ds_path).canonicalize() {
            bin_files.insert(canon, bin_name.clone());
        }
    }
    for (bin_name, ds_path) in bins {
        let file = root.join(ds_path);
        let Ok(src) = fs::read_to_string(&file) else {
            continue;
        };
        let base = file.parent().unwrap_or_else(|| Path::new(""));
        for imp in Translator::new().imports(&src) {
            let Ok((dep, _)) = resolve_local_module(base, &imp.source) else {
                continue; // a missing module surfaces at `cargo build`
            };
            if let Ok(canon) = dep.canonicalize() {
                if let Some(other) = bin_files.get(&canon) {
                    if other != bin_name {
                        return Err(format!(
                            "dashscript: bin '{bin_name}' imports bin '{other}' (from {}); \
                             move the shared code into a lib module (a .ts that is not a bin \
                             entry) — cargo forbids one bin from mod-ing another",
                            imp.source
                        )
                        .into());
                    }
                }
            }
        }
    }
    Ok(())
}
