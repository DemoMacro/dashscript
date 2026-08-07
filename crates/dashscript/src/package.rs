//! `package.json` → `Cargo.toml`.
//!
//! The single manifest for a DashScript project is **`package.json`** (the
//! npm/pnpm standard). DashScript reuses the official package.json fields
//! verbatim (`name`/`version`/`bin`/`main`/`scripts`/`workspaces`/
//! `dependencies`/`devDependencies`/...) and adds only two DashScript-specific
//! keys under a `dashscript` namespace:
//! - `dashscript.target` — output shape (`bin` default / `rust` / `wasm` / `napi`)
//! - `dashscript.cargo.dependencies` / `.devDependencies` — Rust crate deps,
//!   whose values use **Cargo.toml syntax** (`"serde":"1.0"` or
//!   `"serde":{"version":"1.0","features":["derive"]}`), emitted to Cargo.toml
//!   with zero JSON→TOML loss.
//!
//! `dependencies`/`devDependencies` (the package.json standard fields) are
//! **npm packages** (`node_modules`) and **never reach Cargo.toml** —
//! DashScript layers them at build time (a `.ts` source package translates to
//! Rust; a JS dist routes through the engine).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Default output target when the `dashscript.target` field is omitted. `bin`
/// compiles a native binary; `rust` stops at the translated crate; `wasm` /
/// `napi` are planned (all built on the Rust backend).
const DEFAULT_TARGET: &str = "bin";

/// Default `version` when package.json omits it (`Cargo.toml` requires one).
fn default_version() -> String {
    "0.0.0".to_string()
}

fn default_target() -> String {
    DEFAULT_TARGET.to_string()
}

/// package.json `bin`: a single executable (a string path, named after the
/// package) or a map of bin names to paths — mirroring npm's `bin`
/// (string | object). A single-string `bin` borrows the package `name` for its
/// one target; an object uses each key as a bin name.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum BinSpec {
    /// `"bin": "main.ts"` — one executable, named after the package.
    Single(String),
    /// `"bin": { "numbers": "numbers.ts" }` — N executables, each key a bin name.
    Multiple(BTreeMap<String, String>),
}

impl BinSpec {
    /// Resolve to `(bin_name, src_path)` pairs. A `Single` spec names its one
    /// bin after the package (npm's single-bin rule); a `Multiple` spec uses
    /// each map key.
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

/// One cargo crate dependency value: a version string (`"1.0"`) or a Cargo.toml
/// detail spec (`{"version":"1.0","features":["derive"]}`). Mirrors Cargo.toml
/// `[dependencies]` two forms — JSON↔TOML zero loss.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CargoDepSpec {
    /// `"serde": "1.0"` — version string only.
    Version(String),
    /// `"serde": { "version": "1.0", "features": ["derive"], "path": "../x" }`
    /// — a Cargo.toml dependency spec.
    Detail {
        version: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        features: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        git: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        branch: Option<String>,
        /// `defaultFeatures: false` emits `default-features = false`; `true` /
        /// omitted keeps Cargo's default (true). Stored as Option so the
        /// default (None) is indistinguishable from explicit true.
        #[serde(
            default,
            rename = "defaultFeatures",
            skip_serializing_if = "Option::is_none"
        )]
        default_features: Option<bool>,
    },
}

impl CargoDepSpec {
    /// Emit as a Cargo.toml dependency value: `"1.0"` or
    /// `{ version = "1.0", features = ["derive"] }`.
    fn to_toml(&self) -> String {
        match self {
            CargoDepSpec::Version(v) => format!("{v:?}"),
            CargoDepSpec::Detail {
                version,
                features,
                path,
                git,
                branch,
                default_features,
            } => {
                let mut parts = vec![format!("version = {version:?}")];
                if !features.is_empty() {
                    let feats: Vec<String> = features.iter().map(|f| format!("{f:?}")).collect();
                    parts.push(format!("features = [{}]", feats.join(", ")));
                }
                if let Some(p) = path {
                    parts.push(format!("path = {p:?}"));
                }
                if let Some(g) = git {
                    parts.push(format!("git = {g:?}"));
                }
                if let Some(b) = branch {
                    parts.push(format!("branch = {b:?}"));
                }
                if matches!(default_features, Some(false)) {
                    parts.push("default-features = false".to_string());
                }
                format!("{{ {} }}", parts.join(", "))
            }
        }
    }
}

