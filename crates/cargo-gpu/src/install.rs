//! Install a dedicated per-shader crate that has the `rust-gpu` compiler in it.

use crate::spirv_source::{
    get_channel_from_rustc_codegen_spirv_build_script, get_package_from_crate,
};
use crate::{cache_dir, spirv_source::SpirvSource, target_spec_dir};
use anyhow::Context as _;
use log::trace;
use spirv_builder::{SpirvBuilder, TARGET_SPECS};
use std::io::Write as _;
use std::path::{Path, PathBuf};

/// Args for an install
#[expect(
    clippy::struct_excessive_bools,
    reason = "cmdline args have many bools"
)]
#[derive(clap::Parser, Debug, Clone, serde::Deserialize, serde::Serialize)]
#[non_exhaustive]
pub struct Install {
    /// Directory containing the shader crate to compile.
    #[clap(long, default_value = "./")]
    pub shader_crate: PathBuf,

    #[expect(
        clippy::doc_markdown,
        reason = "The URL should appear literally like this. But Clippy wants a markdown clickable link"
    )]
    /// Source of `spirv-builder` dependency
    /// Eg: "https://github.com/Rust-GPU/rust-gpu"
    #[clap(long)]
    pub spirv_builder_source: Option<String>,

    /// Version of `spirv-builder` dependency.
    /// * If `--spirv-builder-source` is not set, then this is assumed to be a crates.io semantic
    ///   version such as "0.9.0".
    /// * If `--spirv-builder-source` is set, then this is assumed to be a Git "commitsh", such
    ///   as a Git commit hash or a Git tag, therefore anything that `git checkout` can resolve.
    #[clap(long, verbatim_doc_comment)]
    pub spirv_builder_version: Option<String>,

    /// Force `rustc_codegen_spirv` to be rebuilt.
    #[clap(long)]
    pub rebuild_codegen: bool,

    /// Assume "yes" to "Install Rust toolchain: [y/n]" prompt.
    #[clap(long, action)]
    pub auto_install_rust_toolchain: bool,

    /// Clear target dir of `rustc_codegen_spirv` build after a successful build, saves about
    /// 200MiB of disk space.
    #[clap(long = "no-clear-target", default_value = "true", action = clap::ArgAction::SetFalse)]
    pub clear_target: bool,

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
    /// So a somewhat hacky workaround is to have `cargo gpu` overwrite lockfile versions. Enabling
    /// this flag will only come into effect if there are a mix of v3/v4 lockfiles. It will also
    /// only overwrite versions for the duration of a build. It will attempt to return the versions
    /// to their original values once the build is finished. However, of course, unexpected errors
    /// can occur and the overwritten values can remain. Hence why this behaviour is not enabled by
    /// default.
    ///
    /// This hack is possible because the change from v3 to v4 only involves a minor change to the
    /// way source URLs are encoded. See these PRs for more details:
    ///   * <https://github.com/rust-lang/cargo/pull/12280>
    ///   * <https://github.com/rust-lang/cargo/pull/14595>
    #[clap(long, action, verbatim_doc_comment)]
    pub force_overwrite_lockfiles_v4_to_v3: bool,
}

/// Represents a functional backend installation, whether it was cached or just installed.
#[derive(Clone, Debug)]
pub struct InstalledBackend {
    /// path to the `rustc_codegen_spirv` dylib
    pub rustc_codegen_spirv_location: PathBuf,
    /// toolchain channel name
    pub toolchain_channel: String,
}

impl InstalledBackend {
    /// Configures the supplied [`SpirvBuilder`]. `SpirvBuilder.target` must be set and must not change after calling this function.
    pub fn configure_spirv_builder(&self, builder: &mut SpirvBuilder) -> anyhow::Result<()> {
        builder.rustc_codegen_spirv_location = Some(self.rustc_codegen_spirv_location.clone());
        builder.toolchain_overwrite = Some(self.toolchain_channel.clone());
        builder.path_to_target_spec = Some(target_spec_dir()?.join(format!(
            "{}.json",
            builder.target.as_ref().context("expect target to be set")?
        )));
        Ok(())
    }
}

impl Default for Install {
    #[inline]
    fn default() -> Self {
        Self {
            shader_crate: PathBuf::from("./"),
            spirv_builder_source: None,
            spirv_builder_version: None,
            rebuild_codegen: false,
            auto_install_rust_toolchain: false,
            clear_target: true,
            force_overwrite_lockfiles_v4_to_v3: false,
        }
    }
}

impl Install {
    /// Create the `rustc_codegen_spirv_dummy` crate that depends on `rustc_codegen_spirv`
    fn write_source_files(source: &SpirvSource, checkout: &Path) -> anyhow::Result<()> {
        // skip writing a dummy project if we use a local rust-gpu checkout
        if matches!(source, SpirvSource::Path { .. }) {
            return Ok(());
        }
        log::debug!(
            "writing `rustc_codegen_spirv_dummy` source files into '{}'",
            checkout.display()
        );

        {
            trace!("writing dummy main.rs");
            let main = "fn main() {}";
            let src = checkout.join("src");
            std::fs::create_dir_all(&src).context("creating directory for 'src'")?;
            std::fs::write(src.join("main.rs"), main).context("writing 'main.rs'")?;
        };

        {
            trace!("writing dummy Cargo.toml");
            let version_spec = match &source {
                SpirvSource::CratesIO(version) => {
                    format!("version = \"{version}\"")
                }
                SpirvSource::Git { url, rev } => format!("git = \"{url}\"\nrev = \"{rev}\""),
                SpirvSource::Path {
                    rust_gpu_repo_root: rust_gpu_path,
                    version,
                } => {
                    let mut new_path = rust_gpu_path.to_owned();
                    new_path.push("crates/spirv-builder");
                    format!("path = \"{new_path}\"\nversion = \"{version}\"")
                }
            };
            let cargo_toml = format!(
                r#"
[package]
name = "rustc_codegen_spirv_dummy"
version = "0.1.0"
edition = "2021"

[dependencies.spirv-builder]
package = "rustc_codegen_spirv"
{version_spec}
            "#
            );
            std::fs::write(checkout.join("Cargo.toml"), cargo_toml)
                .context("writing 'Cargo.toml'")?;
        };
        Ok(())
    }

