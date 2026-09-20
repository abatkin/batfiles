//! `init`: laying the conventional skeleton into the directory it was run in.

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;

use super::support::{Tree, batfiles, display, entries, stderr_of};

/// What a fresh `init` leaves behind, sorted as [`entries`] returns it.
const SKELETON: [&str; 4] = [".gitignore", "batfiles.toml", "bin", "files"];

/// A directory to run `init` in, beside a home it is not.
///
/// The two are siblings, and the home is selected rather than inherited: the
/// only thing `init` asks a home is whether it is the directory being
/// initialized, and that question needs an answer the developer's own account
/// cannot change.
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

    /// The same, in some other directory — the home, for the case that refuses
    /// it.
    fn init_in(&self, dir: &Path) -> Command {
        let mut command = batfiles();
        command
            .current_dir(dir)
            // The home batfiles reads comes from `std::env::home_dir`, which
            // consults `HOME` on Unix and `USERPROFILE` on Windows and neither
            // platform's variable on the other. Both are named so that every
            // case here has the same definite answer wherever the suite runs,
            // rather than one that depends on the real account's home.
            .env("HOME", self.tree.path("home"))
            .env("USERPROFILE", self.tree.path("home"))
            // The binary launches `git` itself, so the developer's own
            // `init.templateDir` or `init.defaultBranch` would otherwise reach
            // a fixture and change what `init` produces.
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
    // What makes the starter manifest more than a file of the right name: it is
    // read, validated, and executed by the command a new repository exists for.
    let workspace = Workspace::new();
    workspace.init().arg("--no-git-init").assert().success();

    let assertion = workspace
        .tree
        .batfiles()
        .env("BATFILES_DIR", workspace.dir())
        .arg("sync")
        .assert()
        .success();

    // Every sample is commented out, so a repository nobody has edited yet
    // installs nothing into the home.
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
    // The tree itself is generated, so nothing creates it until a remote is
    // materialized.
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

    // A second repository below the first: `init` must leave the surrounding
    // work tree alone rather than nesting one inside it.
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

    // Nothing beside it: the refusal comes before the first write.
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

    // The user's file, so it is reported rather than rewritten.
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
