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

/// The `bin` field of a manifest: a single executable (a string path, named
/// after the package) or a map of bin names to paths — mirroring package.json's
/// `bin` (string | object). A single-string `bin` borrows the package `name`
/// for its one target; an object uses each key as a bin name.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum BinSpec {
    /// `"bin": "main.ds"` — one executable, named after the package.
    Single(String),
    /// `"bin": { "numbers": "numbers.ds" }` — N executables, each key a bin name.
    Multiple(BTreeMap<String, String>),
}

impl BinSpec {
    /// Resolve to `(bin_name, ds_path)` pairs. A `Single` spec names its one
    /// bin after the package (package.json's single-bin rule); a `Multiple`
    /// spec uses each map key.
    pub fn entries(&self, package_name: &str) -> Vec<(String, String)> {
        match self {
            BinSpec::Single(path) => vec![(package_name.to_string(), path.clone())],
            BinSpec::Multiple(map) => map
                .iter()
                .map(|(name, path)| (name.clone(), path.clone()))
                .collect(),
        }
    }
}

/// The `src/<stem>.rs` path for a `.ds` entry, flattening any directory prefix
/// to a single `src/` level (MVP: a crate root, no sub-modules yet). The stem
/// is the file name without extension — `numbers.ds`, `./numbers.ds`, and
/// `src/numbers.ds` all map to `src/numbers.rs`.
fn ds_to_rust_path(ds_path: &str) -> String {
    let stem = ds_path.rsplit(['/', '\\']).next().unwrap_or(ds_path);
    let stem = stem.trim_end_matches(".ds").trim_end_matches(".ts");
    format!("src/{stem}.rs")
}