/// `dashscript.cargo`: Rust crate dependencies, mapping to Cargo.toml's
/// `[dependencies]` / `[dev-dependencies]`. Keys are bare crate names (the
/// `cargo` namespace already conveys "Rust crate" — no `cargo:` prefix on
/// each key).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CargoDeps {
    /// `dashscript.cargo.dependencies` → Cargo `[dependencies]`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dependencies: BTreeMap<String, CargoDepSpec>,
    /// `dashscript.cargo.devDependencies` → Cargo `[dev-dependencies]`.
    #[serde(
        default,
        rename = "devDependencies",
        skip_serializing_if = "BTreeMap::is_empty"
    )]
    pub dev_dependencies: BTreeMap<String, CargoDepSpec>,
}

impl CargoDeps {
    fn is_empty(&self) -> bool {
        self.dependencies.is_empty() && self.dev_dependencies.is_empty()
    }
}

/// The `dashscript` namespace in package.json: DashScript-specific config that
/// has no package.json standard equivalent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashscriptCfg {
    /// Output shape: `bin` (default, native binary) / `rust` (translated crate)
    /// / `wasm` / `napi` (planned). Overridable by `ds build --target`.
    #[serde(default = "default_target")]
    pub target: String,
    /// Rust crate dependencies → Cargo.toml `[dependencies]`/`[dev-dependencies]`.
    #[serde(default, skip_serializing_if = "CargoDeps::is_empty")]
    pub cargo: CargoDeps,
}

impl Default for DashscriptCfg {
    fn default() -> Self {
        Self {
            target: default_target(),
            cargo: CargoDeps::default(),
        }
    }
}

/// Deserialize package.json `workspaces` accepting either a single glob string
/// or an array of globs (npm allows both), yielding a `Vec<String>`.
fn deserialize_workspaces<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<serde_json::Value>::deserialize(deserializer)?;
    match opt {
        None | Some(serde_json::Value::Null) => Ok(Vec::new()),
        Some(serde_json::Value::String(s)) => Ok(vec![s]),
        Some(serde_json::Value::Array(arr)) => arr
            .into_iter()
            .map(|v| match v {
                serde_json::Value::String(s) => Ok(s),
                other => Err(serde::de::Error::custom(format!(
                    "workspaces entries must be strings, got {other}"
                ))),
            })
            .collect(),
        // yarn classic object form: `{ "packages": ["packages/*", ...] }` —
        // unwrap the inner list; yarn berry's extra keys are ignored. pnpm's
        // separate `pnpm-workspace.yaml` and deno's singular `deno.json`
        // `workspace` are not package.json fields, so they are read at the
        // `discover_members` call site instead of here.
        Some(serde_json::Value::Object(map)) => match map.get("packages") {
            Some(serde_json::Value::Array(arr)) => arr
                .iter()
                .map(|v| match v {
                    serde_json::Value::String(s) => Ok(s.clone()),
                    other => Err(serde::de::Error::custom(format!(
                        "workspaces.packages entries must be strings, got {other}"
                    ))),
                })
                .collect(),
            _ => Ok(Vec::new()),
        },
        Some(other) => Err(serde::de::Error::custom(format!(
            "workspaces must be a string, array, or {{packages:[]}}, got {other}"
        ))),
    }
}

/// Deserialize package.json `repository` accepting either a shorthand string
/// or the full object form (`{ "type": "git", "url": "…" }`), yielding the URL
/// (the object's `url` field). npm allows both shapes; only the URL reaches
/// `Cargo.toml [package].repository`.
fn deserialize_repository<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(opt.and_then(|v| match v {
        serde_json::Value::String(s) => Some(s),
        serde_json::Value::Object(m) => m.get("url").and_then(|u| u.as_str()).map(String::from),
        _ => None,
    }))
}

/// Deserialize package.json `author` accepting either a shorthand string or
/// the full object form (`{ "name": "…", "email": "…", "url": "…" }`),
/// yielding the name. npm allows both shapes; the name is what
/// `Cargo.toml [package].authors` carries.
fn deserialize_author<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(opt.and_then(|v| match v {
        serde_json::Value::String(s) => Some(s),
        serde_json::Value::Object(m) => m.get("name").and_then(|n| n.as_str()).map(String::from),
        _ => None,
    }))
}

