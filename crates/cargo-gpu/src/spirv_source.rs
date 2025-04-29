//! Use the shader that we're compiling as the default source for which version of `rust-gpu` to use.
//!
//! We do this by calling `cargo tree` inside the shader's crate to get the defined `spirv-std`
//! version. Then with that we `git checkout` the `rust-gpu` repo that corresponds to that version.
//! From there we can look at the source code to get the required Rust toolchain.

use anyhow::{anyhow, Context as _};
use cargo_metadata::camino::{Utf8Path, Utf8PathBuf};
use cargo_metadata::semver::Version;
use cargo_metadata::{MetadataCommand, Package};
use std::fs;
use std::path::{Path, PathBuf};

/// The canonical `rust-gpu` URI
const RUST_GPU_REPO: &str = "https://github.com/Rust-GPU/rust-gpu";

/// The various sources that the `rust-gpu` repo can have.
/// Most commonly it will simply be the canonical version on crates.io. But it could also be the
/// Git version, or a fork.
#[derive(Eq, PartialEq, Clone, Debug)]
pub enum SpirvSource {
    /// If the shader specifies a simple version like `spirv-std = "0.9.0"` then the source of
    /// `rust-gpu` is the conventional crates.io version.
    CratesIO(Version),
    /// If the shader specifies a version like:
    ///   `spirv-std = { git = "https://github.com..." ... }`
    /// then the source of `rust-gpu` is `Git`.
    Git {
        /// URL of the repository
        url: String,
        /// Revision or "commitsh"
        rev: String,
    },
    /// If the shader specifies a version like:
    ///   `spirv-std = { path = "/path/to/rust-gpu" ... }`
    /// then the source of `rust-gpu` is `Path`.
    Path {
        /// File path of rust-gpu repository
        rust_gpu_path: Utf8PathBuf,
        /// Version of specified rust-gpu repository
        version: Version,
    },
}

impl core::fmt::Display for SpirvSource {
    #[expect(
        clippy::min_ident_chars,
        reason = "It's a core library trait implementation"
    )]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::CratesIO(version) => version.fmt(f),
            Self::Git { url, rev } => f.write_str(&format!("{url}+{rev}")),
            Self::Path {
                rust_gpu_path,
                version,
            } => f.write_str(&format!("{rust_gpu_path}+{version}")),
        }
    }
}

impl SpirvSource {
    /// Look into the shader crate to get the version of `rust-gpu` it's using.
    pub fn get_rust_gpu_deps_from_shader<F: AsRef<Path>>(
        shader_crate_path: F,
    ) -> anyhow::Result<(Self, String)> {
        let spirv_std_package = Self::get_spirv_std_package(shader_crate_path.as_ref())?;
        let spirv_source = Self::parse_spirv_std_source_and_version(&spirv_std_package)?;
        let channel = Self::get_channel_from_toolchain_toml(&spirv_std_package)?;
        log::debug!(
            "Parsed version and toolchain channel from shader-defined `rust-gpu`: \
            {spirv_source:?}, {channel}"
        );
        Ok((spirv_source, channel))
    }

    /// Convert the source to just its version.
    pub fn to_version(&self) -> String {
        match self {
            Self::CratesIO(version) | Self::Path { version, .. } => version.to_string(),
            Self::Git { rev, .. } => rev.to_string(),
        }
    }

    /// Convert the source to just its repo or path.
    /// Must be root of git repository.
    fn to_repo(&self) -> String {
        match self {
            Self::CratesIO(_) => RUST_GPU_REPO.to_owned(),
            Self::Git { url, .. } => url.to_owned(),
            Self::Path { rust_gpu_path, .. } => rust_gpu_path.to_string(),
        }
    }

    /// Convert the `rust-gpu` source into a string that can be used as a directory.
    /// It needs to be dynamically created because an end-user might want to swap out the source,
    /// maybe using their own fork for example.
    fn to_dirname(&self) -> anyhow::Result<std::path::PathBuf> {
        let dir = crate::to_dirname(self.to_string().as_ref());
        Ok(crate::cache_dir()?.join("rust-gpu-repo").join(dir))
    }

    /// Make sure shader crate path is absolute and canonical.
    fn shader_crate_path_canonical(shader_crate_path: &Path) -> anyhow::Result<PathBuf> {
        let mut canonical_path = shader_crate_path.to_path_buf();

        if !canonical_path.is_absolute() {
            let cwd = std::env::current_dir().context("no cwd")?;
            canonical_path = cwd.join(canonical_path);
        }
        canonical_path = canonical_path
            .canonicalize()
            .context("could not get absolute path to shader crate")?;

        if !canonical_path.is_dir() {
            log::error!("{shader_crate_path:?} is not a directory, aborting");
            anyhow::bail!("{shader_crate_path:?} is not a directory");
        }
        Ok(canonical_path)
    }