    /// Add the target spec files to the crate.
    fn write_target_spec_files(&self) -> anyhow::Result<()> {
        for (filename, contents) in TARGET_SPECS {
            let path = target_spec_dir()
                .context("creating target spec dir")?
                .join(filename);
            if !path.is_file() || self.rebuild_codegen {
                let mut file = std::fs::File::create(&path)
                    .with_context(|| format!("creating file at [{}]", path.display()))?;
                file.write_all(contents.as_bytes())
                    .context("writing to file")?;
            }
        }
        Ok(())
    }

    /// Install the binary pair and return the `(dylib_path, toolchain_channel)`.
    #[expect(clippy::too_many_lines, reason = "it's fine")]
    pub fn run(&self) -> anyhow::Result<InstalledBackend> {
        // Ensure the cache dir exists
        let cache_dir = cache_dir()?;
        log::info!("cache directory is '{}'", cache_dir.display());
        std::fs::create_dir_all(&cache_dir).with_context(|| {
            format!("could not create cache directory '{}'", cache_dir.display())
        })?;

        let source = SpirvSource::new(
            &self.shader_crate,
            self.spirv_builder_source.as_deref(),
            self.spirv_builder_version.as_deref(),
        )?;
        let source_is_path = matches!(source, SpirvSource::Path { .. });
        let checkout = source.install_dir()?;

        let dylib_filename = format!(
            "{}rustc_codegen_spirv{}",
            std::env::consts::DLL_PREFIX,
            std::env::consts::DLL_SUFFIX
        );

        let dest_dylib_path;
        if source_is_path {
            dest_dylib_path = checkout
                .join("target")
                .join("release")
                .join(&dylib_filename);
        } else {
            dest_dylib_path = checkout.join(&dylib_filename);
            if dest_dylib_path.is_file() {
                log::info!(
                    "cargo-gpu artifacts are already installed in '{}'",
                    checkout.display()
                );
            }
        }

        let skip_rebuild = !source_is_path && dest_dylib_path.is_file() && !self.rebuild_codegen;
        if skip_rebuild {
            log::info!("...and so we are aborting the install step.");
        } else {
            Self::write_source_files(&source, &checkout).context("writing source files")?;
        }

        // TODO cache toolchain channel in a file?
        log::debug!("resolving toolchain version to use");
        let rustc_codegen_spirv = get_package_from_crate(&checkout, "rustc_codegen_spirv")
            .context("get `rustc_codegen_spirv` metadata")?;
        let toolchain_channel =
            get_channel_from_rustc_codegen_spirv_build_script(&rustc_codegen_spirv)
                .context("read toolchain from `rustc_codegen_spirv`'s build.rs")?;
        log::info!("selected toolchain channel `{toolchain_channel:?}`");

        if !skip_rebuild {
            log::debug!("ensure_toolchain_and_components_exist");
            crate::install_toolchain::ensure_toolchain_and_components_exist(
                &toolchain_channel,
                self.auto_install_rust_toolchain,
            )
            .context("ensuring toolchain and components exist")?;

            // to prevent unsupported version errors when using older toolchains
            if !source_is_path {
                log::debug!("remove Cargo.lock");
                std::fs::remove_file(checkout.join("Cargo.lock")).context("remove Cargo.lock")?;
            }

            crate::user_output!("Compiling `rustc_codegen_spirv` from source {}\n", source,);
            let mut build_command = std::process::Command::new("cargo");
            build_command
                .current_dir(&checkout)
                .arg(format!("+{toolchain_channel}"))
                .args(["build", "--release"])
                .env_remove("RUSTC");
            if source_is_path {
                build_command.args(["-p", "rustc_codegen_spirv", "--lib"]);
            }

            log::debug!("building artifacts with `{build_command:?}`");

            build_command
                .stdout(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::inherit())
                .output()
                .context("getting command output")
                .and_then(|output| {
                    if output.status.success() {
                        Ok(output)
                    } else {
                        Err(anyhow::anyhow!("bad status {:?}", output.status))
                    }
                })
                .context("running build command")?;

            let target = checkout.join("target");
            let dylib_path = target.join("release").join(&dylib_filename);
            if dylib_path.is_file() {
                log::info!("successfully built {}", dylib_path.display());
                if !source_is_path {
                    std::fs::rename(&dylib_path, &dest_dylib_path)
                        .context("renaming dylib path")?;

                    if self.clear_target {
                        log::warn!("clearing target dir {}", target.display());
                        std::fs::remove_dir_all(&target).context("clearing target dir")?;
                    }
                }
            } else {
                log::error!("could not find {}", dylib_path.display());
                anyhow::bail!("`rustc_codegen_spirv` build failed");
            }

            log::debug!("write_target_spec_files");
            self.write_target_spec_files()
                .context("writing target spec files")?;
        }

        Ok(InstalledBackend {
            rustc_codegen_spirv_location: dest_dylib_path,
            toolchain_channel,
        })
    }
}
