//! `manifest.json` → `Cargo.toml`.
//!
//! A DashScript project manifest. Dependencies use a **target prefix**
//! (`rust:serde`) so the schema stays forward-compatible. On build, the
//! manifest is translated into a `Cargo.toml` with Cargo-normalized version
//! requirements.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Default output target when a manifest omits `target`. `bin` compiles a
/// native binary; `rust` stops at the translated crate; `wasm` / `napi` are
/// planned (all built on the Rust backend).
const DEFAULT_TARGET: &str = "bin";

/// The single backend all current output targets compile through, so
/// target-prefixed dependencies are always `rust:` today. (`go:` / `zig:`
/// backends were dropped — see the design decisions.) This prefixes both
/// `dependencies` keys and `ds add rust:<crate>`.
const BACKEND: &str = "rust";

/// Default `version` when a manifest omits it (`Cargo.toml` requires one).
fn default_version() -> String {
    "0.0.0".to_string()
}

/// A DashScript project manifest (`manifest.json`) — a blend of
/// `Cargo.toml` `[package]` (metadata) and `package.json` (entry/scripts).
///
/// Field order is the JSON output order: metadata first, then DashScript-
/// specific fields, then dependencies.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    /// Project name → `Cargo.toml` `[package].name` (required).
    pub name: String,
    /// Semantic version → `Cargo.toml` `[package].version`.
    #[serde(default = "default_version")]
    pub version: String,
    /// One-line description → `Cargo.toml` `[package].description`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// SPDX license string → `Cargo.toml` `[package].license`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    /// Source repository URL → `Cargo.toml` `[package].repository`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    /// Project homepage URL → `Cargo.toml` `[package].homepage`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    /// Discoverability keywords → `Cargo.toml` `[package].keywords`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keywords: Vec<String>,
    /// Author names → `Cargo.toml` `[package].authors`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authors: Vec<String>,
    /// Output target: `bin` (default, native binary) / `rust` (translated
    /// crate) / `wasm` / `napi` (planned). Overridable by `ds build --target`.
    /// This is an output shape, not the backend — all targets compile through
    /// [`BACKEND`], so it never filters dependencies.
    #[serde(default = "default_target")]
    pub target: String,
    /// Entry file (e.g. `"src/main.ds"`), like `package.json` `main`. The CLI
    /// defaults to `main.ds` when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<String>,
    /// Shell-command scripts (`"start": "ds main.ds"`), run via `ds run`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub scripts: BTreeMap<String, String>,
    /// Workspace member globs (`["apps/*", "packages/*"]`) on a monorepo root.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workspace: Vec<String>,
    /// Backend-prefixed dependencies, e.g. `{ "rust:serde": "1.0" }`. The
    /// prefix is [`BACKEND`] (`rust:`) today.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dependencies: BTreeMap<String, String>,
}

fn default_target() -> String {
    DEFAULT_TARGET.to_string()
}

