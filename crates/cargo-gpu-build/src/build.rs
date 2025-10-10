//! This module provides a `rust-gpu` shader crate builder
//! usable inside of build scripts or as a part of CLI.

use std::{io, path::Path, process::Stdio};

use crate::{
    lockfile::{LockfileMismatchError, LockfileMismatchHandler},
    metadata::{RustGpuMetadata, RustGpuMetadataSource},
    spirv_builder::{CompileResult, SpirvBuilder, SpirvBuilderError},
    spirv_cache::{
        backend::{
            SpirvCodegenBackend, SpirvCodegenBackendInstallError, SpirvCodegenBackendInstallParams,
            SpirvCodegenBackendInstaller,
        },
        command::CommandExecError,
        toolchain::{
            HaltToolchainInstallation, InheritStderr, InheritStdout, NoopOnComponentsInstall,
            NoopOnToolchainInstall, StdioCfg,
        },
        user_output,
    },
};

#[cfg(feature = "watch")]
use crate::spirv_builder::SpirvWatcher;

/// Metadata specific to the build process.
#[derive(Clone, Debug, Default, serde::Deserialize, serde::Serialize)]
#[cfg_attr(feature = "clap", derive(clap::Parser))]
#[non_exhaustive]
pub struct CargoGpuBuildMetadata {
    /// The flattened [`SpirvBuilder`].
    #[cfg_attr(feature = "clap", clap(flatten))]
    #[serde(flatten)]
    pub spirv_builder: SpirvBuilder,
}

impl From<SpirvBuilder> for CargoGpuBuildMetadata {
    #[inline]
    fn from(spirv_builder: SpirvBuilder) -> Self {
        Self { spirv_builder }
    }
}

/// Metadata specific to the codegen backend installation process.
#[derive(Clone, Debug, Default, serde::Deserialize, serde::Serialize)]
#[cfg_attr(feature = "clap", derive(clap::Parser))]
#[non_exhaustive]
pub struct CargoGpuInstallMetadata {
    /// The flattened [`SpirvCodegenBackendInstaller`].
    #[cfg_attr(feature = "clap", clap(flatten))]
    #[serde(flatten)]
    pub spirv_installer: SpirvCodegenBackendInstaller,
    /// There is a tricky situation where a shader crate that depends on workspace config can have
    /// a different `Cargo.lock` lockfile version from the the workspace's `Cargo.lock`. This can
    /// prevent builds when an old Rust toolchain doesn't recognise the newer lockfile version.
    ///
    /// The ideal way to resolve this would be to match the shader crate's toolchain with the
    /// workspace's toolchain. However, that is not always possible. Another solution is to
    /// `exclude = [...]` the problematic shader crate from the workspace. This also may not be a
    /// suitable solution if there are a number of shader crates all sharing similar config and
    /// you don't want to have to copy/paste and maintain that config across all the shaders.
    ///
    /// So a somewhat hacky workaround is to overwrite lockfile versions. Enabling this flag
    /// will only come into effect if there are a mix of v3/v4 lockfiles. It will also
    /// only overwrite versions for the duration of a build. It will attempt to return the versions
    /// to their original values once the build is finished. However, of course, unexpected errors
    /// can occur and the overwritten values can remain. Hence why this behaviour is not enabled by
    /// default.
    ///
    /// This hack is possible because the change from v3 to v4 only involves a minor change to the
    /// way source URLs are encoded. See these PRs for more details:
    ///   * <https://github.com/rust-lang/cargo/pull/12280>
    ///   * <https://github.com/rust-lang/cargo/pull/14595>
    #[cfg_attr(feature = "clap", clap(long, action, verbatim_doc_comment))]
    pub force_overwrite_lockfiles_v4_to_v3: bool,
}

/// Metadata for both shader crate build and codegen backend installation.
#[derive(Clone, Debug, Default, serde::Deserialize, serde::Serialize)]
#[cfg_attr(feature = "clap", derive(clap::Parser))]
#[non_exhaustive]
pub struct CargoGpuMetadata {
    /// Parameters of the shader crate build.
    #[cfg_attr(feature = "clap", clap(flatten))]
    pub build: CargoGpuBuildMetadata,
    /// Parameters of the codegen backend installation for the shader crate.
    #[cfg_attr(feature = "clap", clap(flatten))]
    pub install: CargoGpuInstallMetadata,
}

