//! The option groups shared between several commands.
//!
//! None of these options is live yet, so each value stays a `String`: an option
//! that is rejected wholesale needs no parsed type, and pulling `VarName` or an
//! address type forward to hold a value nothing reads is how the previous
//! implementation grew its unreachable half.

use clap::Args;

/// Controls accepted by every command that executes actions.
///
/// `clone` accepts them because it forwards them to its follow-up
/// synchronization.
#[derive(Debug, Args)]
#[command(next_help_heading = "Action Execution Options")]
pub(crate) struct ActionOptions {
    /// Set a one-shot variable; repeatable, last value for a key wins
    #[arg(long = "var", value_name = "KEY=VALUE")]
    pub vars: Vec<String>,

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
