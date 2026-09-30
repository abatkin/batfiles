//! The commands that execute actions: `clone`, `sync`, `apply-action`, and
//! `apply-group`. All four take the shared action-execution options and apply
//! actions with the same semantics.

use clap::Args;

use super::options::{BootstrapOptions, ExecutionOptions, SkipActionOptions, SkipOptions};

/// Arguments for `clone`; neither `--dry-run` nor `--refresh-remotes` is accepted.
#[derive(Debug, Args)]
pub(crate) struct CloneArgs {
    /// Repository URL to clone into the selected batfiles directory
    pub url: String,

    #[command(flatten)]
    pub action: ExecutionOptions,

    #[command(flatten)]
    pub selection: SkipOptions,

    #[command(flatten)]
    pub bootstrap: BootstrapOptions,
}

// Declare command options before flattened groups: clap's `next_help_heading` also affects
// subsequent arguments.

#[derive(Debug, Args)]
pub(crate) struct SyncArgs {
    /// Report the action plan without executing it
    #[arg(long, conflicts_with_all = ["refresh_remotes", "interactive"])]
    pub dry_run: bool,

    /// Re-fetch file and archive remotes, replacing their materializations
    #[arg(long)]
    pub refresh_remotes: bool,

    #[command(flatten)]
    pub action: ExecutionOptions,

    #[command(flatten)]
    pub selection: SkipOptions,
}

#[derive(Debug, Args)]
pub(crate) struct ApplyActionArgs {
    /// Action or manifest-entry address
    #[arg(long, value_name = "ID")]
    pub id: String,

    /// Report the application plan without executing it
    #[arg(long, conflicts_with = "interactive")]
    pub dry_run: bool,

    #[command(flatten)]
    pub action: ExecutionOptions,
}

/// Arguments for `apply-group`, including action skips.
#[derive(Debug, Args)]
pub(crate) struct ApplyGroupArgs {
    /// Leaf or qualified included group address
    #[arg(long, value_name = "GROUP")]
    pub group: String,

    /// Report the group application plan without executing it
    #[arg(long, conflicts_with = "interactive")]
    pub dry_run: bool,

    #[command(flatten)]
    pub action: ExecutionOptions,

    #[command(flatten)]
    pub selection: SkipActionOptions,
}

#[cfg(test)]
mod tests {
    use crate::cli::Command;
    use crate::cli::testing::{error_kind, parse};
    use clap::error::ErrorKind;

    #[test]
    fn dry_run_and_refresh_remotes_are_mutually_exclusive() {
        assert_eq!(
            error_kind(&["batfiles", "sync", "--dry-run", "--refresh-remotes"]),
            ErrorKind::ArgumentConflict
        );
    }

    #[test]
    fn no_overwrite_and_interactive_are_mutually_exclusive() {
        assert_eq!(
            error_kind(&["batfiles", "sync", "--no-overwrite", "--interactive"]),
            ErrorKind::ArgumentConflict
        );
    }

    #[test]
    fn dry_run_and_interactive_are_mutually_exclusive() {
        for args in [
            &["batfiles", "sync", "--dry-run", "--interactive"][..],
            &[
                "batfiles",
                "apply-action",
                "--id",
                "a",
                "--dry-run",
                "--interactive",
            ],
            &[
                "batfiles",
                "apply-group",
                "--group",
                "g",
                "--dry-run",
                "--interactive",
            ],
        ] {
            assert_eq!(error_kind(args), ErrorKind::ArgumentConflict, "{args:?}");
        }
    }

    #[test]
    fn clone_rejects_dry_run_and_refresh_remotes() {
        assert_eq!(
            error_kind(&["batfiles", "clone", "url", "--dry-run"]),
            ErrorKind::UnknownArgument
        );
        assert_eq!(
            error_kind(&["batfiles", "clone", "url", "--refresh-remotes"]),
            ErrorKind::UnknownArgument
        );
    }

    #[test]
    fn apply_action_rejects_both_run_only_selectors() {
        for selector in ["--skip-action", "--skip-group"] {
            assert_eq!(
                error_kind(&["batfiles", "apply-action", "--id", "a", selector, "b"]),
                ErrorKind::UnknownArgument,
                "{selector}"
            );
        }
    }

    #[test]
    fn apply_group_takes_the_action_selector_and_not_the_group_one() {
        let cli = parse(&[
            "batfiles",
            "apply-group",
            "--group",
            "shell",
            "--skip-action",
            "zshrc",
        ]);
        let Command::ApplyGroup(args) = cli.command else {
            panic!("expected apply-group");
        };
        assert_eq!(args.selection.skip_actions, vec!["zshrc".to_owned()]);

        assert_eq!(
            error_kind(&[
                "batfiles",
                "apply-group",
                "--group",
                "g",
                "--skip-group",
                "h"
            ]),
            ErrorKind::UnknownArgument
        );
    }

    #[test]
    fn apply_commands_require_their_address() {
        assert_eq!(
            error_kind(&["batfiles", "apply-action"]),
            ErrorKind::MissingRequiredArgument
        );
        assert_eq!(
            error_kind(&["batfiles", "apply-group"]),
            ErrorKind::MissingRequiredArgument
        );
    }

    #[test]
    fn repeated_vars_are_collected_in_order() {
        let cli = parse(&[
            "batfiles",
            "sync",
            "--var",
            "profile=work",
            "--var",
            "profile=home",
        ]);
        let Command::Sync(args) = cli.command else {
            panic!("expected sync");
        };
        assert_eq!(
            args.action
                .vars
                .iter()
                .map(|(key, value)| (key.to_string(), value.clone()))
                .collect::<Vec<_>>(),
            vec![
                ("profile".to_owned(), "work".to_owned()),
                ("profile".to_owned(), "home".to_owned())
            ]
        );
    }

    #[test]
    fn clone_accepts_bootstrap_selection_and_action_options() {
        let cli = parse(&[
            "batfiles",
            "clone",
            "https://example.invalid/dotfiles.git",
            "--disable-group",
            "gui",
            "--enable-action",
            "shell",
            "--skip-group",
            "fonts",
            "--refresh-vars",
        ]);
        let Command::Clone(args) = cli.command else {
            panic!("expected clone");
        };
        assert_eq!(args.url, "https://example.invalid/dotfiles.git");
        assert!(args.action.refresh_vars);
        assert_eq!(args.selection.groups.skip_groups, vec!["fonts".to_owned()]);
        assert_eq!(args.bootstrap.disable_groups, vec!["gui".to_owned()]);
        assert_eq!(args.bootstrap.enable_actions, vec!["shell".to_owned()]);
    }
}