impl Default for Manifest {
    fn default() -> Self {
        Self {
            name: String::new(),
            version: default_version(),
            description: None,
            license: None,
            repository: None,
            homepage: None,
            keywords: Vec::new(),
            authors: Vec::new(),
            target: default_target(),
            entry: None,
            scripts: BTreeMap::new(),
            workspace: Vec::new(),
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
    /// Only dependencies whose prefix matches the backend ([`BACKEND`] = `rust:`)
    /// map to the output (e.g. `rust:serde` → `serde`); the manifest `target`
    /// field is an output shape and does not filter dependencies. Metadata
    /// (version/description/license/repository/homepage/keywords/authors) passes
    /// straight through to `[package]`.
    // NOTE: version requirements are passed through verbatim today — npm-style
    // ranges (`^1.0`) are not yet normalized to Cargo's.
    pub fn to_cargo_toml(&self) -> String {
        // Dependencies are filtered by the backend prefix (`rust:`), not by
        // `target`: `target` is an _output_ shape (bin/rust/wasm/napi), all
        // built on the same Rust backend, so the dependency prefix is fixed.
        let prefix = format!("{}:", BACKEND);
        let deps: Vec<String> = self
            .dependencies
            .iter()
            .filter_map(|(key, req)| {
                key.strip_prefix(&prefix).map(|name| format!("{name} = {req:?}"))
            })
            .collect();

        let mut out = String::from("[package]\n");
        out.push_str(&format!("name = {:?}\n", self.name));
        out.push_str(&format!("version = {:?}\n", self.version));
        out.push_str("edition = \"2021\"\n");
        if let Some(desc) = &self.description {
            out.push_str(&format!("description = {desc:?}\n"));
        }
        if let Some(license) = &self.license {
            out.push_str(&format!("license = {license:?}\n"));
        }
        if let Some(repo) = &self.repository {
            out.push_str(&format!("repository = {repo:?}\n"));
        }
        if let Some(home) = &self.homepage {
            out.push_str(&format!("homepage = {home:?}\n"));
        }
        if !self.keywords.is_empty() {
            let kws: Vec<String> = self.keywords.iter().map(|k| format!("{k:?}")).collect();
            out.push_str(&format!("keywords = [{}]\n", kws.join(", ")));
        }
        if !self.authors.is_empty() {
            let auths: Vec<String> = self.authors.iter().map(|a| format!("{a:?}")).collect();
            out.push_str(&format!("authors = [{}]\n", auths.join(", ")));
        }
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

    #[test]
    fn metadata_passes_through_to_cargo_toml() {
        let json = r#"{
  "name": "demo",
  "version": "1.2.3",
  "description": "a demo",
  "license": "MIT",
  "repository": "https://github.com/x/demo",
  "homepage": "https://demo.example",
  "keywords": ["ts", "rust"],
  "authors": ["Jane <jane@example.com>"],
  "dependencies": { "rust:serde": "1.0" }
}"#;
        let m = Manifest::from_json(json).expect("should parse");
        let toml = m.to_cargo_toml();
        assert!(toml.contains("version = \"1.2.3\""), "got:\n{toml}");
        assert!(toml.contains("description = \"a demo\""), "got:\n{toml}");
        assert!(toml.contains("license = \"MIT\""), "got:\n{toml}");
        assert!(
            toml.contains("repository = \"https://github.com/x/demo\""),
            "got:\n{toml}"
        );
        assert!(
            toml.contains("homepage = \"https://demo.example\""),
            "got:\n{toml}"
        );
        assert!(toml.contains("keywords = [\"ts\", \"rust\"]"), "got:\n{toml}");
        assert!(
            toml.contains("authors = [\"Jane <jane@example.com>\"]"),
            "got:\n{toml}"
        );
    }

    #[test]
    fn target_default_is_bin_and_does_not_filter_deps() {
        // `target` is an output shape, not the dependency backend; a default
        // (`bin`) manifest still emits its `rust:` dependencies.
        let mut m = Manifest {
            name: "demo".to_string(),
            ..Manifest::default()
        };
        assert_eq!(m.target, "bin");
        m.add_dependency("rust", "serde", "1.0");
        let toml = m.to_cargo_toml();
        assert!(toml.contains("serde = \"1.0\""), "got:\n{toml}");
    }

    #[test]
    fn to_json_omits_unset_optional_fields() {
        let m = Manifest {
            name: "demo".to_string(),
            ..Manifest::default()
        };
        let json = m.to_json().expect("should serialize");
        // Optional metadata + empty collections are skipped for a tidy file.
        assert!(!json.contains("description"), "got:\n{json}");
        assert!(!json.contains("scripts"), "got:\n{json}");
        assert!(!json.contains("workspace"), "got:\n{json}");
        assert!(!json.contains("dependencies"), "got:\n{json}");
        assert!(json.contains("\"version\": \"0.0.0\""), "got:\n{json}");
        assert!(json.contains("\"target\": \"bin\""), "got:\n{json}");
    }
}
