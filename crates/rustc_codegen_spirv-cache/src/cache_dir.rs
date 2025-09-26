//! Implementation of cache directory handling.

use core::{
    error::Error,
    fmt::{self, Display},
};
use std::path::PathBuf;

/// Returns path to the directory where all the cache files are located.
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
#[derive(Debug, Clone)]
pub struct CacheDirError(());

impl Display for CacheDirError {
    #[inline]
    #[expect(
        clippy::min_ident_chars,
        reason = "It's a core library trait implementation"
    )]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "could not find cache directory")
    }
}

#[expect(
    clippy::missing_trait_methods,
    reason = "It's a core library trait implementation"
)]
impl Error for CacheDirError {}
