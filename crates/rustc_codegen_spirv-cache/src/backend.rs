//! This module deals with an installation a dedicated per-shader crate
//! that has the `rust-gpu` codegen backend in it.
//!
//! This process could be described as follows:
//! * first retrieve the version of rust-gpu you want to use based on the version of the
//!   `spirv-std` dependency in your shader crate,
//! * then create a dummy project at `<cache_dir>/codegen/<version>/`
//!   that depends on `rustc_codegen_spirv`,
//! * use `cargo metadata` to `cargo update` the dummy project, which downloads the
//!   `rustc_codegen_spirv` crate into cargo's cache, and retrieve the path to the
//!   download location,
//! * search for the required toolchain in `build.rs` of `rustc_codegen_spirv`,
//! * build it with the required toolchain version,
//! * copy out the resulting dylib and clean the target directory.

use std::{
    fs, io,
    path::{Path, PathBuf},
    process::Stdio,
};

use crate::{
    cache::{cache_dir, CacheDirError},
    command::{execute_command, CommandExecError},
    metadata::{query_metadata, MetadataExt as _, MissingPackageError, QueryMetadataError},
    spirv_builder::{cargo_cmd::CargoCmd, SpirvBuilder, SpirvBuilderError},
    spirv_source::{
        rust_gpu_toolchain_channel, RustGpuToolchainChannelError, SpirvSource, SpirvSourceError,
    },
    target_specs::{update_target_specs_files, UpdateTargetSpecsFilesError},
    toolchain::{
        ensure_toolchain_installation, HaltToolchainInstallation, NoopOnComponentsInstall,
        NoopOnToolchainInstall, NullStderr, NullStdout, StdioCfg,
    },
    user_output,
};

/// Represents a functional backend installation, whether it was cached or just installed.
#[derive(Debug, Default, Clone)]
#[non_exhaustive]
#[expect(clippy::module_name_repetitions, reason = "this is intended")]
pub struct SpirvCodegenBackend {
    /// Path to the `rustc_codegen_spirv` dylib.
    pub rustc_codegen_spirv_location: PathBuf,
    /// Toolchain channel name.
    pub toolchain_channel: String,
    /// Directory with target specs json files.
    pub target_spec_dir: PathBuf,
}

impl SpirvCodegenBackend {
    /// Creates a new [`SpirvBuilder`] configured to use this installed backend.
    #[expect(
        clippy::unreachable,
        reason = "it's unreachable, no need to return a Result"
    )]
    #[expect(clippy::impl_trait_in_params, reason = "forwarding spirv-builder API")]
    #[inline]
    pub fn to_spirv_builder(
        &self,
        path_to_crate: impl AsRef<Path>,
        target: impl Into<String>,
    ) -> SpirvBuilder {
        let mut builder = SpirvBuilder::new(path_to_crate, target);
        self.configure_spirv_builder(&mut builder)
            .unwrap_or_else(|_| unreachable!("we set target before calling this function"));
        builder
    }

    /// Configures the supplied [`SpirvBuilder`].
    /// [`SpirvBuilder::target`] must be set and must not change after calling this function.
    ///
    /// # Errors
    ///
    /// Returns an error if [`SpirvBuilder::target`] is not set.
    #[inline]
    pub fn configure_spirv_builder(
        &self,
        builder: &mut SpirvBuilder,
    ) -> Result<(), SpirvBuilderError> {
        builder.rustc_codegen_spirv_location = Some(self.rustc_codegen_spirv_location.clone());
        builder.toolchain_overwrite = Some(self.toolchain_channel.clone());

        let target = builder
            .target
            .as_deref()
            .ok_or(SpirvBuilderError::MissingTarget)?;
        builder.path_to_target_spec = Some(self.target_spec_dir.join(format!("{target}.json")));
        Ok(())
    }
}

