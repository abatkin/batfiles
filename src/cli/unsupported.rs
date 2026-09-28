//! The options each command accepts and does not honor yet (`architecture.md`,
//! rule 12).
//!
//! An option belongs here only while it goes live *later* than the command that
//! takes it. One arriving with its command never does: until the command lands
//! its own not-implemented message covers the whole invocation, and afterwards
//! there is nothing left to withhold. An entry leaves as its step lands —
//! `tests/hygiene.rs` insists — so the list shrinking to empty is how you know a
//! command is finished.

use super::options::ActionOptions;
use super::{Command, VarsCommand};

/// An option that parsed but does nothing yet, and the step that makes it live.
pub(crate) struct Unsupported {
    pub option: &'static str,
    pub step: &'static str,
}

/// The first option `command` accepts and does not honor yet, if any.
///
/// Matched exhaustively so that a command added later is asked the question
/// rather than defaulted past it.
pub(crate) fn first(command: &Command) -> Option<Unsupported> {
    match command {
        // Nothing to withhold: `version` takes no options, `init`'s
        // `--no-git-init` arrives with `init` itself, and the four
        // enable/disable commands take addresses rather than options.
        Command::Version
        | Command::Init(_)
        | Command::DisableAction(_)
        | Command::EnableAction(_)
        | Command::DisableGroup(_)
        | Command::EnableGroup(_) => None,
        Command::Clone(args) => action(&args.action),
        Command::Sync(args) => action(&args.action),
        Command::ApplyAction(args) => action(&args.action),
        Command::ApplyGroup(args) => action(&args.action),
        Command::Vars(command) => match command {
            VarsCommand::Set { .. }
            | VarsCommand::Get { .. }
            | VarsCommand::List { .. }
            | VarsCommand::Unset { .. }
            | VarsCommand::Refresh { .. } => None,
        },
    }
}

/// The shared action-execution options, so an option reports the same step
/// whichever of the four commands accepted it.
fn action(options: &ActionOptions) -> Option<Unsupported> {
    first_given([
        (options.refresh_content, "--refresh-content", "9.4"),
        (options.no_overwrite, "--no-overwrite", "9.4"),
        (options.interactive, "--interactive", "9.4"),
    ])
}

/// The first entry whose option was given.
fn first_given<const N: usize>(
    entries: [(bool, &'static str, &'static str); N],
) -> Option<Unsupported> {
    entries
        .into_iter()
        .find_map(|(given, option, step)| given.then_some(Unsupported { option, step }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::testing::parse;

    fn withheld(args: &[&str]) -> Option<(&'static str, &'static str)> {
        first(&parse(args).command).map(|found| (found.option, found.step))
    }

    #[test]
    fn a_command_with_nothing_pending_withholds_nothing() {
        for args in [
            &["batfiles", "version"][..],
            &["batfiles", "init", "--no-git-init"],
            &["batfiles", "disable-action", "vim"],
            &["batfiles", "vars", "list", "--machine-only", "--no-refresh"],
            &["batfiles", "sync", "--refresh-vars"],
            &["batfiles", "sync"],
            &["batfiles", "sync", "--dry-run"],
            &["batfiles", "sync", "--refresh-remotes"],
        ] {
            assert_eq!(withheld(args), None, "{args:?}");
        }
    }

    #[test]
    fn a_shared_option_names_one_step_whichever_command_took_it() {
        for args in [
            &["batfiles", "sync", "--interactive"][..],
            &["batfiles", "clone", "url", "--interactive"],
            &["batfiles", "apply-action", "--id", "vim", "--interactive"],
            &["batfiles", "apply-group", "--group", "gui", "--interactive"],
        ] {
            assert_eq!(withheld(args), Some(("--interactive", "9.4")), "{args:?}");
        }
    }

    #[test]
    fn the_first_listed_option_is_the_one_reported() {
        assert_eq!(
            withheld(&["batfiles", "sync", "--interactive", "--refresh-content"]),
            Some(("--refresh-content", "9.4"))
        );
    }
}
