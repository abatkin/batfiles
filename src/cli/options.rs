//! The option groups shared between several commands.

use clap::Args;

use crate::var::VarName;

/// Controls accepted by every command that executes actions.
///
/// `clone` accepts them because it forwards them to its follow-up
/// synchronization.
#[derive(Debug, Args)]
#[command(next_help_heading = "Action Execution Options")]
pub(crate) struct ActionOptions {
    /// Set a one-shot variable; repeatable, last value for a key wins
    #[arg(long = "var", value_name = "KEY=VALUE", value_parser = parse_var)]
    pub vars: Vec<(VarName, String)>,

    /// Recompute allowed dynamic variables even when cached values are fresh
    #[arg(long)]
    pub refresh_vars: bool,

    /// Refresh existing seed content
    #[arg(long)]
    pub refresh_content: bool,

    /// Skip unmanaged destination conflicts instead of backing them up
    #[arg(long, conflicts_with = "interactive")]
    pub no_overwrite: bool,

    /// Choose backup-and-replace, overwrite, or skip at each conflict
    #[arg(long)]
    pub interactive: bool,
}

/// Run-only selectors accepted by `sync` and `clone`.
#[derive(Debug, Args)]
#[command(next_help_heading = "Selection Options")]
pub(crate) struct SelectionOptions {
    /// Skip an action or addressable child for this run; repeatable
    #[arg(long = "skip-action", value_name = "ID")]
    pub skip_actions: Vec<String>,

    /// Skip a group for this run; repeatable
    #[arg(long = "skip-group", value_name = "GROUP")]
    pub skip_groups: Vec<String>,
}

/// Bootstrap-only enable/disable adoption, honored by `clone`.
#[derive(Debug, Args)]
#[command(next_help_heading = "Bootstrap Options")]
pub(crate) struct BootstrapOptions {
    /// Remove an action address from persisted disabled state; repeatable
    #[arg(long = "enable-action", value_name = "ID")]
    pub enable_actions: Vec<String>,

    /// Add an action address to persisted disabled state; repeatable
    #[arg(long = "disable-action", value_name = "ID")]
    pub disable_actions: Vec<String>,

    /// Remove a group address from persisted disabled state; repeatable
    #[arg(long = "enable-group", value_name = "GROUP")]
    pub enable_groups: Vec<String>,

    /// Add a group address to persisted disabled state; repeatable
    #[arg(long = "disable-group", value_name = "GROUP")]
    pub disable_groups: Vec<String>,
}

/// Split `KEY=VALUE` and validate the key. An empty value is significant.
///
/// The name rule is applied **here**, in the value parser, rather than wherever
/// the layers are merged: a key typed for this invocation is worth failing on,
/// and failing in clap means an invalid one is a usage error raised before the
/// location roots are resolved and before any file is opened — which is what
/// `docs/cmdline.md` promises. Merging validates nothing, because by then
/// `vars.toml` has already been read off disk.
///
/// The shape check stays first, so `--var profile` reports the missing `=`
/// rather than a name-rule complaint about the whole argument.
fn parse_var(raw: &str) -> Result<(VarName, String), String> {
    match raw.split_once('=') {
        Some((key, value)) if !key.is_empty() => match VarName::new(key) {
            Ok(key) => Ok((key, value.to_owned())),
            Err(error) => Err(format!("`{key}` is not a valid variable name: {error}")),
        },
        _ => Err(format!("expected `KEY=VALUE`, found `{raw}`")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(text: &str) -> VarName {
        VarName::new(text).expect("valid name")
    }

    #[test]
    fn a_var_splits_at_the_first_equals_sign() {
        assert_eq!(parse_var("a=b=c"), Ok((name("a"), "b=c".to_owned())));
    }

    #[test]
    fn an_empty_var_value_is_significant() {
        assert_eq!(parse_var("profile="), Ok((name("profile"), String::new())));
    }

    #[test]
    fn a_var_needs_a_key_and_an_equals_sign() {
        assert!(parse_var("profile").is_err());
        assert!(parse_var("=work").is_err());
    }

    #[test]
    fn a_var_key_must_be_a_valid_variable_name() {
        let error = parse_var("1up=x").expect_err("names do not start with a digit");
        assert!(
            error.contains("`1up` is not a valid variable name"),
            "{error}"
        );
        assert!(error.contains("must start with a letter"), "{error}");

        let error = parse_var("env=x").expect_err("reserved");
        assert!(error.contains("reserved"), "{error}");
    }

    #[test]
    fn the_shape_is_checked_before_the_name() {
        // `--var profile` is a missing `=`, not a variable named `profile`
        // that broke a rule, and the message has to say the former.
        let error = parse_var("profile").expect_err("no equals sign");
        assert_eq!(error, "expected `KEY=VALUE`, found `profile`");
    }
}