/// A DashScript project = a standard **`package.json`** plus a `dashscript`
/// namespace.
///
/// Every standard package.json field is reused verbatim (npm/pnpm semantics);
/// only `dashscript.target` and `dashscript.cargo` are DashScript-specific.
/// Field order is the JSON output order: metadata first, then entries/scripts,
/// then npm deps, then the `dashscript` namespace.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Package {
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
    /// Source repository URL → `Cargo.toml` `[package].repository`. Accepts
    /// npm's shorthand string or the full `{ "url": "…" }` object form.
    #[serde(
        default,
        deserialize_with = "deserialize_repository",
        skip_serializing_if = "Option::is_none"
    )]
    pub repository: Option<String>,
    /// Project homepage URL → `Cargo.toml` `[package].homepage`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    /// Discoverability keywords → `Cargo.toml` `[package].keywords`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keywords: Vec<String>,
    /// Author → `Cargo.toml` `[package].authors`. Accepts npm's shorthand
    /// string or the full `{ "name": "…" }` object form (yields the name).
    #[serde(
        default,
        deserialize_with = "deserialize_author",
        skip_serializing_if = "Option::is_none"
    )]
    pub author: Option<String>,
    /// Executable entry points (npm `bin`, string | object) → Cargo `[[bin]]`
    /// targets. A single executable is `"bin": "main.ts"` (named after the
    /// package); multiple are `"bin": { "<name>": "<file>" }`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bin: Option<BinSpec>,
    /// Library entry (npm `main`) → Cargo `[lib]`. A crate with a `main`
    /// exports its modules for bins to `use` — shared code lives here, never
    /// in another bin (cargo forbids one bin from `mod`-ing another).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main: Option<String>,
    /// Shell-command scripts (npm `scripts`, e.g. `"start": "ds main.ts"`),
    /// run via `ds run`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub scripts: BTreeMap<String, String>,
    /// Workspace member globs (npm `workspaces`, e.g. `["apps/*", "packages/*"]`)
    /// on a monorepo root. Accepts a string or an array.
    #[serde(
        default,
        deserialize_with = "deserialize_workspaces",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub workspaces: Vec<String>,
    /// npm package dependencies (package.json standard) → `node_modules`.
    /// DashScript compiles them by layer (`.ts` source translated / JS dist
    /// routed through the engine); they do **not** flow into Cargo.toml.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dependencies: BTreeMap<String, String>,
    /// npm dev-only dependencies (package.json standard) → `node_modules`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dev_dependencies: BTreeMap<String, String>,
    /// DashScript-specific config (no package.json standard equivalent).
    #[serde(default, skip_serializing_if = "is_default_dashscript")]
    pub dashscript: DashscriptCfg,
}

/// `skip_serializing_if` predicate for [`DashscriptCfg`] — omit the whole
/// `dashscript` object when it is all defaults (target = `bin`, no cargo deps),
/// so a plain npm package.json stays tidy.
fn is_default_dashscript(cfg: &DashscriptCfg) -> bool {
    cfg.target == DEFAULT_TARGET && cfg.cargo.is_empty()
}

impl Default for Package {
    fn default() -> Self {
        Self {
            name: String::new(),
            version: default_version(),
            description: None,
            license: None,
            repository: None,
            homepage: None,
            keywords: Vec::new(),
            author: None,
            bin: None,
            main: None,
            scripts: BTreeMap::new(),
            workspaces: Vec::new(),
            dependencies: BTreeMap::new(),
            dev_dependencies: BTreeMap::new(),
            dashscript: DashscriptCfg::default(),
        }
    }
}

/// The `src/<stem>.rs` path for a source entry, mirroring `stem_of`
/// (project.rs) so the `[lib]`/`[[bin]]` path matches the file the directory
/// walk emits. A barrel `index.ts` flattens to its parent dir name
/// (`src/index.ts` → `src/src.rs`); any other file keeps its own stem
/// (`src/main.ts` → `src/main.rs`). Inputs are source paths (a tsconfig-
/// discovered lib entry or a `bin` source), never dist artifacts — a dist
/// `main` is build output, not a source entry, and never reaches here.
fn src_to_rust_path(src_path: &str) -> String {
    let stem = crate::project::stem_of(std::path::Path::new(src_path));
    format!("src/{stem}.rs")
}

