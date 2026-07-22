//! Human-readable rendering of a parsed invocation.
//!
//! This is presentation, not parsing, so it lives outside `crate::cli`: the
//! argument definitions stay free of formatting, and the one exhaustive match
//! over every command has a single home.

use crate::cli::{
    ActionAddresses, ActionOptions, ApplyActionArgs, ApplyGroupArgs, BootstrapOptions, CloneArgs,
    Command, GroupAddresses, InitArgs, SelectionOptions, SyncArgs, VarsCommand,
};

/// The command name as written on the command line.
pub fn name(command: &Command) -> &'static str {
    match command {
        Command::Init(_) => "init",
        Command::Version => "version",
        Command::Clone(_) => "clone",
        Command::Sync(_) => "sync",
        Command::DisableAction(_) => "disable-action",
        Command::EnableAction(_) => "enable-action",
        Command::DisableGroup(_) => "disable-group",
        Command::EnableGroup(_) => "enable-group",
        Command::ApplyAction(_) => "apply-action",
        Command::ApplyGroup(_) => "apply-group",
        Command::Vars(command) => vars_name(command),
    }
}

/// A one-line description of what was requested, for verbose output.
pub fn summary(command: &Command) -> String {
    let parts = match command {
        Command::Init(args) => args.parts(),
        Command::Version => Vec::new(),
        Command::Clone(args) => args.parts(),
        Command::Sync(args) => args.parts(),
        Command::DisableAction(args) | Command::EnableAction(args) => args.parts(),
        Command::DisableGroup(args) | Command::EnableGroup(args) => args.parts(),
        Command::ApplyAction(args) => args.parts(),
        Command::ApplyGroup(args) => args.parts(),
        Command::Vars(command) => command.parts(),
    };

    let name = name(command);
    if parts.is_empty() {
        name.to_owned()
    } else {
        format!("{name} {}", parts.join(" "))
    }
}

fn vars_name(command: &VarsCommand) -> &'static str {
    match command {
        VarsCommand::Set { .. } => "vars set",
        VarsCommand::Get { .. } => "vars get",
        VarsCommand::List { .. } => "vars list",
        VarsCommand::Unset { .. } => "vars unset",
        VarsCommand::Refresh { .. } => "vars refresh",
    }
}

/// The pieces of a summary contributed by one argument group.
trait Summarize {
    fn parts(&self) -> Vec<String>;
}

impl Summarize for InitArgs {
    fn parts(&self) -> Vec<String> {
        flags([("no-git-init", self.no_git_init)])
    }
}

impl Summarize for CloneArgs {
    fn parts(&self) -> Vec<String> {
        let mut parts = vec![format!("url={}", self.url)];
        parts.extend(self.action.parts());
        parts.extend(self.selection.parts());
        parts.extend(self.bootstrap.parts());
        parts
    }
}

impl Summarize for SyncArgs {
    fn parts(&self) -> Vec<String> {
        let mut parts = flags([
            ("dry-run", self.dry_run),
            ("refresh-remotes", self.refresh_remotes),
        ]);
        parts.extend(self.action.parts());
        parts.extend(self.selection.parts());
        parts
    }
}

impl Summarize for ApplyActionArgs {
    fn parts(&self) -> Vec<String> {
        let mut parts = vec![format!("id={}", self.id)];
        parts.extend(flags([("dry-run", self.dry_run)]));
        parts.extend(self.action.parts());
        parts
    }
}

impl Summarize for ApplyGroupArgs {
    fn parts(&self) -> Vec<String> {
        let mut parts = vec![format!("group={}", self.group)];
        parts.extend(flags([("dry-run", self.dry_run)]));
        parts.extend(self.action.parts());
        parts
    }
}

impl Summarize for ActionAddresses {
    fn parts(&self) -> Vec<String> {
        vec![format!("ids={}", self.ids.join(","))]
    }
}

impl Summarize for GroupAddresses {
    fn parts(&self) -> Vec<String> {
        vec![format!("groups={}", self.groups.join(","))]
    }
}

