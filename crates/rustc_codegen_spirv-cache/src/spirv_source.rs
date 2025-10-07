//! Use the shader that we're compiling as the default source for which version of `rust-gpu` to use.
//!
//! We do this by calling `cargo tree` inside the shader's crate to get the defined `spirv-std`
//! version. Then with that we `git checkout` the `rust-gpu` repo that corresponds to that version.
//! From there we can look at the source code to get the required Rust toolchain.

use core::{
    fmt::{self, Display},
    ops::Range,
};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

use cargo_metadata::{
    camino::{Utf8Path, Utf8PathBuf},
    semver::Version,
    Package,
};

use crate::{
    cache::{cache_dir, CacheDirError},
    metadata::{query_metadata, MetadataExt as _, PackageByNameError, QueryMetadataError},
};

#[expect(
    rustdoc::bare_urls,
    clippy::doc_markdown,
    reason = "The URL should appear literally like this. But Clippy & rustdoc want a markdown clickable link"
)]
#[expect(clippy::exhaustive_enums, reason = "It is expected to be exhaustive")]
/// The source and version of `rust-gpu`.
/// Eg:
///   * From crates.io with version "0.10.0"
///   * From Git with:
///     - a repo of "https://github.com/Rust-GPU/rust-gpu.git"
///     - a revision of "abc213"
///   * a local Path
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
        rust_gpu_repo_root: Utf8PathBuf,
        /// Version of specified rust-gpu repository
        version: Version,
    },
}

impl Display for SpirvSource {
    #[expect(
        clippy::min_ident_chars,
        reason = "It's a core library trait implementation"
    )]
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CratesIO(version) => version.fmt(f),
            Self::Git { url, rev } => {
                // shorten rev to 8 chars, prevents windows compile errors due to too long paths... seriously
                if let Some(short_rev) = rev.get(..8) {
                    write!(f, "{url}+{short_rev}")
                } else {
                    write!(f, "{url}+{rev}")
                }
            }
            Self::Path {
                rust_gpu_repo_root,
                version,
            } => write!(f, "{rust_gpu_repo_root}+{version}"),
        }
    }
}

impl SpirvSource {
    /// Figures out which source of `rust-gpu` to use.
    ///
    /// # Errors
    ///
    /// If unable to determine the source of `rust-gpu`, returns an error.
    #[inline]
    pub fn new(
        shader_crate_path: &Path,
        maybe_rust_gpu_source: Option<&str>,
        maybe_rust_gpu_version: Option<&str>,
    ) -> Result<Self, SpirvSourceError> {
        match (maybe_rust_gpu_source, maybe_rust_gpu_version) {
            (Some(rust_gpu_source), Some(rust_gpu_version)) => Ok(Self::Git {
                url: rust_gpu_source.to_owned(),
                rev: rust_gpu_version.to_owned(),
            }),
            (None, Some(rust_gpu_version)) => Ok(Self::CratesIO(Version::parse(rust_gpu_version)?)),
            _ => Self::get_rust_gpu_deps_from_shader(shader_crate_path),
        }
    }

    /// Look into the shader crate to get the source and version of `rust-gpu` it's using.
    ///
    /// # Errors
    ///
    /// If unable to determine the source and version of `rust-gpu`, returns an error.
    #[inline]
    pub fn get_rust_gpu_deps_from_shader(
        shader_crate_path: &Path,
    ) -> Result<Self, SpirvSourceError> {
        let crate_metadata = query_metadata(shader_crate_path)?;
        let spirv_std_package = crate_metadata.package_by_name("spirv-std")?;
        let spirv_source = Self::parse_spirv_std_source_and_version(spirv_std_package)?;

        log::debug!(
            "Parsed `SpirvSource` from crate `{}`: {spirv_source:?}",
            shader_crate_path.display(),
        );
        Ok(spirv_source)
    }

