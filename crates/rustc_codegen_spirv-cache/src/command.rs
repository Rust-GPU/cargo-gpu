//! Utilities for executing [commands](Command).

use std::{
    io,
    process::{Command, Output},
};

/// An error indicating failure while executing some command.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
#[expect(clippy::module_name_repetitions, reason = "this is intended")]
pub enum CommandExecError {
    /// IO error occurred while calling some command.
    #[error("IO error occurred while calling `{command:?}`: {source}")]
    Io {
        /// The command which was called.
        command: Box<Command>,
        /// Source of the error.
        source: io::Error,
    },
    /// Result of calling some command was not successful.
    #[error("calling `{command:?}` was not successful")]
    ExecFail {
        /// The command which was called.
        command: Box<Command>,
        /// The output of called command.
        output: Output,
    },
}

impl CommandExecError {
    /// Creates [`Io`](CommandExecError::Io) variant from given arguments.
    fn io(command: impl Into<Command>, source: io::Error) -> Self {
        Self::Io {
            command: Box::new(command.into()),
            source,
        }
    }

    /// Creates [`ExecFail`](CommandExecError::ExecFail) variant from given arguments.
    fn exec_fail(command: impl Into<Command>, output: Output) -> Self {
        Self::ExecFail {
            command: Box::new(command.into()),
            output,
        }
    }

    /// Returns the command which was called.
    #[inline]
    #[expect(clippy::must_use_candidate, reason = "returns a reference")]
    pub fn command(&self) -> &Command {
        match self {
            Self::Io { command, .. } | Self::ExecFail { command, .. } => command.as_ref(),
        }
    }

    /// Converts self into the command which was called.
    #[inline]
    #[must_use]
    pub fn into_command(self) -> Command {
        match self {
            Self::Io { command, .. } | Self::ExecFail { command, .. } => *command,
        }
    }
}

/// Executes the command, returning its output.
#[expect(clippy::shadow_reuse, reason = "this is intended")]
pub(crate) fn execute_command(command: impl Into<Command>) -> Result<Output, CommandExecError> {
    let mut command = command.into();
    let output = match command.output() {
        Ok(output) => output,
        Err(source) => return Err(CommandExecError::io(command, source)),
    };
    if !output.status.success() {
        return Err(CommandExecError::exec_fail(command, output));
    }
    Ok(output)
}
