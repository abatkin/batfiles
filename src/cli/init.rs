//! `init`, which lays out the leaf-repository skeleton in the current
//! directory. It resolves no roots and takes no shared option group.

use clap::Args;

#[derive(Debug, Args)]
pub(crate) struct InitArgs {
    /// Do not run `git init`
    #[arg(long)]
    pub no_git_init: bool,
}

#[cfg(test)]
mod tests {
    use crate::cli::Command;
    use crate::cli::testing::parse;

    #[test]
    fn init_takes_only_no_git_init() {
        let Command::Init(args) = parse(&["batfiles", "init", "--no-git-init"]).command else {
            panic!("expected init");
        };
        assert!(args.no_git_init);
    }
}