impl RustGpuMetadata for CargoGpuMetadata {
    #[inline]
    fn patch<P>(&mut self, _shader_crate: P, _source: RustGpuMetadataSource<'_>)
    where
        P: AsRef<Path>,
    {
    }
}

/// Parameters for [`CargoGpuBuilder::new()`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct CargoGpuBuilderParams<W, T, C, O, E> {
    /// Parameters of the shader crate build & codegen backend installation.
    pub metadata: CargoGpuMetadata,
    /// Writer of user output.
    pub writer: W,
    /// Callbacks to halt toolchain installation.
    pub halt: HaltToolchainInstallation<T, C>,
    /// Configuration of [`Stdio`] for commands run during installation.
    pub stdio_cfg: StdioCfg<O, E>,
}

impl<W, T, C, O, E> CargoGpuBuilderParams<W, T, C, O, E> {
    /// Replaces of the shader crate build & codegen backend installation.
    #[inline]
    #[must_use]
    pub fn metadata(self, metadata: CargoGpuMetadata) -> Self {
        Self { metadata, ..self }
    }

    /// Replaces build parameters of the shader crate.
    #[inline]
    #[must_use]
    pub fn build(self, build: CargoGpuBuildMetadata) -> Self {
        let metadata = CargoGpuMetadata {
            build,
            ..self.metadata
        };
        Self { metadata, ..self }
    }

    /// Replaces codegen backend installation parameters of the shader crate.
    #[inline]
    #[must_use]
    pub fn install(self, install: CargoGpuInstallMetadata) -> Self {
        let metadata = CargoGpuMetadata {
            install,
            ..self.metadata
        };
        Self { metadata, ..self }
    }

    /// Replaces the writer of user output.
    #[inline]
    #[must_use]
    pub fn writer<NW>(self, writer: NW) -> CargoGpuBuilderParams<NW, T, C, O, E> {
        CargoGpuBuilderParams {
            metadata: self.metadata,
            writer,
            halt: self.halt,
            stdio_cfg: self.stdio_cfg,
        }
    }

    /// Replaces the callbacks to halt toolchain installation.
    #[inline]
    #[must_use]
    pub fn halt<NT, NC>(
        self,
        halt: HaltToolchainInstallation<NT, NC>,
    ) -> CargoGpuBuilderParams<W, NT, NC, O, E> {
        CargoGpuBuilderParams {
            metadata: self.metadata,
            writer: self.writer,
            halt,
            stdio_cfg: self.stdio_cfg,
        }
    }

    /// Replaces the [`Stdio`] configuration for commands run during installation.
    #[inline]
    #[must_use]
    pub fn stdio_cfg<NO, NE>(
        self,
        stdio_cfg: StdioCfg<NO, NE>,
    ) -> CargoGpuBuilderParams<W, T, C, NO, NE> {
        CargoGpuBuilderParams {
            metadata: self.metadata,
            writer: self.writer,
            halt: self.halt,
            stdio_cfg,
        }
    }
}

/// [`Default`] parameters for [`CargoGpuBuilder::new()`].
pub type DefaultCargoGpuBuilderParams = CargoGpuBuilderParams<
    io::Stdout,
    NoopOnToolchainInstall,
    NoopOnComponentsInstall,
    InheritStdout,
    InheritStderr,
>;

impl<T> From<T> for DefaultCargoGpuBuilderParams
where
    T: Into<CargoGpuBuildMetadata>,
{
    #[inline]
    fn from(value: T) -> Self {
        let metadata = CargoGpuMetadata {
            build: value.into(),
            install: CargoGpuInstallMetadata::default(),
        };
        Self {
            metadata,
            ..Self::default()
        }
    }
}

impl Default for DefaultCargoGpuBuilderParams {
    #[inline]
    fn default() -> Self {
        Self {
            metadata: CargoGpuMetadata::default(),
            writer: io::stdout(),
            halt: HaltToolchainInstallation::noop(),
            stdio_cfg: StdioCfg::inherit(),
        }
    }
}

/// A builder for compiling a `rust-gpu` shader crate.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct CargoGpuBuilder<W = io::Stdout> {
    /// The underlying builder for compiling the shader crate.
    pub builder: SpirvBuilder,
    /// The underlying codegen backend installer for the shader crate.
    pub installer: SpirvCodegenBackendInstaller,
    /// The installed codegen backend.
    pub codegen_backend: SpirvCodegenBackend,
    /// The lockfile mismatch handler.
    pub lockfile_mismatch_handler: LockfileMismatchHandler,
    /// Writer of user output.
    pub writer: W,
}