    /// Convert self into a cache directory in which we can build it.
    ///
    /// It needs to be dynamically created because an end-user might want to swap out the source,
    /// maybe using their own fork for example.
    ///
    /// # Errors
    ///
    /// Returns an error if there is no cache directory available.
    #[inline]
    pub fn install_dir(&self) -> Result<PathBuf, CacheDirError> {
        let dir = match self {
            Self::Path {
                rust_gpu_repo_root, ..
            } => rust_gpu_repo_root.as_std_path().to_owned(),
            Self::CratesIO { .. } | Self::Git { .. } => {
                let dir = to_dirname(self.to_string().as_ref());
                cache_dir()?.join("codegen").join(dir)
            }
        };
        Ok(dir)
    }

    /// Returns `true` if self is a [`Path`](SpirvSource::Path).
    #[expect(
        clippy::must_use_candidate,
        reason = "calculations are cheap, `bool` is `Copy`"
    )]
    #[inline]
    pub const fn is_path(&self) -> bool {
        matches!(self, Self::Path { .. })
    }

    /// Parse a string like:
    ///   `spirv-std v0.9.0 (https://github.com/Rust-GPU/rust-gpu?rev=54f6978c#54f6978c) (*)`
    /// Which would return:
    ///   `SpirvSource::Git("https://github.com/Rust-GPU/rust-gpu", "54f6978c")`
    fn parse_spirv_std_source_and_version(
        spirv_std_package: &Package,
    ) -> Result<Self, ParseSourceVersionError> {
        log::trace!("parsing spirv-std source and version from package: '{spirv_std_package:?}'");

        let result = if let Some(source) = spirv_std_package.source.clone() {
            let is_git = source.repr.starts_with("git+");
            let is_crates_io = source.is_crates_io();

            match (is_git, is_crates_io) {
                (true, true) => return Err(ParseSourceVersionError::AmbiguousSource(source)),
                (true, false) => {
                    let parse_git = || {
                        let link = &source.repr.get(4..)?;
                        let sharp_index = link.find('#')?;
                        let url_end = link.find('?').unwrap_or(sharp_index);
                        let url = link.get(..url_end)?.to_owned();
                        let rev = link.get(sharp_index + 1..)?.to_owned();
                        Some(Self::Git { url, rev })
                    };
                    parse_git().ok_or(ParseSourceVersionError::InvalidGitSource(source))?
                }
                (false, true) => Self::CratesIO(spirv_std_package.version.clone()),
                (false, false) => return Err(ParseSourceVersionError::UnknownSource(source)),
            }
        } else {
            let manifest_path = spirv_std_package.manifest_path.as_path();
            let Some(rust_gpu_repo_root) = manifest_path // rust-gpu/crates/spirv-std/Cargo.toml
                .parent() // rust-gpu/crates/spirv-std
                .and_then(Utf8Path::parent) // rust-gpu/crates
                .and_then(Utf8Path::parent) // rust-gpu
                .filter(|path| path.is_dir())
                .map(ToOwned::to_owned)
            else {
                let err = ParseSourceVersionError::InvalidManifestPath(manifest_path.to_owned());
                return Err(err);
            };
            let version = spirv_std_package.version.clone();
            Self::Path {
                rust_gpu_repo_root,
                version,
            }
        };

        log::debug!("parsed `rust-gpu` source and version: {result:?}");
        Ok(result)
    }
}