/// Settings for an installation of the codegen backend.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[cfg_attr(feature = "clap", derive(clap::Parser))]
#[non_exhaustive]
pub struct SpirvCodegenBackendInstaller {
    #[expect(
        rustdoc::bare_urls,
        clippy::doc_markdown,
        reason = "The URL should appear literally like this. But Clippy & rustdoc want a markdown clickable link"
    )]
    /// Source of [`spirv-builder`](spirv_builder) dependency.
    ///
    /// E.g. "https://github.com/Rust-GPU/rust-gpu".
    #[cfg_attr(feature = "clap", clap(long))]
    pub spirv_builder_source: Option<String>,

    /// Version of [`spirv-builder`](spirv_builder) dependency.
    ///
    /// * If `--spirv-builder-source` is not set, then this is assumed to be a crates.io semantic
    ///   version such as "0.9.0".
    /// * If `--spirv-builder-source` is set, then this is assumed to be a Git "commitsh", such
    ///   as a Git commit hash or a Git tag, therefore anything that `git checkout` can resolve.
    #[cfg_attr(feature = "clap", clap(long, verbatim_doc_comment))]
    pub spirv_builder_version: Option<String>,

    /// Force `rustc_codegen_spirv` to be rebuilt.
    #[cfg_attr(feature = "clap", clap(long))]
    pub rebuild_codegen: bool,

    /// Clear target dir of `rustc_codegen_spirv` build after a successful build,
    /// saves about 200MiB of disk space.
    #[cfg_attr(feature = "clap", clap(long = "no-clear-target", default_value = "true", action = clap::ArgAction::SetFalse))]
    pub clear_target: bool,
}

impl Default for SpirvCodegenBackendInstaller {
    #[inline]
    fn default() -> Self {
        Self {
            spirv_builder_source: None,
            spirv_builder_version: None,
            rebuild_codegen: false,
            clear_target: true,
        }
    }
}

impl SpirvCodegenBackendInstaller {
    /// Sets the source of [`spirv-builder`](spirv_builder) dependency.
    #[inline]
    #[must_use]
    pub fn spirv_builder_source<I>(self, spirv_builder_source: I) -> Self
    where
        I: Into<Option<String>>,
    {
        Self {
            spirv_builder_source: spirv_builder_source.into(),
            ..self
        }
    }

    /// Sets the version of [`spirv-builder`](spirv_builder) dependency.
    #[inline]
    #[must_use]
    pub fn spirv_builder_version<I>(self, spirv_builder_version: I) -> Self
    where
        I: Into<Option<String>>,
    {
        Self {
            spirv_builder_version: spirv_builder_version.into(),
            ..self
        }
    }

    /// Sets whether to force `rustc_codegen_spirv` to be rebuilt.
    #[inline]
    #[must_use]
    pub fn rebuild_codegen(self, rebuild_codegen: bool) -> Self {
        Self {
            rebuild_codegen,
            ..self
        }
    }

    /// Sets whether to clear target dir of `rustc_codegen_spirv` build after a successful build.
    #[inline]
    #[must_use]
    pub fn clear_target(self, clear_target: bool) -> Self {
        Self {
            clear_target,
            ..self
        }
    }

    /// Create the `rustc_codegen_spirv_dummy` crate that depends on `rustc_codegen_spirv`
    fn write_source_files<E>(
        source: &SpirvSource,
        checkout: &Path,
    ) -> Result<(), SpirvCodegenBackendInstallError<E>> {
        // skip writing a dummy project if we use a local rust-gpu checkout
        if source.is_path() {
            return Ok(());
        }

        log::debug!(
            "writing `rustc_codegen_spirv_dummy` source files into {}",
            checkout.display()
        );

        log::trace!("writing dummy lib.rs");
        let src = checkout.join("src");
        fs::create_dir_all(&src).map_err(SpirvCodegenBackendInstallError::CreateDummySrcDir)?;
        fs::File::create(src.join("lib.rs"))
            .map_err(SpirvCodegenBackendInstallError::CreateDummyLibRs)?;

        log::trace!("writing dummy Cargo.toml");
        fs::write(checkout.join("Cargo.toml"), dummy_cargo_toml(source))
            .map_err(SpirvCodegenBackendInstallError::WriteDummyCargoToml)?;

        Ok(())
    }

