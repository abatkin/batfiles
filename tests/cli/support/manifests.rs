//! Single-action manifests and assertions for the leaf fixture.

use super::Tree;
use std::fs;
use std::path::PathBuf;

/// A manifest declaring one symlink and nothing else.
pub(crate) fn one_symlink(source: &str, dest: &str) -> String {
    format!("[[actions]]\ntype = \"symlink\"\nsource = \"{source}\"\ndest = \"{dest}\"\n")
}

/// A manifest declaring one `symlink-dir` and nothing else.
pub(crate) fn one_symlink_dir(source_dir: &str, dest_dir: &str, dot_prefix: bool) -> String {
    format!(
        "[[actions]]\n\
         type = \"symlink-dir\"\n\
         source-dir = \"{source_dir}\"\n\
         dest-dir = \"{dest_dir}\"\n\
         dot-prefix = {dot_prefix}\n"
    )
}

/// A manifest declaring one `create-dir` and nothing else.
pub(crate) fn one_create_dir(dest: &str) -> String {
    format!("[[actions]]\ntype = \"create-dir\"\ndest = \"{dest}\"\n")
}

/// A manifest declaring one `copy` and nothing else.
pub(crate) fn one_copy(source: &str, dest: &str) -> String {
    format!("[[actions]]\ntype = \"copy\"\nsource = \"{source}\"\ndest = \"{dest}\"\n")
}

/// A manifest declaring one `copy-dir` and nothing else.
pub(crate) fn one_copy_dir(source_dir: &str, dest_dir: &str, dot_prefix: bool) -> String {
    format!(
        "[[actions]]\n\
         type = \"copy-dir\"\n\
         source-dir = \"{source_dir}\"\n\
         dest-dir = \"{dest_dir}\"\n\
         dot-prefix = {dot_prefix}\n"
    )
}

/// A repository at `~/dotfiles`, which is where batfiles looks by default and
/// the layout in which a destination can reach the repository through `~`.
pub(crate) fn seeded_repository_in_the_home(tree: &Tree) -> PathBuf {
    let repo = tree.repository("home/dotfiles");
    fs::create_dir(repo.join("seed")).expect("a source directory");
    fs::write(repo.join("seed/a"), "x\n").expect("something to copy");
    repo
}

// The portable half of the `leaf` fixture: the actions that need no symlink,
// which the manifest declares first so that a platform which cannot make one
// still runs them before the refusal stops the list.

/// Every directory `tests/fixtures/leaf` creates outright, as opposed to the
/// ones made on the way to a destination.
pub(crate) const LEAF_DIRS: [&str; 1] = [".cache/zsh"];

/// The destination of the fixture's one gated action, whose condition is false
/// as the repository stands.
pub(crate) const LEAF_CLOSED_DEST: &str = ".cache/work-tools";

/// Every file it seeds, and the repository file each one is a copy of, in the
/// order it seeds them.
pub(crate) const LEAF_SEEDS: [(&str, &str); 4] = [
    ("templates/gitconfig.local", ".config/git/local"),
    ("templates/profile.machine.zsh", ".config/zsh/profile.zsh"),
    ("zsh-local/env.zsh", ".config/zsh/local/env.zsh"),
    ("zsh-local/prompt.zsh", ".config/zsh/local/prompt.zsh"),
];

/// The two `leaf` actions that name one destination, as `(winner, loser)`
/// repository paths, and the destination they contend for.
pub(crate) const LEAF_ORDERED_PAIR: (&str, &str, &str) = (
    "templates/profile.machine.zsh",
    "templates/profile.zsh",
    ".config/zsh/profile.zsh",
);

/// Assert that every action in [`LEAF_DIRS`] and [`LEAF_SEEDS`] has been
/// carried out, which every platform can do.
pub(crate) fn assert_leaf_portable_actions(tree: &Tree) {
    for dest in LEAF_DIRS {
        assert!(
            tree.home(dest).is_dir(),
            "`{dest}` is not a directory that exists"
        );
    }
    // The other half of the same rule: the gated action is the one record here
    // that must *not* have run, and it is a `create-dir` like the first so that
    // nothing but its condition separates them.
    assert!(
        !tree.home(LEAF_CLOSED_DEST).exists(),
        "`{LEAF_CLOSED_DEST}` was installed by an action whose condition is false"
    );
    for (source, dest) in LEAF_SEEDS {
        let installed = tree.home(dest);
        // A seed is the user's copy, not a view of the repository's file: what
        // it holds is what an editor would write to, and nothing links back.
        assert!(
            !installed.is_symlink(),
            "`{dest}` was linked rather than seeded"
        );
        assert_eq!(
            fs::read_to_string(&installed).unwrap_or_else(|error| panic!("`{dest}`: {error}")),
            fs::read_to_string(tree.path("repo").join(source)).expect("the repository file"),
            "`{dest}` does not hold what `{source}` holds"
        );
    }
}
