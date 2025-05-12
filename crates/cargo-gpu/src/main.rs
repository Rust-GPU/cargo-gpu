//! the main of the `cargo gpu` executable

use cargo_gpu::config::Config;
use cargo_gpu::{user_output, Cli, Command};
use clap::Parser;

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
pub fn run() -> anyhow::Result<()> {
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
            let shader_crate_path = install.shader_crate;
            let mut command = Config::clap_command_with_cargo_config(&shader_crate_path, env_args)?;
            log::debug!(
                "installing with final merged arguments: {:#?}",
                command.install
            );
            command.install.run()?;
        }
        Command::Build(build) => {
            let shader_crate_path = build.install.shader_crate;
            let mut command = Config::clap_command_with_cargo_config(&shader_crate_path, env_args)?;
            log::debug!("building with final merged arguments: {command:#?}");

            if command.build.watch {
                //  When watching, do one normal run to setup the `manifest.json` file.
                command.build.watch = false;
                command.run()?;
                command.build.watch = true;
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
