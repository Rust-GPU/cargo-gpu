//! Rust GPU shader crate builder.
//!
//! This program and library allows you to easily compile your rust-gpu shaders,
//! without requiring you to fix your entire project to a specific toolchain.
//!
//! # How it works
//!
//! This program primarily manages installations of `rustc_codegen_spirv`, the
//! codegen backend of rust-gpu to generate SPIR-V shader binaries. The codegen
//! backend builds on internal, ever-changing interfaces of rustc, which requires
//! fixing a version of rust-gpu to a specific version of the rustc compiler.
//! Usually, this would require you to fix your entire project to that specific
//! toolchain, but this project loosens that requirement by managing installations
//! of `rustc_codegen_spirv` and their associated toolchains for you.
//!
//! We continue to use rust-gpu's `spirv_builder` crate to pass the many additional
//! parameters required to configure rustc and our codegen backend, but provide you
//! with a toolchain agnostic version that you may use from stable rustc. And a
//! `cargo gpu` cmdline utility to simplify shader building even more.
//!
//! ## Where the binaries are
//!
//! We store our prebuild `rustc_spirv_builder` binaries in the default cache
//! directory of your OS:
//! * Windows: `C:/users/<user>/AppData/Local/rust-gpu`
//! * Mac: `~/Library/Caches/rust-gpu`
//! * Linux: `~/.cache/rust-gpu`
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

use anyhow::Context as _;

use build::Build;
use clap::Parser as _;
use install::Install;
use show::Show;

mod args;
mod build;
mod config;
mod install;
mod install_toolchain;
mod linkage;
mod lockfile;
mod metadata;
mod show;
mod spirv_source;

/// Central function to write to the user.
#[macro_export]
macro_rules! user_output {
    ($($args: tt)*) => {
        #[allow(
            clippy::allow_attributes,
            clippy::useless_attribute,
            unused_imports,
            reason = "`std::io::Write` is only sometimes called??"
        )]
        use std::io::Write as _;

        #[expect(
            clippy::non_ascii_literal,
            reason = "CRAB GOOD. CRAB IMPORTANT."
        )]
        {
            print!("🦀 ");
        }
        print!($($args)*);
        std::io::stdout().flush().unwrap();
   }
}

fn main() {
    #[cfg(debug_assertions)]
    std::env::set_var("RUST_BACKTRACE", "1");

    env_logger::builder().init();

    if let Err(error) = run() {
        log::error!("{error:?}");

        #[expect(
            clippy::print_stderr,
            reason = "Our central place for outputting error messages"
        )]
        {
            eprintln!("Error: {error}");

            // `clippy::exit` seems to be a false positive in `main()`.
            // See: https://github.com/rust-lang/rust-clippy/issues/13518
            #[expect(clippy::restriction, reason = "Our central place for safely exiting")]
            std::process::exit(1);
        };
    }
}

/// Wrappable "main" to catch errors.
fn run() -> anyhow::Result<()> {
    let env_args = std::env::args()
        .filter(|arg| {
            // Calling our `main()` with the cargo subcommand `cargo gpu` passes "gpu"
            // as the first parameter, so we want to ignore it.
            arg != "gpu"
        })
        .collect::<Vec<_>>();
    log::trace!("CLI args: {env_args:#?}");
    let cli = Cli::parse_from(env_args.clone());

    match cli.command {
        Command::Install(install) => {
            let shader_crate_path = install.spirv_install.shader_crate;
            let mut command =
                config::Config::clap_command_with_cargo_config(&shader_crate_path, env_args)?;
            log::debug!(
                "installing with final merged arguments: {:#?}",
                command.install
            );
            command.install.run()?;
        }
        Command::Build(build) => {
            let shader_crate_path = build.install.spirv_install.shader_crate;
            let mut command =
                config::Config::clap_command_with_cargo_config(&shader_crate_path, env_args)?;
            log::debug!("building with final merged arguments: {command:#?}");

            if command.build_args.watch {
                //  When watching, do one normal run to setup the `manifest.json` file.
                command.build_args.watch = false;
                command.run()?;
                command.build_args.watch = true;
                command.run()?;
            } else {
                command.run()?;
            }
        }
        Command::Show(show) => show.run()?,
        Command::DumpUsage => dump_full_usage_for_readme()?,
    }

    Ok(())
}

