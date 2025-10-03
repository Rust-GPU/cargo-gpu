//! Handles lockfile version conflicts and downgrades.
//!
//! Stable uses lockfile v4, but `rust-gpu` v0.9.0 uses an old toolchain requiring v3
//! and will refuse to build shader crate with a v4 lockfile being present.
//! This module takes care of warning the user and potentially downgrading the lockfile.

#![expect(clippy::non_ascii_literal, reason = "'⚠️' character is really needed")]

use std::{
    fs,
    io::{self, Write as _},
    path::{Path, PathBuf},
};

use crate::spirv_cache::{cargo_metadata::semver::Version, spirv_builder::query_rustc_version};

/// `Cargo.lock` manifest version 4 became the default in Rust 1.83.0. Conflicting manifest
/// versions between the workspace and the shader crate, can cause problems.
const RUST_VERSION_THAT_USES_V4_CARGO_LOCKS: Version = Version::new(1, 83, 0);

/// Cargo dependency for `spirv-builder` and the rust toolchain channel.
#[derive(Debug, Clone)]
#[non_exhaustive]
#[expect(clippy::module_name_repetitions, reason = "it is intended")]
pub struct LockfileMismatchHandler {
    /// `Cargo.lock`s that have had their manifest versions changed by us and need changing back.
    pub cargo_lock_files_with_changed_manifest_versions: Vec<PathBuf>,
}

impl LockfileMismatchHandler {
    /// Creates self from the given parameters.
    ///
    /// # Errors
    ///
    /// Returns an error if there was a problem checking or changing lockfile manifest versions.
    /// See [`LockfileMismatchError`] for details.
    #[inline]
    pub fn new(
        shader_crate_path: &Path,
        toolchain_channel: &str,
        is_force_overwrite_lockfiles_v4_to_v3: bool,
    ) -> Result<Self, LockfileMismatchError> {
        let mut cargo_lock_files_with_changed_manifest_versions = vec![];

        let maybe_shader_crate_lock =
            Self::ensure_workspace_rust_version_does_not_conflict_with_shader(
                shader_crate_path,
                is_force_overwrite_lockfiles_v4_to_v3,
            )?;

        if let Some(shader_crate_lock) = maybe_shader_crate_lock {
            cargo_lock_files_with_changed_manifest_versions.push(shader_crate_lock);
        }

        let maybe_workspace_crate_lock =
            Self::ensure_shader_rust_version_does_not_conflict_with_any_cargo_locks(
                shader_crate_path,
                toolchain_channel,
                is_force_overwrite_lockfiles_v4_to_v3,
            )?;

        if let Some(workspace_crate_lock) = maybe_workspace_crate_lock {
            cargo_lock_files_with_changed_manifest_versions.push(workspace_crate_lock);
        }

        Ok(Self {
            cargo_lock_files_with_changed_manifest_versions,
        })
    }

    /// See docs for [`force_overwrite_lockfiles_v4_to_v3`](field@crate::build::CargoGpuBuilderParams::force_overwrite_lockfiles_v4_to_v3)
    /// flag for why we do this.
    fn ensure_workspace_rust_version_does_not_conflict_with_shader(
        shader_crate_path: &Path,
        is_force_overwrite_lockfiles_v4_to_v3: bool,
    ) -> Result<Option<PathBuf>, LockfileMismatchError> {
        log::debug!("Ensuring no v3/v4 `Cargo.lock` conflicts from workspace Rust...");
        let workspace_rust_version =
            query_rustc_version(None).map_err(LockfileMismatchError::QueryRustcVersion)?;
        if workspace_rust_version >= RUST_VERSION_THAT_USES_V4_CARGO_LOCKS {
            log::debug!(
                "user's Rust is v{workspace_rust_version}, so no v3/v4 conflicts possible."
            );
            return Ok(None);
        }

        Self::handle_conflicting_cargo_lock_v4(
            shader_crate_path,
            is_force_overwrite_lockfiles_v4_to_v3,
        )?;

        if is_force_overwrite_lockfiles_v4_to_v3 {
            Ok(Some(shader_crate_path.join("Cargo.lock")))
        } else {
            Ok(None)
        }
    }

