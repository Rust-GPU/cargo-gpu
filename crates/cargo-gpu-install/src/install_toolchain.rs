//! toolchain installation logic

use anyhow::Context as _;
#[cfg(feature = "tty")]
use crossterm::tty::IsTty as _;

use crate::user_output;

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
    let output_toolchain_list = std::process::Command::new("rustup")
        .args(["toolchain", "list"])
        .output()
        .context("running rustup command")?;
    anyhow::ensure!(
        output_toolchain_list.status.success(),
        "could not list installed toolchains"
    );
    let string_toolchain_list = String::from_utf8_lossy(&output_toolchain_list.stdout);
    if string_toolchain_list
        .split_whitespace()
        .any(|toolchain| toolchain.starts_with(channel))
    {
        log::debug!("toolchain {channel} is already installed");
    } else {
        let message = format!("Rust {channel} with `rustup`");
        get_consent_for_toolchain_install(
            format!("Install {message}").as_ref(),
            skip_toolchain_install_consent,
        )?;
        crate::user_output!("Installing {message}\n");

        let output_toolchain_add = std::process::Command::new("rustup")
            .args(["toolchain", "add"])
            .arg(channel)
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .output()
            .context("adding toolchain")?;
        anyhow::ensure!(
            output_toolchain_add.status.success(),
            "could not install required toolchain"
        );
    }

    // Check for the required components
    let output_component_list = std::process::Command::new("rustup")
        .args(["component", "list", "--toolchain"])
        .arg(channel)
        .output()
        .context("getting toolchain list")?;
    anyhow::ensure!(
        output_component_list.status.success(),
        "could not list installed components"
    );
    let string_component_list = String::from_utf8_lossy(&output_component_list.stdout);
    let required_components = ["rust-src", "rustc-dev", "llvm-tools"];
    let installed_components = string_component_list.lines().collect::<Vec<_>>();
    let all_components_installed = required_components.iter().all(|component| {
        installed_components.iter().any(|installed_component| {
            let is_component = installed_component.starts_with(component);
            let is_installed = installed_component.ends_with("(installed)");
            is_component && is_installed
        })
    });
    if all_components_installed {
        log::debug!("all required components are installed");
    } else {
        let message = "toolchain components [rust-src, rustc-dev, llvm-tools] with `rustup`";
        get_consent_for_toolchain_install(
            format!("Install {message}").as_ref(),
            skip_toolchain_install_consent,
        )?;
        crate::user_output!("Installing {message}\n");

        let output_component_add = std::process::Command::new("rustup")
            .args(["component", "add", "--toolchain"])
            .arg(channel)
            .args(["rust-src", "rustc-dev", "llvm-tools"])
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .output()
            .context("adding rustup component")?;
        anyhow::ensure!(
            output_component_add.status.success(),
            "could not install required components"
        );
    }

    Ok(())
}

#[cfg(not(feature = "tty"))]
/// Prompt user if they want to install a new Rust toolchain.
fn get_consent_for_toolchain_install(
    _prompt: &str,
    skip_toolchain_install_consent: bool,
) -> anyhow::Result<()> {
    if skip_toolchain_install_consent {
        Ok(())
    } else {
        no_tty()
    }
}

#[cfg(feature = "tty")]
/// Prompt user if they want to install a new Rust toolchain.
fn get_consent_for_toolchain_install(
    prompt: &str,
    skip_toolchain_install_consent: bool,
) -> anyhow::Result<()> {
    if skip_toolchain_install_consent {
        return Ok(());
    }

    if !std::io::stdout().is_tty() {
        no_tty()
    }

    log::debug!("asking for consent to install the required toolchain");
    crossterm::terminal::enable_raw_mode().context("enabling raw mode")?;
    crate::user_output!("{prompt} [y/n]: ");
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
        crate::user_output!("Exiting...\n");
        #[expect(clippy::exit, reason = "user requested abort")]
        std::process::exit(0);
    }
}

fn no_tty() -> ! {
    user_output!("No TTY detected so can't ask for consent to install Rust toolchain.");
    log::error!("Attempted to ask for consent when there's no TTY");
    #[expect(clippy::exit, reason = "can't ask for user consent if there's no TTY")]
    std::process::exit(1);
}