    /// Installs the `rust-gpu` [codegen backend](SpirvCodegenBackend) for the shader crate,
    /// from which you can create [`SpirvBuilder`] instances.
    ///
    /// # Errors
    ///
    /// Returns an error if the installation somehow fails.
    /// See [`SpirvCodegenBackendInstallError`] for further details.
    #[inline]
    pub fn install<I, R, W, T, C, O, E>(
        &self,
        params: I,
    ) -> Result<SpirvCodegenBackend, SpirvCodegenBackendInstallError<R>>
    where
        I: Into<SpirvCodegenBackendInstallParams<W, T, C, O, E>>,
        W: io::Write,
        R: From<CommandExecError>,
        T: FnOnce(&str) -> Result<(), R>,
        C: FnOnce(&str) -> Result<(), R>,
        O: FnMut() -> Stdio,
        E: FnMut() -> Stdio,
    {
        // Ensure the cache dir exists
        let cache_dir = cache_dir()?;
        log::info!("cache directory is '{}'", cache_dir.display());
        if let Err(source) = fs::create_dir_all(&cache_dir) {
            return Err(SpirvCodegenBackendInstallError::CreateCacheDir { cache_dir, source });
        }

        let SpirvCodegenBackendInstallParams {
            shader_crate,
            writer,
            halt,
            stdio_cfg,
        } = params.into();

        let source = SpirvSource::new(
            &shader_crate,
            self.spirv_builder_source.as_deref(),
            self.spirv_builder_version.as_deref(),
        )?;
        let install_dir = source.install_dir()?;

        let dylib_filename = dylib_filename("rustc_codegen_spirv");
        let (dest_dylib_path, skip_rebuild) = if source.is_path() {
            (
                install_dir
                    .join("target")
                    .join("release")
                    .join(&dylib_filename),
                // if `source` is a path, always rebuild
                false,
            )
        } else {
            let dest_dylib_path = install_dir.join(&dylib_filename);
            let artifacts_found = dest_dylib_path.is_file()
                && install_dir.join("Cargo.toml").is_file()
                && install_dir.join("src").join("lib.rs").is_file();
            if artifacts_found {
                log::info!("cargo-gpu artifacts found in '{}'", install_dir.display());
            }
            (dest_dylib_path, artifacts_found && !self.rebuild_codegen)
        };

        if skip_rebuild {
            log::info!("...and so we are aborting the install step.");
        } else {
            Self::write_source_files(&source, &install_dir)?;
        }

        // TODO cache toolchain channel in a file?
        log::debug!("resolving toolchain version to use");
        let dummy_metadata = query_metadata(&install_dir)?;
        let rustc_codegen_spirv = dummy_metadata.find_package("rustc_codegen_spirv")?;
        let toolchain_channel = rust_gpu_toolchain_channel(rustc_codegen_spirv)?;
        log::info!("selected toolchain channel `{toolchain_channel:?}`");

        log::debug!("Update target specs files");
        let target_spec_dir = update_target_specs_files(&source, &dummy_metadata, !skip_rebuild)?;

        log::debug!("ensure_toolchain_and_components_exist");
        ensure_toolchain_installation(&toolchain_channel, halt, stdio_cfg)
            .map_err(SpirvCodegenBackendInstallError::EnsureToolchainInstallation)?;

        if !skip_rebuild {
            // to prevent unsupported version errors when using older toolchains
            if !source.is_path() {
                log::debug!("remove Cargo.lock");
                fs::remove_file(install_dir.join("Cargo.lock"))
                    .map_err(SpirvCodegenBackendInstallError::RemoveDummyCargoLock)?;
            }

            user_output!(writer, "Compiling `rustc_codegen_spirv` from {source}\n")
                .map_err(SpirvCodegenBackendInstallError::IoWrite)?;

            let mut cargo = CargoCmd::new();
            cargo
                .current_dir(&install_dir)
                .arg(format!("+{toolchain_channel}"))
                .args(["build", "--release"]);
            if source.is_path() {
                cargo.args(["-p", "rustc_codegen_spirv", "--lib"]);
            }
            cargo.stdout(Stdio::inherit()).stderr(Stdio::inherit());

            log::debug!("building artifacts with `{cargo:?}`");
            execute_command(cargo)?;

            let target = install_dir.join("target");
            let dylib_path = target.join("release").join(&dylib_filename);
            if dylib_path.is_file() {
                log::info!("successfully built {}", dylib_path.display());
                if !source.is_path() {
                    fs::rename(&dylib_path, &dest_dylib_path)
                        .map_err(SpirvCodegenBackendInstallError::MoveRustcCodegenSpirvDylib)?;

                    if self.clear_target {
                        log::warn!("clearing target dir {}", target.display());
                        fs::remove_dir_all(&target).map_err(
                            SpirvCodegenBackendInstallError::RemoveRustcCodegenSpirvTargetDir,
                        )?;
                    }
                }
            } else {
                log::error!("could not find {}", dylib_path.display());
                return Err(SpirvCodegenBackendInstallError::RustcCodegenSpirvDylibNotFound);
            }
        }

        Ok(SpirvCodegenBackend {
            rustc_codegen_spirv_location: dest_dylib_path,
            toolchain_channel,
            target_spec_dir,
        })
    }
}

