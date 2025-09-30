//! User consent acquiring logic.

use std::io;

use cargo_gpu_build::spirv_cache::{
    command::CommandExecError,
    toolchain::{HaltToolchainInstallation, REQUIRED_TOOLCHAIN_COMPONENTS},
    user_output,
};
use crossterm::tty::IsTty as _;

/// Halts the installation process of toolchain or its required components
/// if the user does not consent to install either of them.
#[expect(
    clippy::type_complexity,
    reason = "it is impossible to create an alias for now"
)]
pub fn ask_for_user_consent(
    skip: bool,
) -> HaltToolchainInstallation<
    impl FnOnce(&str) -> Result<(), UserConsentError>,
    impl FnOnce(&str) -> Result<(), UserConsentError>,
> {
    let on_toolchain_install = move |channel: &str| {
        let message = format!("Rust {channel} with `rustup`");
        get_consent_for_toolchain_install(format!("Install {message}").as_ref(), skip)?;
        log::debug!("installing toolchain {channel}");
        user_output!(io::stdout(), "Installing {message}\n").map_err(UserConsentError::IoWrite)?;
        Ok(())
    };
    let on_components_install = move |channel: &str| {
        let message = format!(
            "components {REQUIRED_TOOLCHAIN_COMPONENTS:?} for toolchain {channel} with `rustup`"
        );
        get_consent_for_toolchain_install(format!("Install {message}").as_ref(), skip)?;
        log::debug!("installing required components of toolchain {channel}");
        user_output!(io::stdout(), "Installing {message}\n").map_err(UserConsentError::IoWrite)?;
        Ok(())
    };

    HaltToolchainInstallation {
        on_toolchain_install,
        on_components_install,
    }
}

/// Prompt user if they want to install a new Rust toolchain.
fn get_consent_for_toolchain_install(prompt: &str, skip: bool) -> Result<(), UserConsentError> {
    if skip {
        return Ok(());
    }

    if !io::stdout().is_tty() {
        log::error!("attempted to ask for consent when there's no TTY");
        return Err(UserConsentError::NoTTY);
    }

    log::debug!("asking for consent to install the required toolchain");
    crossterm::terminal::enable_raw_mode().map_err(UserConsentError::IoRead)?;
    user_output!(io::stdout(), "{prompt} [y/n]: ").map_err(UserConsentError::IoWrite)?;
    let mut input = crossterm::event::read().map_err(UserConsentError::IoRead)?;

    if let crossterm::event::Event::Key(crossterm::event::KeyEvent {
        code: crossterm::event::KeyCode::Enter,
        kind: crossterm::event::KeyEventKind::Release,
        ..
    }) = input
    {
        // In Powershell, programs will potentially observe the Enter key release after they started
        // (see crossterm#124). If that happens, re-read the input.
        input = crossterm::event::read().map_err(UserConsentError::IoRead)?;
    }
    crossterm::terminal::disable_raw_mode().map_err(UserConsentError::IoRead)?;

    if let crossterm::event::Event::Key(crossterm::event::KeyEvent {
        code: crossterm::event::KeyCode::Char('y'),
        ..
    }) = input
    {
        Ok(())
    } else {
        Err(UserConsentError::UserDenied)
    }
}

/// An error indicating that user consent were not acquired.
#[derive(Debug, thiserror::Error)]
pub enum UserConsentError {
    /// An error occurred while executing a command.
    #[error(transparent)]
    CommandExec(#[from] CommandExecError),
    /// No TTY detected, so can't ask for consent to install Rust toolchain.
    #[error("no TTY detected, so can't ask for consent to install Rust toolchain")]
    NoTTY,
    /// An I/O error occurred while reading user input.
    #[error("failed to read user input: {0}")]
    IoRead(#[source] io::Error),
    /// An I/O error occurred while writing user output.
    #[error("failed to write user output: {0}")]
    IoWrite(#[source] io::Error),
    /// User denied to install the required toolchain.
    #[error("user denied to install the required toolchain")]
    UserDenied,
}
