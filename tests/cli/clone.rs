//! CLI tests for cloning and synchronizing a leaf repository from local bare origins.

use std::fs;

use assert_cmd::Command;

use crate::support::*;

/// Expected contents of the cloned fixture's installed file.
const GITCONFIG: &str = "[user]\n\tname = cloned\n";

/// Publish the supplied manifest and a seed file to a local bare repository.
fn origin_with(manifest: &str) -> BareRepo {
    let origin = BareRepo::new();
    origin.publish("files/gitconfig", GITCONFIG, "a file to install");
    origin.publish("batfiles.toml", manifest, "declare what to install");
    origin
}

/// The repository above, installing its one file.
fn origin() -> BareRepo {
    origin_with(&one_copy("files/gitconfig", "~/.gitconfig"))
}

/// `clone`, into the repository root the tree selects and
/// [`Tree::roots`](Tree) leaves vacant.
fn cloning(tree: &Tree, origin: &BareRepo) -> Command {
    let mut command = tree.batfiles();
    command.args(["clone", &display(&origin.origin())]);
    command
}

/// Build a clone command with no explicit repository destination.
fn cloning_unselected(tree: &Tree, origin: &BareRepo) -> Command {
    let mut command = cloning(tree, origin);
    command.env_remove("BATFILES_DIR");
    command
}

#[test]
fn a_repository_is_cloned_and_installed_in_one_command() {
    let tree = Tree::roots();
    let origin = origin();

    let assertion = cloning(&tree, &origin).assert().success();

    let repo = tree.path("repo");
    assert!(repo.join(".git").is_dir(), "the clone has no git directory");
    assert_eq!(
        fs::read_to_string(repo.join("batfiles.toml")).expect("the cloned manifest"),
        one_copy("files/gitconfig", "~/.gitconfig")
    );
    assert_eq!(
        fs::read_to_string(tree.home(".gitconfig")).expect("the installed file"),
        GITCONFIG
    );

    let stderr = stderr_of(&assertion);
    for expected in [
        format!(
            "cloned {} from {}",
            display(&repo),
            display(&origin.origin())
        ),
        format!("copied {}", display(&tree.home(".gitconfig"))),
    ] {
        assert!(stderr.contains(&expected), "no `{expected}` in:\n{stderr}");
    }
}

#[test]
fn the_default_destination_is_dotfiles_under_the_selected_home() {
    let tree = Tree::roots();
    let origin = origin();

    cloning_unselected(&tree, &origin).assert().success();

    assert!(
        tree.home("dotfiles").join("batfiles.toml").is_file(),
        "the clone did not land in the default repository directory"
    );
    assert!(tree.home(".gitconfig").is_file(), "nothing was installed");
}

#[test]
fn a_manifest_in_the_working_directory_selects_nothing() {
    let tree = Tree::roots();
    let origin = origin();
    let working = tree.repository("working");

    cloning_unselected(&tree, &origin)
        .current_dir(&working)
        .assert()
        .success();

    assert!(
        tree.home("dotfiles").join("batfiles.toml").is_file(),
        "the clone did not land in the default repository directory"
    );
    assert_eq!(
        entries(&working),
        ["batfiles.toml"],
        "the working directory was written to"
    );
}

#[test]
fn an_occupied_destination_is_refused_before_anything_is_cloned() {
    let origin = origin();
    for occupant in Occupant::ALL {
        let tree = Tree::roots();
        let repo = tree.path("repo");
        occupant.put_at(&repo);

        let assertion = cloning(&tree, &origin).assert().failure().code(1);

        let stderr = stderr_of(&assertion);
        for expected in [display(&repo), "already exists".to_owned()] {
            assert!(
                stderr.contains(&expected),
                "no `{expected}` for {occupant:?} in:\n{stderr}"
            );
        }
        assert!(
            !repo.join(".git").exists(),
            "{occupant:?} was cloned over anyway"
        );
        assert!(
            !tree.home(".gitconfig").exists(),
            "{occupant:?} was installed from anyway"
        );
    }
}

/// Occupied destination kinds that the clone command must refuse, including empty directories.
#[derive(Debug, Clone, Copy)]
enum Occupant {
    EmptyDirectory,
    File,
    #[cfg(unix)]
    LinkToNothing,
}

