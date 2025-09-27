//! toolchain installation logic

use std::process::{Command, Stdio};

use anyhow::Context as _;
use crossterm::tty::IsTty as _;
use rustc_codegen_spirv_cache::toolchain::{install_toolchain, is_toolchain_installed};

use crate::user_output;

/// Components which are required to be installed for a toolchain to be usable with `rust-gpu`.
pub const REQUIRED_TOOLCHAIN_COMPONENTS: [&str; 3] = ["rust-src", "rustc-dev", "llvm-tools"];

/// Checks if all the required components of the given toolchain are installed using `rustup`.
pub fn all_required_toolchain_components_installed(channel: &str) -> anyhow::Result<bool> {
    let output_component_list = Command::new("rustup")
        .args(["component", "list", "--toolchain"])
        .arg(channel)
        .output()
        .context("getting toolchain list")?;
    anyhow::ensure!(
        output_component_list.status.success(),
        "could not list installed components"
    );

    let string_component_list = String::from_utf8_lossy(&output_component_list.stdout);
    let installed_components = string_component_list.lines().collect::<Vec<_>>();
    let all_components_installed = REQUIRED_TOOLCHAIN_COMPONENTS.iter().all(|component| {
        installed_components.iter().any(|installed_component| {
            let is_component = installed_component.starts_with(component);
            let is_installed = installed_component.ends_with("(installed)");
            is_component && is_installed
        })
    });
    Ok(all_components_installed)
}

/// Installs all the required components for the given toolchain using `rustup`.
pub fn install_required_toolchain_components(channel: &str) -> anyhow::Result<()> {
    let output_component_add = Command::new("rustup")
        .args(["component", "add", "--toolchain"])
        .arg(channel)
        .args(REQUIRED_TOOLCHAIN_COMPONENTS)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .output()
        .context("adding rustup component")?;
    anyhow::ensure!(
        output_component_add.status.success(),
        "could not install required components"
    );
    Ok(())
}

/// Use `rustup` to install the toolchain and components, if not already installed.
///
/// Pretty much runs:
///
/// * rustup toolchain add nightly-2024-04-24
/// * rustup component add --toolchain nightly-2024-04-24 rust-src rustc-dev llvm-tools
pub fn ensure_toolchain_and_components_exist(
    channel: &str,
    skip_toolchain_install_consent: bool,
) -> anyhow::Result<()> {
    // Check for the required toolchain
    if is_toolchain_installed(channel)? {
        log::debug!("toolchain {channel} is already installed");
    } else {
        let message = format!("Rust {channel} with `rustup`");
        get_consent_for_toolchain_install(
            format!("Install {message}").as_ref(),
            skip_toolchain_install_consent,
        )?;
        user_output!("Installing {message}\n")?;
        install_toolchain(channel)?;
    }

    // Check for the required components
    if all_required_toolchain_components_installed(channel)? {
        log::debug!("all required components of toolchain {channel} are installed");
    } else {
        let message = format!(
            "components {REQUIRED_TOOLCHAIN_COMPONENTS:?} for toolchain {channel} with `rustup`"
        );
        get_consent_for_toolchain_install(
            format!("Install {message}").as_ref(),
            skip_toolchain_install_consent,
        )?;
        user_output!("Installing {message}\n")?;
        install_required_toolchain_components(channel)?;
    }

    Ok(())
}

/// Prompt user if they want to install a new Rust toolchain.
fn get_consent_for_toolchain_install(
    prompt: &str,
    skip_toolchain_install_consent: bool,
) -> anyhow::Result<()> {
    if skip_toolchain_install_consent {
        return Ok(());
    }

    if !std::io::stdout().is_tty() {
        log::error!("Attempted to ask for consent when there's no TTY");
        anyhow::bail!("no TTY detected, so can't ask for consent to install Rust toolchain")
    }

    log::debug!("asking for consent to install the required toolchain");
    crossterm::terminal::enable_raw_mode().context("enabling raw mode")?;
    user_output!("{prompt} [y/n]: ")?;
    let mut input = crossterm::event::read().context("reading crossterm event")?;

    if let crossterm::event::Event::Key(crossterm::event::KeyEvent {
        code: crossterm::event::KeyCode::Enter,
        kind: crossterm::event::KeyEventKind::Release,
        ..
    }) = input
    {
        // In Powershell, programs will potentially observe the Enter key release after they started
        // (see crossterm#124). If that happens, re-read the input.
        input = crossterm::event::read().context("re-reading crossterm event")?;
    }
    crossterm::terminal::disable_raw_mode().context("disabling raw mode")?;

    if let crossterm::event::Event::Key(crossterm::event::KeyEvent {
        code: crossterm::event::KeyCode::Char('y'),
        ..
    }) = input
    {
        Ok(())
    } else {
        anyhow::bail!("user denied to install the required toolchain\n");
    }
}
