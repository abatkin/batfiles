//! `init`: laying the conventional skeleton into the directory it was run in.

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;

use super::support::{Tree, batfiles, display, entries, stderr_of};

/// What a fresh `init` leaves behind, sorted as [`entries`] returns it.
const SKELETON: [&str; 4] = [".gitignore", "batfiles.toml", "bin", "files"];

/// An isolated working directory and sibling home for initialization tests.
struct Workspace {
    tree: Tree,
}

impl Workspace {
    fn new() -> Self {
        let tree = Tree::roots();
        fs::create_dir(tree.path("work")).expect("a working directory");
        Self { tree }
    }

    fn dir(&self) -> PathBuf {
        self.tree.path("work")
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.dir().join(relative)
    }

    /// `init`, run in the working directory.
    fn init(&self) -> Command {
        self.init_in(&self.dir())
    }

    /// Build an `init` command running in `dir`.
    fn init_in(&self, dir: &Path) -> Command {
        let mut command = batfiles();
        command
            .current_dir(dir)
            // Set both platform-specific home variables to keep fixtures independent of the
            // real account.
            .env("HOME", self.tree.path("home"))
            .env("USERPROFILE", self.tree.path("home"))
            // Ignore the developer's Git templates and default branch settings.
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env(
                "GIT_CONFIG_GLOBAL",
                if cfg!(windows) { "NUL" } else { "/dev/null" },
            )
            .arg("init");
        command
    }
}

#[test]
fn an_empty_directory_gets_the_whole_skeleton() {
    let workspace = Workspace::new();
    let assertion = workspace.init().arg("--no-git-init").assert().success();

    assert_eq!(entries(&workspace.dir()), SKELETON);
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains(&format!(
            "initialized batfiles repository in {}",
            display(&workspace.dir())
        )),
        "unexpected stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("created batfiles.toml, .gitignore, bin/, files/"),
        "the created line is not what was created:\n{stderr}"
    );
}

#[test]
fn a_freshly_initialized_repository_synchronizes() {
    let workspace = Workspace::new();
    workspace.init().arg("--no-git-init").assert().success();

    let assertion = workspace
        .tree
        .batfiles()
        .env("BATFILES_DIR", workspace.dir())
        .arg("sync")
        .assert()
        .success();

    assert!(
        entries(&workspace.tree.path("home")).is_empty(),
        "a starter manifest installed something:\n{}",
        stderr_of(&assertion)
    );
}

#[test]
fn the_exclusion_list_covers_the_generated_remotes_tree() {
    let workspace = Workspace::new();
    workspace.init().arg("--no-git-init").assert().success();

    assert_eq!(
        fs::read_to_string(workspace.path(".gitignore")).expect("a .gitignore"),
        "/remotes/\n"
    );
    assert!(!workspace.path("remotes").exists());
}

#[test]
fn git_initialization_is_skipped_on_request() {
    let workspace = Workspace::new();
    let assertion = workspace.init().arg("--no-git-init").assert().success();

    assert!(!workspace.path(".git").exists());
    let stderr = stderr_of(&assertion);
    assert!(
        !stderr.contains("Git repository"),
        "`--no-git-init` still reported Git:\n{stderr}"
    );
}

#[test]
fn a_repository_is_initialized_by_default() {
    let workspace = Workspace::new();
    let assertion = workspace.init().assert().success();

    assert!(workspace.path(".git").is_dir());
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("initialized a Git repository"),
        "unexpected stderr:\n{stderr}"
    );
}

#[test]
fn a_directory_already_inside_a_work_tree_keeps_the_repository_it_is_in() {
    let workspace = Workspace::new();
    workspace.init().assert().success();

    let below = workspace.path("below");
    fs::create_dir(&below).expect("a directory below the repository");
    let assertion = workspace.init_in(&below).assert().success();

    assert!(!below.join(".git").exists());
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("a Git repository already covers this directory"),
        "unexpected stderr:\n{stderr}"
    );
}

#[test]
fn an_existing_manifest_refuses_the_command() {
    let workspace = Workspace::new();
    fs::write(workspace.path("batfiles.toml"), "").expect("a manifest");

    let assertion = workspace.init().assert().failure().code(1);

    assert_eq!(entries(&workspace.dir()), ["batfiles.toml"]);
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("already a batfiles repository"),
        "unexpected stderr:\n{stderr}"
    );
}

#[test]
fn a_skeleton_path_of_the_wrong_kind_refuses_the_command() {
    let workspace = Workspace::new();
    fs::write(workspace.path("files"), "not a directory\n").expect("a file in the way");

    let assertion = workspace.init().assert().failure().code(1);

    assert_eq!(entries(&workspace.dir()), ["files"]);
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains(&display(&workspace.path("files"))),
        "the refusal does not name the path:\n{stderr}"
    );
    assert!(
        stderr.contains("is not a directory"),
        "the refusal does not say what the path should be:\n{stderr}"
    );
}

#[test]
fn the_home_directory_is_refused() {
    let workspace = Workspace::new();
    let home = workspace.tree.path("home");

    let assertion = workspace.init_in(&home).assert().failure().code(1);

    assert!(entries(&home).is_empty(), "the home was initialized anyway");
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("is your home directory"),
        "unexpected stderr:\n{stderr}"
    );
}

#[test]
fn an_existing_path_of_the_right_kind_is_left_alone() {
    let workspace = Workspace::new();
    let bin = workspace.path("bin");
    fs::create_dir(&bin).expect("a bin directory");
    fs::write(bin.join("batgrep"), "#!/bin/sh\n").expect("a script");

    let assertion = workspace.init().arg("--no-git-init").assert().success();

    assert_eq!(entries(&workspace.dir()), SKELETON);
    assert_eq!(entries(&bin), ["batgrep"]);
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("created batfiles.toml, .gitignore, files/"),
        "`bin/` was reported as created:\n{stderr}"
    );
}

#[test]
fn an_exclusion_list_that_does_not_cover_remotes_is_reported() {
    let workspace = Workspace::new();
    fs::write(workspace.path(".gitignore"), "*.swp\n").expect("an exclusion list");

    let assertion = workspace.init().arg("--no-git-init").assert().success();

    assert_eq!(
        fs::read_to_string(workspace.path(".gitignore")).expect("a .gitignore"),
        "*.swp\n"
    );
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("does not ignore the tool-owned `remotes/` tree"),
        "unexpected stderr:\n{stderr}"
    );
}
