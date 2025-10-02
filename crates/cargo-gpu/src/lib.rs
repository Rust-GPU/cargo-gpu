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

pub use cargo_gpu_build::spirv_cache;

pub use self::spirv_cache::backend::Install;

use self::{
    build::Build,
    dump_usage::dump_full_usage_for_readme,
    show::Show,
    spirv_cache::{backend::InstallRunParams, toolchain::StdioCfg},
    user_consent::ask_for_user_consent,
};

pub mod build;
pub mod show;

mod config;
mod dump_usage;
mod linkage;
mod metadata;
mod test;
mod user_consent;

/// Central function to write to the user.
#[macro_export]
macro_rules! user_output {
    ($($args: tt)*) => { $crate::spirv_cache::user_output!(::std::io::stdout(), $($args)*) };
}

/// All of the available subcommands for `cargo gpu`
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
    #[clap(hide(true))]
    DumpUsage,
}

impl Command {
    /// Runs the command
    ///
    /// # Errors
    /// Any errors during execution, usually printed to the user
    #[inline]
    pub fn run(&self, env_args: Vec<String>) -> anyhow::Result<()> {
        match &self {
            Self::Install(install) => {
                let shader_crate_path = &install.shader_crate;
                let command =
                    config::Config::clap_command_with_cargo_config(shader_crate_path, env_args)?;
                log::debug!(
                    "installing with final merged arguments: {:#?}",
                    command.install
                );

                let skip_consent = command.install.auto_install_rust_toolchain;
                let halt = ask_for_user_consent(skip_consent);
                let install_params = InstallRunParams::default()
                    .writer(std::io::stdout())
                    .halt(halt)
                    .stdio_cfg(StdioCfg::inherit());
                command.install.backend.run(install_params)?;
            }
            Self::Build(build) => {
                let shader_crate_path = &build.install.backend.shader_crate;
                let mut command =
                    config::Config::clap_command_with_cargo_config(shader_crate_path, env_args)?;
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
