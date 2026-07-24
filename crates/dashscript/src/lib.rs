//! dashscript — TypeScript ergonomics, Rust performance, compiled to native.
//!
//! Three responsibilities, no more:
//! - [`translator`] — oxc AST → Rust source
//! - [`package`]    — `package.json` → `Cargo.toml`
//! - [`bindgen`]    — Rust crate → `.ds` type declaration
//!
//! Parsing reuses [oxc](https://oxc.rs/) (`oxc_parser`); `check` and `fmt` are
//! built in-process on the parsed AST (`oxc_linter`/`oxc_formatter` are not on
//! crates.io).

pub mod bindgen;
pub mod fetch;
pub mod package;
pub mod translator;

pub use bindgen::Bindgen;
pub use package::{CargoDepSpec, Package};
pub use translator::{RuntimeDeps, Translator};
