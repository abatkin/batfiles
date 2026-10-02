//! Command-line definitions and color resolution. [`crate::app`] dispatches parsed commands.

mod actions;
mod disabled;
mod init;
mod options;
mod update;
mod vars;

pub(crate) mod color;

pub(crate) use actions::{ApplyActionArgs, ApplyGroupArgs, CloneArgs, SyncArgs};
pub(crate) use disabled::{ActionAddresses, GroupAddresses};
pub(crate) use init::InitArgs;
pub(crate) use options::{BootstrapOptions, ExecutionOptions};
pub(crate) use update::UpdateArgs;
pub(crate) use vars::VarsCommand;

use std::path::PathBuf;

use clap::{Args, ColorChoice, Parser, Subcommand};

/// The version this build reports: the release version a release build compiles in, which may
/// carry a pre-release suffix, else the `Cargo.toml` version.
pub(crate) const VERSION: &str = match option_env!("BATFILES_RELEASE_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};

/// A dotfiles manager built around plain files and explicit composition.
#[derive(Debug, Parser)]
#[command(name = "batfiles", version = VERSION, about, long_about = None)]
pub(crate) struct Cli {
    #[command(flatten)]
    pub global: GlobalOptions,

    #[command(subcommand)]
    pub command: Command,
}

/// Options accepted by every command, before or after the command name.
///
/// A command uses only the locations relevant to its work; `init`,
/// `version`, and `update` resolve no roots at all.
#[derive(Debug, Args)]
#[command(next_help_heading = "Global Options")]
pub(crate) struct GlobalOptions {
    /// Increase diagnostic detail; repeatable, such as `-vv`
    #[arg(short, long, global = true, action = clap::ArgAction::Count, conflicts_with = "quiet")]
    pub verbose: u8,

    /// Suppress informational output, leaving errors and requested data
    #[arg(short, long, global = true)]
    pub quiet: bool,

    /// Control colored output; auto follows the terminal [default: auto]
    #[arg(long, global = true, value_name = "WHEN", value_enum)]
    pub color: Option<ColorChoice>,

    /// Select the leaf repository (defaults to the current repository, then home/dotfiles)
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

/// Supported commands and their arguments.
#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Initialize the current directory with the leaf-repository layout
    Init(InitArgs),

    /// Print the batfiles version
    Version,

    /// Replace this batfiles with the latest release, or with another one
    Update(UpdateArgs),

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
}
