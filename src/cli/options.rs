//! The option groups that `docs/cmdline.md` defines once and shares between
//! several commands.

use clap::Args;

/// Controls accepted by every command that executes actions.
///
/// `clone` accepts them because it forwards them to its follow-up
/// synchronization.
#[derive(Debug, Args)]
#[command(next_help_heading = "Action Execution Options")]
pub struct ActionOptions {
    /// Set a one-shot variable; repeatable, last value for a key wins
    #[arg(long = "var", value_name = "KEY=VALUE", value_parser = parse_var)]
    pub vars: Vec<(String, String)>,

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
pub struct SelectionOptions {
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
pub struct BootstrapOptions {
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

/// Split `KEY=VALUE`. Name validity is [`crate::var::VarName`]'s job, so only
/// the shape is checked here. An empty value is significant.
fn parse_var(raw: &str) -> Result<(String, String), String> {
    match raw.split_once('=') {
        Some((key, value)) if !key.is_empty() => Ok((key.to_owned(), value.to_owned())),
        _ => Err(format!("expected `KEY=VALUE`, found `{raw}`")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_var_splits_at_the_first_equals_sign() {
        assert_eq!(parse_var("a=b=c"), Ok(("a".to_owned(), "b=c".to_owned())));
    }

    #[test]
    fn an_empty_var_value_is_significant() {
        assert_eq!(
            parse_var("profile="),
            Ok(("profile".to_owned(), String::new()))
        );
    }

    #[test]
    fn a_var_needs_a_key_and_an_equals_sign() {
        assert!(parse_var("profile").is_err());
        assert!(parse_var("=work").is_err());
    }
}