impl<W> CargoGpuBuilder<W>
where
    W: io::Write,
{
    /// Creates shader crate builder, allowing to modify install and build parameters separately.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    ///
    /// * the shader crate path / target was not set,
    /// * the shader crate path is not valid,
    /// * the backend installation fails,
    /// * there is a lockfile version mismatch that cannot be resolved automatically.
    #[inline]
    pub fn new<I, R, T, C, O, E>(params: I) -> Result<Self, NewCargoGpuBuilderError<R>>
    where
        I: Into<CargoGpuBuilderParams<W, T, C, O, E>>,
        R: From<CommandExecError>,
        T: FnOnce(&str) -> Result<(), R>,
        C: FnOnce(&str) -> Result<(), R>,
        O: FnMut() -> Stdio,
        E: FnMut() -> Stdio,
    {
        let CargoGpuBuilderParams {
            metadata,
            mut writer,
            halt,
            mut stdio_cfg,
        } = params.into();
        let CargoGpuMetadata { build, install } = metadata;
        let CargoGpuBuildMetadata {
            spirv_builder: mut builder,
        } = build;
        let CargoGpuInstallMetadata {
            spirv_installer: installer,
            force_overwrite_lockfiles_v4_to_v3,
        } = install;

        if builder.target.is_none() {
            return Err(NewCargoGpuBuilderError::MissingTarget);
        }
        let path_to_crate = builder
            .path_to_crate
            .as_ref()
            .ok_or(NewCargoGpuBuilderError::MissingCratePath)?;
        let shader_crate = dunce::canonicalize(path_to_crate)?;
        builder.path_to_crate = Some(shader_crate.clone());

        let backend_install_params = SpirvCodegenBackendInstallParams::from(&shader_crate)
            .writer(&mut writer)
            .halt(HaltToolchainInstallation {
                on_toolchain_install: |channel: &str| (halt.on_toolchain_install)(channel),
                on_components_install: |channel: &str| (halt.on_components_install)(channel),
            })
            .stdio_cfg(StdioCfg {
                stdout: || (stdio_cfg.stdout)(),
                stderr: || (stdio_cfg.stderr)(),
            });
        let codegen_backend = installer.install(backend_install_params)?;

        let lockfile_mismatch_handler = LockfileMismatchHandler::new(
            &shader_crate,
            &codegen_backend.toolchain_channel,
            force_overwrite_lockfiles_v4_to_v3,
        )?;

        #[expect(clippy::unreachable, reason = "target was set")]
        codegen_backend
            .configure_spirv_builder(&mut builder)
            .unwrap_or_else(|_| unreachable!("target was set before calling this function"));

        Ok(Self {
            builder,
            installer,
            codegen_backend,
            lockfile_mismatch_handler,
            writer,
        })
    }

    /// Builds the shader crate using the configured [`SpirvBuilder`].
    ///
    /// # Errors
    ///
    /// Returns an error if building the shader crate failed.
    #[inline]
    pub fn build(&mut self) -> Result<CompileResult, CargoGpuBuildError> {
        let shader_crate = self
            .builder
            .path_to_crate
            .as_ref()
            .ok_or(SpirvBuilderError::MissingCratePath)?
            .display();
        user_output!(&mut self.writer, "Compiling shaders at {shader_crate}...\n")?;

        let result = self.builder.build()?;
        Ok(result)
    }

    /// Watches the shader crate for changes using the configured [`SpirvBuilder`].
    ///
    /// # Errors
    ///
    /// Returns an error if watching shader crate for changes failed.
    #[cfg(feature = "watch")]
    #[inline]
    pub fn watch(&mut self) -> Result<SpirvWatcher, CargoGpuBuildError> {
        let shader_crate = self
            .builder
            .path_to_crate
            .as_ref()
            .ok_or(SpirvBuilderError::MissingCratePath)?
            .display();
        user_output!(
            &mut self.writer,
            "Watching shaders for changes at {shader_crate}...\n"
        )?;

        let watcher = self.builder.clone().watch()?;
        Ok(watcher)
    }
}

/// An error indicating what went wrong when creating a [`CargoGpuBuilder`].
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum NewCargoGpuBuilderError<E = CommandExecError> {
    /// Shader crate target is missing from parameters of the build.
    #[error("shader crate target must be set, for example `spirv-unknown-vulkan1.2`")]
    MissingTarget,
    /// Shader path is missing from parameters of the build.
    #[error("path to shader crate must be set")]
    MissingCratePath,
    /// The given shader crate path is not valid.
    #[error("shader crate path is not valid: {0}")]
    InvalidCratePath(#[from] io::Error),
    /// The backend installation failed.
    #[error("could not install backend: {0}")]
    Install(#[from] SpirvCodegenBackendInstallError<E>),
    /// There is a lockfile version mismatch that cannot be resolved automatically.
    #[error(transparent)]
    LockfileMismatch(#[from] LockfileMismatchError),
}

/// An error indicating what went wrong when building the shader crate.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CargoGpuBuildError {
    /// Failed to write user output.
    #[error("failed to write user output: {0}")]
    IoWrite(#[from] io::Error),
    /// Failed to build shader crate.
    #[error("failed to build shader crate: {0}")]
    Build(#[from] SpirvBuilderError),
}
