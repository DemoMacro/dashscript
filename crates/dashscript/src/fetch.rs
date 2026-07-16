//! crates.io fetch via cargo — cargo is DashScript's package store **and**
//! resolver (the Rust analogue of pnpm's global store + npm's resolver).
//! `cargo add` downloads the crate plus all transitive deps into cargo's
//! global registry (`~/.cargo`); `cargo metadata` reports the resolved
//! version. DashScript keeps no store of its own and resolves no version
//! conflicts — cargo's PubGrub resolver + `Cargo.lock` do, exactly as
//! rust-analyzer relies on them.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Resolve `<name>`'s latest compatible version and download its source (and
/// transitive deps) into cargo's global registry, by running `cargo add` in a
/// scratch project. Returns the resolved version string.
///
/// This reuses cargo as the dependency resolver and store — DashScript does
/// not keep its own registry or resolve transitive/version conflicts.
///
/// # Errors
/// Returns an error string if `cargo` is missing, the crate does not exist, or
/// the network is unreachable (the message comes from cargo).
pub fn add_via_cargo(name: &str, cargo_bin: &Path) -> Result<String, String> {
    let dir = scratch_project()?;
    let result = (|| {
        run_cargo(cargo_bin, &["add", name], &dir)
            .map_err(|e| format!("cargo add {name} failed: {e}"))?;
        let json = run_cargo_capture(cargo_bin, &["metadata", "--format-version", "1"], &dir)
            .map_err(|e| format!("cargo metadata failed: {e}"))?;
        extract_version(&json, name)
            .ok_or_else(|| format!("cargo metadata did not report crate '{name}'"))
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}

/// A minimal throwaway Cargo project (a `[package]` + empty `src/lib.rs`) for
/// running `cargo add` / `cargo metadata` without touching the user's project.
fn scratch_project() -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join(format!("ds-fetch-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("src")).map_err(|e| format!("create scratch project: {e}"))?;
    std::fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"ds-fetch\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
    )
    .map_err(|e| format!("write scratch Cargo.toml: {e}"))?;
    std::fs::write(dir.join("src").join("lib.rs"), "")
        .map_err(|e| format!("write scratch lib.rs: {e}"))?;
    Ok(dir)
}

fn run_cargo(bin: &Path, args: &[&str], cwd: &Path) -> Result<(), String> {
    let out = Command::new(bin)
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|e| format!("failed to invoke cargo (is it on PATH?): {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(())
}

fn run_cargo_capture(bin: &Path, args: &[&str], cwd: &Path) -> Result<String, String> {
    let out = Command::new(bin)
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|e| format!("failed to invoke cargo: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The resolved version of `name` from a `cargo metadata` JSON document.
fn extract_version(metadata_json: &str, name: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(metadata_json).ok()?;
    v.get("packages")?
        .as_array()?
        .iter()
        .find(|p| p.get("name").and_then(|n| n.as_str()) == Some(name))
        .and_then(|p| p.get("version")?.as_str())
        .map(String::from)
}

#[cfg(test)]
mod tests {
    use super::extract_version;

    #[test]
    fn extract_version_finds_named_crate() {
        let json = r#"{"packages":[
            {"name":"ds-fetch","version":"0.0.0"},
            {"name":"adler","version":"1.0.2"}
        ]}"#;
        assert_eq!(extract_version(json, "adler"), Some("1.0.2".to_string()));
    }

    #[test]
    fn extract_version_missing_crate_is_none() {
        let json = r#"{"packages":[{"name":"ds-fetch","version":"0.0.0"}]}"#;
        assert_eq!(extract_version(json, "serde"), None);
    }
}
