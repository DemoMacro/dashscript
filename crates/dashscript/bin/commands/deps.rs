//! `ds add`, `ds remove`, and `ds install`: dependency management — crates go
//! through cargo into `~/.cargo/registry`; local `.rs` files run bindgen.

use std::{error::Error, fs, path::Path, process::ExitCode};

use dashscript::{fetch, Bindgen};

use super::project::{
    cargo_bin, default_manifest, invoke_cargo, manifest_root, read_manifest, status_to_code,
};

/// Add a dependency to the project.
///
/// A `.rs` path runs bindgen on that local file (writes `<stem>.ds` beside
/// it — the `bindgen-demo` flow). Any other spec is a crate name, with or
/// without a `rust:` prefix: cargo downloads it into its global registry and
/// DashScript records it in `manifest.json`. No `.ds` declaration is generated
/// — type information comes from the crate source itself (read directly by the
/// language server, the way rust-analyzer reads `~/.cargo`).
pub(crate) fn add(spec: &str) -> Result<ExitCode, Box<dyn Error>> {
    if spec.ends_with(".rs") {
        return add_local_file(spec);
    }
    let crate_name = spec.strip_prefix("rust:").unwrap_or(spec);
    let version = fetch::add_via_cargo(crate_name, cargo_bin())
        .map_err(|e| format!("ds add {crate_name}: {e}"))?;
    let manifest_path = manifest_root().join("manifest.json");
    let mut manifest = read_manifest(&manifest_path).unwrap_or_else(|_| default_manifest());
    manifest.add_dependency("rust", crate_name, &version);
    fs::write(&manifest_path, format!("{}\n", manifest.to_json()?))?;
    println!("ds: added rust:{crate_name} = {version}");
    // Like `pnpm add`: record the dep, then refresh the lockfile (install) so
    // the new dependency is fetched and pinned in one step.
    install()
}

/// Remove a crate dependency from `manifest.json`.
pub(crate) fn remove(spec: &str) -> Result<ExitCode, Box<dyn Error>> {
    let name = spec.strip_prefix("rust:").unwrap_or(spec);
    let manifest_path = manifest_root().join("manifest.json");
    let mut manifest = read_manifest(&manifest_path)?;
    if !manifest.remove_dependency("rust", name) {
        return Err(format!("rust:{name} is not in {}", manifest_path.display()).into());
    }
    fs::write(&manifest_path, format!("{}\n", manifest.to_json()?))?;
    println!("ds: removed rust:{name}");
    Ok(ExitCode::SUCCESS)
}

/// Generate a `.ds` type declaration from a local Rust source file (bindgen),
/// written beside it as `<stem>.ds`.
fn add_local_file(file: &str) -> Result<ExitCode, Box<dyn Error>> {
    let path = Path::new(file);
    let rust =
        fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let ds = Bindgen::new()
        .generate(&rust)
        .map_err(|e| format!("bindgen {}: {e}", path.display()))?;
    let out = path.with_extension("ds");
    fs::write(&out, ds)?;
    println!("ds: generated {}", out.display());
    Ok(ExitCode::SUCCESS)
}

/// Ensure the manifest's dependencies are fetched and a `Cargo.lock` exists
/// (`ds install`). Emits a throwaway Cargo project under `.cache/install/` and
/// runs `cargo fetch`, which downloads crate sources to `~/.cargo/registry` —
/// the dependency store, no `node_modules` equivalent — so a later `build`/run
/// compiles without re-downloading.
pub(crate) fn install() -> Result<ExitCode, Box<dyn Error>> {
    let root = manifest_root();
    let manifest =
        read_manifest(&root.join("manifest.json")).unwrap_or_else(|_| default_manifest());
    // Reuse the build cache (`<root>/.cache/dash/<name>/`) — not a separate dir — so
    // the `Cargo.lock` `cargo fetch` writes here is the same one `build`/`run`
    // use. No duplicate cargo project, no throwaway lockfile.
    let dir = root.join(".cache").join("dash").join(&manifest.name);
    fs::create_dir_all(dir.join("src"))?;
    fs::write(dir.join("Cargo.toml"), manifest.to_cargo_toml())?;
    // `cargo fetch` requires a target to exist; a placeholder main.rs is never
    // compiled (fetch only resolves + downloads deps) and `ds build` overwrites
    // it with the real translated source.
    fs::write(dir.join("src").join("main.rs"), "fn main() {}\n")?;
    println!("ds: fetching dependencies...");
    let status = invoke_cargo(&dir, ["fetch", "--quiet"])?;
    Ok(status_to_code(status))
}
