//! dashscript — a TypeScript-frontend language that transpiles to idiomatic Rust.
//!
//! Three responsibilities, no more:
//! - [`translator`] — oxc AST → Rust source
//! - [`manifest`]   — `manifest.json` → `Cargo.toml`
//! - [`bindgen`]    — Rust crate → `.ds` type declaration
//!
//! Parsing, linting, and formatting of `.ds` reuse [oxc](https://oxc.rs/);
//! DashScript does not reimplement them.

pub mod bindgen;
pub mod manifest;
pub mod translator;

pub use bindgen::Bindgen;
pub use manifest::Manifest;
pub use translator::Translator;