impl Summarize for VarsCommand {
    fn parts(&self) -> Vec<String> {
        match self {
            Self::Set { key, value } => vec![format!("key={key}"), format!("value={value}")],
            Self::Get { key } | Self::Unset { key } => vec![format!("key={key}")],
            Self::List {
                machine_only,
                no_refresh,
            } => flags([("machine-only", *machine_only), ("no-refresh", *no_refresh)]),
            Self::Refresh { keys } if keys.is_empty() => vec!["keys=all".to_owned()],
            Self::Refresh { keys } => vec![format!("keys={}", keys.join(","))],
        }
    }
}

impl Summarize for ActionOptions {
    fn parts(&self) -> Vec<String> {
        let mut parts = flags([
            ("refresh-vars", self.refresh_vars),
            ("refresh-content", self.refresh_content),
            ("no-overwrite", self.no_overwrite),
            ("interactive", self.interactive),
        ]);
        for (key, value) in &self.vars {
            parts.push(format!("var:{key}={value}"));
        }
        parts
    }
}

impl Summarize for SelectionOptions {
    fn parts(&self) -> Vec<String> {
        named_lists([
            ("skip-actions", &self.skip_actions),
            ("skip-groups", &self.skip_groups),
        ])
    }
}

impl Summarize for BootstrapOptions {
    fn parts(&self) -> Vec<String> {
        named_lists([
            ("enable-actions", &self.enable_actions),
            ("disable-actions", &self.disable_actions),
            ("enable-groups", &self.enable_groups),
            ("disable-groups", &self.disable_groups),
        ])
    }
}

/// Name the flags that are set, dropping the rest.
fn flags<const N: usize>(flags: [(&str, bool); N]) -> Vec<String> {
    flags
        .into_iter()
        .filter(|(_, set)| *set)
        .map(|(name, _)| name.to_owned())
        .collect()
}

/// Render the non-empty repeatable options as `name=a,b`.
fn named_lists<const N: usize>(lists: [(&str, &Vec<String>); N]) -> Vec<String> {
    lists
        .into_iter()
        .filter(|(_, values)| !values.is_empty())
        .map(|(name, values)| format!("{name}={}", values.join(",")))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::testing::parse;

    fn summary_of(args: &[&str]) -> String {
        summary(&parse(args).command)
    }

    #[test]
    fn a_bare_command_is_just_its_name() {
        assert_eq!(summary_of(&["batfiles", "sync"]), "sync");
        assert_eq!(summary_of(&["batfiles", "version"]), "version");
    }

    #[test]
    fn unset_flags_are_left_out() {
        assert_eq!(summary_of(&["batfiles", "init"]), "init");
        assert_eq!(
            summary_of(&["batfiles", "init", "--no-git-init"]),
            "init no-git-init"
        );
    }

    #[test]
    fn shared_option_groups_are_rendered_in_a_stable_order() {
        let summary = summary_of(&[
            "batfiles",
            "sync",
            "--dry-run",
            "--skip-group",
            "gui",
            "--var",
            "profile=work",
            "--refresh-vars",
        ]);
        assert_eq!(
            summary,
            "sync dry-run refresh-vars var:profile=work skip-groups=gui"
        );
    }

    #[test]
    fn addresses_are_listed() {
        let summary = summary_of(&["batfiles", "disable-action", "vim", "git.clone"]);
        assert_eq!(summary, "disable-action ids=vim,git.clone");
    }

    #[test]
    fn a_vars_summary_names_the_subcommand() {
        assert_eq!(
            summary_of(&["batfiles", "vars", "refresh"]),
            "vars refresh keys=all"
        );
        assert_eq!(
            summary_of(&["batfiles", "vars", "refresh", "os"]),
            "vars refresh keys=os"
        );
        assert_eq!(
            summary_of(&["batfiles", "vars", "get", "os"]),
            "vars get key=os"
        );
    }

    #[test]
    fn every_command_has_a_name() {
        for args in [
            vec!["batfiles", "init"],
            vec!["batfiles", "version"],
            vec!["batfiles", "clone", "url"],
            vec!["batfiles", "sync"],
            vec!["batfiles", "disable-action", "a"],
            vec!["batfiles", "enable-action", "a"],
            vec!["batfiles", "disable-group", "g"],
            vec!["batfiles", "enable-group", "g"],
            vec!["batfiles", "apply-action", "--id", "a"],
            vec!["batfiles", "apply-group", "--group", "g"],
            vec!["batfiles", "vars", "list"],
        ] {
            let name = name(&parse(&args).command);
            assert!(!name.is_empty(), "{args:?} has no name");
        }
    }
}
