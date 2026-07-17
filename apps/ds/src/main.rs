//! `ds` — the DashScript toolchain entry point.
//!
//! Wired: `<file.ds>` (run a file), `run <script>`, `build [--target]`, `add`,
//! `remove`, `lint`, `check`, `fmt`, `install`, `cache clean`, `lsp`.

use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::{Command, ExitCode, ExitStatus},
};

use dashscript::{fetch, Bindgen, Manifest, Translator};

mod lsp;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        // `ds <file.ds>` — run a file directly (like `node a.js` / `vp node`).
        Some(arg) if is_ds_file(arg) => report(run_file(arg)),
        // `ds run <script>` — run a `manifest.json` script (like `pnpm run`).
        // `run` is always explicit: `ds <script>` would collide with `ds <file.ds>`.
        Some("run") => match args.next() {
            Some(script) => report(run_script(&script)),
            None => usage_exit("usage: ds run <script>"),
        },
        Some("build") => {
            let rest: Vec<String> = args.collect();
            match parse_build_args(&rest) {
                Ok((file, target, filter)) => {
                    // No file at a workspace root → build every member (--filter
                    // picks one). `--filter` is workspace-only.
                    if file.is_none() && is_workspace_root(Path::new(".")) {
                        report(workspace_build(Path::new("."), filter.as_deref(), target.as_deref()))
                    } else if filter.is_some() {
                        usage_exit("ds build: --filter <name> only applies at a workspace root")
                    } else {
                        report(build(file.as_deref(), target.as_deref()))
                    }
                }
                Err(msg) => usage_exit(&msg),
            }
        }
        Some("add") => match args.next() {
            Some(spec) => report(add(&spec)),
            None => usage_exit("usage: ds add <crate|rust:crate|file.rs>"),
        },
        Some("remove") => match args.next() {
            Some(name) => report(remove(&name)),
            None => usage_exit("usage: ds remove <crate|rust:crate>"),
        },
        // `ds lint` = translatability only (the old `ds check`). `ds check`
        // below is the composite lint + fmt check, matching `vp check`.
        Some("lint") => match args.next() {
            Some(file) => report(lint(&file)),
            None => usage_exit("usage: ds lint <file.ds>"),
        },
        Some("check") => match args.next() {
            Some(file) => report(check(&file)),
            None => usage_exit("usage: ds check <file.ds>"),
        },
        Some("fmt") => match args.next() {
            Some(file) => report(fmt(&file)),
            None => usage_exit("usage: ds fmt <file.ds>"),
        },
        // `ds install` = ensure manifest deps are fetched + a Cargo.lock exists
        // (like `pnpm install` / `vp install`). No node_modules equivalent —
        // cargo's `~/.cargo/registry` is the dependency store.
        Some("install") => report(install()),
        Some("cache") => match args.next().as_deref() {
            Some("clean") => report(cache_clean()),
            Some(other) => usage_exit(&format!("ds cache: unknown action '{other}' (try 'clean')")),
            None => usage_exit("usage: ds cache clean"),
        },
        Some("lsp") => match lsp::run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("ds lsp: {err}");
                ExitCode::FAILURE
            }
        },
        Some(other) => {
            eprintln!("ds: unknown subcommand '{other}'");
            eprintln!(
                "available: <file.ds>, run <script>, build [<file>] [--target], add, remove, lint, check, fmt, install, cache clean"
            );
            ExitCode::FAILURE
        }
        None => {
            eprintln!("ds: DashScript toolchain");
            eprintln!("usage: ds <command> [args]   (or: ds <file.ds>)");
            eprintln!(
                "commands: <file.ds>, run <script>, build, add, remove, lint, check, fmt, install, cache clean"
            );
            ExitCode::FAILURE
        }
    }
}

/// Whether an argument is a direct `.ds` file run (`ds main.ds`). We only look
/// at the suffix — a missing file is reported by `run_file`, not the dispatch.
fn is_ds_file(arg: &str) -> bool {
    arg.ends_with(".ds")
}

/// Parsed `ds build` flags: optional entry file, optional `--target`, optional
/// `--filter` (workspace member).
type BuildArgs = (Option<String>, Option<String>, Option<String>);