    /// Parse the `rust-toolchain.toml` in the working tree of the checked-out version of the `rust-gpu` repo.
    fn get_channel_from_toolchain_toml(spirv_std_package: &Package) -> anyhow::Result<String> {
        // TODO can we query the toolchain toml from spirv_std, or should we rather do that from rustc_codegen_spirv within our build dir?
        let path = spirv_std_package
            .manifest_path
            .parent()
            .and_then(Utf8Path::parent)
            .and_then(Utf8Path::parent)
            .context("parent of spirv_std")?;

        log::debug!("Parsing `rust-toolchain.toml` at {path:?} for the used toolchain");

        let contents = fs::read_to_string(path.join("rust-toolchain.toml"))?;
        let toml: toml::Table = toml::from_str(&contents)?;
        let Some(toolchain) = toml.get("toolchain") else {
            anyhow::bail!(
                "Couldn't find `[toolchain]` section in `rust-toolchain.toml` at {path:?}"
            );
        };
        let Some(channel) = toolchain.get("channel") else {
            anyhow::bail!("Couldn't find `channel` field in `rust-toolchain.toml` at {path:?}");
        };

        Ok(channel.to_string().replace('"', ""))
    }

    /// Get the shader crate's resolved `spirv_std = ...` definition in its `Cargo.toml`/`Cargo.lock`
    pub fn get_spirv_std_package(shader_crate_path: &Path) -> anyhow::Result<Package> {
        let canonical_shader_path = Self::shader_crate_path_canonical(shader_crate_path)?;

        log::debug!(
            "Running `cargo metadata` on {}",
            canonical_shader_path.display()
        );
        let metadata = MetadataCommand::new()
            .current_dir(&canonical_shader_path)
            .exec()?;

        let Some(spirv_std_package) = metadata
            .packages
            .into_iter()
            .find(|package| package.name.eq("spirv-std"))
        else {
            anyhow::bail!(
                "`spirv-std` not found in shader's `Cargo.toml` at {canonical_shader_path:?}"
            );
        };
        log::trace!("  found {spirv_std_package:?}");
        Ok(spirv_std_package)
    }

    /// Parse a string like:
    ///   `spirv-std v0.9.0 (https://github.com/Rust-GPU/rust-gpu?rev=54f6978c#54f6978c) (*)`
    /// Which would return:
    ///   `SpirvSource::Git("https://github.com/Rust-GPU/rust-gpu", "54f6978c")`
    fn parse_spirv_std_source_and_version(spirv_std_package: &Package) -> anyhow::Result<Self> {
        log::trace!(
            "parsing spirv-std source and version from package: '{:?}'",
            spirv_std_package
        );

        let result = match &spirv_std_package.source {
            Some(source) => {
                let is_git = source.repr.starts_with("git+");
                let is_crates_io = source.is_crates_io();

                match (is_git, is_crates_io) {
                    (true, true) => unreachable!(),
                    (true, false) => {
                        let link = &source.repr[4..];
                        let sharp_index = link.find('#').ok_or(anyhow!(
                            "Git url of spirv-std package does not contain revision!"
                        ))?;
                        let question_mark_index = link.find('?').ok_or(anyhow!(
                            "Git url of spirv-std package does not contain revision!"
                        ))?;
                        let url = link[..question_mark_index].to_string();
                        let rev = link[sharp_index + 1..].to_string();
                        Self::Git { url, rev }
                    }
                    (false, true) => Self::CratesIO(spirv_std_package.version.clone()),
                    (false, false) => {
                        anyhow::bail!("Metadata of spirv-std package uses unknown url format!")
                    }
                }
            }
            None => {
                let rust_gpu_path = spirv_std_package
                    .manifest_path // rust-gpu/crates/spirv-std/Cargo.toml
                    .parent()
                    .unwrap() // rust-gpu/crates/spirv-std
                    .parent()
                    .unwrap() // rust-gpu/crates
                    .parent()
                    .unwrap() // rust-gpu
                    .to_owned();
                let version = spirv_std_package.version.clone();
                Self::Path {
                    rust_gpu_path,
                    version,
                }
            }
        };

        log::debug!("Parsed `rust-gpu` source and version: {result:?}");

        Ok(result)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test_log::test]
    fn parsing_spirv_std_dep_for_shader_template() {
        let shader_template_path = crate::test::shader_crate_template_path();
        let (source, _) =
            SpirvSource::get_rust_gpu_deps_from_shader(&shader_template_path).unwrap();
        assert_eq!(
            source,
            SpirvSource::Git {
                url: "https://github.com/Rust-GPU/rust-gpu".to_owned(),
                rev: "82a0f69008414f51d59184763146caa6850ac588".to_owned()
            }
        );
    }

    #[test_log::test]
    fn path_sanity() {
        let path = std::path::PathBuf::from("./");
        assert!(path.is_relative());
    }
}