/// Parameters for [`SpirvCodegenBackendInstaller::install()`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct SpirvCodegenBackendInstallParams<W, T, C, O, E> {
    /// Path to the shader crate to install the codegen backend for.
    pub shader_crate: PathBuf,
    /// Writer of user output.
    pub writer: W,
    /// Callbacks to halt toolchain installation.
    pub halt: HaltToolchainInstallation<T, C>,
    /// Configuration of [`Stdio`] for commands run during installation.
    pub stdio_cfg: StdioCfg<O, E>,
}

impl<W, T, C, O, E> SpirvCodegenBackendInstallParams<W, T, C, O, E> {
    /// Replaces path to the shader crate to install the codegen backend for.
    #[inline]
    #[must_use]
    pub fn shader_crate<P>(self, shader_crate: P) -> Self
    where
        P: Into<PathBuf>,
    {
        Self {
            shader_crate: shader_crate.into(),
            ..self
        }
    }

    /// Replaces the writer of user output.
    #[inline]
    #[must_use]
    pub fn writer<NW>(self, writer: NW) -> SpirvCodegenBackendInstallParams<NW, T, C, O, E> {
        SpirvCodegenBackendInstallParams {
            shader_crate: self.shader_crate,
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
    ) -> SpirvCodegenBackendInstallParams<W, NT, NC, O, E> {
        SpirvCodegenBackendInstallParams {
            shader_crate: self.shader_crate,
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
    ) -> SpirvCodegenBackendInstallParams<W, T, C, NO, NE> {
        SpirvCodegenBackendInstallParams {
            shader_crate: self.shader_crate,
            writer: self.writer,
            halt: self.halt,
            stdio_cfg,
        }
    }
}

/// [`Default`] parameters for [`SpirvCodegenBackendInstaller::install()`].
pub type DefaultSpirvCodegenBackendInstallParams = SpirvCodegenBackendInstallParams<
    io::Empty,
    NoopOnToolchainInstall,
    NoopOnComponentsInstall,
    NullStdout,
    NullStderr,
>;

impl<P> From<P> for DefaultSpirvCodegenBackendInstallParams
where
    P: Into<PathBuf>,
{
    #[inline]
    fn from(path_to_crate: P) -> Self {
        Self {
            shader_crate: path_to_crate.into(),
            writer: io::empty(),
            halt: HaltToolchainInstallation::noop(),
            stdio_cfg: StdioCfg::null(),
        }
    }
}

/// Returns the platform-specific filename of the dylib with the given name.
#[inline]
fn dylib_filename(name: impl AsRef<str>) -> String {
    use std::env::consts::{DLL_PREFIX, DLL_SUFFIX};

    let str_name = name.as_ref();
    format!("{DLL_PREFIX}{str_name}{DLL_SUFFIX}")
}

/// Contents of the `Cargo.toml` file for the local `rustc_codegen_spirv_dummy` crate
/// without the version specification of the `rustc_codegen_spirv` dependency.
const DUMMY_CARGO_TOML_NO_VERSION_SPEC: &str = include_str!("dummy/Cargo.toml");

/// Returns the contents of the `Cargo.toml` file for the local `rustc_codegen_spirv_dummy` crate.
fn dummy_cargo_toml(source: &SpirvSource) -> String {
    let version_spec = match source {
        SpirvSource::CratesIO(version) => format!("version = \"{version}\""),
        SpirvSource::Git { url, rev } => format!("git = \"{url}\"\nrev = \"{rev}\""),
        SpirvSource::Path {
            rust_gpu_repo_root,
            version,
        } => {
            // this branch is currently unreachable, as we just build `rustc_codegen_spirv` directly,
            // since we don't need the `dummy` crate to make cargo download it for us
            let new_path = rust_gpu_repo_root.join("crates").join("spirv-builder");
            format!("path = \"{new_path}\"\nversion = \"{version}\"")
        }
    };
    format!("{DUMMY_CARGO_TOML_NO_VERSION_SPEC}{version_spec}\n")
}

/// An error indicating codegen `rustc_codegen_spirv` installation failure.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SpirvCodegenBackendInstallError<E = CommandExecError> {
    /// Failed to write user output.
    #[error("failed to write user output: {0}")]
    IoWrite(#[source] io::Error),
    /// There is no cache directory available.
    #[error(transparent)]
    NoCacheDir(#[from] CacheDirError),
    /// Failed to create the cache directory.
    #[error("failed to create cache directory {cache_dir}: {source}")]
    CreateCacheDir {
        /// Path to the cache directory we tried to create.
        cache_dir: PathBuf,
        /// The source of the error.
        source: io::Error,
    },
    /// Failed to determine the source of `rust-gpu`.
    #[error(transparent)]
    SpirvSource(#[from] SpirvSourceError),
    /// Failed to create `src` directory for local `rustc_codegen_spirv_dummy` crate.
    #[error("failed to create `src` directory for `rustc_codegen_spirv_dummy`: {0}")]
    CreateDummySrcDir(#[source] io::Error),
    /// Failed to create `src/lib.rs` file for local `rustc_codegen_spirv_dummy` crate.
    #[error("failed to create `src/lib.rs` file for `rustc_codegen_spirv_dummy`: {0}")]
    CreateDummyLibRs(#[source] io::Error),
    /// Failed to write `Cargo.toml` file for local `rustc_codegen_spirv_dummy` crate.
    #[error("failed to write `Cargo.toml` file for `rustc_codegen_spirv_dummy`: {0}")]
    WriteDummyCargoToml(#[source] io::Error),
    /// Failed to query cargo metadata of the local `rustc_codegen_spirv_dummy` crate.
    #[error(transparent)]
    QueryDummyMetadata(#[from] QueryMetadataError),
    /// Could not find `rustc_codegen_spirv` dependency in the local `rustc_codegen_spirv_dummy` crate.
    #[error(transparent)]
    NoRustcCodegenSpirv(#[from] MissingPackageError),
    /// Failed to determine the toolchain channel of `rustc_codegen_spirv`.
    #[error("could not get toolchain channel of `rustc_codegen_spirv`: {0}")]
    RustGpuToolchainChannel(#[from] RustGpuToolchainChannelError),
    /// Failed to update target specs files.
    #[error(transparent)]
    UpdateTargetSpecsFiles(#[from] UpdateTargetSpecsFilesError),
    /// Failed to ensure installation of a toolchain and its required components.
    #[error("failed to ensure toolchain and components exist: {0}")]
    EnsureToolchainInstallation(#[source] E),
    /// Failed to remove `Cargo.lock` file for local `rustc_codegen_spirv_dummy` crate.
    #[error("failed to remove `Cargo.lock` file for `rustc_codegen_spirv_dummy`: {0}")]
    RemoveDummyCargoLock(#[source] io::Error),
    /// Failed to move `rustc_codegen_spirv` to its final location.
    #[error("failed to move `rustc_codegen_spirv` to final location: {0}")]
    MoveRustcCodegenSpirvDylib(#[source] io::Error),
    /// Failed to remove target dir from `rustc_codegen_spirv`.
    #[error("failed to remove `target` dir from compiled codegen `rustc_codegen_spirv`: {0}")]
    RemoveRustcCodegenSpirvTargetDir(#[source] io::Error),
    /// Failed to build `rustc_codegen_spirv` by `cargo`.
    #[error(transparent)]
    RustcCodegenSpirvBuild(#[from] CommandExecError),
    /// The `rustc_codegen_spirv` build did not produce the expected dylib.
    #[error("`rustc_codegen_spirv` build did not produce the expected dylib")]
    RustcCodegenSpirvDylibNotFound,
}