/// Parse `ds build` arguments: an optional `.ds` file, an optional
/// `--target <bin|rust>` override, and an optional `--filter <name>` (workspace
/// member). Returns an error message on misuse (shown as usage). No file means
/// build the project entry (`manifest.entry`/`main.ds`) — or, at a workspace
/// root, every member.
fn parse_build_args(args: &[String]) -> Result<BuildArgs, String> {
    let mut file = None;
    let mut target = None;
    let mut filter = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--target" => {
                if i + 1 < args.len() {
                    target = Some(args[i + 1].clone());
                    i += 2;
                } else {
                    return Err("usage: ds build [<file.ds>] [--target <bin|rust>] [--filter <name>]".into());
                }
            }
            "--filter" => {
                if i + 1 < args.len() {
                    filter = Some(args[i + 1].clone());
                    i += 2;
                } else {
                    return Err("usage: ds build [<file.ds>] [--target <bin|rust>] [--filter <name>]".into());
                }
            }
            f if !f.starts_with('-') => {
                file = Some(f.to_string());
                i += 1;
            }
            other => return Err(format!("ds build: unknown option '{other}'")),
        }
    }
    Ok((file, target, filter))
}

/// Report a command result, printing any error to stderr.
fn report(result: Result<ExitCode, Box<dyn Error>>) -> ExitCode {
    match result {
        Ok(code) => code,
        Err(err) => {
            eprintln!("ds: {err}");
            ExitCode::FAILURE
        }
    }
}

fn usage_exit(msg: &str) -> ExitCode {
    eprintln!("{msg}");
    ExitCode::FAILURE
}

/// Translate `src` and write `src/main.rs` (plus each imported local module as
/// `src/<module>.rs`, declared with a leading `mod <module>;`) into
/// `project_dir/src/`. The caller writes `Cargo.toml`. Shared by a single-
/// package build ([`emit_cargo_project`]) and by workspace members (whose
/// Cargo.toml the workspace root owns). v1: a single layer of imports — an
/// imported module that itself imports is not followed.
fn translate_sources(
    src: &str,
    src_path: &Path,
    project_dir: &Path,
) -> Result<(), Box<dyn Error>> {
    let rust = Translator::new()
        .translate(src)
        .map_err(|e| format!("translate {}: {e}", src_path.display()))?;

    let base = src_path.parent().unwrap_or_else(|| Path::new(""));
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut mod_decls = String::new();
    for imp in Translator::new().imports(src) {
        if !seen.insert(imp.module.clone()) {
            continue; // dedupe repeated imports of the same module
        }
        let dep_path = resolve_local_module(base, &imp.source)?;
        let dep_src = fs::read_to_string(&dep_path)
            .map_err(|e| format!("cannot read import {}: {e}", dep_path.display()))?;
        let dep_rust = Translator::new()
            .translate(&dep_src)
            .map_err(|e| format!("translate {}: {e}", dep_path.display()))?;
        fs::write(
            project_dir.join("src").join(format!("{}.rs", imp.module)),
            dep_rust,
        )?;
        mod_decls.push_str(&format!("mod {};\n", imp.module));
    }

    let main = if mod_decls.is_empty() {
        rust
    } else {
        format!("{mod_decls}\n{rust}")
    };
    fs::write(project_dir.join("src").join("main.rs"), main)?;
    Ok(())
}

/// Translate a `.ds` file into a buildable Cargo project at `project_dir`: the
/// resolved manifest as `Cargo.toml` + the translated source as `src/main.rs`.
pub(crate) fn emit_cargo_project(
    src: &str,
    src_path: &Path,
    project_dir: &Path,
) -> Result<(), Box<dyn Error>> {
    let cargo_toml = resolve_manifest(src_path);
    fs::create_dir_all(project_dir.join("src"))?;
    fs::write(project_dir.join("Cargo.toml"), cargo_toml)?;
    translate_sources(src, src_path, project_dir)?;
    Ok(())
}

