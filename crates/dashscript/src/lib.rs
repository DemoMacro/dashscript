//! dashscript — TypeScript ergonomics, Rust performance, compiled to native.
//!
//! Three responsibilities, no more:
//! - [`translator`] — oxc AST → Rust source
//! - [`manifest`]   — `manifest.json` → `Cargo.toml`
//! - [`bindgen`]    — Rust crate → `.ds` type declaration
//!
//! Parsing reuses [oxc](https://oxc.rs/) (`oxc_parser`); `check` and `fmt` are
//! built in-process on the parsed AST (`oxc_linter`/`oxc_formatter` are not on
//! crates.io).

pub mod bindgen;
pub mod fetch;
pub mod manifest;
pub mod translator;

pub use bindgen::Bindgen;
pub use manifest::Manifest;
pub use translator::{RuntimeDeps, Translator};