impl Package {
    /// Parse a `package.json` document.
    ///
    /// # Errors
    /// Returns a [`serde_json::Error`] if the document is not valid JSON or
    /// does not match the package shape.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// The package name as a legal cargo `[package].name`, injective over npm's
    /// charset and `ds_`-prefixed to separate npm-origin crates from
    /// cargo-native crates (`cargo:serde` stays `serde`). npm scoped names
    /// (`@scope/name`) carry `@`/`/` that cargo forbids; the escape is lossless
    /// and the result doubles as the crate ident, path-dep key, and member cache
    /// dir — see [`crate::translator::imports::npm_to_ds_ident`].
    #[must_use]
    pub fn cargo_name(&self) -> String {
        crate::translator::imports::npm_to_ds_ident(&self.name)
    }

    /// The `[package]` + `[dependencies]` body — the shared core emitted for a
    /// single-package project ([`to_cargo_toml`]), a workspace member
    /// ([`to_member_toml`]), or a lone-file throwaway. No `[profile]` and no
    /// `[workspace]`: those belong to the single-package root or the workspace
    /// root. Only `dashscript.cargo.dependencies` flow into `[dependencies]`
    /// (package.json `dependencies` are npm packages, handled separately).
    /// Metadata (version/description/license/repository/homepage/keywords/author)
    /// passes straight through to `[package]`.
    fn package_body(&self) -> String {
        let mut out = String::from("[package]\n");
        out.push_str(&format!("name = {:?}\n", self.cargo_name()));
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
        if let Some(author) = &self.author {
            out.push_str(&format!("authors = [{author:?}]\n"));
        }
        let cargo = &self.dashscript.cargo;
        if !cargo.dependencies.is_empty() {
            let deps: Vec<String> = cargo
                .dependencies
                .iter()
                .map(|(name, spec)| format!("{name} = {}", spec.to_toml()))
                .collect();
            out.push_str("\n[dependencies]\n");
            out.push_str(&deps.join("\n"));
            out.push('\n');
        }
        if !cargo.dev_dependencies.is_empty() {
            let dev_deps: Vec<String> = cargo
                .dev_dependencies
                .iter()
                .map(|(name, spec)| format!("{name} = {}", spec.to_toml()))
                .collect();
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
        // unwind) so a `.ts` `try/catch` — which lowers to `catch_unwind` —
        // reliably catches a `throw` (→ `panic!`). DashScript generates this
        // Cargo.toml, so it owns the panic strategy: that is precisely what makes
        // `catch_unwind` sound, where on an arbitrary user `Cargo.toml` it
        // would not be (a `panic = "abort"` build silently drops the catch).
        out.push_str(
            "\n[profile.release]\npanic = \"unwind\"\nopt-level = 3\nlto = \"thin\"\ncodegen-units = 1\n",
        );
        out.push_str("\n[workspace]\n");
        out
    }

    /// The `(bin_name, src_path)` entries declared by `bin`, resolved against
    /// the package name for a single-string `bin`. Empty when `bin` is unset.
    /// `ds build`/`ds run` use this to emit one `[[bin]]` per declared entry.
    pub fn bin_entries(&self) -> Vec<(String, String)> {
        self.bin
            .as_ref()
            .map_or_else(Vec::new, |spec| spec.entries(&self.name))
    }

    /// A single-package `Cargo.toml` with explicit `[[bin]]` / `[lib]` targets
    /// for the project-as-one-crate model (every `.ts` translates to
    /// `src/<stem>.rs`). Emits [`package_body`] + one `[[bin]] name/path` per
    /// declared bin + an optional `[lib]` + `[profile.release]` + an empty
    /// `[workspace]`; the no-arg [`to_cargo_toml`] is for a lone file.
    pub fn to_cargo_toml_with_bins(&self, bins: &[(String, String)], lib: Option<&str>) -> String {
        let mut out = self.package_body();
        for (name, src_path) in bins {
            out.push_str(&format!(
                "\n[[bin]]\nname = {name:?}\npath = {:?}\n",
                src_to_rust_path(src_path)
            ));
        }
        if let Some(lib_path) = lib {
            out.push_str(&format!(
                "\n[lib]\npath = {:?}\n",
                src_to_rust_path(lib_path)
            ));
        }
        out.push_str(
            "\n[profile.release]\npanic = \"unwind\"\nopt-level = 3\nlto = \"thin\"\ncodegen-units = 1\n",
        );
        out.push_str("\n[workspace]\n");
        out
    }

    /// A workspace member's `Cargo.toml` with inheritance from the workspace
    /// root. `[package]` name + `version.workspace`/`edition.workspace` (shared
    /// via `[workspace.package]`) + each bin/lib target. Run-time cargo deps
    /// become `name.workspace = true` when in `inherited_deps` (the root's
    /// `[workspace.dependencies]` union); member-only deps are inline (with
    /// their full Cargo.toml spec). Dev-deps stay inline — cargo pools
    /// `[dependencies]`, not `[dev-dependencies]`. The workspace root owns
    /// `[profile]` and `[workspace]`, so neither is emitted here.
    pub fn to_member_toml(
        &self,
        bins: &[(String, String)],
        lib: Option<&str>,
        inherited_deps: &std::collections::BTreeSet<String>,
        path_deps: &[String],
    ) -> String {
        let mut out = String::from("[package]\n");
        out.push_str(&format!("name = {:?}\n", self.cargo_name()));
        out.push_str("version.workspace = true\n");
        out.push_str("edition.workspace = true\n");
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
        if let Some(author) = &self.author {
            out.push_str(&format!("authors = [{author:?}]\n"));
        }

        let cargo = &self.dashscript.cargo;
        let mut deps: Vec<String> = cargo
            .dependencies
            .iter()
            .map(|(name, spec)| {
                if inherited_deps.contains(name) {
                    format!("{name}.workspace = true")
                } else {
                    format!("{name} = {}", spec.to_toml())
                }
            })
            .collect();
        // Cross-workspace-member path dependencies: the translator records a
        // bare specifier resolving to a sibling member crate (e.g.
        // `@office-open/xml`) as a cargo path dep rather than merging that
        // member's source into this crate. The stored crate ident is already
        // the injective `ds_`-prefixed name (e.g. `ds_office_openSxml`) —
        // identical to that member's `[package].name` and cache dir — so it
        // serves verbatim as both the dep key and the `../<name>` path.
        for crate_ident in path_deps {
            deps.push(format!("{crate_ident} = {{ path = \"../{crate_ident}\" }}"));
        }
        if !deps.is_empty() {
            out.push_str("\n[dependencies]\n");
            out.push_str(&deps.join("\n"));
            out.push('\n');
        }
        let dev_deps: Vec<String> = cargo
            .dev_dependencies
            .iter()
            .map(|(name, spec)| format!("{name} = {}", spec.to_toml()))
            .collect();
        if !dev_deps.is_empty() {
            out.push_str("\n[dev-dependencies]\n");
            out.push_str(&dev_deps.join("\n"));
            out.push('\n');
        }

        for (name, src_path) in bins {
            out.push_str(&format!(
                "\n[[bin]]\nname = {name:?}\npath = {:?}\n",
                src_to_rust_path(src_path)
            ));
        }
        if let Some(lib_path) = lib {
            out.push_str(&format!(
                "\n[lib]\npath = {:?}\n",
                src_to_rust_path(lib_path)
            ));
        }
        out
    }

    /// A workspace root `Cargo.toml`: `[workspace] members` +
    /// `[workspace.package]` (metadata members inherit via
    /// `field.workspace = true`) + `[workspace.dependencies]` (the union of
    /// every member's cargo deps, so a dep two members use is declared once) +
    /// `[profile.release]`. One root means one shared `target/` and one
    /// `Cargo.lock`, so a dependency used by several members compiles once —
    /// cargo's native hoisted `node_modules`. Members sit directly under the
    /// root (`<name>/`), mirroring the single-package `.cache/dash/<name>/`
    /// layout (no `members/` layer).
    pub fn workspace_root_toml(
        &self,
        member_names: &[String],
        all_deps: &BTreeMap<String, CargoDepSpec>,
    ) -> String {
        let members: Vec<String> = member_names.iter().map(|n| format!("\"{n}\"")).collect();
        let mut out = String::from("[workspace]\n");
        out.push_str(&format!("members = [{}]\n", members.join(", ")));
        out.push_str("resolver = \"2\"\n");

        // [workspace.package]: the metadata every member inherits.
        out.push_str("\n[workspace.package]\n");
        out.push_str(&format!("version = {:?}\n", self.version));
        out.push_str("edition = \"2021\"\n");
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
        if let Some(author) = &self.author {
            out.push_str(&format!("authors = [{author:?}]\n"));
        }

        // [workspace.dependencies]: union of member cargo deps (name -> spec).
        if !all_deps.is_empty() {
            out.push_str("\n[workspace.dependencies]\n");
            for (name, spec) in all_deps {
                out.push_str(&format!("{name} = {}\n", spec.to_toml()));
            }
        }

        out.push_str(
            "\n[profile.release]\npanic = \"unwind\"\nopt-level = 3\nlto = \"thin\"\ncodegen-units = 1\n",
        );
        out
    }

    /// Record a cargo crate dependency under `dashscript.cargo.dependencies`.
    /// Returns `true` if newly added, `false` if it already existed (the spec
    /// is still updated in place).
    pub fn add_cargo_dependency(&mut self, name: &str, spec: CargoDepSpec) -> bool {
        self.dashscript
            .cargo
            .dependencies
            .insert(name.to_string(), spec)
            .is_none()
    }

    /// Remove a cargo crate dependency. Returns `true` if it was present.
    pub fn remove_cargo_dependency(&mut self, name: &str) -> bool {
        self.dashscript.cargo.dependencies.remove(name).is_some()
    }

    /// Serialize back to a pretty `package.json` document (2-space indent), so
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
    use super::*;

    #[test]
    fn from_json_accepts_object_repository_and_author() {
        // npm package.json allows `repository` and `author` as either a
        // shorthand string or a full object. Both must parse (object → url/name).
        let json = r#"{
            "name": "demo",
            "version": "1.0.0",
            "repository": { "type": "git", "url": "https://example.com/repo.git" },
            "author": { "name": "Demo Macro", "email": "abc@example.com" }
        }"#;
        let pkg = Package::from_json(json).expect("object-form repository/author must parse");
        assert_eq!(
            pkg.repository.as_deref(),
            Some("https://example.com/repo.git")
        );
        assert_eq!(pkg.author.as_deref(), Some("Demo Macro"));
    }