/// Resolve a relative `.ds` import (`"./other"` or `"./other.ds"`) against the
/// importing file's directory. Errors clearly when no matching file exists.
fn resolve_local_module(base: &Path, source: &str) -> Result<PathBuf, Box<dyn Error>> {
    let candidate = if source.ends_with(".ds") {
        base.join(source)
    } else {
        base.join(format!("{source}.ds"))
    };
    if candidate.exists() {
        return Ok(candidate);
    }
    Err(format!(
        "dashscript: import '{source}' does not resolve to a .ds file (tried {})",
        candidate.display()
    )
    .into())
}

/// Resolve the Cargo manifest for `src_path`: the `manifest.json` found walking
/// up from the file (Deno-style), otherwise a minimal manifest named after the
/// project (`project_name`).
fn resolve_manifest(src_path: &Path) -> String {
    if let Some(root) = find_manifest_root(src_path) {
        if let Ok(json) = fs::read_to_string(root.join("manifest.json")) {
            if let Ok(manifest) = Manifest::from_json(&json) {
                return manifest.to_cargo_toml();
            }
        }
    }
    Manifest {
        name: project_name(src_path),
        ..Manifest::default()
    }
    .to_cargo_toml()
}

/// The cache directory for a `.ds` entry file, Deno-style: walk up from the
/// file for a `manifest.json`; found → in-project `.cache/dash/<project>/` —
/// **one per project** (keyed by project name, not the entry stem, so two
/// `main.ds` files in different projects don't collide and one project's
/// entries share a cache); not found (a lone file) → global
/// `~/.cache/dash/<hash>/`. The `dash` segment mirrors the global cache root,
/// so DashScript owns one namespace under `.cache/`. `run`, `build`, and
/// `install` all share this directory, so repeat invocations reuse cargo's
/// incremental `target/` instead of recompiling std and every dependency from
/// scratch. Falls back to a temp dir if no platform cache dir is resolvable,
/// so a lone file always runs.
fn cache_project_dir(src_path: &Path) -> PathBuf {
    if let Some(root) = find_manifest_root(src_path) {
        return root
            .join(".cache")
            .join("dash")
            .join(project_name(src_path));
    }
    global_cache_dir(src_path)
}

/// Walk up from the `.ds` file's directory for the nearest `manifest.json`,
/// returning its directory (the project root) if one exists.
fn find_manifest_root(src_path: &Path) -> Option<PathBuf> {
    let dir = src_path.parent()?;
    for ancestor in dir.ancestors() {
        if ancestor.join("manifest.json").exists() {
            return Some(ancestor.to_path_buf());
        }
    }
    None
}

/// The global fallback cache for a lone `.ds` file (no `manifest.json` found
/// walking up): `~/.cache/dash/<hash(canonical_path)>/`, keyed by the file's
/// canonical path so the same file reuses it across runs.
fn global_cache_dir(src_path: &Path) -> PathBuf {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let key = {
        let canonical = fs::canonicalize(src_path).unwrap_or_else(|_| src_path.to_path_buf());
        let mut hasher = DefaultHasher::new();
        canonical.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    };
    match dirs::cache_dir() {
        Some(cache) => cache.join("dash").join(&key),
        None => std::env::temp_dir().join(format!("dash-{key}")),
    }
}

/// The file stem of a path as an owned `String` ("main.ds" → "main").
fn stem_of(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("dash")
        .to_string()
}

/// The build output name: the `manifest.json` `name` if present, else the
/// project directory name, else the file stem — never the bare stem when a
/// project exists, so two entry files don't clobber `dist/<name>`.
fn project_name(src_path: &Path) -> String {
    if let Some(root) = find_manifest_root(src_path) {
        if let Ok(json) = fs::read_to_string(root.join("manifest.json")) {
            if let Ok(manifest) = Manifest::from_json(&json) {
                if !manifest.name.trim().is_empty() {
                    return manifest.name;
                }
            }
        }
        if let Some(dir) = root.file_name().and_then(|s| s.to_str()) {
            if !dir.is_empty() {
                return dir.to_string();
            }
        }
    }
    stem_of(src_path)
}

