use std::{error::Error, ffi::OsString, fmt};

/// Selects one explicit operating mode for the `FleetIQ` process.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessCommand {
    /// Run the long-lived HTTP backend.
    Serve,
    /// Establish the installation's first platform operator and exit.
    BootstrapPlatformOperator,
    /// Print command usage without reading runtime configuration.
    Help,
}

impl ProcessCommand {
    /// Parses the process arguments after the executable name.
    ///
    /// No argument preserves the original server-start behavior. The explicit
    /// `serve` alias supports deployment manifests that prefer a named command.
    /// Bootstrap accepts no inline identity values so the external subject is
    /// not exposed through the process list.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessCommandError`] for an unknown, non-Unicode, or
    /// additional argument.
    pub fn parse(
        arguments: impl IntoIterator<Item = OsString>,
    ) -> Result<Self, ProcessCommandError> {
        let mut arguments = arguments.into_iter();
        let Some(command) = arguments.next() else {
            return Ok(Self::Serve);
        };
        let command = command
            .into_string()
            .map_err(|_| ProcessCommandError::NonUnicodeArgument)?;
        let parsed = match command.as_str() {
            "serve" => Self::Serve,
            "bootstrap-platform-operator" => Self::BootstrapPlatformOperator,
            "help" | "--help" | "-h" => Self::Help,
            _ => return Err(ProcessCommandError::UnknownCommand),
        };
        if arguments.next().is_some() {
            return Err(ProcessCommandError::UnexpectedArgument);
        }
        Ok(parsed)
    }

    /// Returns bounded command usage containing no environment values.
    pub const fn usage() -> &'static str {
        "Usage: fleetiq-server [serve|bootstrap-platform-operator|help]\n\
         bootstrap-platform-operator requires DATABASE_URL, \
         FLEETIQ_BOOTSTRAP_ACTOR_ID, FLEETIQ_BOOTSTRAP_AUTHORITY_ID, and \
         FLEETIQ_BOOTSTRAP_EXTERNAL_SUBJECT"
    }
}

/// Invalid process command-line input.
#[derive(Debug, Eq, PartialEq)]
pub enum ProcessCommandError {
    /// An argument could not be represented as Unicode command text.
    NonUnicodeArgument,
    /// The selected command is not supported.
    UnknownCommand,
    /// The selected command does not accept another argument.
    UnexpectedArgument,
}

impl fmt::Display for ProcessCommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonUnicodeArgument => {
                formatter.write_str("process argument is not valid Unicode")
            }
            Self::UnknownCommand => formatter.write_str("unknown process command"),
            Self::UnexpectedArgument => formatter.write_str("unexpected process argument"),
        }
    }
}

impl Error for ProcessCommandError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_argument_and_explicit_serve_select_the_server() {
        assert_eq!(
            ProcessCommand::parse(Vec::<OsString>::new()).expect("default command should parse"),
            ProcessCommand::Serve
        );
        assert_eq!(
            ProcessCommand::parse([OsString::from("serve")]).expect("explicit serve should parse"),
            ProcessCommand::Serve
        );
    }

    #[test]
    fn bootstrap_and_help_are_explicit_modes() {
        assert_eq!(
            ProcessCommand::parse([OsString::from("bootstrap-platform-operator")])
                .expect("bootstrap command should parse"),
            ProcessCommand::BootstrapPlatformOperator
        );
        for argument in ["help", "--help", "-h"] {
            assert_eq!(
                ProcessCommand::parse([OsString::from(argument)])
                    .expect("help command should parse"),
                ProcessCommand::Help
            );
        }
    }

    #[test]
    fn unknown_and_additional_arguments_are_rejected() {
        assert_eq!(
            ProcessCommand::parse([OsString::from("unknown")]),
            Err(ProcessCommandError::UnknownCommand)
        );
        assert_eq!(
            ProcessCommand::parse([
                OsString::from("bootstrap-platform-operator"),
                OsString::from("subject-must-not-be-inline"),
            ]),
            Err(ProcessCommandError::UnexpectedArgument)
        );
    }
}
