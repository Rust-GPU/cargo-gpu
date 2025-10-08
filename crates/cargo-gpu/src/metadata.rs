//! Get config from the shader crate's `Cargo.toml` `[*.metadata.rust-gpu.*]`

#![cfg(feature = "clap")]

use cargo_metadata::MetadataCommand;
use serde_json::Value;

/// `Metadata` refers to the `[metadata.*]` section of `Cargo.toml` that `cargo` formally
/// ignores so that packages can implement their own behaviour with it.
#[derive(Debug)]
pub struct Metadata;

impl Metadata {
    /// Convert `rust-gpu`-specific sections in `Cargo.toml` to `clap`-compatible arguments.
    /// The section in question is: `[package.metadata.rust-gpu.*]`. See the `shader-crate-template`
    /// for an example.
    ///
    /// First we generate the CLI arg defaults as JSON. Then on top of those we merge any config
    /// from the workspace `Cargo.toml`, then on top of those we merge any config from the shader
    /// crate's `Cargo.toml`.
    pub fn as_json(path: &std::path::PathBuf) -> anyhow::Result<Value> {
        let cargo_json = Self::get_cargo_toml_as_json(path)?;
        let config = Self::merge_configs(&cargo_json, path)?;
        Ok(config)
    }

    /// Merge the various source of config: defaults, workspace and shader crate.
    fn merge_configs(
        cargo_json: &cargo_metadata::Metadata,
        path: &std::path::Path,
    ) -> anyhow::Result<Value> {
        let mut metadata = crate::config::Config::defaults_as_json()?;
        crate::config::Config::json_merge(
            &mut metadata,
            {
                log::debug!("looking for workspace metadata");
                let ws_meta = Self::get_rust_gpu_from_metadata(&cargo_json.workspace_metadata);
                log::trace!("workspace_metadata: {ws_meta:#?}");
                ws_meta
            },
            None,
        )?;
        crate::config::Config::json_merge(
            &mut metadata,
            {
                log::debug!("looking for crate metadata");
                let mut crate_meta = Self::get_crate_metadata(cargo_json, path)?;
                log::trace!("crate_metadata: {crate_meta:#?}");
                if let Some(output_path) = crate_meta.pointer_mut("/build/output_dir") {
                    log::debug!("found output-dir path in crate metadata: {output_path:?}");
                    if let Some(output_dir) = output_path.clone().as_str() {
                        let new_output_path = path.join(output_dir);
                        *output_path = Value::String(format!("{}", new_output_path.display()));
                        log::debug!(
                            "setting that to be relative to the Cargo.toml it was found in: {}",
                            new_output_path.display()
                        );
                    }
                }
                crate_meta
            },
            None,
        )?;

        Ok(metadata)
    }

    /// Convert a `Cargo.toml` to JSON
    fn get_cargo_toml_as_json(
        path: &std::path::PathBuf,
    ) -> anyhow::Result<cargo_metadata::Metadata> {
        Ok(MetadataCommand::new().current_dir(path).exec()?)
    }

    /// Get any `rust-gpu` metadata set in the crate's `Cargo.toml`
    fn get_crate_metadata(
        json: &cargo_metadata::Metadata,
        path: &std::path::Path,
    ) -> anyhow::Result<Value> {
        let shader_crate_path = std::fs::canonicalize(path)?.join("Cargo.toml");

        for package in &json.packages {
            let manifest_path = std::fs::canonicalize(package.manifest_path.as_std_path())?;
            log::debug!(
                "Matching shader crate path with manifest path: '{}' == '{}'?",
                shader_crate_path.display(),
                manifest_path.display()
            );
            if manifest_path == shader_crate_path {
                log::debug!("...matches! Getting metadata");
                return Ok(Self::get_rust_gpu_from_metadata(&package.metadata));
            }
        }
        Ok(serde_json::json!({}))
    }

    /// Get `rust-gpu` value from some metadata
    fn get_rust_gpu_from_metadata(metadata: &Value) -> Value {
        Self::keys_to_snake_case(
            metadata
                .pointer("/rust-gpu")
                .cloned()
                .unwrap_or(Value::Null),
        )
    }

    /// Convert JSON keys from kebab case to snake case. Eg: `a-b` to `a_b`.
    ///
    /// Detection of keys for serde deserialization must match the case in the Rust structs.
    /// However clap defaults to detecting CLI args in kebab case. So here we do the conversion.
    #[expect(clippy::wildcard_enum_match_arm, reason = "we only want objects")]
    fn keys_to_snake_case(json: Value) -> Value {
        match json {
            Value::Object(object) => Value::Object(
                object
                    .into_iter()
                    .map(|(key, value)| (key.replace('-', "_"), Self::keys_to_snake_case(value)))
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
    use super::*;
    use std::path::Path;

    #[test_log::test]
    fn generates_defaults() {
        let mut metadata = MetadataCommand::new()
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .exec()
            .unwrap();
        metadata.packages.first_mut().unwrap().metadata = serde_json::json!({});
        let configs = Metadata::merge_configs(&metadata, Path::new("./")).unwrap();
        assert_eq!(configs["build"]["release"], Value::Bool(true));
        assert_eq!(
            configs["install"]["auto_install_rust_toolchain"],
            Value::Bool(false)
        );
    }

    #[test_log::test]
    fn can_override_config_from_workspace_toml() {
        let mut metadata = MetadataCommand::new()
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .exec()
            .unwrap();
        metadata.workspace_metadata = serde_json::json!({
            "rust-gpu": {
                "build": {
                    "release": false
                },
                "install": {
                    "auto-install-rust-toolchain": true
                }
            }
        });
        let configs = Metadata::merge_configs(&metadata, Path::new("./")).unwrap();
        assert_eq!(configs["build"]["release"], Value::Bool(false));
        assert_eq!(
            configs["install"]["auto_install_rust_toolchain"],
            Value::Bool(true)
        );
    }

    #[test_log::test]
    fn can_override_config_from_crate_toml() {
        let mut metadata = MetadataCommand::new()
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .exec()
            .unwrap();
        let cargo_gpu = metadata
            .packages
            .iter_mut()
            .find(|package| package.name.contains("cargo-gpu"))
            .unwrap();
        cargo_gpu.metadata = serde_json::json!({
            "rust-gpu": {
                "build": {
                    "release": false
                },
                "install": {
                    "auto-install-rust-toolchain": true
                }
            }
        });
        let configs = Metadata::merge_configs(&metadata, Path::new(".")).unwrap();
        assert_eq!(configs["build"]["release"], Value::Bool(false));
        assert_eq!(
            configs["install"]["auto_install_rust_toolchain"],
            Value::Bool(true)
        );
    }
}
