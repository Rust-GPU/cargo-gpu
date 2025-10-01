//! This module provides a `rust-gpu` shader crate builder
//! usable inside of build scripts or as a part of CLI.

use std::{io, process::Stdio};

use crate::{
    lockfile::{LockfileMismatchError, LockfileMismatchHandler},
    spirv_builder::{CompileResult, SpirvBuilder, SpirvBuilderError},
    spirv_cache::{
        backend::{Install, InstallError, InstallParams, InstallRunParams, InstalledBackend},
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

/// Parameters for [`ShaderCrateBuilder::new()`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ShaderCrateBuilderParams<W, T, C, O, E> {
    /// Parameters of the shader crate build.
    pub build: SpirvBuilder,
    /// Parameters of the codegen backend installation for the shader crate.
    pub install: InstallParams,
    /// Writer of user output.
    pub writer: W,
    /// Callbacks to halt toolchain installation.
    pub halt: HaltToolchainInstallation<T, C>,
    /// Configuration of [`Stdio`] for commands run during installation.
    pub stdio_cfg: StdioCfg<O, E>,
}

impl<W, T, C, O, E> ShaderCrateBuilderParams<W, T, C, O, E> {
    /// Replaces build parameters of the shader crate.
    #[inline]
    #[must_use]
    pub fn build(self, build: SpirvBuilder) -> Self {
        Self { build, ..self }
    }

    /// Replaces codegen backend installation parameters of the shader crate.
    #[inline]
    #[must_use]
    pub fn install(self, install: InstallParams) -> Self {
        Self { install, ..self }
    }

    /// Replaces the writer of user output.
    #[inline]
    #[must_use]
    pub fn writer<NW>(self, writer: NW) -> ShaderCrateBuilderParams<NW, T, C, O, E> {
        ShaderCrateBuilderParams {
            build: self.build,
            install: self.install,
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
    ) -> ShaderCrateBuilderParams<W, NT, NC, O, E> {
        ShaderCrateBuilderParams {
            build: self.build,
            install: self.install,
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
    ) -> ShaderCrateBuilderParams<W, T, C, NO, NE> {
        ShaderCrateBuilderParams {
            build: self.build,
            install: self.install,
            writer: self.writer,
            halt: self.halt,
            stdio_cfg,
        }
    }
}

/// [`Default`] parameters for [`ShaderCrateBuilder::new()`].
pub type DefaultShaderCrateBuilderParams = ShaderCrateBuilderParams<
    io::Stdout,
    NoopOnToolchainInstall,
    NoopOnComponentsInstall,
    InheritStdout,
    InheritStderr,
>;

impl From<SpirvBuilder> for DefaultShaderCrateBuilderParams {
    #[inline]
    fn from(build: SpirvBuilder) -> Self {
        Self {
            build,
            ..Self::default()
        }
    }
}

impl Default for DefaultShaderCrateBuilderParams {
    #[inline]
    fn default() -> Self {
        Self {
            build: SpirvBuilder::default(),
            install: InstallParams::default(),
            writer: io::stdout(),
            halt: HaltToolchainInstallation::noop(),
            stdio_cfg: StdioCfg::inherit(),
        }
    }
}

/// A builder for compiling a `rust-gpu` shader crate.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ShaderCrateBuilder<W = io::Stdout> {
    /// The underlying builder for compiling the shader crate.
    pub builder: SpirvBuilder,
    /// The arguments used to install the backend.
    pub installed_backend_args: Install,
    /// The installed backend.
    pub installed_backend: InstalledBackend,
    /// The lockfile mismatch handler.
    pub lockfile_mismatch_handler: LockfileMismatchHandler,
    /// Writer of user output.
    pub writer: W,
}

impl<W> ShaderCrateBuilder<W>
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
    pub fn new<I, R, T, C, O, E>(params: I) -> Result<Self, NewShaderCrateBuilderError<R>>
    where
        I: Into<ShaderCrateBuilderParams<W, T, C, O, E>>,
        R: From<CommandExecError>,
        T: FnOnce(&str) -> Result<(), R>,
        C: FnOnce(&str) -> Result<(), R>,
        O: FnMut() -> Stdio,
        E: FnMut() -> Stdio,
    {
        let ShaderCrateBuilderParams {
            mut build,
            install,
            mut writer,
            halt,
            mut stdio_cfg,
        } = params.into();

        if build.target.is_none() {
            return Err(NewShaderCrateBuilderError::MissingTarget);
        }
        let path_to_crate = build
            .path_to_crate
            .as_ref()
            .ok_or(NewShaderCrateBuilderError::MissingCratePath)?;
        let shader_crate = dunce::canonicalize(path_to_crate)?;

        let backend_to_install = Install::new(shader_crate, install);
        let backend_install_params = InstallRunParams::default()
            .writer(&mut writer)
            .halt(HaltToolchainInstallation {
                on_toolchain_install: |channel: &str| (halt.on_toolchain_install)(channel),
                on_components_install: |channel: &str| (halt.on_components_install)(channel),
            })
            .stdio_cfg(StdioCfg {
                stdout: || (stdio_cfg.stdout)(),
                stderr: || (stdio_cfg.stderr)(),
            });
        let backend = backend_to_install.run(backend_install_params)?;

        let lockfile_mismatch_handler = LockfileMismatchHandler::new(
            &backend_to_install.shader_crate,
            &backend.toolchain_channel,
            backend_to_install.params.force_overwrite_lockfiles_v4_to_v3,
        )?;

        #[expect(clippy::unreachable, reason = "target was already set")]
        backend
            .configure_spirv_builder(&mut build)
            .unwrap_or_else(|_| unreachable!("target was checked before calling this function"));

        Ok(Self {
            builder: build,
            installed_backend_args: backend_to_install,
            installed_backend: backend,
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
    pub fn build(&mut self) -> Result<CompileResult, ShaderCrateBuildError> {
        let shader_crate = self.installed_backend_args.shader_crate.display();
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
    pub fn watch(&mut self) -> Result<SpirvWatcher, ShaderCrateBuildError> {
        let shader_crate = self.installed_backend_args.shader_crate.display();
        user_output!(
            &mut self.writer,
            "Watching shaders for changes at {shader_crate}...\n"
        )?;

        let watcher = self.builder.clone().watch()?;
        Ok(watcher)
    }
}

/// An error indicating what went wrong when creating a [`ShaderCrateBuilder`].
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum NewShaderCrateBuilderError<E = CommandExecError> {
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
    Install(#[from] InstallError<E>),
    /// There is a lockfile version mismatch that cannot be resolved automatically.
    #[error(transparent)]
    LockfileMismatch(#[from] LockfileMismatchError),
}

/// An error indicating what went wrong when building the shader crate.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ShaderCrateBuildError {
    /// Failed to write user output.
    #[error("failed to write user output: {0}")]
    IoWrite(#[from] io::Error),
    /// Failed to build shader crate.
    #[error("failed to build shader crate: {0}")]
    Build(#[from] SpirvBuilderError),
}
