//! Install a dedicated per-shader crate that has the `rust-gpu` compiler in it.

use crate::args::InstallArgs;
use crate::spirv_source::{
    get_channel_from_rustc_codegen_spirv_build_script, get_package_from_crate,
};
use crate::{cache_dir, spirv_source::SpirvSource, target_spec_dir};
use anyhow::Context as _;
use log::trace;
use spirv_builder::TARGET_SPECS;
use std::io::Write as _;
use std::path::{Path, PathBuf};

/// `cargo gpu install`
#[derive(Clone, clap::Parser, Debug, serde::Deserialize, serde::Serialize)]
pub struct Install {
    /// CLI arguments for installing the Rust toolchain and components
    #[clap(flatten)]
    pub spirv_install: InstallArgs,
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
            if !path.is_file() || self.spirv_install.force_spirv_cli_rebuild {
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
    pub fn run(&mut self) -> anyhow::Result<(PathBuf, String)> {
        // Ensure the cache dir exists
        let cache_dir = cache_dir()?;
        log::info!("cache directory is '{}'", cache_dir.display());
        std::fs::create_dir_all(&cache_dir).with_context(|| {
            format!("could not create cache directory '{}'", cache_dir.display())
        })?;

        let source = SpirvSource::new(
            &self.spirv_install.shader_crate,
            self.spirv_install.spirv_builder_source.as_deref(),
            self.spirv_install.spirv_builder_version.as_deref(),
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

        let skip_rebuild = !source_is_path
            && dest_dylib_path.is_file()
            && !self.spirv_install.force_spirv_cli_rebuild;
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
                self.spirv_install.auto_install_rust_toolchain,
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

            let dylib_path = checkout
                .join("target")
                .join("release")
                .join(&dylib_filename);
            if dylib_path.is_file() {
                log::info!("successfully built {}", dylib_path.display());
                if !source_is_path {
                    std::fs::rename(&dylib_path, &dest_dylib_path)
                        .context("renaming dylib path")?;
                }
            } else {
                log::error!("could not find {}", dylib_path.display());
                anyhow::bail!("`rustc_codegen_spirv` build failed");
            }

            log::debug!("write_target_spec_files");
            self.write_target_spec_files()
                .context("writing target spec files")?;
        }

        self.spirv_install.dylib_path.clone_from(&dest_dylib_path);
        Ok((dest_dylib_path, toolchain_channel))
    }
}
