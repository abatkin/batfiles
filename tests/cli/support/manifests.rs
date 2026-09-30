//! Single-action manifests, and what the committed fixtures install.

use super::{Tree, display, stderr_of};
use std::fs;
use std::path::PathBuf;

/// Run `sync` with an invalid manifest and return diagnostics. Require exit status 1, a named
/// manifest path, and no action progress.
pub(crate) fn rejected(manifest: &str) -> String {
    let tree = Tree::new();
    tree.write_manifest(manifest);

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains(&display(&tree.manifest())),
        "the manifest was not named:\n{stderr}"
    );
    assert!(
        !stderr.contains("is not implemented yet"),
        "the stub ran anyway:\n{stderr}"
    );
    stderr
}

/// A manifest declaring one symlink and nothing else.
pub(crate) fn one_symlink(source: &str, dest: &str) -> String {
    format!(
        r#"[[actions]]
type = "symlink"
source = "{source}"
dest = "{dest}"
"#
    )
}

/// A manifest declaring one `symlink-dir` and nothing else.
pub(crate) fn one_symlink_dir(source_dir: &str, dest_dir: &str, dot_prefix: bool) -> String {
    format!(
        r#"[[actions]]
type = "symlink-dir"
source-dir = "{source_dir}"
dest-dir = "{dest_dir}"
dot-prefix = {dot_prefix}
"#
    )
}

/// A manifest declaring one `create-dir` and nothing else.
pub(crate) fn one_create_dir(dest: &str) -> String {
    format!(
        r#"[[actions]]
type = "create-dir"
dest = "{dest}"
"#
    )
}

/// A manifest declaring one `copy` and nothing else.
pub(crate) fn one_copy(source: &str, dest: &str) -> String {
    format!(
        r#"[[actions]]
type = "copy"
source = "{source}"
dest = "{dest}"
"#
    )
}

/// A manifest declaring one `copy-dir` and nothing else.
pub(crate) fn one_copy_dir(source_dir: &str, dest_dir: &str, dot_prefix: bool) -> String {
    format!(
        r#"[[actions]]
type = "copy-dir"
source-dir = "{source_dir}"
dest-dir = "{dest_dir}"
dot-prefix = {dot_prefix}
"#
    )
}

/// Create a seeded repository at `~/dotfiles` and return its path.
pub(crate) fn seeded_repository_in_the_home(tree: &Tree) -> PathBuf {
    let repo = tree.repository("home/dotfiles");
    fs::create_dir(repo.join("seed")).expect("a source directory");
    fs::write(repo.join("seed/a"), "x\n").expect("something to copy");
    repo
}

// Portable fixture actions run before symlink actions so they complete on platforms without
// symlink support.

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

/// Source paths and shared destination for the leaf fixture's ordered seed pair, as `(winner,
/// loser, destination)`.
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
    assert!(
        !tree.home(LEAF_CLOSED_DEST).exists(),
        "`{LEAF_CLOSED_DEST}` was installed by an action whose condition is false"
    );
    for (source, dest) in LEAF_SEEDS {
        let installed = tree.home(dest);
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

/// One record `tests/fixtures/corporate/batfiles.toml` declares.
pub(crate) struct CorporateAction {
    /// Declared action ID, qualified by the inclusion when addressed or reported.
    pub id: &'static str,
    /// Where it installs, relative to the selected home.
    pub dest: &'static str,
}

/// Corporate fixture actions in declaration order.
pub(crate) const CORPORATE_ACTIONS: [CorporateAction; 3] = [
    CorporateAction {
        id: "zshrc",
        dest: ".zshrc.corporate",
    },
    CorporateAction {
        id: "p10k",
        dest: ".p10k.zsh",
    },
    CorporateAction {
        id: "seeds",
        dest: ".config/corporate",
    },
];

/// Return IDs whose destinations exist, in declaration order. Check presence only, not content.
pub(crate) fn installed_corporate(tree: &Tree) -> Vec<&'static str> {
    CORPORATE_ACTIONS
        .iter()
        .filter(|action| tree.home(action.dest).exists())
        .map(|action| action.id)
        .collect()
}
