//! `init`, which lays out the leaf-repository skeleton in the current
//! directory, or adds its stubs to a repository there. It resolves no roots and
//! takes no shared option group.

use clap::Args;

#[derive(Debug, Args)]
pub(crate) struct InitArgs {
    /// Do not run `git init`
    #[arg(long)]
    pub no_git_init: bool,

    /// Add only the stubs an existing repository here lacks, and run no `git init`
    #[arg(long, conflicts_with = "no_git_init")]
    pub stubs: bool,
}

#[cfg(test)]
mod tests {
    use crate::cli::Command;
    use crate::cli::testing::{error_kind, parse};

    #[test]
    fn init_takes_only_no_git_init() {
        let Command::Init(args) = parse(&["batfiles", "init", "--no-git-init"]).command else {
            panic!("expected init");
        };
        assert!(args.no_git_init);
        assert!(!args.stubs);
    }

    #[test]
    fn stubs_and_no_git_init_are_mutually_exclusive() {
        let Command::Init(args) = parse(&["batfiles", "init", "--stubs"]).command else {
            panic!("expected init");
        };
        assert!(args.stubs);
        assert_eq!(
            error_kind(&["batfiles", "init", "--stubs", "--no-git-init"]),
            clap::error::ErrorKind::ArgumentConflict
        );
    }
}