/// Resolve the project entry file for a file-less `ds build`: the
/// `manifest.json` `entry` if it exists, else `main.ds` in the cwd.
fn resolve_entry() -> Result<String, Box<dyn Error>> {
    if let Ok(manifest) = read_manifest(Path::new("manifest.json")) {
        if let Some(entry) = &manifest.entry {
            if Path::new(entry).exists() {
                return Ok(entry.clone());
            }
        }
    }
    if Path::new("main.ds").exists() {
        return Ok("main.ds".to_string());
    }
    Err("ds build: no entry file (pass <file.ds>, set manifest entry, or add main.ds)".into())
}

/// The build target for `src_path`: the `--target` override, else the
/// `manifest.json` `target`, else `bin`.
fn resolve_target(src_path: &Path, override_target: Option<&str>) -> String {
    if let Some(t) = override_target {
        return t.to_string();
    }
    if let Some(root) = find_manifest_root(src_path) {
        if let Ok(json) = fs::read_to_string(root.join("manifest.json")) {
            if let Ok(manifest) = Manifest::from_json(&json) {
                return manifest.target;
            }
        }
    }
    "bin".to_string()
}

/// Translate a `.ds` file into its cached Cargo project and `cargo run` it
/// (`ds <file.ds>`).
///
/// The cache is resolved Deno-style (`cache_project_dir`): in-project
/// `.cache/dash/<project>/` when a `manifest.json` is found walking up, else a
/// global `~/.cache/dash/<hash>/`. Execution is delegated to the system `cargo`
/// for now — a DashScript-managed toolchain (downloaded on demand, no `rustup`)
/// will replace this later.
fn run_file(file: &str) -> Result<ExitCode, Box<dyn Error>> {
    let path = Path::new(file);
    let src = fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let project = cache_project_dir(path);
    emit_cargo_project(&src, path, &project)?;
    let status = invoke_cargo(&project, ["run", "--quiet"])?;
    Ok(status_to_code(status))
}

/// Run a `manifest.json` script by name (`ds run <script>`), executing its
/// value through the system shell — so a script may be any shell command
/// (`"ds main.ds"`, `"cargo test"`, …), like a `package.json` script.
fn run_script(script: &str) -> Result<ExitCode, Box<dyn Error>> {
    let manifest_path = Path::new("manifest.json");
    let manifest = read_manifest(manifest_path)?;
    let command = manifest
        .scripts
        .get(script)
        .ok_or_else(|| format!("no script '{script}' in {}", manifest_path.display()))?;
    println!("ds> {script}: {command}");
    shell_exec(command)
}

/// Run a shell command string through the system shell (POSIX `sh -c` on Unix,
/// `cmd /C` on Windows), so `scripts` entries can be arbitrary shell.
fn shell_exec(command: &str) -> Result<ExitCode, Box<dyn Error>> {
    #[cfg(unix)]
    let status = Command::new("sh").arg("-c").arg(command).status();
    #[cfg(windows)]
    let status = Command::new("cmd").arg("/C").arg(command).status();
    let status = status.map_err(|e| format!("failed to spawn shell: {e}"))?;
    Ok(status_to_code(status))
}

/// Build a `.ds` file or the project entry. `--target rust` emits the
/// translated Rust crate under `dist/<name>/` (no `target/`); the default
/// `bin` target compiles (`cargo build --release`) and copies the native
/// binary to `dist/<name>`. The compile uses the shared cache
/// (`cache_project_dir`), so `target/` never lands in `dist/`.
fn build(file: Option<&str>, target_override: Option<&str>) -> Result<ExitCode, Box<dyn Error>> {
    let file = match file {
        Some(f) => f.to_string(),
        None => resolve_entry()?,
    };
    build_at(&file, target_override, Path::new("dist"))
}

