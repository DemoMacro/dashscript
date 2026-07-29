//! `ds` subcommands, one module per command group; each is a thin caller over
//! `dashscript::project` (package-level translation/packaging) plus argument
//! parsing and output reporting.

pub(crate) mod build;
pub(crate) mod cache;
pub(crate) mod check;
pub(crate) mod deps;
pub(crate) mod run;
