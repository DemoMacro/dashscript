//! `ds cache clean`: remove the in-project `.cache/`.

use std::{error::Error, fs, path::PathBuf, process::ExitCode};

/// Remove the in-project `.cache/` (`ds cache clean`) — the cached build
/// projects and the `install` fetch dir. The global `~/.cache/dash/` for lone
/// files is left untouched.
pub(crate) fn cache_clean() -> Result<ExitCode, Box<dyn Error>> {
    let cache = PathBuf::from(".cache");
    if !cache.exists() {
        println!("ds: no .cache to clean");
        return Ok(ExitCode::SUCCESS);
    }
    fs::remove_dir_all(&cache)?;
    println!("ds: cleaned {}", cache.display());
    Ok(ExitCode::SUCCESS)
}
