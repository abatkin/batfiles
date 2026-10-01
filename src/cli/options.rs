//! Shared command options and `--var` parsing. Invalid variable keys are usage errors; invalid
//! skip names are handled during selection.

use clap::Args;

use crate::var::VarName;

/// Options shared by commands that execute actions. `--interactive` conflicts with `--dry-run`.
#[derive(Debug, Args)]
#[command(next_help_heading = "Action Execution Options")]
pub(crate) struct ExecutionOptions {
    /// Set a one-shot variable; repeatable, last value for a key wins
    #[arg(long = "var", value_name = "KEY=VALUE", value_parser = parse_var)]
    pub vars: Vec<(VarName, String)>,

    /// Recompute allowed dynamic variables even when cached values are fresh
    #[arg(long)]
    pub refresh_vars: bool,

    /// Copy and fetch seeds again over what is already at their destinations
    #[arg(long)]
    pub refresh_content: bool,

    /// Skip whatever is in a destination's way instead of backing it up and
    /// replacing it
    #[arg(long, conflicts_with = "interactive")]
    pub no_overwrite: bool,

    /// Ask at each destination in the way: back up and replace, overwrite, or
    /// skip
    #[arg(long)]
    pub interactive: bool,
}

/// Both run-only selectors, accepted by `sync` and `clone`.
#[derive(Debug, Args)]
pub(crate) struct SkipOptions {
    #[command(flatten)]
    pub actions: SkipActionOptions,

    #[command(flatten)]
    pub groups: SkipGroupOptions,
}

/// The action half, accepted by `sync`, `clone`, and `apply-group`.
#[derive(Debug, Args)]
#[command(next_help_heading = "Selection Options")]
pub(crate) struct SkipActionOptions {
    /// Skip an action or addressable child for this run; repeatable
    #[arg(long = "skip-action", value_name = "ID")]
    pub skip_actions: Vec<String>,
}

/// The group half, accepted by `sync` and `clone`.
#[derive(Debug, Args)]
#[command(next_help_heading = "Selection Options")]
pub(crate) struct SkipGroupOptions {
    /// Skip a group for this run; repeatable
    #[arg(long = "skip-group", value_name = "GROUP")]
    pub skip_groups: Vec<String>,
}

/// Parse `KEY=VALUE`, splitting at the first `=` and validating the key. Empty values are
/// allowed; missing `=` or empty keys are errors.
fn parse_var(raw: &str) -> Result<(VarName, String), String> {
    match raw.split_once('=') {
        Some((key, value)) if !key.is_empty() => match VarName::try_from(key.to_owned()) {
            Ok(key) => Ok((key, value.to_owned())),
            Err(error) => Err(format!("`{key}` is not a valid variable name: {error}")),
        },
        _ => Err(format!("expected `KEY=VALUE`, found `{raw}`")),
    }
}

/// Bootstrap-only enable/disable adoption, honored by `clone` and `sync --bootstrap`.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn name(text: &str) -> VarName {
        VarName::try_from(text.to_owned()).expect("valid name")
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
    fn the_shape_is_checked_before_the_name() {
        assert_eq!(
            parse_var("profile"),
            Err("expected `KEY=VALUE`, found `profile`".to_owned())
        );
        assert_eq!(
            parse_var("=work"),
            Err("expected `KEY=VALUE`, found `=work`".to_owned())
        );
    }

    #[test]
    fn a_var_key_must_be_a_valid_variable_name() {
        let error = parse_var("1up=x").expect_err("names do not start with a digit");
        assert!(
            error.contains("`1up` is not a valid variable name"),
            "{error}"
        );
        assert!(error.contains("must start with a letter"), "{error}");
    }

    #[test]
    fn a_reserved_var_key_is_rejected_like_any_other_invalid_one() {
        let error = parse_var("env=x").expect_err("reserved");
        assert!(
            error.contains("`env` is not a valid variable name"),
            "{error}"
        );
        assert!(error.contains("reserved"), "{error}");
    }
}
