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
    let inherited: std::collections::BTreeSet<String> = ["serde".to_string()].into_iter().collect();
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
