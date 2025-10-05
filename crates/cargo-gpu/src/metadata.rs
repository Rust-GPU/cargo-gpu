//! Get config from the shader crate's `Cargo.toml` `[*.metadata.rust-gpu.*]`.
//!
//! `cargo` formally ignores this metadata,
//! so that packages can implement their own behaviour with it.

use core::fmt::Debug;
use std::{fs, path::Path};

use cargo_gpu_build::spirv_cache::cargo_metadata::{Metadata, MetadataCommand, Package};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{from_value, json, to_value, Value};

use crate::merge::{json_merge_in, merge};

/// Metadata that could be extracted
/// from `[*.metadata.*]` sections of `Cargo.toml` files.
pub trait CargoMetadata: Debug + Default + Serialize + DeserializeOwned {
    /// Patches self metadata with its source available.
    fn patch(&mut self, shader_crate: &Path, source: CargoMetadataSource<'_>);
}

/// Source of the extracted [metadata](CargoMetadata).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[expect(clippy::allow_attributes, reason = "expect doesn't work for dead_code")]
pub enum CargoMetadataSource<'borrow> {
    /// Metadata was extracted from the workspace `Cargo.toml`.
    Workspace(#[allow(dead_code, reason = "part of public API")] &'borrow Metadata),
    /// Metadata was extracted from the crate `Cargo.toml`.
    Crate(#[allow(dead_code, reason = "part of public API")] &'borrow Package),
}

/// Converts `rust-gpu`-specific sections from `Cargo.toml`
/// to the value of specified type.
///
/// The section in question is: `[*.metadata.rust-gpu.*]`.
/// See the `shader-crate-template` for an example.
pub fn from_cargo_metadata<M>(shader_crate: &Path) -> anyhow::Result<M>
where
    M: CargoMetadata,
{
    let cargo_metadata = cargo_metadata(shader_crate)?;
    let config = merge_configs(&cargo_metadata, shader_crate)?;
    Ok(config)
}

/// Retrieves cargo metadata from a `Cargo.toml` by provided path.
fn cargo_metadata(path: &Path) -> anyhow::Result<Metadata> {
    let metadata = MetadataCommand::new().current_dir(path).exec()?;
    Ok(metadata)
}

/// Merges the various sources of config: defaults, workspace and shader crate.
fn merge_configs<M>(cargo_metadata: &Metadata, shader_crate: &Path) -> anyhow::Result<M>
where
    M: CargoMetadata,
{
    let workspace_metadata = workspace_metadata(cargo_metadata, shader_crate)?;
    let crate_metadata = crate_metadata(cargo_metadata, shader_crate)?;
    let merged = merge(&workspace_metadata, &crate_metadata)?;
    Ok(merged)
}

/// Retrieves any `rust-gpu` metadata set in the workspace's `Cargo.toml`.
fn workspace_metadata<M>(cargo_metadata: &Metadata, shader_crate: &Path) -> anyhow::Result<M>
where
    M: CargoMetadata,
{
    log::debug!("looking for workspace metadata...");

    let mut metadata = rust_gpu_metadata::<M>(&cargo_metadata.workspace_metadata)?;
    metadata.patch(shader_crate, CargoMetadataSource::Workspace(cargo_metadata));

    log::debug!("found workspace metadata: {metadata:#?}");
    Ok(metadata)
}

/// Retrieves any `rust-gpu` metadata set in the crate's `Cargo.toml`.
fn crate_metadata<M>(cargo_metadata: &Metadata, shader_crate: &Path) -> anyhow::Result<M>
where
    M: CargoMetadata,
{
    log::debug!("looking for crate metadata...");

    let Some(package) = find_shader_crate(cargo_metadata, shader_crate)? else {
        return Ok(M::default());
    };
    let mut metadata = rust_gpu_metadata::<M>(&package.metadata)?;
    metadata.patch(shader_crate, CargoMetadataSource::Crate(package));

    log::debug!("found crate metadata: {metadata:#?}");
    Ok(metadata)
}

/// Searches for the shader crate in the cargo metadata by its path.
fn find_shader_crate<'meta>(
    cargo_metadata: &'meta Metadata,
    shader_crate: &Path,
) -> anyhow::Result<Option<&'meta Package>> {
    let shader_crate_path = fs::canonicalize(shader_crate)?.join("Cargo.toml");
    for package in &cargo_metadata.packages {
        let manifest_path = fs::canonicalize(&package.manifest_path)?;
        log::debug!(
            "matching shader crate path with manifest path: '{}' == '{}'?",
            shader_crate_path.display(),
            manifest_path.display()
        );
        if manifest_path == shader_crate_path {
            log::debug!("...matches crate `{}`!", package.name);
            return Ok(Some(package));
        }
    }
    Ok(None)
}

/// Retrieves `rust-gpu` value from some metadata in JSON format.
fn rust_gpu_metadata<T>(metadata: &Value) -> anyhow::Result<T>
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

#[expect(
    clippy::indexing_slicing,
    reason = "We don't need to be so strict in tests"
)]
#[cfg(test)]
mod test {
    use std::path::Path;

    use crate::build::Build;

    use super::*;

    const MANIFEST: &str = env!("CARGO_MANIFEST_DIR");
    const PACKAGE: &str = env!("CARGO_PKG_NAME");

    #[test_log::test]
    fn generates_defaults() {
        let mut metadata = MetadataCommand::new().current_dir(MANIFEST).exec().unwrap();
        metadata.packages.first_mut().unwrap().metadata = json!({});

        let configs = merge_configs::<Build>(&metadata, Path::new("./")).unwrap();
        assert!(configs.build.spirv_builder.release);
        assert!(!configs.install.auto_install_rust_toolchain);
    }

    #[test_log::test]
    fn can_override_config_from_workspace_toml() {
        let mut metadata = MetadataCommand::new().current_dir(MANIFEST).exec().unwrap();
        metadata.workspace_metadata = json!({
            "rust-gpu": {
                "build": {
                    "release": false
                },
                "install": {
                    "auto-install-rust-toolchain": true
                }
            }
        });

        let configs = merge_configs::<Build>(&metadata, Path::new("./")).unwrap();
        assert!(!configs.build.spirv_builder.release);
        assert!(configs.install.auto_install_rust_toolchain);
    }

    #[test_log::test]
    fn can_override_config_from_crate_toml() {
        let mut metadata = MetadataCommand::new().current_dir(MANIFEST).exec().unwrap();
        let cargo_gpu = metadata
            .packages
            .iter_mut()
            .find(|package| package.name.contains(PACKAGE))
            .unwrap();
        cargo_gpu.metadata = json!({
            "rust-gpu": {
                "build": {
                    "release": false
                },
                "install": {
                    "auto-install-rust-toolchain": true
                }
            }
        });

        let configs = merge_configs::<Build>(&metadata, Path::new(".")).unwrap();
        assert!(!configs.build.spirv_builder.release);
        assert!(configs.install.auto_install_rust_toolchain);
    }
}
