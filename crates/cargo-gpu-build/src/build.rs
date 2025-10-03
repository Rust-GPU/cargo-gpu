//! This module provides a `rust-gpu` shader crate builder
//! usable inside of build scripts or as a part of CLI.

use std::{io, process::Stdio};

use crate::{
    lockfile::{LockfileMismatchError, LockfileMismatchHandler},
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

/// Parameters for [`CargoGpuBuilder::new()`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct CargoGpuBuilderParams<W, T, C, O, E> {
    /// Parameters of the shader crate build.
    pub build: SpirvBuilder,
    /// Parameters of the codegen backend installation for the shader crate.
    pub install: SpirvCodegenBackendInstaller,
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
    pub force_overwrite_lockfiles_v4_to_v3: bool,
    /// Writer of user output.
    pub writer: W,
    /// Callbacks to halt toolchain installation.
    pub halt: HaltToolchainInstallation<T, C>,
    /// Configuration of [`Stdio`] for commands run during installation.
    pub stdio_cfg: StdioCfg<O, E>,
}

impl<W, T, C, O, E> CargoGpuBuilderParams<W, T, C, O, E> {
    /// Replaces build parameters of the shader crate.
    #[inline]
    #[must_use]
    pub fn build(self, build: SpirvBuilder) -> Self {
        Self { build, ..self }
    }

    /// Replaces codegen backend installation parameters of the shader crate.
    #[inline]
    #[must_use]
    pub fn install(self, install: SpirvCodegenBackendInstaller) -> Self {
        Self { install, ..self }
    }

    /// Sets whether to force overwriting lockfiles from v4 to v3.
    #[inline]
    #[must_use]
    pub fn force_overwrite_lockfiles_v4_to_v3(
        self,
        force_overwrite_lockfiles_v4_to_v3: bool,
    ) -> Self {
        Self {
            force_overwrite_lockfiles_v4_to_v3,
            ..self
        }
    }

    /// Replaces the writer of user output.
    #[inline]
    #[must_use]
    pub fn writer<NW>(self, writer: NW) -> CargoGpuBuilderParams<NW, T, C, O, E> {
        CargoGpuBuilderParams {
            build: self.build,
            install: self.install,
            force_overwrite_lockfiles_v4_to_v3: self.force_overwrite_lockfiles_v4_to_v3,
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
            build: self.build,
            install: self.install,
            force_overwrite_lockfiles_v4_to_v3: self.force_overwrite_lockfiles_v4_to_v3,
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
            build: self.build,
            install: self.install,
            force_overwrite_lockfiles_v4_to_v3: self.force_overwrite_lockfiles_v4_to_v3,
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

impl From<SpirvBuilder> for DefaultCargoGpuBuilderParams {
    #[inline]
    fn from(build: SpirvBuilder) -> Self {
        Self {
            build,
            ..Self::default()
        }
    }
}

impl Default for DefaultCargoGpuBuilderParams {
    #[inline]
    fn default() -> Self {
        Self {
            build: SpirvBuilder::default(),
            install: SpirvCodegenBackendInstaller::default(),
            force_overwrite_lockfiles_v4_to_v3: false,
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
            mut build,
            install,
            force_overwrite_lockfiles_v4_to_v3,
            mut writer,
            halt,
            mut stdio_cfg,
        } = params.into();

        if build.target.is_none() {
            return Err(NewCargoGpuBuilderError::MissingTarget);
        }
        let path_to_crate = build
            .path_to_crate
            .as_ref()
            .ok_or(NewCargoGpuBuilderError::MissingCratePath)?;
        let shader_crate = dunce::canonicalize(path_to_crate)?;
        build.path_to_crate = Some(shader_crate.clone());

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
        let codegen_backend = install.install(backend_install_params)?;

        let lockfile_mismatch_handler = LockfileMismatchHandler::new(
            &shader_crate,
            &codegen_backend.toolchain_channel,
            force_overwrite_lockfiles_v4_to_v3,
        )?;

        #[expect(clippy::unreachable, reason = "target was set")]
        codegen_backend
            .configure_spirv_builder(&mut build)
            .unwrap_or_else(|_| unreachable!("target was set before calling this function"));

        Ok(Self {
            builder: build,
            installer: install,
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
