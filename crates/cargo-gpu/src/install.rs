//! Install a dedicated per-shader crate that has the `rust-gpu` compiler in it.

use anyhow::Context as _;
use std::io::Write as _;
use std::path::Path;

use crate::args::InstallArgs;
use crate::spirv_source::{
    get_channel_from_rustc_codegen_spirv_build_script, get_package_from_crate,
};
use crate::{cache_dir, spirv_source::SpirvSource, target_spec_dir};

/// Metadata for the compile targets supported by `rust-gpu`
const TARGET_SPECS: &[(&str, &str)] = &[
    (
        "spirv-unknown-opengl4.0.json",
        include_str!("../target-specs/spirv-unknown-opengl4.0.json"),
    ),
    (
        "spirv-unknown-opengl4.1.json",
        include_str!("../target-specs/spirv-unknown-opengl4.1.json"),
    ),
    (
        "spirv-unknown-opengl4.2.json",
        include_str!("../target-specs/spirv-unknown-opengl4.2.json"),
    ),
    (
        "spirv-unknown-opengl4.3.json",
        include_str!("../target-specs/spirv-unknown-opengl4.3.json"),
    ),
    (
        "spirv-unknown-opengl4.5.json",
        include_str!("../target-specs/spirv-unknown-opengl4.5.json"),
    ),
    (
        "spirv-unknown-spv1.0.json",
        include_str!("../target-specs/spirv-unknown-spv1.0.json"),
    ),
    (
        "spirv-unknown-spv1.1.json",
        include_str!("../target-specs/spirv-unknown-spv1.1.json"),
    ),
    (
        "spirv-unknown-spv1.2.json",
        include_str!("../target-specs/spirv-unknown-spv1.2.json"),
    ),
    (
        "spirv-unknown-spv1.3.json",
        include_str!("../target-specs/spirv-unknown-spv1.3.json"),
    ),
    (
        "spirv-unknown-spv1.4.json",
        include_str!("../target-specs/spirv-unknown-spv1.4.json"),
    ),
    (
        "spirv-unknown-spv1.5.json",
        include_str!("../target-specs/spirv-unknown-spv1.5.json"),
    ),
    (
        "spirv-unknown-vulkan1.0.json",
        include_str!("../target-specs/spirv-unknown-vulkan1.0.json"),
    ),
    (
        "spirv-unknown-vulkan1.1.json",
        include_str!("../target-specs/spirv-unknown-vulkan1.1.json"),
    ),
    (
        "spirv-unknown-vulkan1.1spv1.4.json",
        include_str!("../target-specs/spirv-unknown-vulkan1.1spv1.4.json"),
    ),
    (
        "spirv-unknown-vulkan1.2.json",
        include_str!("../target-specs/spirv-unknown-vulkan1.2.json"),
    ),
];

/// `cargo gpu install`
#[derive(clap::Parser, Debug, serde::Deserialize, serde::Serialize)]
pub struct Install {
    /// CLI arguments for installing the Rust toolchain and components
    #[clap(flatten)]
    pub spirv_install: InstallArgs,
}

impl Install {
    /// Create the `rustc_codegen_spirv_dummy` crate that depends on `rustc_codegen_spirv`
    fn write_source_files(source: &SpirvSource, checkout: &Path) -> anyhow::Result<()> {
        {
            let main = "fn main() {}";
            let src = checkout.join("src");
            std::fs::create_dir_all(&src).context("creating directory for 'src'")?;
            std::fs::write(src.join("main.rs"), main).context("writing 'main.rs'")?;
        };

        {
            let version_spec = match &source {
                SpirvSource::CratesIO(version) => {
                    format!("version = \"{}\"", version)
                }
                SpirvSource::Git { url, rev } => format!("git = \"{url}\"\nrev = \"{rev}\""),
                SpirvSource::Path {
                    rust_gpu_path,
                    version,
                } => {
                    let mut new_path = rust_gpu_path.to_owned();
                    new_path.push("crates/spirv-builder");
                    format!("path = \"{new_path}\"\nversion = \"{}\"", version)
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

    /// Install the binary pair and return the paths, (dylib, cli).
    pub fn run(&mut self) -> anyhow::Result<()> {
        // Ensure the cache dir exists
        let cache_dir = cache_dir()?;
        log::info!("cache directory is '{}'", cache_dir.display());
        std::fs::create_dir_all(&cache_dir).with_context(|| {
            format!("could not create cache directory '{}'", cache_dir.display())
        })?;

        // TODO what about lockfiles?
        // let spirv_version = self.spirv_cli().context("running spirv cli")?;
        let source = SpirvSource::new(
            &self.spirv_install.shader_crate,
            self.spirv_install.spirv_builder_source.as_deref(),
            self.spirv_install.spirv_builder_version.as_deref(),
        )?;
        let checkout = source.install_dir()?;

        let dylib_filename = format!(
            "{}rustc_codegen_spirv{}",
            std::env::consts::DLL_PREFIX,
            std::env::consts::DLL_SUFFIX
        );
        let dest_dylib_path = checkout.join(&dylib_filename);
        if dest_dylib_path.is_file() {
            log::info!(
                "cargo-gpu artifacts are already installed in '{}'",
                checkout.display()
            );
        }

        if dest_dylib_path.is_file() && !self.spirv_install.force_spirv_cli_rebuild {
            log::info!("...and so we are aborting the install step.");
        } else {
            log::debug!(
                "writing `rustc_codegen_spirv_dummy` source files into '{}'",
                checkout.display()
            );
            Self::write_source_files(&source, &checkout).context("writing source files")?;

            log::debug!("resolving toolchain version to use");
            let rustc_codegen_spirv = get_package_from_crate(&checkout, "rustc_codegen_spirv")
                .context("get `rustc_codegen_spirv` metadata")?;
            let toolchain_channel =
                get_channel_from_rustc_codegen_spirv_build_script(&rustc_codegen_spirv)
                    .context("read toolchain from `rustc_codegen_spirv`'s build.rs")?;
            log::info!("selected toolchain channel `{toolchain_channel:?}`");

            log::debug!("ensure_toolchain_and_components_exist");
            crate::install_toolchain::ensure_toolchain_and_components_exist(
                &toolchain_channel,
                self.spirv_install.auto_install_rust_toolchain,
            )
            .context("ensuring toolchain and components exist")?;

            log::debug!("write_target_spec_files");
            self.write_target_spec_files()
                .context("writing target spec files")?;

            crate::user_output!("Compiling `rustc_codegen_spirv` from source {}\n", source,);

            let mut build_command = std::process::Command::new("cargo");
            build_command
                .current_dir(&checkout)
                .arg(format!("+{}", toolchain_channel))
                .args(["build", "--release"])
                .env_remove("RUSTC");

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
                std::fs::rename(&dylib_path, &dest_dylib_path).context("renaming dylib path")?;
            } else {
                log::error!("could not find {}", dylib_path.display());
                anyhow::bail!("`rustc_codegen_spirv` build failed");
            }
        }

        self.spirv_install.dylib_path = dest_dylib_path;
        Ok(())
    }
}
