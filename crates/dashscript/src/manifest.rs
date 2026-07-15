//! `manifest.json` → `Cargo.toml`.
//!
//! A DashScript project manifest. Dependencies use a **target prefix**
//! (`rust:serde`; `go:` / `zig:` reserved for future backends) so multiple
//! targets can coexist. On build, the manifest is translated into a
//! `Cargo.toml` with Cargo-normalized version requirements.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Default transpilation target when a manifest omits `target`.
const DEFAULT_TARGET: &str = "rust";

/// A DashScript project manifest (`manifest.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    /// Project name → `Cargo.toml` `[package].name`.
    pub name: String,
    /// Primary transpilation target (`rust`, `go`, `zig`).
    #[serde(default = "default_target")]
    pub target: String,
    /// Target-prefixed dependencies, e.g. `{ "rust:serde": "1.0" }`.
    #[serde(default)]
    pub dependencies: BTreeMap<String, String>,
}

fn default_target() -> String {
    DEFAULT_TARGET.to_string()
}

impl Default for Manifest {
    fn default() -> Self {
        Self {
            name: String::new(),
            target: default_target(),
            dependencies: BTreeMap::new(),
        }
    }
}

impl Manifest {
    /// Parse a `manifest.json` document.
    ///
    /// # Errors
    /// Returns a [`serde_json::Error`] if the document is not valid JSON or
    /// does not match the manifest shape.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Emit a `Cargo.toml` string for this manifest.
    ///
    /// Only dependencies whose prefix matches `target` map to the output
    /// (e.g. `rust:serde` → `serde`); other-target prefixes are dropped.
    // NOTE: version requirements are passed through verbatim today — npm-style
    // ranges (`^1.0`) are not yet normalized to Cargo's.
    pub fn to_cargo_toml(&self) -> String {
        let prefix = format!("{}:", self.target);
        let deps: Vec<String> = self
            .dependencies
            .iter()
            .filter_map(|(key, req)| {
                key.strip_prefix(&prefix).map(|name| format!("{name} = {req:?}"))
            })
            .collect();

        let mut out = String::from("[package]\n");
        out.push_str(&format!(
            "name = {:?}\nversion = \"0.0.0\"\nedition = \"2021\"\n",
            self.name
        ));
        if !deps.is_empty() {
            out.push_str("\n[dependencies]\n");
            out.push_str(&deps.join("\n"));
            out.push('\n');
        }
        out
    }
}
