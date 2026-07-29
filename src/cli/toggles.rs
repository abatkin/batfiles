//! The persistent enable/disable commands. They edit the machine-local
//! disabled lists only: they run no synchronization and remove no installed
//! content.

use clap::Args;

/// Addresses for `disable-action` and `enable-action`.
#[derive(Debug, Args)]
pub(crate) struct ActionAddresses {
    /// Action addresses
    #[arg(value_name = "ID", required = true)]
    pub ids: Vec<String>,
}

/// Addresses for `disable-group` and `enable-group`.
#[derive(Debug, Args)]
pub(crate) struct GroupAddresses {
    /// Group addresses
    #[arg(value_name = "GROUP", required = true)]
    pub groups: Vec<String>,
}

#[cfg(test)]
mod tests {
    use crate::cli::Command;
    use crate::cli::testing::{error_kind, parse};
    use clap::error::ErrorKind;

    #[test]
    fn one_or_more_addresses_are_accepted() {
        let cli = parse(&["batfiles", "disable-action", "git.clone", "vim"]);
        let Command::DisableAction(args) = cli.command else {
            panic!("expected disable-action");
        };
        assert_eq!(args.ids, vec!["git.clone".to_owned(), "vim".to_owned()]);
    }

    #[test]
    fn at_least_one_address_is_required() {
        for command in [
            "disable-action",
            "enable-action",
            "disable-group",
            "enable-group",
        ] {
            assert_eq!(
                error_kind(&["batfiles", command]),
                ErrorKind::MissingRequiredArgument,
                "{command} should require an address"
            );
        }
    }
}