    /// See docs for [`force_overwrite_lockfiles_v4_to_v3`](field@crate::build::CargoGpuBuilderParams::force_overwrite_lockfiles_v4_to_v3)
    /// flag for why we do this.
    fn ensure_shader_rust_version_does_not_conflict_with_any_cargo_locks(
        shader_crate_path: &Path,
        channel: &str,
        is_force_overwrite_lockfiles_v4_to_v3: bool,
    ) -> Result<Option<PathBuf>, LockfileMismatchError> {
        log::debug!("Ensuring no v3/v4 `Cargo.lock` conflicts from shader's Rust...");
        let shader_rust_version =
            query_rustc_version(Some(channel)).map_err(LockfileMismatchError::QueryRustcVersion)?;
        if shader_rust_version >= RUST_VERSION_THAT_USES_V4_CARGO_LOCKS {
            log::debug!("shader's Rust is v{shader_rust_version}, so no v3/v4 conflicts possible.");
            return Ok(None);
        }

        log::debug!(
            "shader's Rust is v{shader_rust_version}, so checking both shader and workspace `Cargo.lock` manifest versions..."
        );

        if shader_crate_path.join("Cargo.lock").exists() {
            // Note that we don't return the `Cargo.lock` here (so that it's marked for reversion
            // after the build), because we can be sure that updating it now is actually updating it
            // to the state it should have been all along. Therefore it doesn't need reverting once
            // fixed.
            Self::handle_conflicting_cargo_lock_v4(
                shader_crate_path,
                is_force_overwrite_lockfiles_v4_to_v3,
            )?;
        }

        if let Some(workspace_root) = Self::get_workspace_root(shader_crate_path)? {
            Self::handle_conflicting_cargo_lock_v4(
                workspace_root,
                is_force_overwrite_lockfiles_v4_to_v3,
            )?;
            return Ok(Some(workspace_root.join("Cargo.lock")));
        }

        Ok(None)
    }

    /// Get the path to the shader crate's workspace, if it has one. We can't use the traditional
    /// `cargo metadata` because if the workspace has a conflicting `Cargo.lock` manifest version
    /// then that command won't work. Instead we do an old school recursive file tree walk.
    fn get_workspace_root(
        shader_crate_path: &Path,
    ) -> Result<Option<&Path>, LockfileMismatchError> {
        let shader_cargo_toml_path = shader_crate_path.join("Cargo.toml");
        let shader_cargo_toml = match fs::read_to_string(shader_cargo_toml_path) {
            Ok(contents) => contents,
            Err(source) => {
                let file = shader_crate_path.join("Cargo.toml");
                return Err(LockfileMismatchError::ReadFile { file, source });
            }
        };
        if !shader_cargo_toml.contains("workspace = true") {
            return Ok(None);
        }

        let mut current_path = shader_crate_path;
        #[expect(clippy::default_numeric_fallback, reason = "It's just a loop")]
        for _ in 0..15 {
            if let Some(parent_path) = current_path.parent() {
                if parent_path.join("Cargo.lock").exists() {
                    return Ok(Some(parent_path));
                }
                current_path = parent_path;
            } else {
                break;
            }
        }

        Ok(None)
    }

    /// When Rust < 1.83.0 is being used an error will occur if it tries to parse `Cargo.lock`
    /// files that use lockfile manifest version 4. Here we check and handle that.
    fn handle_conflicting_cargo_lock_v4(
        folder: &Path,
        is_force_overwrite_lockfiles_v4_to_v3: bool,
    ) -> Result<(), LockfileMismatchError> {
        let shader_cargo_lock_path = folder.join("Cargo.lock");
        let shader_cargo_lock = match fs::read_to_string(&shader_cargo_lock_path) {
            Ok(contents) => contents,
            Err(source) => {
                let file = shader_cargo_lock_path;
                return Err(LockfileMismatchError::ReadFile { file, source });
            }
        };

        let Some(third_line) = shader_cargo_lock.lines().nth(2) else {
            let file = shader_cargo_lock_path;
            return Err(LockfileMismatchError::TooFewLinesInLockfile { file });
        };
        if third_line.contains("version = 4") {
            Self::handle_v3v4_conflict(
                &shader_cargo_lock_path,
                is_force_overwrite_lockfiles_v4_to_v3,
            )?;
            return Ok(());
        }
        if third_line.contains("version = 3") {
            return Ok(());
        }

        let file = shader_cargo_lock_path;
        let version_line = third_line.to_owned();
        Err(LockfileMismatchError::UnrecognizedLockfileVersion { file, version_line })
    }

    /// Handle conflicting `Cargo.lock` manifest versions by either overwriting the manifest
    /// version or exiting with advice on how to handle the conflict.
    fn handle_v3v4_conflict(
        offending_cargo_lock: &Path,
        is_force_overwrite_lockfiles_v4_to_v3: bool,
    ) -> Result<(), LockfileMismatchError> {
        if !is_force_overwrite_lockfiles_v4_to_v3 {
            return Err(LockfileMismatchError::ConflictingVersions);
        }

        Self::replace_cargo_lock_manifest_version(offending_cargo_lock, "4", "3")
    }

