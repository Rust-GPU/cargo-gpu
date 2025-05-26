//! main executable of cargo gpu
use cargo_gpu::Cli;
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
    let cli = Cli::parse_from(&env_args);
    cli.command.run(env_args)
}
