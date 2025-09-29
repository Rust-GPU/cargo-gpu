//! Defines caching policy of the crate.

#![expect(clippy::module_name_repetitions, reason = "this is intended")]

use std::path::PathBuf;

/// Returns path to the directory where all the cache files are located.
///
/// Possible values by OS are:
/// * Windows: `C:/users/<user>/AppData/Local/rust-gpu`
/// * Mac: `~/Library/Caches/rust-gpu`
/// * Linux: `~/.cache/rust-gpu`
///
/// # Errors
///
/// Fails if there is no cache directory available.
#[inline]
pub fn cache_dir() -> Result<PathBuf, CacheDirError> {
    let dir = directories::BaseDirs::new()
        .ok_or(CacheDirError(()))?
        .cache_dir()
        .join("rust-gpu");
    Ok(dir)
}

/// An error indicating that there is no cache directory available.
#[derive(Debug, Clone, thiserror::Error)]
#[error("could not find cache directory")]
pub struct CacheDirError(());
