//! The `facts` command family, which reads only the host.

use clap::Subcommand;

#[derive(Debug, Subcommand)]
pub(crate) enum FactsCommand {
    /// List the host facts that conditions and templates read as `facts`
    List,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Command;
    use crate::cli::testing::{error_kind, parse};
    use clap::error::ErrorKind;

    #[test]
    fn list_takes_no_arguments() {
        assert!(matches!(
            parse(&["batfiles", "facts", "list"]).command,
            Command::Facts(FactsCommand::List)
        ));
        assert_eq!(
            error_kind(&["batfiles", "facts", "list", "os"]),
            ErrorKind::UnknownArgument
        );
    }
}