impl Occupant {
    #[cfg(unix)]
    const ALL: [Self; 3] = [Self::EmptyDirectory, Self::File, Self::LinkToNothing];
    #[cfg(not(unix))]
    const ALL: [Self; 2] = [Self::EmptyDirectory, Self::File];

    fn put_at(self, path: &std::path::Path) {
        match self {
            Self::EmptyDirectory => fs::create_dir(path).expect("an empty directory"),
            Self::File => fs::write(path, "not a repository\n").expect("a file"),
            #[cfg(unix)]
            Self::LinkToNothing => {
                std::os::unix::fs::symlink(path.with_file_name("absent"), path)
                    .expect("a dangling link");
            }
        }
    }
}

#[test]
fn a_git_repository_that_is_not_a_batfiles_one_is_named_as_such() {
    let tree = Tree::roots();
    let origin = BareRepo::new();

    let assertion = cloning(&tree, &origin).assert().failure().code(1);

    let stderr = stderr_of(&assertion);
    for expected in ["batfiles.toml", "not a batfiles one"] {
        assert!(stderr.contains(expected), "no `{expected}` in:\n{stderr}");
    }
    assert!(
        tree.path("repo").join("README.md").is_file(),
        "the clone was removed"
    );
}

#[test]
fn a_synchronization_that_fails_keeps_the_clone() {
    let tree = Tree::roots();
    let origin = origin_with(&one_copy("files/absent", "~/.gitconfig"));

    let assertion = cloning(&tree, &origin).assert().failure().code(1);

    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("no such file in the repository"),
        "unexpected stderr:\n{stderr}"
    );
    assert!(
        tree.path("repo").join("batfiles.toml").is_file(),
        "the clone was removed, so the manifest cannot be fixed and synchronized"
    );
    assert!(
        !tree.home(".gitconfig").exists(),
        "the failed action installed something"
    );
}

#[test]
fn the_selection_and_variable_options_reach_the_synchronization() {
    let tree = Tree::roots();
    let origin = BareRepo::new();
    for name in ["always", "skipped", "gated"] {
        origin.publish(&format!("files/{name}"), name, "a file to install");
    }
    origin.publish(
        "batfiles.toml",
        r#"[[actions]]
type = "copy"
id = "always"
source = "files/always"
dest = "~/always"

[[actions]]
type = "copy"
id = "skipped"
source = "files/skipped"
dest = "~/skipped"

[[actions]]
type = "copy"
id = "gated"
source = "files/gated"
dest = "~/gated"
when = "profile == 'work'"
"#,
        "three actions, two of them conditional on the invocation",
    );

    cloning(&tree, &origin)
        .args(["--skip-action", "skipped", "--var", "profile=work"])
        .assert()
        .success();

    assert!(
        tree.home("always").is_file(),
        "the plain action was skipped"
    );
    assert!(
        !tree.home("skipped").exists(),
        "--skip-action did not reach the synchronization"
    );
    assert!(
        tree.home("gated").is_file(),
        "--var did not reach the synchronization"
    );
}

#[test]
fn a_cloned_leaf_materializes_the_remotes_it_composes() {
    let tree = Tree::roots();
    let core = BareRepo::new();
    core.publish(
        "batfiles.toml",
        &one_copy("files/corerc", "~/.corerc"),
        "what the remote installs",
    );
    core.publish("files/corerc", "# core\n", "the file it installs");

    let origin = origin_with(&format!(
        r#"[remotes.core]
type = "git"
url = "{}"

[[actions]]
type = "include-remote"
remote = "core"
"#,
        display(&core.origin())
    ));

    cloning(&tree, &origin).assert().success();

    assert!(
        tree.path("repo")
            .join("remotes")
            .join("core")
            .join("batfiles.toml")
            .is_file(),
        "the remote was not materialized"
    );
    assert_eq!(
        fs::read_to_string(tree.home(".corerc")).expect("the included action's file"),
        "# core\n"
    );
}

#[test]
fn a_url_that_cannot_be_cloned_leaves_no_repository() {
    let tree = Tree::roots();
    let absent = tree.path("no-such-origin.git");

    let assertion = tree
        .batfiles()
        .args(["clone", &display(&absent)])
        .assert()
        .failure()
        .code(1);

    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("git clone failed"),
        "unexpected stderr:\n{stderr}"
    );
    assert!(
        !tree.path("repo").exists(),
        "a failed clone left a repository behind"
    );
}