/// Core build (single package): translate `entry`, then emit a native binary
/// (`bin`) or Rust crate (`rust`) to `<dest_root>/<name>`. A single package
/// passes `dist`; a workspace member passes its own `<member>/dist` so each
/// package's artifact stays independent (publishable on its own, like a pnpm
/// workspace package). Workspace bin builds go through [`workspace_build`]
/// instead (one cargo workspace, shared `target/`).
fn build_at(
    entry: &str,
    target_override: Option<&str>,
    dest_root: &Path,
) -> Result<ExitCode, Box<dyn Error>> {
    let path = Path::new(entry);
    let src = fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let name = project_name(path);
    let target = resolve_target(path, target_override);

    // Clear any prior output at <dest_root>/<name> so switching targets
    // (bin ↔ rust) does not collide: a `bin` build leaves a file, a `rust` build
    // a dir.
    fs::create_dir_all(dest_root)?;
    let dest_base = dest_root.join(&name);
    let _ = fs::remove_dir_all(&dest_base);
    let _ = fs::remove_file(&dest_base);
    if cfg!(windows) {
        let _ = fs::remove_file(format!("{}.exe", dest_base.display()));
    }

    match target.as_str() {
        "rust" => {
            emit_cargo_project(&src, path, &dest_base)?;
            // `dist/` holds a clean crate — drop any `target/` a prior run left.
            let _ = fs::remove_dir_all(dest_base.join("target"));
            println!("ds: emitted {} (Rust crate)", dest_base.display());
            Ok(ExitCode::SUCCESS)
        }
        "bin" => {
            let cache = cache_project_dir(path);
            emit_cargo_project(&src, path, &cache)?;
            let status = invoke_cargo(&cache, ["build", "--release", "--quiet"])?;
            if !status.success() {
                return Ok(status_to_code(status));
            }
            // cargo names the binary `<name>` (Unix) / `<name>.exe` (Windows);
            // the dist output mirrors that so the file is runnable as-is.
            let bin_name = if cfg!(windows) {
                format!("{name}.exe")
            } else {
                name.clone()
            };
            let bin = cache.join("target").join("release").join(&bin_name);
            let dest = dest_base.with_file_name(&bin_name);
            fs::copy(&bin, &dest)?;
            println!("ds: built {}", dest.display());
            Ok(ExitCode::SUCCESS)
        }
        other => Err(format!(
            "ds build: target '{other}' not yet supported (use --target <bin|rust>)"
        )
        .into()),
    }
}

/// Add a dependency to the project.
///
/// A `.rs` path runs bindgen on that local file (writes `<stem>.ds` beside
/// it — the `bindgen-demo` flow). Any other spec is a crate name, with or
/// without a `rust:` prefix: cargo downloads it into its global registry and
/// DashScript records it in `manifest.json`. No `.ds` declaration is generated
/// — type information comes from the crate source itself (read directly by the
/// language server, the way rust-analyzer reads `~/.cargo`).
fn add(spec: &str) -> Result<ExitCode, Box<dyn Error>> {
    if spec.ends_with(".rs") {
        return add_local_file(spec);
    }
    let crate_name = spec.strip_prefix("rust:").unwrap_or(spec);
    let version = fetch::add_via_cargo(crate_name, cargo_bin())
        .map_err(|e| format!("ds add {crate_name}: {e}"))?;
    let manifest_path = Path::new("manifest.json");
    let mut manifest = read_manifest(manifest_path).unwrap_or_else(|_| default_manifest());
    manifest.add_dependency("rust", crate_name, &version);
    fs::write(manifest_path, format!("{}\n", manifest.to_json()?))?;
    println!("ds: added rust:{crate_name} = {version}");
    // Like `pnpm add`: record the dep, then refresh the lockfile (install) so
    // the new dependency is fetched and pinned in one step.
    install()
}

/// Remove a crate dependency from `manifest.json`.
fn remove(spec: &str) -> Result<ExitCode, Box<dyn Error>> {
    let name = spec.strip_prefix("rust:").unwrap_or(spec);
    let manifest_path = Path::new("manifest.json");
    let mut manifest = read_manifest(manifest_path)?;
    if !manifest.remove_dependency("rust", name) {
        return Err(format!("rust:{name} is not in {}", manifest_path.display()).into());
    }
    fs::write(manifest_path, format!("{}\n", manifest.to_json()?))?;
    println!("ds: removed rust:{name}");
    Ok(ExitCode::SUCCESS)
}