    #[test]
    fn from_json_accepts_string_repository_and_author() {
        let json = r#"{
            "name": "demo",
            "version": "1.0.0",
            "repository": "https://example.com/repo.git",
            "author": "Demo Macro <abc@example.com>"
        }"#;
        let pkg = Package::from_json(json).expect("string-form repository/author must parse");
        assert_eq!(
            pkg.repository.as_deref(),
            Some("https://example.com/repo.git")
        );
        assert_eq!(pkg.author.as_deref(), Some("Demo Macro <abc@example.com>"));
    }

    #[test]
    fn add_cargo_dependency_inserts_and_reports_new() {
        let mut m = Package::default();
        assert!(m.add_cargo_dependency("serde", CargoDepSpec::Version("1.0".to_string())));
        assert!(!m.add_cargo_dependency(
            // already present
            "serde",
            CargoDepSpec::Version("2.0".to_string()),
        ));
        assert_eq!(
            m.dashscript.cargo.dependencies.get("serde"),
            Some(&CargoDepSpec::Version("2.0".to_string()))
        );
    }

    #[test]
    fn remove_cargo_dependency_reports_presence() {
        let mut m = Package::default();
        m.add_cargo_dependency("serde", CargoDepSpec::Version("1.0".to_string()));
        assert!(m.remove_cargo_dependency("serde"));
        assert!(!m.remove_cargo_dependency("serde"));
    }

    #[test]
    fn add_cargo_dependency_flows_into_cargo_toml() {
        let mut m = Package {
            name: "demo".to_string(),
            ..Package::default()
        };
        m.add_cargo_dependency("serde", CargoDepSpec::Version("1.0".to_string()));
        let toml = m.to_cargo_toml();
        assert!(toml.contains("serde = \"1.0\""), "got:\n{toml}");
    }

    #[test]
    fn cargo_detail_spec_emits_features() {
        let mut m = Package {
            name: "demo".to_string(),
            ..Package::default()
        };
        m.add_cargo_dependency(
            "serde",
            CargoDepSpec::Detail {
                version: "1.0".to_string(),
                features: vec!["derive".to_string()],
                path: None,
                git: None,
                branch: None,
                default_features: None,
            },
        );
        let toml = m.to_cargo_toml();
        assert!(
            toml.contains("serde = { version = \"1.0\", features = [\"derive\"] }"),
            "got:\n{toml}"
        );
    }

    #[test]
    fn to_json_roundtrips_through_from_json() {
        let mut m = Package {
            name: "demo".to_string(),
            ..Package::default()
        };
        m.add_cargo_dependency("serde", CargoDepSpec::Version("1.0".to_string()));
        let json = m.to_json().expect("should serialize");
        assert!(
            json.contains("\"serde\": \"1.0\""),
            "cargo dep should serialize under dashscript.cargo, got:\n{json}"
        );
        let m2 = Package::from_json(&json).expect("should parse");
        assert_eq!(m2.name, "demo");
        assert_eq!(
            m2.dashscript.cargo.dependencies.get("serde"),
            Some(&CargoDepSpec::Version("1.0".to_string()))
        );
    }

    #[test]
    fn cargo_toml_pins_panic_unwind_for_try_catch() {
        let m = Package {
            name: "demo".to_string(),
            ..Package::default()
        };
        let toml = m.to_cargo_toml();
        assert!(
            toml.contains("[profile.release]\npanic = \"unwind\""),
            "release must pin panic=unwind so try/catch's catch_unwind is sound, got:\n{toml}"
        );
    }

    #[test]
    fn npm_dependencies_do_not_leak_into_cargo_toml() {
        // package.json `dependencies` are npm packages (node_modules); only
        // `dashscript.cargo.dependencies` flow into Cargo.toml.
        let json = r#"{
  "name": "demo",
  "dependencies": { "lodash": "^4.17" },
  "dashscript": { "cargo": { "dependencies": { "serde": "1.0" } } }
}"#;
        let m = Package::from_json(json).expect("should parse");
        let toml = m.to_cargo_toml();
        assert!(toml.contains("serde = \"1.0\""), "cargo dep, got:\n{toml}");
        assert!(
            !toml.contains("lodash"),
            "npm dep must not leak, got:\n{toml}"
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
  "author": "Jane <jane@example.com>",
  "dashscript": { "cargo": { "dependencies": { "serde": "1.0" } } }
}"#;
        let m = Package::from_json(json).expect("should parse");
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
    fn target_default_is_bin() {
        let m = Package {
            name: "demo".to_string(),
            ..Package::default()
        };
        assert_eq!(m.dashscript.target, "bin");
    }

    #[test]
    fn target_override_via_dashscript_namespace() {
        let json = r#"{ "name": "demo", "dashscript": { "target": "rust" } }"#;
        let m = Package::from_json(json).expect("should parse");
        assert_eq!(m.dashscript.target, "rust");
    }

    #[test]
    fn to_json_omits_unset_optional_fields() {
        let m = Package {
            name: "demo".to_string(),
            ..Package::default()
        };
        let json = m.to_json().expect("should serialize");
        assert!(!json.contains("description"), "got:\n{json}");
        assert!(!json.contains("scripts"), "got:\n{json}");
        assert!(!json.contains("workspaces"), "got:\n{json}");
        assert!(!json.contains("dependencies"), "got:\n{json}");
        assert!(
            !json.contains("dashscript"),
            "default dashscript omitted, got:\n{json}"
        );
        assert!(json.contains("\"version\": \"0.0.0\""), "got:\n{json}");
    }

    #[test]
    fn workspaces_accepts_string_or_array() {
        let m1 = Package::from_json(r#"{ "name": "a", "workspaces": "packages/*" }"#)
            .expect("string workspaces");
        assert_eq!(m1.workspaces, vec!["packages/*".to_string()]);
        let m2 = Package::from_json(r#"{ "name": "a", "workspaces": ["apps/*", "packages/*"] }"#)
            .expect("array workspaces");
        assert_eq!(
            m2.workspaces,
            vec!["apps/*".to_string(), "packages/*".to_string()]
        );
    }

    #[test]
    fn bin_uses_main_for_lib() {
        // package.json `bin` → [[bin]]; `main` → [lib] (reused official fields).
        let m = Package::from_json(
            r#"{ "name": "tour", "bin": { "numbers": "numbers.ts" }, "main": "lib.ts" }"#,
        )
        .expect("should parse");
        let toml = m.to_cargo_toml_with_bins(&m.bin_entries(), m.main.as_deref());
        assert!(toml.contains("[[bin]]"), "missing [[bin]], got:\n{toml}");
        assert!(
            toml.contains("name = \"numbers\""),
            "bin name, got:\n{toml}"
        );
        assert!(
            toml.contains("path = \"src/numbers.rs\""),
            "bin path flattened to src/, got:\n{toml}"
        );
        assert!(toml.contains("[lib]"), "missing [lib], got:\n{toml}");
        assert!(
            toml.contains("path = \"src/lib.rs\""),
            "lib path, got:\n{toml}"
        );
    }

    #[test]
    fn dev_dependencies_emit_separate_section() {
        let json = r#"{
  "name": "app",
  "dashscript": {
    "cargo": {
      "dependencies": { "serde": "1.0" },
      "devDependencies": { "tempfile": "3.0" }
    }
  }
}"#;
        let m = Package::from_json(json).expect("should parse");
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

    #[test]
    fn to_member_toml_inherits_via_workspace() {
        let mut m = Package {
            name: "demo".to_string(),
            ..Package::default()
        };
        m.add_cargo_dependency("serde", CargoDepSpec::Version("1.0".to_string()));
        let inherited: std::collections::BTreeSet<String> =
            ["serde".to_string()].into_iter().collect();
        let toml = m.to_member_toml(&[], None, &inherited, &[]);
        assert!(toml.contains("[package]"), "got:\n{toml}");
        assert!(toml.contains("version.workspace = true"), "got:\n{toml}");
        assert!(toml.contains("edition.workspace = true"), "got:\n{toml}");
        assert!(toml.contains("serde.workspace = true"), "got:\n{toml}");
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
    fn to_member_toml_declares_member_only_dep_inline() {
        let mut m = Package {
            name: "demo".to_string(),
            ..Package::default()
        };
        m.add_cargo_dependency("local-only", CargoDepSpec::Version("0.1".to_string()));
        let inherited = std::collections::BTreeSet::new();
        let toml = m.to_member_toml(&[], None, &inherited, &[]);
        assert!(toml.contains("local-only = \"0.1\""), "got:\n{toml}");
    }

    #[test]
    fn to_member_toml_emits_member_path_deps() {
        let m = Package {
            name: "demo".to_string(),
            ..Package::default()
        };
        let inherited = std::collections::BTreeSet::new();
        // The translator records a cross-member bare specifier
        // (`@office-open/xml`) as its injective ds_-prefixed crate ident, which
        // is identical to that member's `[package].name` and cache dir, so it
        // serves verbatim as both the dep key and the `../<name>` path.
        let path_deps = vec!["ds_office_openSxml".to_string()];
        let toml = m.to_member_toml(&[], None, &inherited, &path_deps);
        assert!(
            toml.contains("ds_office_openSxml = { path = \"../ds_office_openSxml\" }"),
            "got:\n{toml}"
        );
    }

    #[test]
    fn workspace_root_toml_inherits_package_and_deps() {
        let root = Package {
            name: "ws".to_string(),
            version: "1.2.3".to_string(),
            license: Some("MIT".to_string()),
            ..Package::default()
        };
        let mut deps = BTreeMap::new();
        deps.insert(
            "serde".to_string(),
            CargoDepSpec::Version("1.0".to_string()),
        );
        let toml = root.workspace_root_toml(&["app-a".to_string(), "app-b".to_string()], &deps);
        assert!(
            toml.contains("members = [\"app-a\", \"app-b\"]"),
            "got:\n{toml}"
        );
        assert!(toml.contains("resolver = \"2\""), "got:\n{toml}");
        assert!(toml.contains("[workspace.package]"), "got:\n{toml}");
        assert!(toml.contains("version = \"1.2.3\""), "got:\n{toml}");
        assert!(toml.contains("license = \"MIT\""), "got:\n{toml}");
        assert!(toml.contains("[workspace.dependencies]"), "got:\n{toml}");
        assert!(toml.contains("serde = \"1.0\""), "got:\n{toml}");
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
        let m = Package::from_json(r#"{ "name": "app", "bin": "main.ts" }"#).expect("should parse");
        assert_eq!(
            m.bin_entries(),
            vec![("app".to_string(), "main.ts".to_string())]
        );
    }

    #[test]
    fn bin_multiple_uses_keys_as_names() {
        let m = Package::from_json(
            r#"{ "name": "tour", "bin": { "numbers": "numbers.ts", "globals": "globals.ts" } }"#,
        )
        .expect("should parse");
        let mut bins = m.bin_entries();
        bins.sort();
        assert_eq!(
            bins,
            vec![
                ("globals".to_string(), "globals.ts".to_string()),
                ("numbers".to_string(), "numbers.ts".to_string()),
            ]
        );
    }

    #[test]
    fn bin_unset_yields_no_entries() {
        let m = Package::from_json(r#"{ "name": "app" }"#).expect("should parse");
        assert!(m.bin_entries().is_empty());
    }
}
