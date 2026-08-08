//! Emitted `__ds`/`__ds::engine` runtime helper sources — the `const &str`
//! slices concatenated into the generated `src/__ds.rs` (and the engine
//! module). Each slice maps to a [`super::RuntimeDep`]; [`super::RuntimeDeps`]
//! concatenates whichever a translation flagged (see `helper_module` /
//! `engine_helper_module`).

mod async_streams;
mod crypto;
mod engine;
mod regex;
mod scalars;
mod type_traits;
mod url;
mod web_api;

pub use self::{
    async_streams::*, crypto::*, engine::*, regex::*, scalars::*, type_traits::*, url::*,
    web_api::*,
};
