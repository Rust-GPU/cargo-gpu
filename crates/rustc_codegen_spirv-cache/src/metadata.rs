//! Functionality of the crate which is tightly linked
//! with cargo [metadata](Metadata).

#![expect(clippy::module_name_repetitions, reason = "this is intended")]

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use cargo_metadata::{camino::Utf8PathBuf, Metadata, MetadataCommand, Package};

/// Get the package metadata from the shader crate located at `crate_path`.
///
/// # Errors
///
/// Returns an error if the path does not exist, non-final part of it is not a directory
/// or if `cargo metadata` invocation fails.
#[inline]
pub fn query_metadata<P>(crate_path: P) -> Result<Metadata, QueryMetadataError>
where
    P: AsRef<Path>,
{
    let path_ref = crate_path.as_ref();
    log::debug!("running `cargo metadata` on '{}'", path_ref.display());

    let path = path_ref.canonicalize()?;
    let metadata = MetadataCommand::new().current_dir(path).exec()?;
    Ok(metadata)
}

/// An error indicating that querying metadata failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum QueryMetadataError {
    /// Provided shader crate path is invalid.
    #[error("failed to get an absolute path to the crate: {0}")]
    InvalidCratePath(#[from] io::Error),
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
    fn package_by_name<S>(&self, name: S) -> Result<&Package, PackageByNameError>
    where
        S: AsRef<str>;

    /// Search for a package by provided package path,
    /// appending `Cargo.toml` to it before the search.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - manifest path (provided or from [`Metadata`]) was not valid
    /// - no package with the specified manifest path exists
    fn package_by_manifest_path<P>(
        &self,
        package_path: P,
    ) -> Result<&Package, PackageByManifestPathError>
    where
        P: AsRef<Path>;
}

impl MetadataExt for Metadata {
    #[inline]
    fn package_by_name<S>(&self, name: S) -> Result<&Package, PackageByNameError>
    where
        S: AsRef<str>,
    {
        let inspect = |package: &&Package| {
            log::debug!(
                "matching provided name with package name: `{}` == `{}`?",
                name.as_ref(),
                package.name
            );
        };
        let Some(package) = self
            .packages
            .iter()
            .inspect(inspect)
            .find(|package| package.name.as_ref() == name.as_ref())
        else {
            let workspace_root = self.workspace_root.clone();
            return Err(PackageByNameError::new(name, workspace_root));
        };

        log::trace!(
            "...matches package `{}` of version `{}`!",
            package.name,
            package.version
        );
        Ok(package)
    }

    #[inline]
    fn package_by_manifest_path<P>(
        &self,
        package_path: P,
    ) -> Result<&Package, PackageByManifestPathError>
    where
        P: AsRef<Path>,
    {
        let path = fs::canonicalize(package_path)?.join("Cargo.toml");
        for package in &self.packages {
            let manifest_path = fs::canonicalize(&package.manifest_path)?;
            log::debug!(
                "matching provided manifest path with package manifest path: '{}' == '{}'?",
                path.display(),
                manifest_path.display()
            );
            if manifest_path == path {
                log::trace!(
                    "...matches package `{}` of version `{}`!",
                    package.name,
                    package.version
                );
                return Ok(package);
            }
        }

        let workspace_root = self.workspace_root.clone();
        Err(PackageByManifestPathError::new(path, workspace_root))
    }
}

/// An error indicating that a package by the specified name was not found.
#[derive(Debug, Clone, thiserror::Error)]
#[error("`{name}` not found in `Cargo.toml` at '{workspace_root}'")]
pub struct PackageByNameError {
    /// The name of the package that was not found.
    name: String,
    /// The workspace root of the [`Metadata`].
    workspace_root: Utf8PathBuf,
}

impl PackageByNameError {
    /// Creates self from the given package name and workspace root.
    fn new(name: impl AsRef<str>, workspace_root: impl Into<Utf8PathBuf>) -> Self {
        Self {
            name: name.as_ref().into(),
            workspace_root: workspace_root.into(),
        }
    }
}

/// An error indicating that a package by the specified manifest path was not found.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PackageByManifestPathError {
    /// Path to the package manifest was not valid.
    #[error("package manifest path was not valid: {0}")]
    InvalidManifestPath(#[from] io::Error),
    /// No package with the specified manifest path exists.
    #[error("no package with manifest path '{path}' found at '{workspace_root}'")]
    NoPackageFound {
        /// The manifest path of the package that was not found.
        path: PathBuf,
        /// The workspace root of the [`Metadata`].
        workspace_root: Utf8PathBuf,
    },
}

impl PackageByManifestPathError {
    /// Creates self from the given manifest path and workspace root.
    fn new(path: impl Into<PathBuf>, workspace_root: impl Into<Utf8PathBuf>) -> Self {
        Self::NoPackageFound {
            path: path.into(),
            workspace_root: workspace_root.into(),
        }
    }
}
