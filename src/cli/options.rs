//! The option groups shared between several commands.
//!
//! Every value here stays a `String`. For the options that are still rejected
//! wholesale (`guidance.md`, rule 12) that is because a rejected option needs no
//! parsed type, and pulling `VarName` or an address type forward to hold a value
//! nothing reads is how the previous implementation grew its unreachable half.
//! For the two skip lists, which are live, it is because an unusable name warns
//! and is dropped rather than failing the run — a decision that needs the
//! reporter, which clap's value parsers do not have.

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
///
/// The two halves are separate types because `apply-group` takes the first and
/// not the second: it is already restricted to one group, so naming a group to
/// leave out has nothing to say. Splitting the struct is what keeps the option
/// spelled once — a second `#[arg]` for `--skip-action` on that command is a
/// help string and a value name free to drift.
#[derive(Debug, Args)]
pub(crate) struct SelectionOptions {
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