/// Generate a `.ds` type declaration from a local Rust source file (bindgen),
/// written beside it as `<stem>.ds`.
fn add_local_file(file: &str) -> Result<ExitCode, Box<dyn Error>> {
    let path = Path::new(file);
    let rust = fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let ds = Bindgen::new()
        .generate(&rust)
        .map_err(|e| format!("bindgen {}: {e}", path.display()))?;
    let out = path.with_extension("ds");
    fs::write(&out, ds)?;
    println!("ds: generated {}", out.display());
    Ok(ExitCode::SUCCESS)
}

/// Path to the cargo binary — the system `cargo` today; a DashScript-managed
/// toolchain replaces this once the self-contained Rust layer lands.
fn cargo_bin() -> &'static Path {
    Path::new("cargo")
}

/// A manifest named after the current directory, with defaults.
fn default_manifest() -> Manifest {
    let name = std::env::current_dir()
        .ok()
        .and_then(|d| d.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "dashscript".to_string());
    Manifest {
        name,
        ..Manifest::default()
    }
}

/// Read and parse a `manifest.json`.
fn read_manifest(path: &Path) -> Result<Manifest, Box<dyn Error>> {
    let json = fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    Ok(Manifest::from_json(&json)?)
}

/// Lint a `.ds` file for translatability in-process: syntax errors (from
/// `oxc_parser`) plus any top-level statement the translator cannot lower to
/// Rust. No external oxlint dependency.
fn lint(file: &str) -> Result<ExitCode, Box<dyn Error>> {
    let path = Path::new(file);
    let source = fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let diagnostics = Translator::new().check(&source);
    if diagnostics.is_empty() {
        println!("ds: no issues found in {file}");
        return Ok(ExitCode::SUCCESS);
    }
    for diag in &diagnostics {
        // `with_source_code` attaches the file text so the fancy Debug render
        // (miette `fancy-no-syscall`) can print line/column + context.
        let report = diag.clone().with_source_code(source.clone());
        eprintln!("{report:?}");
    }
    Ok(ExitCode::FAILURE)
}

/// Check a `.ds` file the way `vp check` does: translatability (`lint`) plus a
/// format check (reports whether `ds fmt` would change the file — does not
/// write). Fails if either surfaces an issue.
fn check(file: &str) -> Result<ExitCode, Box<dyn Error>> {
    let path = Path::new(file);
    let source = fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;

    let mut failed = false;

    // 1. translatability
    let diagnostics = Translator::new().check(&source);
    for diag in &diagnostics {
        let report = diag.clone().with_source_code(source.clone());
        eprintln!("{report:?}");
    }
    if !diagnostics.is_empty() {
        failed = true;
    }

    // 2. format check (no write)
    let formatted = Translator::new().format(&source)?;
    if formatted != source {
        eprintln!("ds: {file} is not formatted (run `ds fmt {file}`)");
        failed = true;
    }

    if !failed {
        println!("ds: no issues found in {file}");
        Ok(ExitCode::SUCCESS)
    } else {
        Ok(ExitCode::FAILURE)
    }
}

/// Format a `.ds` file in place with `oxc_codegen` (pretty-print). No external
/// oxfmt dependency.
fn fmt(file: &str) -> Result<ExitCode, Box<dyn Error>> {
    let path = Path::new(file);
    let source = fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let formatted = Translator::new().format(&source)?;
    fs::write(path, formatted)?;
    println!("ds: formatted {file}");
    Ok(ExitCode::SUCCESS)
}

