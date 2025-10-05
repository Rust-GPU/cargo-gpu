//! Command line tool for building Rust shaders using `rust-gpu`.
//!
//! This program allows you to easily compile your rust-gpu shaders,
//! without requiring you to fix your entire project to a specific toolchain.
//!
//! ## Building shader crates
//!
//! It takes a path to a shader crate to build, as well as a path to a directory to put
//! the compiled `spv` source files. It also takes a path to an output manifest file
//! where all shader entry points will be mapped to their `spv` source files.
//! This manifest file can be used by build scripts (`build.rs` files) to generate linkage
//! or conduct other post-processing, like converting the `spv` files into `wgsl` files, for example.
//!
//! For additional information, see the [`cargo-gpu-build`](cargo_gpu_build) crate documentation.
//!
//! ## Where the binaries are
//!
//! Prebuilt binaries are stored in the [cache directory](spirv_cache::cache::cache_dir()),
//! which path differs by OS you are using.

#![expect(clippy::pub_use, reason = "part of public API")]

pub use cargo_gpu_build::{spirv_builder, spirv_cache};

use self::{
    build::Build, config::from_cargo_metadata_with_config, dump_usage::dump_full_usage_for_readme,
    install::Install, show::Show,
};

pub mod build;
pub mod install;
pub mod show;

mod config;
mod dump_usage;
mod linkage;
mod merge;
mod metadata;
mod test;
mod user_consent;

/// All of the available subcommands for `cargo gpu`.
#[derive(clap::Subcommand)]
#[non_exhaustive]
pub enum Command {
    /// Install rust-gpu compiler artifacts.
    Install(Box<Install>),

    /// Compile a shader crate to SPIR-V.
    Build(Box<Build>),

    /// Show some useful values.
    Show(Show),

    /// A hidden command that can be used to recursively print out all the subcommand help messages:
    ///   `cargo gpu dump-usage`
    /// Useful for updating the README.
    #[doc(hidden)]
    #[clap(hide(true))]
    DumpUsage,
}

impl Command {
    /// Runs the command.
    ///
    /// # Errors
    ///
    /// Any errors during execution, usually printed to the user.
    #[inline]
    pub fn run(&self) -> anyhow::Result<()> {
        match &self {
            Self::Install(install) => {
                let shader_crate = &install.install.shader_crate;
                let command = from_cargo_metadata_with_config(shader_crate, install.as_ref())?;
                log::debug!("installing with final merged arguments: {command:#?}");

                command.run()?;
            }
            Self::Build(build) => {
                let shader_crate = &build.install.shader_crate;
                let mut command = from_cargo_metadata_with_config(shader_crate, build.as_ref())?;
                log::debug!("building with final merged arguments: {command:#?}");

                // When watching, do one normal run to setup the `manifest.json` file.
                if command.build.watch {
                    command.build.watch = false;
                    command.run()?;
                    command.build.watch = true;
                }

                command.run()?;
            }
            Self::Show(show) => show.run()?,
            Self::DumpUsage => dump_full_usage_for_readme()?,
        }

        Ok(())
    }
}

/// The struct representing the main CLI.
#[derive(clap::Parser)]
#[clap(author, version, about, subcommand_required = true)]
#[non_exhaustive]
pub struct Cli {
    /// The command to run.
    #[clap(subcommand)]
    pub command: Command,
}
