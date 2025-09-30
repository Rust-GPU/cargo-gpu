//! Functionality of the crate which is tightly linked
//! with cargo [metadata](Metadata).

#![expect(clippy::module_name_repetitions, reason = "this is intended")]

use std::{io, path::Path};

use cargo_metadata::{camino::Utf8PathBuf, Metadata, MetadataCommand, Package};

/// Get the package metadata from the shader crate located at `crate_path`.
///
/// # Errors
///
/// Returns an error if the path does not exist, non-final part of it is not a directory
/// or if `cargo metadata` invocation fails.
#[inline]
pub fn query_metadata(crate_path: &Path) -> Result<Metadata, QueryMetadataError> {
    log::debug!("Running `cargo metadata` on `{}`", crate_path.display());
    let path = &crate_path.canonicalize()?;
    let metadata = MetadataCommand::new().current_dir(path).exec()?;
    Ok(metadata)
}

/// An error indicating that querying metadata failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum QueryMetadataError {
    /// Provided shader crate path is invalid.
    #[error("failed to get an absolute path to the crate: {0}")]
    InvalidPath(#[from] io::Error),
    /// Failed to run `cargo metadata` for provided shader crate.
    #[error(transparent)]
    CargoMetadata(#[from] cargo_metadata::Error),
}

/// Extension trait for [`Metadata`].
pub trait MetadataExt {
    /// Search for a package by provided name.
    ///
    /// # Errors
    ///
    /// If no package with the specified name was found, returns an error.
    fn find_package(&self, name: &str) -> Result<&Package, MissingPackageError>;
}

impl MetadataExt for Metadata {
    #[inline]
    fn find_package(&self, name: &str) -> Result<&Package, MissingPackageError> {
        let Some(package) = self
            .packages
            .iter()
            .find(|package| package.name.as_str() == name)
        else {
            let workspace_root = self.workspace_root.clone();
            return Err(MissingPackageError::new(name, workspace_root));
        };

        log::trace!("  found `{}` version `{}`", package.name, package.version);
        Ok(package)
    }
}

/// An error indicating that a package with the specified crate name was not found.
#[derive(Debug, Clone, thiserror::Error)]
#[error("`{crate_name}` not found in `Cargo.toml` at `{workspace_root:?}`")]
pub struct MissingPackageError {
    /// The crate name that was not found.
    crate_name: String,
    /// The workspace root of the [`Metadata`].
    workspace_root: Utf8PathBuf,
}

impl MissingPackageError {
    /// Creates self from the given crate name and workspace root.
    fn new(crate_name: impl Into<String>, workspace_root: impl Into<Utf8PathBuf>) -> Self {
        Self {
            crate_name: crate_name.into(),
            workspace_root: workspace_root.into(),
        }
    }
}
