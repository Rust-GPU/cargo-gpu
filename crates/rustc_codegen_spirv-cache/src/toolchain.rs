//! This module deals with an installation of Rust toolchain required by `rust-gpu`
//! (and all of its [required components](REQUIRED_TOOLCHAIN_COMPONENTS)).

use std::process::{Command, Stdio};

use crate::command::{execute_command, CommandExecError};

/// Allows to halt the installation process of toolchain or its [required components](REQUIRED_TOOLCHAIN_COMPONENTS).
#[derive(Debug, Clone, Copy)]
#[expect(clippy::exhaustive_structs, reason = "intended to be exhaustive")]
pub struct HaltToolchainInstallation<T, C> {
    /// Closure which is called to halt the installation process of toolchain.
    pub on_toolchain_install: T,
    /// Closure which is called to halt the installation process of required toolchain components.
    pub on_components_install: C,
}

/// Type of [`HaltToolchainInstallation`] which does nothing.
// FIXME: replace `fn` with `impl FnOnce` once it's stabilized
pub type NoopHaltToolchainInstallation = HaltToolchainInstallation<
    fn(&str) -> Result<(), CommandExecError>,
    fn(&str) -> Result<(), CommandExecError>,
>;

impl NoopHaltToolchainInstallation {
    /// Do not halt the installation process of toolchain or its [required components](REQUIRED_TOOLCHAIN_COMPONENTS).
    ///
    /// Calling either [`on_toolchain_install`] or [`on_components_install`]
    /// returns [`Ok`] without any side effects.
    ///
    /// [`on_toolchain_install`]: HaltToolchainInstallation::on_toolchain_install
    /// [`on_components_install`]: HaltToolchainInstallation::on_components_install
    #[inline]
    #[expect(clippy::must_use_candidate, reason = "contains no state")]
    pub fn noop() -> Self {
        Self {
            on_toolchain_install: |_: &str| Ok(()),
            on_components_install: |_: &str| Ok(()),
        }
    }
}

/// Uses `rustup` to install the toolchain and all the [required components](REQUIRED_TOOLCHAIN_COMPONENTS),
/// if not already installed.
///
/// Pretty much runs:
///
/// ```text
/// rustup toolchain add nightly-2024-04-24
/// rustup component add --toolchain nightly-2024-04-24 rust-src rustc-dev llvm-tools
/// ```
///
/// where `nightly-2024-04-24` is an example of a toolchain
/// provided as an argument to this function.
///
/// The second parameter allows you to halt the installation process
/// of toolchain or its required components.
///
/// # Errors
///
/// Returns an error if any error occurs while using `rustup`
/// or the installation process was halted.
#[inline]
pub fn ensure_toolchain_installation<E, T, C>(
    channel: &str,
    halt_installation: HaltToolchainInstallation<T, C>,
) -> Result<(), E>
where
    E: From<CommandExecError>,
    T: FnOnce(&str) -> Result<(), E>,
    C: FnOnce(&str) -> Result<(), E>,
{
    let HaltToolchainInstallation {
        on_toolchain_install,
        on_components_install,
    } = halt_installation;

    if is_toolchain_installed(channel)? {
        log::debug!("toolchain {channel} is already installed");
    } else {
        log::debug!("toolchain {channel} is not installed yet");
        on_toolchain_install(channel)?;
        install_toolchain(channel)?;
    }

    if all_required_toolchain_components_installed(channel)? {
        log::debug!("all required components of toolchain {channel} are installed");
    } else {
        log::debug!("not all required components of toolchain {channel} are installed yet");
        on_components_install(channel)?;
        install_required_toolchain_components(channel)?;
    }

    Ok(())
}

/// Checks if the given toolchain is installed using `rustup`.
///
/// # Errors
///
/// Returns an error if any error occurs while using `rustup`.
#[inline]
pub fn is_toolchain_installed(channel: &str) -> Result<bool, CommandExecError> {
    let mut command = Command::new("rustup");
    command.args(["toolchain", "list"]);
    let output = execute_command(command)?;

    let toolchain_list = String::from_utf8_lossy(&output.stdout);
    let installed = toolchain_list
        .split_whitespace()
        .any(|toolchain| toolchain.starts_with(channel));
    Ok(installed)
}

/// Installs the given toolchain using `rustup`.
///
/// # Errors
///
/// Returns an error if any error occurs while using `rustup`.
#[inline]
#[expect(clippy::module_name_repetitions, reason = "this is intended")]
pub fn install_toolchain(channel: &str) -> Result<(), CommandExecError> {
    let mut command = Command::new("rustup");
    command
        .args(["toolchain", "add"])
        .arg(channel)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    let _output = execute_command(command)?;

    Ok(())
}

/// Components which are required to be installed for a toolchain to be usable with `rust-gpu`.
pub const REQUIRED_TOOLCHAIN_COMPONENTS: [&str; 3] = ["rust-src", "rustc-dev", "llvm-tools"];

/// Checks if all the [required components](REQUIRED_TOOLCHAIN_COMPONENTS)
/// of the given toolchain are installed using `rustup`.
///
/// # Errors
///
/// Returns an error if any error occurs while using `rustup`.
#[inline]
pub fn all_required_toolchain_components_installed(
    channel: &str,
) -> Result<bool, CommandExecError> {
    let mut command = Command::new("rustup");
    command
        .args(["component", "list", "--toolchain"])
        .arg(channel);
    let output = execute_command(command)?;

    let component_list = String::from_utf8_lossy(&output.stdout);
    let component_list_lines = component_list.lines().collect::<Vec<_>>();
    let installed = REQUIRED_TOOLCHAIN_COMPONENTS.iter().all(|component| {
        component_list_lines
            .iter()
            .any(|maybe_installed_component| {
                let is_component = maybe_installed_component.starts_with(component);
                let is_installed = maybe_installed_component.ends_with("(installed)");
                is_component && is_installed
            })
    });
    Ok(installed)
}

/// Installs all the [required components](REQUIRED_TOOLCHAIN_COMPONENTS)
/// for the given toolchain using `rustup`.
///
/// # Errors
///
/// Returns an error if any error occurs while using `rustup`.
#[inline]
pub fn install_required_toolchain_components(channel: &str) -> Result<(), CommandExecError> {
    let mut command = Command::new("rustup");
    command
        .args(["component", "add", "--toolchain"])
        .arg(channel)
        .args(REQUIRED_TOOLCHAIN_COMPONENTS)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    let _output = execute_command(command)?;

    Ok(())
}
