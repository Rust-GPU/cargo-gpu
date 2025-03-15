//! Get config from the shader crate's `Cargo.toml` `[*.metadata.rust-gpu.*]`

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
    pub fn as_json(path: &std::path::PathBuf) -> anyhow::Result<serde_json::Value> {
        let cargo_json = Self::get_cargo_toml_as_json(path)?;
        let config = Self::merge_configs(&cargo_json, path)?;
        Ok(config)
    }

    /// Convert JSON keys from kebab case to snake case. Eg: `a-b` to `a_b`.
    ///
    /// Detection of keys for serde deserialization must match the case in the Rust structs.
    /// However clap defaults to detecting CLI args in kebab case. So here we do the conversion.
    fn keys_to_snake_case(json: &mut serde_json::Value) {
        let serde_json::Value::Object(object) = json else {
            return;
        };

        *object = core::mem::take(object)
            .into_iter()
            .map(|(key, mut value)| {
                if let serde_json::Value::Object(_) = value {
                    Self::keys_to_snake_case(&mut value);
                }
                (key.replace('-', "_"), value)
            })
            .collect();
    }

    /// Merge the various source of config: defaults, workspace and shader crate.
    fn merge_configs(
        cargo_json: &serde_json::Value,
        path: &std::path::Path,
    ) -> anyhow::Result<serde_json::Value> {
        let mut metadata = crate::config::Config::defaults_as_json()?;
        crate::config::Config::json_merge(
            &mut metadata,
            {
                log::debug!("looking for workspace metadata");
                let ws_meta = Self::get_workspace_metadata(cargo_json);
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
                    log::debug!("found output-dir path in crate metadata: {:?}", output_path);
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
    //
    // TODO: reuse for getting the default `rust-gpu` source and toolchain.
    fn get_cargo_toml_as_json(path: &std::path::PathBuf) -> anyhow::Result<serde_json::Value> {
        let cargo_toml_path = path.join("Cargo.toml");
        if !cargo_toml_path.exists() {
            anyhow::bail!("{path:?} must be a shader crate directory");
        }

        log::debug!("Querying Cargo metadata for {cargo_toml_path:?}");
        let output_cargo = std::process::Command::new("cargo")
            .args([
                "metadata",
                "--no-deps",
                "--manifest-path",
                cargo_toml_path.display().to_string().as_ref(),
            ])
            .output()?;
        anyhow::ensure!(
            output_cargo.status.success(),
            "could not run `cargo metadata` on {cargo_toml_path:?}"
        );

        Ok(serde_json::from_slice(&output_cargo.stdout)?)
    }

    /// Get any `rust-gpu` metadata set in the root workspace `Cargo.toml`
    fn get_workspace_metadata(json: &serde_json::Value) -> serde_json::Value {
        let empty_json_object = serde_json::json!({});
        let mut metadata = json
            .pointer("/metadata/rust-gpu")
            .unwrap_or(&empty_json_object)
            .clone();

        Self::keys_to_snake_case(&mut metadata);
        metadata.clone()
    }

    /// Get any `rust-gpu` metadata set in the crate's `Cargo.toml`
    fn get_crate_metadata(
        json: &serde_json::Value,
        path: &std::path::Path,
    ) -> anyhow::Result<serde_json::Value> {
        let empty_json_object = serde_json::json!({});
        if let Some(serde_json::Value::Array(packages)) = json.pointer("/packages") {
            for package in packages {
                if let Some(serde_json::Value::String(manifest_path_dirty)) =
                    package.pointer("/manifest_path")
                {
                    let mut shader_crate_path = std::fs::canonicalize(path)?
                        .join("Cargo.toml")
                        .display()
                        .to_string();

                    // Windows prefixs paths with `\\?\`
                    shader_crate_path = shader_crate_path.replace(r"\\?\", "");
                    let manifest_path = manifest_path_dirty.replace(r"\\?\", "");
                    log::debug!("Matching shader crate path with manifest path: {shader_crate_path} == {manifest_path}?");
                    if manifest_path == shader_crate_path {
                        log::debug!("...matches! Getting metadata");
                        let mut metadata = package
                            .pointer("/metadata/rust-gpu")
                            .unwrap_or(&empty_json_object)
                            .clone();
                        Self::keys_to_snake_case(&mut metadata);
                        return Ok(metadata);
                    }
                }
            }
        }
        Ok(empty_json_object)
    }
}

#[expect(
    clippy::indexing_slicing,
    reason = "We don't need to be so strict in tests"
)]
#[cfg(test)]
mod test {
    use super::*;

    #[test_log::test]
    fn generates_defaults() {
        let json = serde_json::json!({});
        let configs = Metadata::merge_configs(&json, std::path::Path::new("./")).unwrap();
        assert_eq!(configs["build"]["debug"], serde_json::Value::Bool(false));
        assert_eq!(
            configs["install"]["auto_install_rust_toolchain"],
            serde_json::Value::Bool(false)
        );
    }

    #[test_log::test]
    fn can_override_config_from_workspace_toml() {
        let json = serde_json::json!(
            { "metadata": { "rust-gpu": {
                "build": {
                    "debug": true
                },
                "install": {
                    "auto-install-rust-toolchain": true
                }
            }}}
        );
        let configs = Metadata::merge_configs(&json, std::path::Path::new("./")).unwrap();
        assert_eq!(configs["build"]["debug"], serde_json::Value::Bool(true));
        assert_eq!(
            configs["install"]["auto_install_rust_toolchain"],
            serde_json::Value::Bool(true)
        );
    }

    #[test_log::test]
    fn can_override_config_from_crate_toml() {
        let marker = std::path::Path::new("./Cargo.toml");
        let json = serde_json::json!(
            { "packages": [{
                "metadata": { "rust-gpu": {
                    "build": {
                        "debug": true
                    },
                    "install": {
                        "auto-install-rust-toolchain": true
                    }
                }},
                "manifest_path": std::fs::canonicalize(marker).unwrap()
            }]}
        );
        let configs = Metadata::merge_configs(&json, marker.parent().unwrap()).unwrap();
        assert_eq!(configs["build"]["debug"], serde_json::Value::Bool(true));
        assert_eq!(
            configs["install"]["auto_install_rust_toolchain"],
            serde_json::Value::Bool(true)
        );
    }
}