/// All of the available subcommands for `cargo gpu`
#[derive(clap::Subcommand)]
enum Command {
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

#[derive(clap::Parser)]
#[clap(author, version, about, subcommand_required = true)]
pub(crate) struct Cli {
    /// The command to run.
    #[clap(subcommand)]
    command: Command,
}

fn cache_dir() -> anyhow::Result<std::path::PathBuf> {
    let dir = directories::BaseDirs::new()
        .with_context(|| "could not find the user home directory")?
        .cache_dir()
        .join("rust-gpu");

    Ok(if cfg!(test) {
        let thread_id = std::thread::current().id();
        let id = format!("{thread_id:?}").replace('(', "-").replace(')', "");
        dir.join("tests").join(id)
    } else {
        dir
    })
}

/// Location of the target spec metadata files
fn target_spec_dir() -> anyhow::Result<std::path::PathBuf> {
    let dir = cache_dir()?.join("target-specs");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Convenience function for internal use. Dumps all the CLI usage instructions. Useful for
/// updating the README.
fn dump_full_usage_for_readme() -> anyhow::Result<()> {
    use clap::CommandFactory as _;
    let mut command = Cli::command();

    let mut buffer: Vec<u8> = Vec::default();
    command.build();

    write_help(&mut buffer, &mut command, 0)?;
    user_output!("{}", String::from_utf8(buffer)?);

    Ok(())
}

/// Recursive function to print the usage instructions for each subcommand.
fn write_help(
    buffer: &mut impl std::io::Write,
    cmd: &mut clap::Command,
    depth: usize,
) -> anyhow::Result<()> {
    if cmd.get_name() == "help" {
        return Ok(());
    }

    let mut command = cmd.get_name().to_owned();
    let indent_depth = if depth == 0 || depth == 1 { 0 } else { depth };
    let indent = " ".repeat(indent_depth * 4);
    writeln!(
        buffer,
        "\n{}* {}{}",
        indent,
        command.remove(0).to_uppercase(),
        command
    )?;

    for line in cmd.render_long_help().to_string().lines() {
        writeln!(buffer, "{indent}  {line}")?;
    }

    for sub in cmd.get_subcommands_mut() {
        writeln!(buffer)?;
        write_help(buffer, sub, depth + 1)?;
    }

    Ok(())
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
    use crate::cache_dir;
    use std::io::Write as _;

    fn copy_dir_all(
        src: impl AsRef<std::path::Path>,
        dst: impl AsRef<std::path::Path>,
    ) -> anyhow::Result<()> {
        std::fs::create_dir_all(&dst)?;
        for maybe_entry in std::fs::read_dir(src)? {
            let entry = maybe_entry?;
            let ty = entry.file_type()?;
            if ty.is_dir() {
                copy_dir_all(entry.path(), dst.as_ref().join(entry.file_name()))?;
            } else {
                std::fs::copy(entry.path(), dst.as_ref().join(entry.file_name()))?;
            }
        }
        Ok(())
    }

    pub fn shader_crate_template_path() -> std::path::PathBuf {
        let project_base = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        project_base.join("../shader-crate-template")
    }

    pub fn shader_crate_test_path() -> std::path::PathBuf {
        let shader_crate_path = crate::cache_dir().unwrap().join("shader_crate");
        copy_dir_all(shader_crate_template_path(), shader_crate_path.clone()).unwrap();
        shader_crate_path
    }

    pub fn overwrite_shader_cargo_toml(shader_crate_path: &std::path::Path) -> std::fs::File {
        let cargo_toml = shader_crate_path.join("Cargo.toml");
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(cargo_toml)
            .unwrap();
        writeln!(file, "[package]").unwrap();
        writeln!(file, "name = \"test\"").unwrap();
        file
    }

    pub fn tests_teardown() {
        let cache_dir = cache_dir().unwrap();
        if !cache_dir.exists() {
            return;
        }
        std::fs::remove_dir_all(cache_dir).unwrap();
    }
}