    /// Once all install and builds have completed put their manifest versions
    /// back to how they were.
    ///
    /// # Errors
    ///
    /// Returns an error if there was a problem reverting any of the lockfiles.
    /// See [`LockfileMismatchError`] for details.
    #[inline]
    pub fn revert_cargo_lock_manifest_versions(&mut self) -> Result<(), LockfileMismatchError> {
        for offending_cargo_lock in &self.cargo_lock_files_with_changed_manifest_versions {
            log::debug!("Reverting: {}", offending_cargo_lock.display());
            Self::replace_cargo_lock_manifest_version(offending_cargo_lock, "3", "4")?;
        }
        Ok(())
    }

    /// Replace the manifest version, eg `version = 4`, in a `Cargo.lock` file.
    fn replace_cargo_lock_manifest_version(
        offending_cargo_lock: &Path,
        from_version: &str,
        to_version: &str,
    ) -> Result<(), LockfileMismatchError> {
        log::warn!(
            "Replacing manifest version 'version = {from_version}' with 'version = {to_version}' in: {}",
            offending_cargo_lock.display()
        );
        let old_contents = match fs::read_to_string(offending_cargo_lock) {
            Ok(contents) => contents,
            Err(source) => {
                let file = offending_cargo_lock.to_path_buf();
                return Err(LockfileMismatchError::ReadFile { file, source });
            }
        };
        let new_contents = old_contents.replace(
            &format!("\nversion = {from_version}\n"),
            &format!("\nversion = {to_version}\n"),
        );

        if let Err(source) = fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(offending_cargo_lock)
            .and_then(|mut file| file.write_all(new_contents.as_bytes()))
        {
            let err = LockfileMismatchError::RewriteLockfile {
                file: offending_cargo_lock.to_path_buf(),
                from_version: from_version.to_owned(),
                to_version: to_version.to_owned(),
                source,
            };
            return Err(err);
        }

        Ok(())
    }
}

impl Drop for LockfileMismatchHandler {
    #[inline]
    fn drop(&mut self) {
        let result = self.revert_cargo_lock_manifest_versions();
        if let Err(error) = result {
            log::error!("could not revert some or all of the shader `Cargo.lock` files ({error})");
        }
    }
}

/// An error indicating a problem occurred
/// while handling lockfile manifest version mismatches.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
#[expect(clippy::module_name_repetitions, reason = "it is intended")]
pub enum LockfileMismatchError {
    /// Could not query current rustc version.
    #[error("could not query rustc version: {0}")]
    QueryRustcVersion(#[source] io::Error),
    /// Could not read contents of the file.
    #[error("could not read file {file}: {source}")]
    ReadFile {
        /// Path to the file that couldn't be read.
        file: PathBuf,
        /// Source of the error.
        source: io::Error,
    },
    /// Could not rewrite the lockfile with new manifest version.
    #[error(
        "could not rewrite lockfile {file} from version {from_version} to {to_version}: {source}"
    )]
    RewriteLockfile {
        /// Path to the file that couldn't be rewritten.
        file: PathBuf,
        /// Old manifest version we were changing from.
        from_version: String,
        /// New manifest version we were changing to.
        to_version: String,
        /// Source of the error.
        source: io::Error,
    },
    /// Lockfile has too few lines to determine manifest version.
    #[error("lockfile at {file} has too few lines to determine manifest version")]
    TooFewLinesInLockfile {
        /// Path to the lockfile that contains too few lines.
        file: PathBuf,
    },
    /// Lockfile manifest version could not be recognized.
    #[error("unrecognized lockfile {file} manifest version at \"{version_line}\"")]
    UnrecognizedLockfileVersion {
        /// Path to the lockfile that contains the unrecognized version line.
        file: PathBuf,
        /// The unrecognized version line.
        version_line: String,
    },
    /// Conflicting lockfile manifest versions detected, with advice on how to resolve them
    /// by setting the [`force_overwrite_lockfiles_v4_to_v3`] flag.
    ///
    /// [`force_overwrite_lockfiles_v4_to_v3`]: field@crate::build::CargoGpuBuilderParams::force_overwrite_lockfiles_v4_to_v3
    #[error(
        r#"conflicting `Cargo.lock` versions detected ⚠️

Because a dedicated Rust toolchain for compiling shaders is being used,
it's possible that the `Cargo.lock` manifest version of the shader crate
does not match the `Cargo.lock` manifest version of the workspace.
This is due to a change in the defaults introduced in Rust 1.83.0.

One way to resolve this is to force the workspace to use the same version
of Rust as required by the shader. However, that is not often ideal or even
possible. Another way is to exclude the shader from the workspace. This is
also not ideal if you have many shaders sharing config from the workspace.

Therefore, `cargo gpu build/install` offers a workaround with the argument:
  --force-overwrite-lockfiles-v4-to-v3

which corresponds to the `force_overwrite_lockfiles_v4_to_v3` flag of `InstallParams`.

See `cargo gpu build --help` or flag docs for more information."#
    )]
    ConflictingVersions,
}
