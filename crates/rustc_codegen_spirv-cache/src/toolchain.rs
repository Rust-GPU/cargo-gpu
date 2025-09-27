//! This module deals with an installation of Rust toolchain required by `rust-gpu`
//! (and its components).

use std::{
    io,
    process::{Command, Output, Stdio},
};

/// Checks if the given toolchain is installed using `rustup`.
///
/// # Errors
///
/// Returns an error if any error occurs while using `rustup`.
#[inline]
pub fn is_toolchain_installed(channel: &str) -> Result<bool, RustupCommandError> {
    let mut command = Command::new("rustup");
    command.args(["toolchain", "list"]);

    let output = output(command)?;
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
pub fn install_toolchain(channel: &str) -> Result<(), RustupCommandError> {
    let mut command = Command::new("rustup");
    command
        .args(["toolchain", "add"])
        .arg(channel)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    let _output = output(command)?;
    Ok(())
}

/// Executes the command, returning its output.
fn output(mut command: Command) -> Result<Output, RustupCommandError> {
    let output = match command.output() {
        Ok(output) => output,
        Err(source) => return Err(RustupCommandError::io(command, source)),
    };
    if !output.status.success() {
        return Err(RustupCommandError::command_fail(command, output));
    }
    Ok(output)
}

/// An error indicating failure while using `rustup`.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RustupCommandError {
    /// IO error occurred while calling some `rustup` command.
    #[error("IO error occurred while calling `{command:?}`: {source}")]
    Io {
        /// The command which was called.
        command: Box<Command>,
        /// Source of the error.
        source: io::Error,
    },
    /// Result of calling some `rustup` command was not successful.
    #[error("calling `{command:?}` was not successful")]
    CommandFail {
        /// The command which was called.
        command: Box<Command>,
        /// The output of called command.
        output: Output,
    },
}

impl RustupCommandError {
    /// Creates [`Io`](RustupCommandError::Io) variant from given arguments.
    fn io(command: impl Into<Box<Command>>, source: io::Error) -> Self {
        Self::Io {
            command: command.into(),
            source,
        }
    }

    /// Creates [`CommandFail`](RustupCommandError::CommandFail) variant from given arguments.
    fn command_fail(command: impl Into<Box<Command>>, output: Output) -> Self {
        Self::CommandFail {
            command: command.into(),
            output,
        }
    }

    /// Returns the command which was called.
    #[inline]
    #[expect(clippy::must_use_candidate, reason = "returns a reference")]
    pub fn command(&self) -> &Command {
        match self {
            Self::Io { command, .. } | Self::CommandFail { command, .. } => command.as_ref(),
        }
    }

    /// Converts self into the command which was called.
    #[inline]
    #[must_use]
    pub fn into_command(self) -> Command {
        match self {
            Self::Io { command, .. } | Self::CommandFail { command, .. } => *command,
        }
    }
}
