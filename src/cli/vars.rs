//! The `vars` command family. These read or edit machine-local variable state
//! and the dynamic-variable cache; `vars list`, `vars eval`, and `vars refresh`
//! also read the repository.

use clap::Subcommand;

use super::options::parse_var;
use crate::manifest::item::ItemId;
use crate::manifest::remote::Tree;
use crate::vars::dynamic::DynamicVarKey;
use crate::vars::name::VarName;

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

        /// Do not capture dynamic variables or write the cache
        #[arg(long)]
        no_refresh: bool,
    },

    /// Evaluate a template as a [vars] value would be, and print the result
    Eval {
        /// The template, such as `me@${profile}.example.com`
        template: String,

        /// Leave out the leaf's [vars]; read no repository
        #[arg(long)]
        no_manifest_vars: bool,

        /// Do not capture dynamic variables or write the cache
        #[arg(long)]
        no_refresh: bool,

        /// Set a one-shot variable; repeatable, last value for a key wins
        #[arg(long = "var", value_name = "KEY=VALUE", value_parser = parse_var)]
        vars: Vec<(VarName, String)>,
    },

    /// Remove one persisted machine-local variable
    Unset { key: String },

    /// Capture dynamic variables, even where the cached value is fresh
    Refresh {
        /// A leaf variable, or `<remote-id>.<name>`; every one in play when
        /// omitted
        #[arg(value_name = "KEY", value_parser = parse_key)]
        keys: Vec<DynamicVarKey>,
    },
}

/// Parse a leaf variable name or `<remote-id>.<name>`, splitting at the first `.`. Invalid
/// names return a usage-error message.
fn parse_key(raw: &str) -> Result<DynamicVarKey, String> {
    let (remote, name) = match raw.split_once('.') {
        Some((remote, name)) => (
            Some(ItemId::try_from(remote.to_owned()).map_err(|error| error.to_string())?),
            name,
        ),
        None => (None, raw),
    };
    let name = VarName::try_from(name.to_owned())
        .map_err(|error| format!("`{name}` is not a valid variable name: {error}"))?;
    Ok(DynamicVarKey {
        tree: Tree::from(remote),
        name,
    })
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
    fn eval_takes_a_template_and_one_shot_variables() {
        let VarsCommand::Eval {
            template,
            no_manifest_vars,
            no_refresh,
            vars: overrides,
        } = vars(&[
            "batfiles",
            "vars",
            "eval",
            "${profile}",
            "--var",
            "profile=work",
            "--no-manifest-vars",
            "--no-refresh",
        ])
        else {
            panic!("expected vars eval");
        };
        assert_eq!(template, "${profile}");
        assert!(no_manifest_vars && no_refresh);
        assert_eq!(overrides.len(), 1);

        assert_eq!(
            error_kind(&["batfiles", "vars", "eval"]),
            ErrorKind::MissingRequiredArgument
        );
        assert_eq!(
            error_kind(&["batfiles", "vars", "eval", "x", "--var", "1up=x"]),
            ErrorKind::ValueValidation
        );
    }

    fn keys(args: &[&str]) -> Vec<String> {
        let VarsCommand::Refresh { keys } = vars(args) else {
            panic!("expected vars refresh");
        };
        keys.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn refresh_takes_no_keys_or_leaf_and_remote_ones() {
        assert!(keys(&["batfiles", "vars", "refresh"]).is_empty());
        assert_eq!(
            keys(&["batfiles", "vars", "refresh", "email", "core.has_op"]),
            ["email", "core.has_op"]
        );
    }

    #[test]
    fn a_malformed_refresh_key_is_a_usage_error() {
        for key in [
            "remote:core.has_op",
            "core.has-op",
            "1up",
            "core.",
            ".email",
        ] {
            assert_eq!(
                error_kind(&["batfiles", "vars", "refresh", key]),
                ErrorKind::ValueValidation,
                "{key}"
            );
        }
    }
}