/// A DashScript project manifest (`manifest.json`) — a blend of
/// `Cargo.toml` `[package]` (metadata) and `package.json` (entry/scripts).
///
/// Field order is the JSON output order: metadata first, then DashScript-
/// specific fields, then dependencies.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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
    /// Executable entry points → Cargo `[[bin]]` targets. A single executable
    /// is `"bin": "main.ds"` (named after the package, package.json's
    /// single-bin rule); multiple are `"bin": { "<name>": "<file>" }` where
    /// each key is a bin name. Omit for a library-only crate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bin: Option<BinSpec>,
    /// Library entry (`"lib": "lib.ds"`) → Cargo `[lib]`. A crate with a `lib`
    /// exports its modules for bins to `use` — shared code lives here, never
    /// in another bin (cargo forbids one bin from `mod`-ing another).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lib: Option<String>,
    /// Entry file (e.g. `"src/main.ds"`), like `package.json` `main`. The CLI
    /// defaults to `main.ds` when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<String>,
    /// Shell-command scripts (`"start": "ds main.ds"`), run via `ds run`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub scripts: BTreeMap<String, String>,
    /// Workspace member globs (`["apps/*", "packages/*"]`) on a monorepo root.
    /// Plural `workspaces` mirrors npm/yarn/bun's package.json field (pnpm
    /// instead uses a separate `pnpm-workspace.yaml`, but DashScript keeps
    /// membership in `manifest.json`, so it follows the npm/bun convention).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workspaces: Vec<String>,
    /// Backend-prefixed *dev* dependencies, e.g. `{ "rust:tempfile": "3.0" }`
    /// → Cargo `[dev-dependencies]`. Same `rust:` prefix as [`dependencies`].
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dev_dependencies: BTreeMap<String, String>,
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
            bin: None,
            lib: None,
            entry: None,
            scripts: BTreeMap::new(),
            workspaces: Vec::new(),
            dev_dependencies: BTreeMap::new(),
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
    /// The `[package]` + `[dependencies]` body — the shared core emitted for a
    /// single-package project ([`to_cargo_toml`]), a workspace member
    /// ([`to_member_toml`]), or a lone-file throwaway. No `[profile]` and no
    /// `[workspace]`: those belong to the single-package root or the workspace
    /// root. Only dependencies whose prefix matches the backend ([`BACKEND`] =
    /// `rust:`) map to the output (e.g. `rust:serde` → `serde`); the manifest
    /// `target` field is an output shape and does not filter dependencies.
    /// Metadata (version/description/license/repository/homepage/keywords/authors)
    /// passes straight through to `[package]`.
    // NOTE: version requirements are passed through verbatim today — npm-style
    // ranges (`^1.0`) are not yet normalized to Cargo's.
    fn package_body(&self) -> String {
        // Dependencies are filtered by the backend prefix (`rust:`), not by
        // `target`: `target` is an _output_ shape (bin/rust/wasm/napi), all
        // built on the same Rust backend, so the dependency prefix is fixed.
        let prefix = format!("{}:", BACKEND);
        let deps: Vec<String> = self
            .dependencies
            .iter()
            .filter_map(|(key, req)| {
                key.strip_prefix(&prefix)
                    .map(|name| format!("{name} = {req:?}"))
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
        // Dev-dependencies share the backend-prefix filter, emitted as a
        // separate `[dev-dependencies]` section (mirroring Cargo.toml).
        let dev_deps: Vec<String> = self
            .dev_dependencies
            .iter()
            .filter_map(|(key, req)| {
                key.strip_prefix(&prefix)
                    .map(|name| format!("{name} = {req:?}"))
            })
            .collect();
        if !dev_deps.is_empty() {
            out.push_str("\n[dev-dependencies]\n");
            out.push_str(&dev_deps.join("\n"));
            out.push('\n');
        }
        out
    }

    /// A single-package `Cargo.toml`: [`package_body`] + `[profile.release]` +
    /// an empty `[workspace]` so the emitted project is its own workspace root
    /// (never absorbed by a parent workspace, e.g. DashScript's own repo when
    /// `ds build` emits under `dist/`).
    pub fn to_cargo_toml(&self) -> String {
        let mut out = self.package_body();
        // `panic = "unwind"` is pinned on release (dev already defaults to
        // unwind) so a `.ds` `try/catch` — which lowers to `catch_unwind` —
        // reliably catches a `throw` (→ `panic!`). DashScript owns this
        // manifest, so it owns the panic strategy: that is precisely what makes
        // `catch_unwind` sound, where on an arbitrary user `Cargo.toml` it
        // would not be (a `panic = "abort"` build silently drops the catch).
        out.push_str("\n[profile.release]\npanic = \"unwind\"\n");
        out.push_str("\n[workspace]\n");
        out
    }

    /// The `(bin_name, ds_path)` entries declared by `bin`, resolved against
    /// the package name for a single-string `bin`. Empty when `bin` is unset.
    /// `ds build`/`ds run` use this to emit one `[[bin]]` per declared entry.
    pub fn bin_entries(&self) -> Vec<(String, String)> {
        self.bin
            .as_ref()
            .map_or_else(Vec::new, |spec| spec.entries(&self.name))
    }

    /// A single-package `Cargo.toml` with explicit `[[bin]]` / `[lib]` targets
    /// for the project-as-one-crate model (every `.ds` translates to
    /// `src/<stem>.rs`). Emits [`package_body`] + one `[[bin]] name/path` per
    /// declared bin + an optional `[lib]` + `[profile.release]` + an empty
    /// `[workspace]`; the no-arg [`to_cargo_toml`] is for a lone file.
    pub fn to_cargo_toml_with_bins(&self, bins: &[(String, String)], lib: Option<&str>) -> String {
        let mut out = self.package_body();
        for (name, ds_path) in bins {
            out.push_str(&format!(
                "\n[[bin]]\nname = {name:?}\npath = {:?}\n",
                ds_to_rust_path(ds_path)
            ));
        }
        if let Some(lib_path) = lib {
            out.push_str(&format!(
                "\n[lib]\npath = {:?}\n",
                ds_to_rust_path(lib_path)
            ));
        }
        out.push_str("\n[profile.release]\npanic = \"unwind\"\n");
        out.push_str("\n[workspace]\n");
        out
    }

    /// A workspace member's `Cargo.toml`: [`package_body`] only. The workspace
    /// root owns `[profile]` and `[workspace]`; a member that repeats them is
    /// rejected by cargo, so this omits both. Each member resolves its own
    /// declared dependencies, but the shared workspace `target/` dedupes any
    /// dependency several members have in common (serde compiles once).
    pub fn to_member_toml(&self) -> String {
        self.package_body()
    }

    /// A workspace root `Cargo.toml`: `[workspace] members` (each expected at
    /// `<name>/`, directly under the workspace root — no extra `members/` layer,
    /// since cargo does not require one and it would diverge from the
    /// single-package `.cache/dash/<name>/` layout) + `[profile.release]`. One
    /// root means one shared `target/` and one `Cargo.lock`, so a dependency
    /// used by several members compiles once — cargo's native equivalent of a
    /// hoisted `node_modules`.
    pub fn workspace_root_toml(member_names: &[String]) -> String {
        let members: Vec<String> = member_names.iter().map(|n| format!("\"{n}\"")).collect();
        let mut out = String::from("[workspace]\n");
        out.push_str(&format!("members = [{}]\n", members.join(", ")));
        out.push_str("resolver = \"2\"\n");
        out.push_str("\n[profile.release]\npanic = \"unwind\"\n");
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
        self.dependencies
            .remove(&format!("{target}:{name}"))
            .is_some()
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
        assert!(
            toml.contains("keywords = [\"ts\", \"rust\"]"),
            "got:\n{toml}"
        );
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
        assert!(!json.contains("workspaces"), "got:\n{json}");
        assert!(!json.contains("dependencies"), "got:\n{json}");
        assert!(json.contains("\"version\": \"0.0.0\""), "got:\n{json}");
        assert!(json.contains("\"target\": \"bin\""), "got:\n{json}");
    }

    #[test]
    fn to_member_toml_is_package_body_without_profile_or_workspace() {
        let mut m = Manifest {
            name: "demo".to_string(),
            ..Manifest::default()
        };
        m.add_dependency("rust", "serde", "1.0");
        let toml = m.to_member_toml();
        assert!(toml.contains("[package]"), "got:\n{toml}");
        assert!(toml.contains("serde = \"1.0\""), "got:\n{toml}");
        // A member must not repeat [profile] or [workspace] — the workspace
        // root owns them, and cargo rejects a member that redeclares either.
        assert!(
            !toml.contains("[profile"),
            "member must not pin profile, got:\n{toml}"
        );
        assert!(
            !toml.contains("[workspace]"),
            "member must not declare workspace, got:\n{toml}"
        );
    }

    #[test]
    fn workspace_root_toml_lists_members_directly_under_root() {
        let toml = Manifest::workspace_root_toml(&["app-a".to_string(), "app-b".to_string()]);
        // Members sit directly under the root (<name>), mirroring the
        // single-package `.cache/dash/<name>/` — no `members/` layer.
        assert!(
            toml.contains("members = [\"app-a\", \"app-b\"]"),
            "got:\n{toml}"
        );
        assert!(toml.contains("resolver = \"2\""), "got:\n{toml}");
        assert!(
            toml.contains("[profile.release]\npanic = \"unwind\""),
            "workspace pins release panic=unwind, got:\n{toml}"
        );
        assert!(
            !toml.contains("[package]"),
            "workspace root has no [package], got:\n{toml}"
        );
    }

    #[test]
    fn bin_single_named_after_package() {
        let m =
            Manifest::from_json(r#"{ "name": "app", "bin": "main.ds" }"#).expect("should parse");
        assert_eq!(
            m.bin_entries(),
            vec![("app".to_string(), "main.ds".to_string())]
        );
    }

    #[test]
    fn bin_multiple_uses_keys_as_names() {
        let m = Manifest::from_json(
            r#"{ "name": "tour", "bin": { "numbers": "numbers.ds", "globals": "globals.ds" } }"#,
        )
        .expect("should parse");
        let mut bins = m.bin_entries();
        bins.sort();
        assert_eq!(
            bins,
            vec![
                ("globals".to_string(), "globals.ds".to_string()),
                ("numbers".to_string(), "numbers.ds".to_string()),
            ]
        );
    }

    #[test]
    fn bin_unset_yields_no_entries() {
        let m = Manifest::from_json(r#"{ "name": "app" }"#).expect("should parse");
        assert!(m.bin_entries().is_empty());
    }

    #[test]
    fn to_cargo_toml_with_bins_emits_targets() {
        let m = Manifest::from_json(
            r#"{ "name": "tour", "bin": { "numbers": "numbers.ds" }, "lib": "lib.ds" }"#,
        )
        .expect("should parse");
        let toml = m.to_cargo_toml_with_bins(&m.bin_entries(), m.lib.as_deref());
        assert!(toml.contains("[[bin]]"), "missing [[bin]], got:\n{toml}");
        assert!(
            toml.contains("name = \"numbers\""),
            "bin name missing, got:\n{toml}"
        );
        assert!(
            toml.contains("path = \"src/numbers.rs\""),
            "bin path not flattened to src/, got:\n{toml}"
        );
        assert!(toml.contains("[lib]"), "missing [lib], got:\n{toml}");
        assert!(
            toml.contains("path = \"src/lib.rs\""),
            "lib path wrong, got:\n{toml}"
        );
    }

    #[test]
    fn dev_dependencies_emit_separate_section() {
        let m = Manifest::from_json(
            r#"{ "name": "app", "dependencies": { "rust:serde": "1.0" }, "devDependencies": { "rust:tempfile": "3.0" } }"#,
        )
        .expect("should parse");
        let toml = m.to_cargo_toml();
        assert!(
            toml.contains("[dependencies]\nserde = \"1.0\""),
            "deps section, got:\n{toml}"
        );
        assert!(
            toml.contains("[dev-dependencies]\ntempfile = \"3.0\""),
            "dev-deps missing, got:\n{toml}"
        );
    }
}
