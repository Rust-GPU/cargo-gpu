//! Manage and merge the various sources of config:
//! shader crate's `Cargo.toml`(s) and provided args.

use std::path::Path;

use crate::{
    merge::merge,
    metadata::{from_cargo_metadata, CargoMetadata},
};

/// Overrides the config options from `Cargo.toml` of the shader crate
/// with options from the provided config.
pub fn from_cargo_metadata_with_config<M>(shader_crate: &Path, config: &M) -> anyhow::Result<M>
where
    M: CargoMetadata,
{
    let from_cargo = from_cargo_metadata(shader_crate)?;
    let merged = merge(&from_cargo, config)?;
    Ok(merged)
}

#[cfg(test)]
mod test {
    use std::{io::Write as _, path::PathBuf};

    use clap::Parser as _;
    use spirv_builder::Capability;

    use crate::{
        build::Build,
        test::{overwrite_shader_cargo_toml, shader_crate_test_path},
    };

    use super::*;

    #[test_log::test]
    fn booleans_from_cli() {
        let shader_crate_path = shader_crate_test_path();
        let config = Build::parse_from([
            "gpu".as_ref(),
            // "build".as_ref(),
            "--shader-crate".as_ref(),
            shader_crate_path.as_os_str(),
            "--debug".as_ref(),
            "--auto-install-rust-toolchain".as_ref(),
        ]);

        let args = from_cargo_metadata_with_config(&shader_crate_path, &config).unwrap();
        assert!(!args.build.spirv_builder.release);
        assert!(args.install.auto_install_rust_toolchain);
    }

    #[test_log::test]
    fn booleans_from_cargo() {
        let shader_crate_path = shader_crate_test_path();

        let mut file = overwrite_shader_cargo_toml(&shader_crate_path);
        file.write_all(
            [
                "[package.metadata.rust-gpu.build]",
                "release = false",
                "[package.metadata.rust-gpu.install]",
                "auto-install-rust-toolchain = true",
            ]
            .join("\n")
            .as_bytes(),
        )
        .unwrap();

        let config = Build::parse_from([
            "gpu".as_ref(),
            // "build".as_ref(),
            "--shader-crate".as_ref(),
            shader_crate_path.as_os_str(),
        ]);

        let args = from_cargo_metadata_with_config(&shader_crate_path, &config).unwrap();
        assert!(!args.build.spirv_builder.release);
        assert!(args.install.auto_install_rust_toolchain);
    }

    fn update_cargo_output_dir() -> PathBuf {
        let shader_crate_path = shader_crate_test_path();
        let mut file = overwrite_shader_cargo_toml(&shader_crate_path);
        file.write_all(
            [
                "[package.metadata.rust-gpu.build]",
                "output-dir = \"/the/moon\"",
            ]
            .join("\n")
            .as_bytes(),
        )
        .unwrap();
        shader_crate_path
    }

    #[test_log::test]
    fn string_from_cargo() {
        let shader_crate_path = update_cargo_output_dir();
        let config = Build::parse_from([
            "gpu".as_ref(),
            // "build".as_ref(),
            "--shader-crate".as_ref(),
            shader_crate_path.as_os_str(),
        ]);

        let args = from_cargo_metadata_with_config(&shader_crate_path, &config).unwrap();
        if cfg!(target_os = "windows") {
            assert_eq!(args.build.output_dir, Path::new("C:/the/moon"));
        } else {
            assert_eq!(args.build.output_dir, Path::new("/the/moon"));
        }
    }

    #[test_log::test]
    fn string_from_cargo_overwritten_by_cli() {
        let shader_crate_path = update_cargo_output_dir();
        let config = Build::parse_from([
            "gpu".as_ref(),
            // "build".as_ref(),
            "--shader-crate".as_ref(),
            shader_crate_path.as_os_str(),
            "--output-dir".as_ref(),
            "/the/river".as_ref(),
        ]);

        let args = from_cargo_metadata_with_config(&shader_crate_path, &config).unwrap();
        assert_eq!(args.build.output_dir, Path::new("/the/river"));
    }

    #[test_log::test]
    fn arrays_from_cargo() {
        let shader_crate_path = shader_crate_test_path();

        let mut file = overwrite_shader_cargo_toml(&shader_crate_path);
        file.write_all(
            [
                "[package.metadata.rust-gpu.build]",
                "capabilities = [\"AtomicStorage\", \"Matrix\"]",
            ]
            .join("\n")
            .as_bytes(),
        )
        .unwrap();

        let config = Build::parse_from([
            "gpu".as_ref(),
            // "build".as_ref(),
            "--shader-crate".as_ref(),
            shader_crate_path.as_os_str(),
        ]);

        let args = from_cargo_metadata_with_config(&shader_crate_path, &config).unwrap();
        assert_eq!(
            args.build.spirv_builder.capabilities,
            [Capability::AtomicStorage, Capability::Matrix]
        );
    }

    #[test_log::test]
    fn rename_manifest_parse() {
        let shader_crate_path = shader_crate_test_path();
        let config = Build::parse_from([
            "gpu".as_ref(),
            // "build".as_ref(),
            "--shader-crate".as_ref(),
            shader_crate_path.as_os_str(),
            "--manifest-file".as_ref(),
            "mymanifest".as_ref(),
        ]);

        let args = from_cargo_metadata_with_config(&shader_crate_path, &config).unwrap();
        assert_eq!(args.build.manifest_file, "mymanifest".to_owned());
    }
}