/// Ensure the manifest's dependencies are fetched and a `Cargo.lock` exists
/// (`ds install`). Emits a throwaway Cargo project under `.cache/install/` and
/// runs `cargo fetch`, which downloads crate sources to `~/.cargo/registry` —
/// the dependency store, no `node_modules` equivalent — so a later `build`/run
/// compiles without re-downloading.
fn install() -> Result<ExitCode, Box<dyn Error>> {
    let manifest = read_manifest(Path::new("manifest.json")).unwrap_or_else(|_| default_manifest());
    // Reuse the build cache (`.cache/dash/<name>/`) — not a separate dir — so
    // the `Cargo.lock` `cargo fetch` writes here is the same one `build`/`run`
    // use. No duplicate cargo project, no throwaway lockfile.
    let dir = PathBuf::from(".cache").join("dash").join(&manifest.name);
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

/// Remove the in-project `.cache/` (`ds cache clean`) — the cached build
/// projects and the `install` fetch dir. The global `~/.cache/dash/` for lone
/// files is left untouched.
fn cache_clean() -> Result<ExitCode, Box<dyn Error>> {
    let cache = PathBuf::from(".cache");
    if !cache.exists() {
        println!("ds: no .cache to clean");
        return Ok(ExitCode::SUCCESS);
    }
    fs::remove_dir_all(&cache)?;
    println!("ds: cleaned {}", cache.display());
    Ok(ExitCode::SUCCESS)
}

/// Whether `dir` is a workspace root: its `manifest.json` has a non-empty
/// `workspace` member-glob list that resolves to at least one member.
fn is_workspace_root(dir: &Path) -> bool {
    !discover_members(dir).is_empty()
}

/// Build the workspace at `root` — every member, or just the one named by
/// `--filter` (manifest name or member directory). For `bin`, members are
/// emitted under `.cache/dash/members/<name>/` of one cargo workspace, so they
/// share a single `target/` and `Cargo.lock`: a dependency two members use
/// compiles once (cargo's native hoisted-`node_modules`). For `rust`, each
/// member's crate is emitted independently to `dist/<name>/` (no compilation,
/// nothing to share).
fn workspace_build(
    root: &Path,
    filter: Option<&str>,
    target_override: Option<&str>,
) -> Result<ExitCode, Box<dyn Error>> {
    let members = discover_members(root);
    if members.is_empty() {
        return Err(
            "ds build: no workspace members matched (check `workspaces` globs in manifest.json)"
                .into(),
        );
    }

    // Select members, applying --filter (manifest name or member directory).
    let mut selected: Vec<(String, PathBuf, String)> = Vec::new();
    for member in &members {
        let dir_name = member_name_fallback(member);
        let name = manifest_name_of(member).unwrap_or_else(|| dir_name.clone());
        if let Some(want) = filter {
            if name != want && dir_name != want {
                continue;
            }
        }
        let entry = resolve_member_entry(member)?;
        selected.push((name, member.to_path_buf(), entry));
    }
    if selected.is_empty() {
        return Err(format!(
            "ds build: --filter '{}' matched no workspace member",
            filter.unwrap_or("?")
        )
        .into());
    }

    let target =
        target_override.map(|t| t.to_string()).unwrap_or_else(|| "bin".to_string());
    if target == "rust" {
        // Rust crates are emitted, not compiled — no shared `target/` to gain.
        // Each member's crate lands in its own `<member>/dist/<name>/` so the
        // package stays independently publishable.
        for (name, member_dir, entry) in &selected {
            println!("ds: {name} (workspace member, rust crate)");
            build_at(entry, Some("rust"), &member_dir.join("dist"))?;
        }
        return Ok(ExitCode::SUCCESS);
    }
    if target != "bin" {
        return Err(format!(
            "ds build: target '{target}' not yet supported (use --target <bin|rust>)"
        )
        .into());
    }

    // bin: emit one cargo workspace — members share `target/` + `Cargo.lock`.
    // Member dirs sit directly under the cache root (`<cache>/<name>/`), mirroring
    // the single-package `.cache/dash/<name>/`; a stale member from a prior run
    // (renamed/removed) is simply absent from `members` and ignored by cargo —
    // `ds cache clean` reclaims the space.
    let cache = root.join(".cache").join("dash");
    fs::create_dir_all(&cache)?;
    let names: Vec<String> = selected.iter().map(|(n, _, _)| n.clone()).collect();
    fs::write(cache.join("Cargo.toml"), Manifest::workspace_root_toml(&names))?;

    for (name, member_dir, entry) in &selected {
        let path = Path::new(entry);
        let src = fs::read_to_string(path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let member_manifest =
            read_manifest(&member_dir.join("manifest.json")).unwrap_or_else(|_| default_manifest());
        let member_cache = cache.join(name);
        fs::create_dir_all(member_cache.join("src"))?;
        fs::write(member_cache.join("Cargo.toml"), member_manifest.to_member_toml())?;
        translate_sources(&src, path, &member_cache)?;
        println!("ds: {name} (workspace member)");
    }

    println!("ds: building workspace (shared target)...");
    let status = invoke_cargo(&cache, ["build", "--release", "--quiet"])?;
    if !status.success() {
        return Ok(status_to_code(status));
    }

    // Copy each member binary to its own `<member>/dist/<name>` — not the
    // workspace root — so each package's artifact is independent and
    // publishable (like a pnpm workspace package's own dist).
    for (name, member_dir, _) in &selected {
        let bin_name = if cfg!(windows) {
            format!("{name}.exe")
        } else {
            name.clone()
        };
        let bin = cache.join("target").join("release").join(&bin_name);
        let dest_dir = member_dir.join("dist");
        let _ = fs::remove_dir_all(&dest_dir);
        fs::create_dir_all(&dest_dir)?;
        let dest = dest_dir.join(&bin_name);
        fs::copy(&bin, &dest)?;
        println!("ds: built {}", dest.display());
    }
    Ok(ExitCode::SUCCESS)
}

/// Resolve a root manifest's `workspaces` globs (e.g. `["apps/*", "packages/*"]`)
/// into member directories — each a subdirectory holding its own `manifest.json`.
/// Empty if `root` has no `workspaces` field or no members match.
fn discover_members(root: &Path) -> Vec<PathBuf> {
    let Ok(json) = fs::read_to_string(root.join("manifest.json")) else {
        return Vec::new();
    };
    let Ok(manifest) = Manifest::from_json(&json) else {
        return Vec::new();
    };
    if manifest.workspaces.is_empty() {
        return Vec::new();
    }
    let mut members = Vec::new();
    for glob in &manifest.workspaces {
        for member in expand_member_glob(root, glob) {
            if !members.contains(&member) {
                members.push(member);
            }
        }
    }
    members
}

/// Expand one workspace glob (`<dir>/*`) relative to `root` into the
/// subdirectories of `<dir>` that hold a `manifest.json`. Only the trailing
/// `/*` form is supported (the common pnpm-workspace / cargo-members case).
fn expand_member_glob(root: &Path, glob: &str) -> Vec<PathBuf> {
    let Some(dir_name) = glob.strip_suffix("/*") else {
        return Vec::new();
    };
    let dir = root.join(dir_name);
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.join("manifest.json").exists())
        .collect();
    out.sort();
    out
}

/// A member's entry: its manifest `entry`, else `main.ds` inside the member.
fn resolve_member_entry(member: &Path) -> Result<String, Box<dyn Error>> {
    let manifest_path = member.join("manifest.json");
    if let Ok(manifest) = read_manifest(&manifest_path) {
        if let Some(entry) = &manifest.entry {
            let p = member.join(entry);
            if p.exists() {
                return Ok(p.to_string_lossy().into_owned());
            }
        }
    }
    let main = member.join("main.ds");
    if main.exists() {
        return Ok(main.to_string_lossy().into_owned());
    }
    Err(format!(
        "ds build: member {} has no entry (set manifest entry or add main.ds)",
        member.display()
    )
    .into())
}

/// Read a member's manifest `name` (for `--filter` matching and display).
fn manifest_name_of(member: &Path) -> Option<String> {
    read_manifest(&member.join("manifest.json"))
        .ok()
        .map(|m| m.name)
}

/// Fallback member name: the directory's own name.
fn member_name_fallback(member: &Path) -> String {
    member
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("member")
        .to_string()
}

fn invoke_cargo<const N: usize>(project: &Path, args: [&str; N]) -> Result<ExitStatus, Box<dyn Error>> {
    Command::new("cargo")
        .args(args)
        .current_dir(project)
        .status()
        .map_err(|e| format!("failed to invoke cargo (is it on PATH?): {e}").into())
}

fn status_to_code(status: ExitStatus) -> ExitCode {
    if status.success() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