/// An error indicating that construction of [`SpirvSource`] failed.
#[expect(clippy::module_name_repetitions, reason = "this is intended")]
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SpirvSourceError {
    /// Querying metadata failed.
    #[error(transparent)]
    QueryMetadata(#[from] QueryMetadataError),
    /// The package was missing from the metadata.
    #[error(transparent)]
    MissingPackage(#[from] PackageByNameError),
    /// Parsing the source and version of `spirv-std` crate from the package failed.
    #[error(transparent)]
    ParseSourceVersion(#[from] ParseSourceVersionError),
    /// Parsed version of the crate is not valid.
    #[error("invalid version: {0}")]
    InvalidVersion(#[from] cargo_metadata::semver::Error),
}

/// An error indicating that parsing the source and version of `rust-gpu` from the package failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ParseSourceVersionError {
    /// The source was found to be ambiguous.
    #[error("both git and crates.io were found at source {0}")]
    AmbiguousSource(cargo_metadata::Source),
    /// The source was found to be of git format but it is not valid.
    #[error("invalid git format of source {0}")]
    InvalidGitSource(cargo_metadata::Source),
    /// The source has unknown / unsupported format.
    #[error("unknown format of source {0}")]
    UnknownSource(cargo_metadata::Source),
    /// Manifest path of the package is not valid.
    #[error("invalid manifest path {0}")]
    InvalidManifestPath(Utf8PathBuf),
}

/// Parse the `rust-toolchain.toml` in the working tree of the checked-out version of the `rust-gpu` repo.
///
/// # Errors
///
/// Returns an error if the package is at the root of the filesystem,
/// build script does not exist, or there is no definition of `channel` in it.
#[inline]
pub fn rust_gpu_toolchain_channel(
    rustc_codegen_spirv: &Package,
) -> Result<String, RustGpuToolchainChannelError> {
    let path = rustc_codegen_spirv
        .manifest_path
        .parent()
        .ok_or(RustGpuToolchainChannelError::ManifestAtRoot)?;
    let build_script = path.join("build.rs");

    log::debug!("parsing `build.rs` at {build_script:?} for the used toolchain");
    let contents = match fs::read_to_string(&build_script) {
        Ok(contents) => contents,
        Err(source) => {
            let err = RustGpuToolchainChannelError::InvalidBuildScript {
                source,
                build_script,
            };
            return Err(err);
        }
    };

    let channel_start = "channel = \"";
    let Some(channel_line) = contents
        .lines()
        .find(|line| line.starts_with(channel_start))
    else {
        let err = RustGpuToolchainChannelError::ChannelStartNotFound {
            channel_start: channel_start.to_owned(),
            build_script,
        };
        return Err(err);
    };
    let start = channel_start.len();

    let channel_end = "\"";
    #[expect(clippy::string_slice, reason = "line starts with `channel_start`")]
    let Some(end) = channel_line[start..]
        .find(channel_end)
        .map(|end| end + start)
    else {
        let err = RustGpuToolchainChannelError::ChannelEndNotFound {
            channel_end: channel_end.to_owned(),
            channel_line: channel_line.to_owned(),
            build_script,
        };
        return Err(err);
    };

    let range = start..end;
    let Some(channel) = channel_line.get(range.clone()) else {
        let err = RustGpuToolchainChannelError::InvalidChannelSlice {
            range,
            channel_line: channel_line.to_owned(),
            build_script,
        };
        return Err(err);
    };
    Ok(channel.to_owned())
}

/// An error indicating that getting the channel of a Rust toolchain from the package failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RustGpuToolchainChannelError {
    /// Manifest of the package is located at the root of the file system
    /// and cannot have a parent.
    #[error("package manifest was located at root")]
    ManifestAtRoot,
    /// Build script file is not valid or does not exist.
    #[error("invalid build script {build_script}: {source}")]
    InvalidBuildScript {
        /// Source of the error.
        source: io::Error,
        /// Path to the build script file.
        build_script: Utf8PathBuf,
    },
    /// There is no line starting with `channel_start`
    /// in the build script file contents.
    #[error("`{channel_start}` line in {build_script:?} not found")]
    ChannelStartNotFound {
        /// Start of the channel line.
        channel_start: String,
        /// Path to the build script file.
        build_script: Utf8PathBuf,
    },
    /// Channel line does not contain `channel_end`
    /// in the build script file contents.
    #[error("ending `{channel_end}` of line \"{channel_line}\" in {build_script:?} not found")]
    ChannelEndNotFound {
        /// End of the channel line.
        channel_end: String,
        /// The line containing the channel information.
        channel_line: String,
        /// Path to the build script file.
        build_script: Utf8PathBuf,
    },
    /// The range to slice the channel line is not valid.
    #[error("cannot slice line \"{channel_line}\" of {build_script:?} by range {range:?}")]
    InvalidChannelSlice {
        /// The invalid range.
        range: Range<usize>,
        /// The line containing the channel information.
        channel_line: String,
        /// Path to the build script file.
        build_script: Utf8PathBuf,
    },
}

/// Returns a string suitable to use as a directory.
///
/// Created from the spirv-builder source dep and the rustc channel.
fn to_dirname(text: &str) -> String {
    text.replace(
        [std::path::MAIN_SEPARATOR, '\\', '/', '.', ':', '@', '='],
        "_",
    )
    .split(['{', '}', ' ', '\n', '"', '\''])
    .collect::<Vec<_>>()
    .concat()
}

#[cfg(test)]
mod test {
    use super::*;
    use cargo_metadata::{PackageBuilder, PackageId, Source};
    use cargo_util_schemas::manifest::PackageName;

    pub fn shader_crate_template_path() -> std::path::PathBuf {
        let project_base = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        project_base.join("../shader-crate-template")
    }

    #[test_log::test]
    fn parsing_spirv_std_dep_for_shader_template() {
        let shader_template_path = shader_crate_template_path();
        let source = SpirvSource::get_rust_gpu_deps_from_shader(&shader_template_path).unwrap();
        assert_eq!(
            source,
            SpirvSource::Git {
                url: "https://github.com/Rust-GPU/rust-gpu".to_owned(),
                rev: "86fc48032c4cd4afb74f1d81ae859711d20386a1".to_owned()
            }
        );
    }

    #[test_log::test]
    fn path_sanity() {
        let path = std::path::PathBuf::from("./");
        assert!(path.is_relative());
    }

    #[test_log::test]
    fn cached_checkout_dir_sanity() {
        let shader_template_path = shader_crate_template_path();
        let source = SpirvSource::get_rust_gpu_deps_from_shader(&shader_template_path).unwrap();
        let dir = source.install_dir().unwrap();
        let name = dir
            .file_name()
            .unwrap()
            .to_str()
            .map(std::string::ToString::to_string)
            .unwrap();
        assert_eq!("https___github_com_Rust-GPU_rust-gpu+86fc4803", &name);
    }

    #[test_log::test]
    fn parse_git_with_rev() {
        let source = parse_git(
            "git+https://github.com/Rust-GPU/rust-gpu?rev=86fc48032c4cd4afb74f1d81ae859711d20386a1#86fc4803",
        );
        assert_eq!(
            source,
            SpirvSource::Git {
                url: "https://github.com/Rust-GPU/rust-gpu".to_owned(),
                rev: "86fc4803".to_owned(),
            }
        );
    }

    #[test_log::test]
    fn parse_git_no_question_mark() {
        // taken directly from Graphite
        let source = parse_git(
            "git+https://github.com/Rust-GPU/rust-gpu.git#6e2c84d4fe64e32df4c060c5a7f3e35a32e45421",
        );
        assert_eq!(
            source,
            SpirvSource::Git {
                url: "https://github.com/Rust-GPU/rust-gpu.git".to_owned(),
                rev: "6e2c84d4fe64e32df4c060c5a7f3e35a32e45421".to_owned(),
            }
        );
    }

    fn parse_git(source: &str) -> SpirvSource {
        let package = PackageBuilder::new(
            PackageName::new("spirv-std".to_owned()).unwrap(),
            Version::new(0, 9, 0),
            PackageId {
                repr: String::new(),
            },
            "",
        )
        .source(Some(Source {
            repr: source.to_owned(),
        }))
        .build()
        .unwrap();
        SpirvSource::parse_spirv_std_source_and_version(&package).unwrap()
    }
}
