//! The `vars` command family. These read or edit machine-local variable state
//! and the dynamic-variable cache; only `vars list` consults the repository.
//!
//! The variants carry their arguments inline rather than in named structs
//! because none of them takes more than two.

use clap::Subcommand;

#[derive(Debug, Subcommand)]
pub(crate) enum VarsCommand {
    /// Set one persisted machine-local variable
    Set { key: String, value: String },

    /// Print the stored machine-local string for one variable
    Get { key: String },

    /// List the effective variables
    List {
        /// List only persisted machine-local variables
        #[arg(long)]
        machine_only: bool,

        /// Do not run dynamic commands or write the cache
        #[arg(long)]
        no_refresh: bool,
    },

    /// Remove one persisted machine-local variable
    Unset { key: String },

    /// Refresh the leaf repository's dynamic variables
    Refresh {
        /// Variables to refresh; all leaf dynamic variables when omitted
        #[arg(value_name = "KEY")]
        keys: Vec<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Command;
    use crate::cli::testing::{error_kind, parse};
    use clap::error::ErrorKind;

    fn vars(args: &[&str]) -> VarsCommand {
        let Command::Vars(command) = parse(args).command else {
            panic!("expected a vars command");
        };
        command
    }

    #[test]
    fn set_takes_a_key_and_a_value() {
        let VarsCommand::Set { key, value } = vars(&["batfiles", "vars", "set", "profile", "work"])
        else {
            panic!("expected vars set");
        };
        assert_eq!((key.as_str(), value.as_str()), ("profile", "work"));

        assert_eq!(
            error_kind(&["batfiles", "vars", "set", "profile"]),
            ErrorKind::MissingRequiredArgument
        );
    }

    #[test]
    fn get_and_unset_take_one_key() {
        assert!(matches!(
            vars(&["batfiles", "vars", "get", "profile"]),
            VarsCommand::Get { .. }
        ));
        assert!(matches!(
            vars(&["batfiles", "vars", "unset", "profile"]),
            VarsCommand::Unset { .. }
        ));
    }

    #[test]
    fn list_accepts_both_filters_together() {
        let VarsCommand::List {
            machine_only,
            no_refresh,
        } = vars(&["batfiles", "vars", "list", "--machine-only", "--no-refresh"])
        else {
            panic!("expected vars list");
        };
        assert!(machine_only && no_refresh);
    }

    #[test]
    fn refresh_defaults_to_every_leaf_variable() {
        let VarsCommand::Refresh { keys } = vars(&["batfiles", "vars", "refresh"]) else {
            panic!("expected vars refresh");
        };
        assert!(keys.is_empty());
    }
}
