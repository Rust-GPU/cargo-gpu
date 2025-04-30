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

#[expect(
    clippy::doc_markdown,
    reason = "The URL should appear literally like this. But Clippy wants it to be a in markdown clickable link"
)]
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
                rust_gpu_repo_root,
                version,
            } => f.write_str(&format!("{rust_gpu_repo_root}+{version}")),
        }
    }
}

impl SpirvSource {
    pub fn new(
        shader_crate_path: &Path,
        maybe_rust_gpu_source: Option<&str>,
        maybe_rust_gpu_version: Option<&str>,
    ) -> anyhow::Result<Self> {
        let source = if let Some(rust_gpu_version) = maybe_rust_gpu_version {
            if let Some(rust_gpu_source) = maybe_rust_gpu_source {
                SpirvSource::Git {
                    url: rust_gpu_source.to_owned(),
                    rev: rust_gpu_version.to_owned(),
                }
            } else {
                SpirvSource::CratesIO(Version::parse(&rust_gpu_version)?)
            }
        } else {
            SpirvSource::get_rust_gpu_deps_from_shader(shader_crate_path)
                .context("get_rust_gpu_deps_from_shader")?
        };
        Ok(source)
    }

    /// Look into the shader crate to get the version of `rust-gpu` it's using.
    pub fn get_rust_gpu_deps_from_shader(shader_crate_path: &Path) -> anyhow::Result<Self> {
        let spirv_std_package = get_package_from_crate(&shader_crate_path, "spirv-std")?;
        let spirv_source = Self::parse_spirv_std_source_and_version(&spirv_std_package)?;
        log::debug!(
            "Parsed `SpirvSource` from crate `{}`: \
            {spirv_source:?}",
            shader_crate_path.display(),
        );
        Ok(spirv_source)
    }

    /// Convert the `SpirvSource` to a cache directory in which we can build it.
    /// It needs to be dynamically created because an end-user might want to swap out the source,
    /// maybe using their own fork for example.
    pub fn install_dir(&self) -> anyhow::Result<PathBuf> {
        match self {
            SpirvSource::Path {
                rust_gpu_repo_root, ..
            } => Ok(rust_gpu_repo_root.as_std_path().to_owned()),
            SpirvSource::CratesIO { .. } | SpirvSource::Git { .. } => {
                let dir = crate::to_dirname(self.to_string().as_ref());
                Ok(crate::cache_dir()?
                    .join("rustc_backend_spirv_install")
                    .join(dir))
            }
        }
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
                let rust_gpu_repo_root = spirv_std_package
                    .manifest_path // rust-gpu/crates/spirv-std/Cargo.toml
                    .parent() // rust-gpu/crates/spirv-std
                    .and_then(Utf8Path::parent) // rust-gpu/crates
                    .and_then(Utf8Path::parent) // rust-gpu
                    .context("selecting rust-gpu workspace root dir in local path")?
                    .to_owned();
                if !rust_gpu_repo_root.is_dir() {
                    anyhow::bail!("path {rust_gpu_repo_root} is not a directory");
                }
                let version = spirv_std_package.version.clone();
                Self::Path {
                    rust_gpu_repo_root,
                    version,
                }
            }
        };

        log::debug!("Parsed `rust-gpu` source and version: {result:?}");

        Ok(result)
    }
}

/// Make sure shader crate path is absolute and canonical.
fn crate_path_canonical(shader_crate_path: &Path) -> anyhow::Result<PathBuf> {
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

/// get the Package metadata from some crate
pub fn get_package_from_crate(crate_path: &Path, crate_name: &str) -> anyhow::Result<Package> {
    let canonical_crate_path = crate_path_canonical(crate_path)?;

    log::debug!(
        "Running `cargo metadata` on `{}` to query for package `{crate_name}`",
        canonical_crate_path.display()
    );
    let metadata = MetadataCommand::new()
        .current_dir(&canonical_crate_path)
        .exec()?;

    let Some(package) = metadata
        .packages
        .into_iter()
        .find(|package| package.name.eq(crate_name))
    else {
        anyhow::bail!("`{crate_name}` not found in `Cargo.toml` at `{canonical_crate_path:?}`");
    };
    log::trace!("  found `{}` version `{}`", package.name, package.version);
    Ok(package)
}

/// Parse the `rust-toolchain.toml` in the working tree of the checked-out version of the `rust-gpu` repo.
pub fn get_channel_from_rustc_codegen_spirv_build_script(
    rustc_codegen_spirv_package: &Package,
) -> anyhow::Result<String> {
    let path = rustc_codegen_spirv_package
        .manifest_path
        .parent()
        .context("finding `rustc_codegen_spirv` crate root")?;
    let build_rs = path.join("build.rs");

    log::debug!("Parsing `build.rs` at {build_rs:?} for the used toolchain");
    let contents = fs::read_to_string(&build_rs)?;
    let channel_start = "channel = \"";
    let channel_line = contents
        .lines()
        .find_map(|line| line.strip_prefix(channel_start))
        .context(format!("Can't find `{channel_start}` line in {build_rs:?}"))?;
    let channel = &channel_line[..channel_line.find("\"").context("ending \" missing")?];
    Ok(channel.to_string())
}

#[cfg(test)]
mod test {
    use super::*;

    #[test_log::test]
    fn parsing_spirv_std_dep_for_shader_template() {
        let shader_template_path = crate::test::shader_crate_template_path();
        let source = SpirvSource::get_rust_gpu_deps_from_shader(&shader_template_path).unwrap();
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

    #[test_log::test]
    fn cached_checkout_dir_sanity() {
        let shader_template_path = crate::test::shader_crate_template_path();
        let source = SpirvSource::get_rust_gpu_deps_from_shader(&shader_template_path).unwrap();
        let dir = source.install_dir().unwrap();
        let name = dir
            .file_name()
            .unwrap()
            .to_str()
            .map(std::string::ToString::to_string)
            .unwrap();
        assert_eq!(
            "https___github_com_Rust-GPU_rust-gpu+82a0f69008414f51d59184763146caa6850ac588",
            &name
        );
    }
}
