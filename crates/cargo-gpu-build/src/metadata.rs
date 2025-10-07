//! Retrieves structured metadata from `[*.metadata.rust-gpu.*]` sections
//! of `Cargo.toml` files of the shader crate.
//!
//! `cargo` formally ignores this metadata, which allows us to implement our own behaviour with it.

use core::fmt::Debug;
use std::path::{Path, PathBuf};

use serde::{de::DeserializeOwned, Serialize};
use serde_json::{from_value, json, to_value, Value};

use crate::{
    merge::{json_merge_in, merge},
    spirv_cache::{
        cargo_metadata::{Metadata, Package},
        metadata::{
            query_metadata, MetadataExt as _, PackageByManifestPathError, QueryMetadataError,
        },
    },
};

/// Structured metadata specific to `rust-gpu` project that could be extracted
/// from `[*.metadata.rust-gpu.*]` sections of `Cargo.toml` manifest files.
///
/// See the `shader-crate-template` for an example of such metadata.
#[expect(clippy::module_name_repetitions, reason = "this is intended")]
pub trait RustGpuMetadata: Debug + Default + Serialize + DeserializeOwned {
    /// Patches self metadata with its source available.
    fn patch<P>(&mut self, shader_crate: P, source: RustGpuMetadataSource<'_>)
    where
        P: AsRef<Path>;
}

/// Source of the extracted `rust-gpu` [structured metadata](RustGpuMetadata).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RustGpuMetadataSource<'borrow> {
    /// Metadata was extracted from the workspace `Cargo.toml`.
    Workspace(&'borrow Metadata),
    /// Metadata was extracted from the crate `Cargo.toml`.
    Crate(&'borrow Package),
}

/// An error indicating failure to extract `rust-gpu` [structured metadata](RustGpuMetadata)
/// from the shader crate's `Cargo.toml` manifest files.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum FromShaderCrateError {
    /// Failed to query `cargo metadata`.
    #[error(transparent)]
    QueryMetadata(#[from] QueryMetadataError),
    /// Failed to extract `rust-gpu` structured metadata
    /// from the [result](Metadata) of executing `cargo metadata` command.
    #[error(transparent)]
    FromCargoMetadata(#[from] FromCargoMetadataError),
}

/// An error indicating failure to extract `rust-gpu` [structured metadata](RustGpuMetadata)
/// of some crate & its workspace from the [result](Metadata) of executing `cargo metadata` command.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum FromCargoMetadataError {
    /// The provided shader crate path is not a directory.
    #[error("the provided shader crate path is not a directory: {0}")]
    PathIsNotDir(PathBuf),
    /// Failed to find a package by its manifest path.
    #[error(transparent)]
    PackageByManifestPath(#[from] PackageByManifestPathError),
    /// Failed to (de)serialize JSON metadata.
    #[error("JSON metadata (de)serialization error: {0}")]
    ParseJson(#[from] serde_json::Error),
}

/// Overrides properties of the provided `rust-gpu` [structured metadata](RustGpuMetadata)
/// with other metadata retrieved from `Cargo.toml` manifest files
/// of the shader crate & its workspace.
///
/// # Errors
///
/// Returns an error if retrieving structured metadata from the shader crate fails.
/// See [error type](FromShaderCrateError) for details.
#[inline]
pub fn with_shader_crate<M, P>(metadata: &M, shader_crate: P) -> Result<M, FromShaderCrateError>
where
    M: RustGpuMetadata,
    P: AsRef<Path>,
{
    let from_shader_crate = from_shader_crate(shader_crate)?;
    let merged = merge(&from_shader_crate, metadata).map_err(FromCargoMetadataError::ParseJson)?;
    Ok(merged)
}

/// Retrieves `rust-gpu` [structured metadata](RustGpuMetadata) from `Cargo.toml` manifest files
/// of the shader crate & its workspace.
///
/// # Errors
///
/// Returns an error if retrieving structured metadata from the shader crate fails.
/// See [error type](FromShaderCrateError) for details.
#[inline]
pub fn from_shader_crate<M, P>(shader_crate: P) -> Result<M, FromShaderCrateError>
where
    M: RustGpuMetadata,
    P: AsRef<Path>,
{
    let cargo_metadata = query_metadata(shader_crate.as_ref())?;
    let metadata = from_cargo_metadata(&cargo_metadata, shader_crate)?;
    Ok(metadata)
}

/// Retrieves `rust-gpu` [structured metadata](RustGpuMetadata) from `Cargo.toml` manifest files
/// of some crate & its workspace from the [result](Metadata) of executing `cargo metadata` command.
///
/// # Errors
///
/// Returns an error if retrieving structured metadata from the shader crate fails.
/// See [error type](FromCargoMetadataError) for details.
#[inline]
#[expect(clippy::module_name_repetitions, reason = "this is intended")]
pub fn from_cargo_metadata<M, P>(
    metadata: &Metadata,
    shader_crate: P,
) -> Result<M, FromCargoMetadataError>
where
    M: RustGpuMetadata,
    P: AsRef<Path>,
{
    let shader_crate_path = shader_crate.as_ref();
    if !shader_crate_path.is_dir() {
        let path = shader_crate_path.to_path_buf();
        return Err(FromCargoMetadataError::PathIsNotDir(path));
    }

    let workspace_metadata = workspace_metadata(metadata, shader_crate_path)?;
    let crate_metadata = crate_metadata(metadata, shader_crate_path)?;
    let merged = merge(&workspace_metadata, &crate_metadata)?;
    Ok(merged)
}

/// Retrieves any `rust-gpu` [structured metadata](RustGpuMetadata)
/// set in the workspace's `Cargo.toml` manifest file.
fn workspace_metadata<M>(
    cargo_metadata: &Metadata,
    shader_crate: &Path,
) -> Result<M, FromCargoMetadataError>
where
    M: RustGpuMetadata,
{
    log::debug!("looking for workspace metadata...");
    let mut metadata = rust_gpu_metadata::<M>(&cargo_metadata.workspace_metadata)?;

    let source = RustGpuMetadataSource::Workspace(cargo_metadata);
    metadata.patch(shader_crate, source);

    log::debug!("found workspace metadata: {metadata:#?}");
    Ok(metadata)
}

/// Retrieves any `rust-gpu` [structured metadata](RustGpuMetadata)
/// set in the shader crate's `Cargo.toml` manifest file.
fn crate_metadata<M>(
    cargo_metadata: &Metadata,
    shader_crate: &Path,
) -> Result<M, FromCargoMetadataError>
where
    M: RustGpuMetadata,
{
    log::debug!("looking for crate metadata...");

    let package = cargo_metadata.package_by_manifest_path(shader_crate)?;
    let mut metadata = rust_gpu_metadata::<M>(&package.metadata)?;

    let source = RustGpuMetadataSource::Crate(package);
    metadata.patch(shader_crate, source);

    log::debug!("found crate metadata: {metadata:#?}");
    Ok(metadata)
}

/// Retrieves `rust-gpu` value from some metadata in JSON format.
fn rust_gpu_metadata<T>(metadata: &Value) -> Result<T, FromCargoMetadataError>
where
    T: Default + Serialize + DeserializeOwned,
{
    let json_patch_from_metadata = metadata
        .pointer("/rust-gpu")
        .cloned()
        .unwrap_or_else(|| json!({}))
        .keys_to_snake_case();
    log::debug!("got `rust-gpu` metadata: {json_patch_from_metadata:#?}");

    let json_default = to_value(T::default())?;
    let mut json_value = json_default.clone();
    json_merge_in(&mut json_value, json_patch_from_metadata, &json_default);

    let value = from_value(json_value)?;
    Ok(value)
}

/// Extension trait for [JSON value](Value).
trait JsonKeysToSnakeCase {
    /// Converts JSON keys from kebab case to snake case, e.g. from `a-b` to `a_b`.
    ///
    /// Detection of keys for [`serde`] deserialization must match the case in the Rust structs.
    /// However, [`clap`] defaults to detecting CLI args in kebab case. So here we do the conversion.
    fn keys_to_snake_case(self) -> Value;
}

impl JsonKeysToSnakeCase for Value {
    #[inline]
    #[expect(clippy::wildcard_enum_match_arm, reason = "we only want objects")]
    fn keys_to_snake_case(self) -> Value {
        match self {
            Self::Object(object) => Self::Object(
                object
                    .into_iter()
                    .map(|(key, value)| (key.replace('-', "_"), value.keys_to_snake_case()))
                    .collect(),
            ),
            other => other,
        }
    }
}
