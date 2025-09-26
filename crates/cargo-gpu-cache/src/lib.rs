#![expect(clippy::pub_use, reason = "pub use for build scripts")]

//! Rust GPU shader crate builder.
//!
//! This library allows you to easily compile your rust-gpu shaders,
//! without requiring you to fix your entire project to a specific toolchain.
//!
//! # How it works
//!
//! This library manages installations of `rustc_codegen_spirv`
//! using rust-gpu's [`rustc_codegen_spirv-cache`](rustc_codegen_spirv_cache) crate.
//!
//! Then we continue to use rust-gpu's [`spirv-builder`](spirv_builder) crate
//! to pass the many additional parameters required to configure rustc and our codegen backend,
//! but provide you with a toolchain agnostic version that you may use from stable rustc.
//! And a `cargo gpu` command line utility to simplify shader building even more.
//!
//! ## How we build the backend
//!
//! * retrieve the version of rust-gpu you want to use based on the version of the
//!   `spirv-std` dependency in your shader crate.
//! * create a dummy project at `<cache_dir>/codegen/<version>/` that depends on
//!   `rustc_codegen_spirv`
//! * use `cargo metadata` to `cargo update` the dummy project, which downloads the
//!   `rustc_codegen_spirv` crate into cargo's cache, and retrieve the path to the
//!   download location.
//! * search for the required toolchain in `build.rs` of `rustc_codegen_spirv`
//! * build it with the required toolchain version
//! * copy out the binary and clean the target dir
//!
//! ## Building shader crates
//!
//! `cargo-gpu` takes a path to a shader crate to build, as well as a path to a directory
//! to put the compiled `spv` source files. It also takes a path to an output manifest
//! file where all shader entry points will be mapped to their `spv` source files. This
//! manifest file can be used by build scripts (`build.rs` files) to generate linkage or
//! conduct other post-processing, like converting the `spv` files into `wgsl` files,
//! for example.

use build::Build;
use show::Show;

#[cfg(feature = "clap")]
use crate::dump_usage::dump_full_usage_for_readme;

mod build;
mod config;
mod dump_usage;
mod install;
mod install_toolchain;
mod linkage;
mod lockfile;
mod metadata;
mod show;
mod spirv_source;
mod target_specs;
mod test;

pub use install::*;
pub use spirv_builder;

/// Writes formatted user output into a [writer](std::io::Write).
#[macro_export]
macro_rules! write_user_output {
    ($dst:expr, $($args:tt)*) => {{
        #[allow(
            clippy::allow_attributes,
            clippy::useless_attribute,
            unused_imports,
            reason = "`std::io::Write` is only sometimes called??"
        )]
        use ::std::io::Write as _;

        let mut writer = $dst;
        #[expect(clippy::non_ascii_literal, reason = "CRAB GOOD. CRAB IMPORTANT.")]
        ::std::write!(writer, "🦀 ")
            .and_then(|()| ::std::write!(writer, $($args)*))
            .and_then(|()| ::std::io::Write::flush(&mut writer))
    }};
}

/// Central function to write to the user.
#[macro_export]
macro_rules! user_output {
    ($($args: tt)*) => { $crate::write_user_output!(::std::io::stdout(), $($args)*) };
}

/// All of the available subcommands for `cargo gpu`
#[cfg_attr(feature = "clap", derive(clap::Subcommand))]
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
    #[cfg(feature = "clap")]
    #[clap(hide(true))]
    DumpUsage,
}

impl Command {
    /// Runs the command
    ///
    /// # Errors
    /// Any errors during execution, usually printed to the user
    #[inline]
    #[cfg(feature = "clap")]
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
                command.install.run()?;
            }
            Self::Build(build) => {
                let shader_crate_path = &build.install.shader_crate;
                let mut command =
                    config::Config::clap_command_with_cargo_config(shader_crate_path, env_args)?;
                log::debug!("building with final merged arguments: {command:#?}");

                // When watching, do one normal run to setup the `manifest.json` file.
                #[cfg(feature = "watch")]
                if command.build.watch {
                    command.build.watch = false;
                    command.run()?;
                    command.build.watch = true;
                }

                command.run()?;
            }
            Self::Show(show) => show.run()?,
            #[cfg(feature = "clap")]
            Self::DumpUsage => dump_full_usage_for_readme()?,
        }

        Ok(())
    }
}

/// the Cli struct representing the main cli
#[cfg_attr(feature = "clap", derive(clap::Parser))]
#[cfg_attr(
    feature = "clap",
    clap(author, version, about, subcommand_required = true)
)]
#[non_exhaustive]
pub struct Cli {
    /// The command to run.
    #[cfg_attr(feature = "clap", clap(subcommand))]
    pub command: Command,
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
