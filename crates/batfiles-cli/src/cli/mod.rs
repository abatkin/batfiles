//! The command-line surface described by `docs/cmdline.md`.
//!
//! This module tree owns argument definitions and nothing else. Rendering a
//! parsed invocation for humans belongs to `crate::trace`, and environment
//! inputs other than color are deliberately not read here: the environment
//! specification places the merge of command-line arguments, environment
//! variables, configuration files, and defaults in the configuration layer.

mod actions;
mod options;
mod toggles;
mod vars;

pub use actions::{ApplyActionArgs, ApplyGroupArgs, CloneArgs, SyncArgs};
pub use options::{ActionOptions, BootstrapOptions, SelectionOptions};
pub use toggles::{ActionAddresses, GroupAddresses};
pub use vars::VarsCommand;

use std::path::PathBuf;

// `--color` reuses clap's own `ColorChoice`: it already spells the three modes
// the specification names, and it is the type clap wants back when told how to
// render its help and errors, so an equivalent local enum would only add a
// mapping that can drift.
use clap::{Args, ColorChoice, Parser, Subcommand};

/// A dotfiles manager built around plain files and explicit composition.
#[derive(Debug, Parser)]
#[command(name = "batfiles", version, about, long_about = None)]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalOptions,

    #[command(subcommand)]
    pub command: Command,
}

/// Options accepted by every command, before or after the command name.
///
/// A command uses only the locations relevant to its work; `init` and
/// `version` resolve no roots at all.
#[derive(Debug, Args)]
#[command(next_help_heading = "Global Options")]
pub struct GlobalOptions {
    /// Increase diagnostic detail; repeatable, such as `-vv`
    #[arg(short, long, global = true, action = clap::ArgAction::Count, conflicts_with = "quiet")]
    pub verbose: u8,

    /// Suppress informational output, leaving errors and requested data
    #[arg(short, long, global = true)]
    pub quiet: bool,

    /// Control colored output; auto follows the terminal [default: auto]
    #[arg(long, global = true, value_name = "WHEN", value_enum)]
    pub color: Option<ColorChoice>,

    /// Select the leaf repository [default: <selected-home>/dotfiles]
    #[arg(long, global = true, value_name = "PATH")]
    pub batfiles_dir: Option<PathBuf>,

    /// Select the destination home directory [default: the current user's home]
    #[arg(long, global = true, value_name = "PATH")]
    pub home_dir: Option<PathBuf>,

    /// Select the directory holding vars.toml and disabled.toml [default: XDG config]
    #[arg(long, global = true, value_name = "PATH")]
    pub config_dir: Option<PathBuf>,

    /// Select the directory holding dynamic-vars.toml [default: XDG cache]
    #[arg(long, global = true, value_name = "PATH")]
    pub cache_dir: Option<PathBuf>,
}

/// Each variant carries a named argument type, so a command implementation can
/// take exactly the arguments it owns.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Initialize the current directory with the leaf-repository layout
    Init(InitArgs),

    /// Print the batfiles version
    Version,

    /// Clone a leaf repository, adopt its bootstrap policy, then synchronize
    Clone(CloneArgs),

    /// Build and apply the desired installation plan
    Sync(SyncArgs),

    /// Persistently disable one or more actions
    DisableAction(ActionAddresses),

    /// Persistently enable one or more actions
    EnableAction(ActionAddresses),

    /// Persistently disable one or more groups
    DisableGroup(GroupAddresses),

    /// Persistently enable one or more groups
    EnableGroup(GroupAddresses),

    /// Apply one action or addressable manifest entry
    ApplyAction(ApplyActionArgs),

    /// Apply the enabled, directly executable actions in one group
    ApplyGroup(ApplyGroupArgs),

    /// Inspect and manage variables
    #[command(subcommand)]
    Vars(VarsCommand),
}

#[derive(Debug, Args)]
pub struct InitArgs {
    /// Do not run `git init`
    #[arg(long)]
    pub no_git_init: bool,
}

/// Parsing helpers shared by the command modules' tests.
#[cfg(test)]
pub(crate) mod testing {
    use super::Cli;
    use clap::Parser;
    use clap::error::ErrorKind;

    pub(crate) fn parse(args: &[&str]) -> Cli {
        Cli::try_parse_from(args).expect("expected the arguments to parse")
    }

    pub(crate) fn error_kind(args: &[&str]) -> ErrorKind {
        Cli::try_parse_from(args)
            .expect_err("expected the arguments to be rejected")
            .kind()
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{error_kind, parse};
    use super::*;
    use clap::CommandFactory;
    use clap::error::ErrorKind;

    #[test]
    fn command_definitions_are_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn global_options_are_accepted_after_the_command() {
        let cli = parse(&["batfiles", "sync", "-vv", "--home-dir", "/tmp/home"]);
        assert_eq!(cli.global.verbose, 2);
        assert_eq!(cli.global.home_dir, Some(PathBuf::from("/tmp/home")));
        assert!(matches!(cli.command, Command::Sync(_)));
    }

    #[test]
    fn global_options_are_accepted_before_the_command() {
        let cli = parse(&["batfiles", "--color", "never", "--quiet", "version"]);
        assert_eq!(cli.global.color, Some(ColorChoice::Never));
        assert!(cli.global.quiet);
        assert!(matches!(cli.command, Command::Version));
    }

    #[test]
    fn verbose_and_quiet_are_mutually_exclusive() {
        assert_eq!(
            error_kind(&["batfiles", "sync", "-v", "-q"]),
            ErrorKind::ArgumentConflict
        );
    }

    #[test]
    fn init_takes_only_no_git_init() {
        let Command::Init(args) = parse(&["batfiles", "init", "--no-git-init"]).command else {
            panic!("expected init");
        };
        assert!(args.no_git_init);
    }
}
