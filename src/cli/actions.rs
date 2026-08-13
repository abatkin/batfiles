//! The commands that execute actions: `clone`, `sync`, `apply-action`, and
//! `apply-group`. All four take the shared action-execution options and apply
//! actions with the same semantics.

use clap::Args;

use super::options::{ActionOptions, BootstrapOptions, SelectionOptions};

/// `clone` intentionally accepts neither `--dry-run` nor `--refresh-remotes`: a
/// fresh clone materializes its remotes during the follow-up sync.
#[derive(Debug, Args)]
pub(crate) struct CloneArgs {
    /// Repository URL to clone into the selected batfiles directory
    pub url: String,

    #[command(flatten)]
    pub action: ActionOptions,

    #[command(flatten)]
    pub selection: SelectionOptions,

    #[command(flatten)]
    pub bootstrap: BootstrapOptions,
}

#[derive(Debug, Args)]
pub(crate) struct SyncArgs {
    #[command(flatten)]
    pub action: ActionOptions,

    #[command(flatten)]
    pub selection: SelectionOptions,

    /// Report the action plan without executing it
    #[arg(long, conflicts_with = "refresh_remotes")]
    pub dry_run: bool,

    /// Re-fetch file and archive remotes, replacing their materializations
    #[arg(long)]
    pub refresh_remotes: bool,
}

#[derive(Debug, Args)]
pub(crate) struct ApplyActionArgs {
    /// Action or manifest-entry address
    #[arg(long, value_name = "ID")]
    pub id: String,

    #[command(flatten)]
    pub action: ActionOptions,

    /// Report the application plan without executing it
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Args)]
pub(crate) struct ApplyGroupArgs {
    /// Leaf or qualified included group address
    #[arg(long, value_name = "GROUP")]
    pub group: String,

    #[command(flatten)]
    pub action: ActionOptions,

    /// Report the group application plan without executing it
    #[arg(long)]
    pub dry_run: bool,
}

#[cfg(test)]
mod tests {
    use crate::cli::Command;
    use crate::cli::testing::{error_kind, parse};
    use crate::var::VarName;
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
    fn apply_commands_reject_the_run_only_selectors() {
        assert_eq!(
            error_kind(&[
                "batfiles",
                "apply-action",
                "--id",
                "a",
                "--skip-action",
                "b"
            ]),
            ErrorKind::UnknownArgument
        );
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
        let profile = VarName::new("profile").expect("valid name");
        assert_eq!(
            args.action.vars,
            vec![
                (profile.clone(), "work".to_owned()),
                (profile, "home".to_owned())
            ]
        );
    }

    #[test]
    fn a_var_without_an_equals_sign_is_rejected() {
        assert_eq!(
            error_kind(&["batfiles", "sync", "--var", "profile"]),
            ErrorKind::ValueValidation
        );
    }

    #[test]
    fn a_var_with_an_invalid_key_is_rejected_before_anything_is_loaded() {
        // A usage error, so it lands before roots are resolved and before any
        // file is opened — which is what makes `docs/cmdline.md`'s promise
        // about `--var` true.
        assert_eq!(
            error_kind(&["batfiles", "sync", "--var", "1up=x"]),
            ErrorKind::ValueValidation
        );
        assert_eq!(
            error_kind(&["batfiles", "sync", "--var", "env=x"]),
            ErrorKind::ValueValidation
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
        assert_eq!(args.selection.skip_groups, vec!["fonts".to_owned()]);
        assert_eq!(args.bootstrap.disable_groups, vec!["gui".to_owned()]);
        assert_eq!(args.bootstrap.enable_actions, vec!["shell".to_owned()]);
    }
}
