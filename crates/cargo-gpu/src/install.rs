//! `cargo gpu install`

use std::{
    io,
    path::{Path, PathBuf},
};

use cargo_gpu_build::{
    build::CargoGpuInstallMetadata,
    metadata::{RustGpuMetadata, RustGpuMetadataSource},
    spirv_cache::{backend::SpirvCodegenBackendInstallParams, toolchain::StdioCfg},
};

use crate::user_consent::ask_for_user_consent;

/// Arguments for just an install.
#[derive(Clone, Debug, clap::Parser, serde::Deserialize, serde::Serialize)]
#[non_exhaustive]
#[expect(clippy::module_name_repetitions, reason = "it is intended")]
pub struct InstallArgs {
    /// Directory containing the shader crate to compile.
    #[clap(long, alias("package"), short_alias('p'), default_value = "./")]
    #[serde(alias = "package")]
    pub shader_crate: PathBuf,

    /// The flattened [`CargoGpuInstallMetadata`].
    #[clap(flatten)]
    #[serde(flatten)]
    pub install_meta: CargoGpuInstallMetadata,

    /// Assume "yes" to "Install Rust toolchain: [y/n]" prompt.
    #[clap(long, action)]
    pub auto_install_rust_toolchain: bool,
}

impl Default for InstallArgs {
    #[inline]
    fn default() -> Self {
        Self {
            shader_crate: PathBuf::from("./"),
            install_meta: CargoGpuInstallMetadata::default(),
            auto_install_rust_toolchain: false,
        }
    }
}

/// `cargo gpu install` subcommands.
#[derive(Clone, Debug, Default, clap::Parser, serde::Deserialize, serde::Serialize)]
#[non_exhaustive]
pub struct Install {
    /// The flattened [`InstallArgs`].
    #[clap(flatten)]
    pub install: InstallArgs,
}

impl Install {
    /// Install the `rust-gpu` codegen backend for the shader crate.
    ///
    /// # Errors
    ///
    /// Returns an error if the build process fails somehow.
    #[inline]
    pub fn run(&self) -> anyhow::Result<()> {
        let Self { install } = self;
        let InstallArgs {
            shader_crate,
            install_meta,
            auto_install_rust_toolchain,
        } = install;
        let CargoGpuInstallMetadata {
            spirv_installer, ..
        } = install_meta;

        let skip_consent = *auto_install_rust_toolchain;
        let halt = ask_for_user_consent(skip_consent);
        let install_params = SpirvCodegenBackendInstallParams::from(shader_crate)
            .writer(io::stdout())
            .halt(halt)
            .stdio_cfg(StdioCfg::inherit());
        spirv_installer.install(install_params)?;
        Ok(())
    }
}

impl RustGpuMetadata for Install {
    #[inline]
    fn patch<P>(&mut self, _shader_crate: P, _source: RustGpuMetadataSource<'_>)
    where
        P: AsRef<Path>,
    {
    }
}
