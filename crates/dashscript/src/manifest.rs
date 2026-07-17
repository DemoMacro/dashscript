//! `manifest.json` → `Cargo.toml`.
//!
//! A DashScript project manifest. Dependencies use a **target prefix**
//! (`rust:serde`) so the schema stays forward-compatible. On build, the
//! manifest is translated into a `Cargo.toml` with Cargo-normalized version
//! requirements.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Default transpilation target when a manifest omits `target`.
const DEFAULT_TARGET: &str = "rust";

/// A DashScript project manifest (`manifest.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    /// Project name → `Cargo.toml` `[package].name`.
    pub name: String,
    /// Primary output target (`rust` today; `bin` / `wasm` / `napi` planned).
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
        // `panic = "unwind"` is pinned on release (dev already defaults to
        // unwind) so a `.ds` `try/catch` — which lowers to `catch_unwind` —
        // reliably catches a `throw` (→ `panic!`). DashScript owns this
        // manifest, so it owns the panic strategy: that is precisely what makes
        // `catch_unwind` sound, where on an arbitrary user `Cargo.toml` it
        // would not be (a `panic = "abort"` build silently drops the catch).
        out.push_str("\n[profile.release]\npanic = \"unwind\"\n");
        // An empty `[workspace]` table makes the emitted project its own
        // workspace root, so it is never absorbed by a parent workspace (e.g.
        // DashScript's own repo when `ds build` emits under `dist/`).
        out.push_str("\n[workspace]\n");
        out
    }

    /// Record a dependency under its target prefix (`rust:serde`). Returns
    /// `true` if newly added, `false` if it already existed (the requirement is
    /// still updated in place).
    pub fn add_dependency(&mut self, target: &str, name: &str, req: &str) -> bool {
        self.dependencies
            .insert(format!("{target}:{name}"), req.to_string())
            .is_none()
    }

    /// Remove a target-prefixed dependency (`rust:serde`). Returns `true` if it
    /// was present and removed.
    pub fn remove_dependency(&mut self, target: &str, name: &str) -> bool {
        self.dependencies.remove(&format!("{target}:{name}")).is_some()
    }

    /// Serialize back to a pretty `manifest.json` document (2-space indent), so
    /// `ds add` / `ds remove` can persist dependency changes.
    ///
    /// # Errors
    /// Returns a [`serde_json::Error`] if serialization fails (symmetric with
    /// [`Self::from_json`]).
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

#[cfg(test)]
mod tests {
    use super::Manifest;

    #[test]
    fn add_dependency_inserts_and_reports_new() {
        let mut m = Manifest::default();
        assert!(m.add_dependency("rust", "serde", "1.0"));
        assert!(!m.add_dependency("rust", "serde", "2.0")); // already present
        assert_eq!(m.dependencies.get("rust:serde"), Some(&"2.0".to_string()));
    }

    #[test]
    fn remove_dependency_reports_presence() {
        let mut m = Manifest::default();
        m.add_dependency("rust", "serde", "1.0");
        assert!(m.remove_dependency("rust", "serde"));
        assert!(!m.remove_dependency("rust", "serde"));
    }

    #[test]
    fn add_dependency_flows_into_cargo_toml() {
        let mut m = Manifest {
            name: "demo".to_string(),
            ..Manifest::default()
        };
        m.add_dependency("rust", "serde", "1.0");
        let toml = m.to_cargo_toml();
        assert!(toml.contains("serde = \"1.0\""), "got:\n{toml}");
    }

    #[test]
    fn to_json_roundtrips_through_from_json() {
        let mut m = Manifest {
            name: "demo".to_string(),
            ..Manifest::default()
        };
        m.add_dependency("rust", "serde", "1.0");
        let json = m.to_json().expect("should serialize");
        assert!(json.contains("\"rust:serde\": \"1.0\""), "got:\n{json}");
        let m2 = Manifest::from_json(&json).expect("should parse");
        assert_eq!(m2.name, "demo");
        assert_eq!(m2.dependencies.get("rust:serde"), Some(&"1.0".to_string()));
    }

    #[test]
    fn cargo_toml_pins_panic_unwind_for_try_catch() {
        let m = Manifest {
            name: "demo".to_string(),
            ..Manifest::default()
        };
        let toml = m.to_cargo_toml();
        assert!(
            toml.contains("[profile.release]\npanic = \"unwind\""),
            "release must pin panic=unwind so try/catch's catch_unwind is sound, got:\n{toml}"
        );
    }
}
