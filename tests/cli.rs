//! End-to-end checks of the built `batfiles` binary.

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use tempfile::TempDir;

/// A command with nothing selected, for the cases that resolve no roots:
/// `version`, `--help`, and anything clap rejects before dispatch.
///
/// It inherits the developer's location variables, which is harmless only
/// because nothing it runs consults them. Anything that resolves a root goes
/// through [`Tree`].
fn batfiles() -> Command {
    let mut command = Command::cargo_bin("batfiles").expect("the batfiles binary should be built");
    // The tests must not inherit the developer's own color environment.
    command.env_remove("BATFILES_COLOR").env_remove("NO_COLOR");
    command
}

/// A throwaway tree standing in for the four location roots, with an empty leaf
/// manifest in place.
///
/// `sync` opens the repository root now, so tests point at directories that
/// exist rather than at fixed absolute paths. A path that is never opened —
/// an alternative home, an `$XDG_*` base — can still be written inline.
struct Tree {
    dir: TempDir,
}

impl Tree {
    /// The four roots as sibling directories, with `repo/batfiles.toml` empty
    /// but present, which is what a command needs to get past reading it.
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("a temporary directory");
        for name in ["home", "config", "cache"] {
            fs::create_dir(dir.path().join(name)).expect("a root directory");
        }
        let tree = Self { dir };
        tree.repository("repo");
        tree
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.dir.path().join(relative)
    }

    /// The tree the four roots sit in, for the cases that run from inside it.
    #[cfg(unix)]
    fn root(&self) -> &Path {
        self.dir.path()
    }

    /// Create a repository directory holding an empty manifest, and return it.
    fn repository(&self, relative: &str) -> PathBuf {
        let repo = self.path(relative);
        fs::create_dir_all(&repo).expect("a repository directory");
        fs::write(repo.join("batfiles.toml"), "").expect("a manifest");
        repo
    }

    fn manifest(&self) -> PathBuf {
        self.path("repo").join("batfiles.toml")
    }

    /// Replace the leaf manifest.
    fn write_manifest(&self, contents: &str) {
        fs::write(self.manifest(), contents).expect("a manifest");
    }

    /// Put a file in the leaf repository, and return where it landed.
    fn repo_file(&self, relative: &str, contents: &str) -> PathBuf {
        let path = self.path("repo").join(relative);
        fs::create_dir_all(path.parent().expect("a parent")).expect("a source directory");
        fs::write(&path, contents).expect("a source file");
        path
    }

    /// A path inside the selected home, which need not exist.
    fn home(&self, relative: &str) -> PathBuf {
        self.path("home").join(relative)
    }

    /// A command with all four roots selected inside this tree.
    fn batfiles(&self) -> Command {
        let mut command = batfiles();
        command
            .env("BATFILES_HOME", self.path("home"))
            .env("BATFILES_DIR", self.path("repo"))
            .env("BATFILES_CONFIG_DIR", self.path("config"))
            .env("BATFILES_CACHE_DIR", self.path("cache"))
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("XDG_CACHE_HOME");
        command
    }
}

fn display(path: &Path) -> String {
    path.display().to_string()
}

/// A manifest declaring one symlink and nothing else.
fn one_symlink(source: &str, dest: &str) -> String {
    format!("[[actions]]\ntype = \"symlink\"\nsource = \"{source}\"\ndest = \"{dest}\"\n")
}

/// Where a symlink points, without following it.
#[cfg(unix)]
fn link_target(path: &Path) -> PathBuf {
    fs::read_link(path)
        .unwrap_or_else(|error| panic!("{} is not a symlink: {error}", path.display()))
}

fn stderr_of(assertion: &assert_cmd::assert::Assert) -> String {
    String::from_utf8_lossy(&assertion.get_output().stderr).into_owned()
}

#[test]
fn version_prints_the_package_version_and_succeeds() {
    let expected = format!("batfiles {}\n", env!("CARGO_PKG_VERSION"));
    batfiles()
        .arg("version")
        .assert()
        .success()
        .stdout(expected);
}

#[test]
fn version_matches_the_version_flag() {
    let command = batfiles().arg("version").assert().success();
    let flag = batfiles().arg("--version").assert().success();
    assert_eq!(command.get_output().stdout, flag.get_output().stdout);
}

#[test]
fn help_lists_every_documented_command() {
    let assertion = batfiles().arg("--help").assert().success();
    let help = String::from_utf8_lossy(&assertion.get_output().stdout).into_owned();

    for command in [
        "init",
        "version",
        "clone",
        "sync",
        "disable-action",
        "enable-action",
        "disable-group",
        "enable-group",
        "apply-action",
        "apply-group",
        "vars",
    ] {
        assert!(
            help.contains(command),
            "`{command}` missing from help:\n{help}"
        );
    }
}

/// A command that resolves its roots and then reports that it does not exist
/// yet. `sync` used to be the specimen; it runs now.
fn a_stub() -> [&'static str; 2] {
    ["clone", "https://example.invalid/dotfiles.git"]
}

#[test]
fn unimplemented_commands_fail_with_a_clear_message() {
    let tree = Tree::new();
    let assertion = tree.batfiles().args(a_stub()).assert().failure().code(2);
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("`clone` is not implemented yet"),
        "unexpected stderr:\n{stderr}"
    );
}

#[test]
fn an_unimplemented_subcommand_is_named_in_full() {
    let tree = Tree::new();
    let assertion = tree
        .batfiles()
        .args(["vars", "get", "profile"])
        .assert()
        .failure()
        .code(2);
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("`vars get` is not implemented yet"),
        "unexpected stderr:\n{stderr}"
    );
}

#[test]
fn an_unimplemented_command_writes_nothing_to_standard_output() {
    let tree = Tree::new();
    tree.batfiles().args(a_stub()).assert().failure().stdout("");
}

#[test]
fn usage_errors_exit_with_two() {
    batfiles().arg("not-a-command").assert().failure().code(2);
    batfiles()
        .args(["sync", "-v", "-q"])
        .assert()
        .failure()
        .code(2);
}

#[test]
fn an_invalid_batfiles_color_warns_without_changing_the_exit_status() {
    let assertion = batfiles()
        .env("BATFILES_COLOR", "sometimes")
        .arg("version")
        .assert()
        .success();
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("BATFILES_COLOR"),
        "unexpected stderr:\n{stderr}"
    );
}

#[test]
fn a_valid_batfiles_color_is_quiet() {
    batfiles()
        .env("BATFILES_COLOR", "never")
        .arg("version")
        .assert()
        .success()
        .stderr("");
}

// The next three cases cover output that clap produces itself, which is
// rendered before there is a parsed command to consult.

#[test]
fn an_invalid_batfiles_color_warns_even_when_clap_handles_the_arguments() {
    for args in [vec!["--version"], vec!["--help"], vec!["not-a-command"]] {
        let assertion = batfiles()
            .env("BATFILES_COLOR", "sometimes")
            .args(&args)
            .assert();
        let stderr = stderr_of(&assertion);
        assert!(
            stderr.contains("BATFILES_COLOR"),
            "no warning for {args:?}:\n{stderr}"
        );
    }
}

#[test]
fn color_always_applies_to_a_clap_usage_error() {
    let assertion = batfiles()
        .args(["--color", "always", "not-a-command"])
        .assert()
        .code(2);
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains('\x1b'),
        "expected escape sequences:\n{stderr}"
    );
}

#[test]
fn color_never_applies_to_a_clap_usage_error() {
    let assertion = batfiles()
        .args(["--color=never", "not-a-command"])
        .assert()
        .code(2);
    let stderr = stderr_of(&assertion);
    assert!(
        !stderr.contains('\x1b'),
        "expected no escape sequences:\n{stderr}"
    );
}

// Location resolution. The repository and the home are read and written, so
// `-v` is how the other two are observed from outside the binary.

#[test]
fn verbose_reports_every_resolved_root() {
    let tree = Tree::new();
    let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();
    let stderr = stderr_of(&assertion);
    for (label, root) in [
        ("repository: ", "repo"),
        ("home:       ", "home"),
        ("config:     ", "config"),
        ("cache:      ", "cache"),
    ] {
        let expected = format!("{label}{}", display(&tree.path(root)));
        assert!(stderr.contains(&expected), "no `{expected}` in:\n{stderr}");
    }
}

#[test]
fn the_roots_are_reported_only_when_asked_for() {
    let tree = Tree::new();
    let assertion = tree.batfiles().arg("sync").assert().success();
    let stderr = stderr_of(&assertion);
    assert!(
        !stderr.contains(&display(&tree.path("repo"))),
        "unexpected detail:\n{stderr}"
    );
}

#[test]
fn a_location_option_outranks_its_variable() {
    let tree = Tree::new();
    let chosen = tree.repository("from-the-option");
    let assertion = tree
        .batfiles()
        .args(["sync", "-v", "--batfiles-dir"])
        .arg(&chosen)
        .assert()
        .success();
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains(&format!("repository: {}", display(&chosen))),
        "the option did not win:\n{stderr}"
    );
    assert!(
        stderr.contains(&format!("home:       {}", display(&tree.path("home")))),
        "an unselected root changed:\n{stderr}"
    );
}

#[test]
fn the_leaf_repository_defaults_under_the_selected_home() {
    let tree = Tree::new();
    let default = tree.repository("home/dotfiles");
    let assertion = tree
        .batfiles()
        .env_remove("BATFILES_DIR")
        .args(["sync", "-v"])
        .assert()
        .success();
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains(&format!("repository: {}", display(&default))),
        "unexpected default:\n{stderr}"
    );
}

#[test]
fn config_and_cache_do_not_follow_the_selected_home() {
    // Batfiles' own state belongs to the invoking user, not to whichever home
    // is being installed into, so `--home-dir` must not move it. None of the
    // three paths named inline here is opened by any command yet.
    let tree = Tree::new();
    let assertion = tree
        .batfiles()
        .env_remove("BATFILES_CONFIG_DIR")
        .env_remove("BATFILES_CACHE_DIR")
        .env("XDG_CONFIG_HOME", "/xdg-config")
        .env("XDG_CACHE_HOME", "/xdg-cache")
        .args(["sync", "-v", "--home-dir", "/elsewhere"])
        .assert()
        .success();
    let stderr = stderr_of(&assertion);
    for expected in [
        "home:       /elsewhere",
        "config:     /xdg-config/batfiles",
        "cache:      /xdg-cache/batfiles",
    ] {
        assert!(stderr.contains(expected), "no `{expected}` in:\n{stderr}");
    }
}

#[test]
fn version_resolves_no_roots() {
    batfiles()
        .args(["version", "-v"])
        .assert()
        .success()
        .stderr("");
}

#[test]
fn init_resolves_no_roots() {
    let assertion = batfiles().args(["init", "-v"]).assert().failure().code(2);
    let stderr = stderr_of(&assertion);
    assert!(
        !stderr.contains("repository:"),
        "`init` resolved roots:\n{stderr}"
    );
    assert!(
        stderr.contains("`init` is not implemented yet"),
        "unexpected stderr:\n{stderr}"
    );
}

// The leaf manifest. `sync` parses it before anything else, so a repository
// without one fails there rather than at the unimplemented stub.

#[test]
fn a_repository_without_a_manifest_fails_and_names_the_file() {
    let tree = Tree::new();
    fs::remove_file(tree.manifest()).expect("the fixture manifest");

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains(&display(&tree.manifest())),
        "the missing file was not named:\n{stderr}"
    );
    assert!(
        !stderr.contains("is not implemented yet"),
        "the stub ran anyway:\n{stderr}"
    );
}

#[test]
fn a_malformed_manifest_names_the_file_and_where_it_broke() {
    let tree = Tree::new();
    fs::write(tree.manifest(), "[[actions]\n").expect("a malformed manifest");

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);
    let stderr = stderr_of(&assertion);
    for expected in [display(&tree.manifest()), "line 1".to_owned()] {
        assert!(stderr.contains(&expected), "no `{expected}` in:\n{stderr}");
    }
}

/// Run `sync` against a manifest expected to be rejected, and return the
/// diagnostic.
///
/// Every rejection is the same shape: status 1, the file named, and the stub
/// never reached — a manifest batfiles cannot make sense of stops the command
/// before it claims to have done anything.
fn rejected(manifest: &str) -> String {
    let tree = Tree::new();
    fs::write(tree.manifest(), manifest).expect("a manifest");

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

// Asserting the link makes this one platform-specific, unlike the rejections
// around it, which never get as far as executing anything.
#[cfg(unix)]
#[test]
fn a_symlink_action_parses_with_every_field_it_accepts() {
    let tree = Tree::new();
    tree.repo_file("files/zshrc", "# zsh\n");
    tree.write_manifest(
        "[[actions]]\n\
         type = \"symlink\"\n\
         id = \"zshrc\"\n\
         group = \"shell\"\n\
         source = \"files/zshrc\"\n\
         dest = \"~/.zshrc\"\n",
    );

    tree.batfiles().arg("sync").assert().success();
    assert!(tree.home(".zshrc").is_symlink());
}

#[test]
fn a_section_from_a_slice_that_has_not_landed_is_rejected() {
    // The document is closed, so a section batfiles will understand later is an
    // error now rather than something that looks as though it took effect.
    let stderr = rejected("[vars]\nwork = \"true\"\n");
    assert!(
        stderr.contains("vars"),
        "the section was not named:\n{stderr}"
    );
}

#[test]
fn a_field_the_symlink_record_does_not_have_yet_is_rejected() {
    // Directory mode is specified and not built. Ignoring it would link
    // nothing while looking like it linked a directory's worth.
    let stderr = rejected(
        "[[actions]]\n\
         type = \"symlink\"\n\
         source-dir = \"files\"\n\
         dest = \"~\"\n",
    );
    assert!(
        stderr.contains("source-dir"),
        "the field was not named:\n{stderr}"
    );
}

#[test]
fn an_action_type_that_has_not_landed_is_rejected() {
    let stderr = rejected(
        "[[actions]]\n\
         type = \"copy\"\n\
         source = \"local-files\"\n\
         dest = \"~\"\n",
    );
    assert!(stderr.contains("copy"), "the type was not named:\n{stderr}");
}

#[test]
fn an_action_missing_a_required_field_is_rejected() {
    let stderr = rejected(
        "[[actions]]\n\
         type = \"symlink\"\n\
         source = \"files/zshrc\"\n",
    );
    assert!(
        stderr.contains("dest"),
        "the field was not named:\n{stderr}"
    );
}

#[test]
fn an_id_that_breaks_the_id_rule_is_rejected() {
    let stderr = rejected(
        "[[actions]]\n\
         type = \"symlink\"\n\
         id = \"core.zshrc\"\n\
         source = \"files/zshrc\"\n\
         dest = \"~/.zshrc\"\n",
    );
    for expected in ["core.zshrc", "not a valid ID"] {
        assert!(stderr.contains(expected), "no `{expected}` in:\n{stderr}");
    }
}

#[test]
fn a_repeated_action_id_is_rejected_and_both_uses_located() {
    // Serde cannot see across records, so this is the rule `validate` exists
    // for; the diagnostic points at both actions because either one could be
    // the mistake.
    let stderr = rejected(
        "[[actions]]\n\
         type = \"symlink\"\n\
         id = \"zshrc\"\n\
         source = \"files/zshrc\"\n\
         dest = \"~/.zshrc\"\n\
         \n\
         [[actions]]\n\
         type = \"symlink\"\n\
         id = \"zshrc\"\n\
         source = \"files/zshrc.local\"\n\
         dest = \"~/.zshrc.local\"\n",
    );
    for expected in ["zshrc", "action 2", "action 1"] {
        assert!(stderr.contains(expected), "no `{expected}` in:\n{stderr}");
    }
}

#[test]
fn a_command_that_does_not_need_the_manifest_does_not_read_it() {
    // Reading is the responsibility of the commands that use the manifest, so
    // one that never looks at it is unaffected by a broken one.
    let tree = Tree::new();
    fs::write(tree.manifest(), "[[actions]\n").expect("a malformed manifest");

    let assertion = tree
        .batfiles()
        .args(["vars", "get", "profile"])
        .assert()
        .failure()
        .code(2);
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("`vars get` is not implemented yet"),
        "unexpected stderr:\n{stderr}"
    );
}

// Executing symlink actions, which is the whole of what `sync` does so far.
//
// Gated as a whole: where batfiles cannot make a symlink it refuses the action
// before resolving anything, so none of these has a meaningful non-unix form —
// and several build their fixtures with `symlink` themselves. The portable
// tests stay outside so the suite still compiles and runs elsewhere. Action
// types that are not platform-specific do not belong in here.
#[cfg(unix)]
mod linking {
    use super::*;

    #[test]
    fn a_symlink_action_creates_the_link_and_says_so() {
        let tree = Tree::new();
        let source = tree.repo_file("shell/zshrc", "# zsh\n");
        tree.write_manifest(&one_symlink("shell/zshrc", "~/.zshrc"));

        let assertion = tree
            .batfiles()
            .args(["--color", "never", "sync"])
            .assert()
            .success();
        assert_eq!(
            stderr_of(&assertion),
            format!(
                "linked {} -> {}\n",
                display(&tree.home(".zshrc")),
                display(&source)
            )
        );
        assert_eq!(link_target(&tree.home(".zshrc")), source);
    }

    #[test]
    fn every_action_in_the_manifest_runs() {
        let tree = Tree::new();
        tree.repo_file("shell/zshrc", "# zsh\n");
        tree.repo_file("shell/inputrc", "# readline\n");
        tree.write_manifest(&format!(
            "{}{}",
            one_symlink("shell/zshrc", "~/.zshrc"),
            one_symlink("shell/inputrc", "~/.inputrc")
        ));

        tree.batfiles().arg("sync").assert().success();
        assert!(tree.home(".zshrc").is_symlink());
        assert!(tree.home(".inputrc").is_symlink());
    }

    #[test]
    fn a_link_into_the_repository_is_repaired() {
        // Repointing a link batfiles would have made loses nothing: the link holds
        // no content of its own, and the file it pointed at is untouched.
        let tree = Tree::new();
        let stale = tree.repo_file("shell/zshrc.old", "# old\n");
        let wanted = tree.repo_file("shell/zshrc", "# zsh\n");
        std::os::unix::fs::symlink(&stale, tree.home(".zshrc")).expect("a stale link");
        tree.write_manifest(&one_symlink("shell/zshrc", "~/.zshrc"));

        let assertion = tree
            .batfiles()
            .args(["--color", "never", "sync"])
            .assert()
            .success();
        assert_eq!(
            stderr_of(&assertion),
            format!(
                "relinked {} -> {} (was {})\n",
                display(&tree.home(".zshrc")),
                display(&wanted),
                display(&stale)
            )
        );
        assert_eq!(link_target(&tree.home(".zshrc")), wanted);
        assert!(stale.exists(), "the old source was removed");
    }

    #[test]
    fn a_relative_link_pointing_at_the_wrong_file_is_repaired() {
        // The spelling decides nothing: this one resolves into the repository,
        // so it is repairable, and the replacement is written anchored like any
        // other. The neighbouring cases cover a relative link that is already
        // right and an absolute one that is wrong.
        let tree = Tree::new();
        tree.repo_file("shell/zshrc.old", "# old\n");
        let wanted = tree.repo_file("shell/zshrc", "# zsh\n");
        std::os::unix::fs::symlink("../repo/shell/zshrc.old", tree.home(".zshrc"))
            .expect("a stale relative link");
        tree.write_manifest(&one_symlink("shell/zshrc", "~/.zshrc"));

        tree.batfiles().arg("sync").assert().success();
        assert_eq!(link_target(&tree.home(".zshrc")), wanted);
    }

    #[test]
    fn a_relative_repository_still_yields_a_link_that_resolves() {
        // A symlink stores the target it is handed, and a relative one is read back
        // from the link's own directory — not from wherever batfiles was run. So a
        // relative root has to be anchored before it is written into a link, or the
        // command reports success and leaves something pointing nowhere.
        let tree = Tree::new();
        let source = tree.repo_file("shell/zshrc", "# zsh\n");
        tree.write_manifest(&one_symlink("shell/zshrc", "~/.zshrc"));

        tree.batfiles()
            .current_dir(tree.root())
            .args(["sync", "--batfiles-dir", "repo", "--home-dir", "home"])
            .assert()
            .success();
        assert_eq!(link_target(&tree.home(".zshrc")), source);
        assert_eq!(
            fs::read_to_string(tree.home(".zshrc")).expect("the link resolves"),
            "# zsh\n"
        );
    }

    #[test]
    fn a_relative_link_into_the_repository_is_recognized() {
        // Someone may well have written this link by hand, or with a tool that
        // spells targets relatively. It points where the action asks, so there is
        // nothing to do.
        let tree = Tree::new();
        tree.repo_file("shell/zshrc", "# zsh\n");
        std::os::unix::fs::symlink("../repo/shell/zshrc", tree.home(".zshrc"))
            .expect("a relative link");
        tree.write_manifest(&one_symlink("shell/zshrc", "~/.zshrc"));

        tree.batfiles().arg("sync").assert().success().stderr("");
        assert_eq!(
            link_target(&tree.home(".zshrc")),
            PathBuf::from("../repo/shell/zshrc"),
            "the spelling was rewritten"
        );
    }

    #[test]
    fn a_link_that_only_looks_like_it_points_into_the_repository_is_refused() {
        // `<repo>/../outside` starts with the repository when compared as text and
        // leaves it when resolved. Reading the spelling rather than the destination
        // would delete a link batfiles never made.
        let tree = Tree::new();
        tree.repo_file("shell/zshrc", "# zsh\n");
        let escaping = tree.path("repo").join("../outside");
        std::os::unix::fs::symlink(&escaping, tree.home(".zshrc")).expect("an escaping link");

        let stderr = refused(&tree, &one_symlink("shell/zshrc", "~/.zshrc"));
        assert!(
            stderr.contains(&display(&tree.home(".zshrc"))),
            "the destination was not named:\n{stderr}"
        );
        assert_eq!(link_target(&tree.home(".zshrc")), escaping);
    }

    #[test]
    fn a_link_that_is_already_right_is_left_alone() {
        let tree = Tree::new();
        tree.repo_file("shell/zshrc", "# zsh\n");
        tree.write_manifest(&one_symlink("shell/zshrc", "~/.zshrc"));
        tree.batfiles().arg("sync").assert().success();

        // A converged repository is the common case, so it says nothing at all —
        // and `-v` is how you check that it looked.
        tree.batfiles().arg("sync").assert().success().stderr("");
        let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();
        let stderr = stderr_of(&assertion);
        let expected = format!("unchanged {}", display(&tree.home(".zshrc")));
        assert!(stderr.contains(&expected), "no `{expected}` in:\n{stderr}");
    }

    #[test]
    fn quiet_suppresses_what_sync_did() {
        let tree = Tree::new();
        tree.repo_file("shell/zshrc", "# zsh\n");
        tree.write_manifest(&one_symlink("shell/zshrc", "~/.zshrc"));

        tree.batfiles()
            .args(["sync", "--quiet"])
            .assert()
            .success()
            .stderr("");
        assert!(tree.home(".zshrc").is_symlink());
    }

    #[test]
    fn a_missing_parent_of_a_destination_is_created() {
        let tree = Tree::new();
        let source = tree.repo_file("nvim/init.lua", "-- nvim\n");
        tree.write_manifest(&one_symlink("nvim/init.lua", "~/.config/nvim/init.lua"));

        tree.batfiles().arg("sync").assert().success();
        assert_eq!(link_target(&tree.home(".config/nvim/init.lua")), source);
    }

    /// Run `sync` against a manifest expected to fail while executing, and return
    /// the diagnostic. Status 1: the command started work and stopped.
    fn refused(tree: &Tree, manifest: &str) -> String {
        tree.write_manifest(manifest);
        let assertion = tree.batfiles().arg("sync").assert().failure().code(1);
        stderr_of(&assertion)
    }

    #[test]
    fn a_destination_holding_a_file_is_refused_and_the_file_is_left() {
        let tree = Tree::new();
        tree.repo_file("shell/zshrc", "# zsh\n");
        fs::write(tree.home(".zshrc"), "mine\n").expect("an existing file");

        let stderr = refused(&tree, &one_symlink("shell/zshrc", "~/.zshrc"));
        for expected in [display(&tree.home(".zshrc")), "a regular file".to_owned()] {
            assert!(stderr.contains(&expected), "no `{expected}` in:\n{stderr}");
        }
        assert_eq!(
            fs::read_to_string(tree.home(".zshrc")).expect("the file"),
            "mine\n"
        );
    }

    #[test]
    fn a_destination_holding_a_directory_is_refused_and_the_directory_is_left() {
        let tree = Tree::new();
        tree.repo_file("nvim/init.lua", "-- nvim\n");
        fs::create_dir(tree.home(".config")).expect("an existing directory");
        fs::write(tree.home(".config/theirs"), "mine\n").expect("a file inside it");

        let stderr = refused(&tree, &one_symlink("nvim/init.lua", "~/.config"));
        for expected in [display(&tree.home(".config")), "a directory".to_owned()] {
            assert!(stderr.contains(&expected), "no `{expected}` in:\n{stderr}");
        }
        assert_eq!(
            fs::read_to_string(tree.home(".config/theirs")).expect("the file"),
            "mine\n"
        );
    }

    /// A destination that is none of the three node types batfiles reasons
    /// about.
    ///
    /// A fifo rather than a unix socket: binding one needs `socket(2)`, which a
    /// restricted runner may refuse, and caps the path at the length of
    /// `sun_path`, which a long `TMPDIR` exceeds on its own. `mkfifo` is an
    /// ordinary filesystem call in a directory the suite already writes to.
    fn mkfifo(path: &Path) {
        let status = std::process::Command::new("mkfifo")
            .arg(path)
            .status()
            .expect("mkfifo(1) is POSIX and this module is unix-only");
        assert!(status.success(), "mkfifo {} failed", path.display());
    }

    #[test]
    fn a_destination_that_is_neither_file_directory_nor_link_is_refused() {
        // Whatever this is, batfiles has no way to give it back, which is the
        // whole of rule 13's reasoning.
        let tree = Tree::new();
        tree.repo_file("shell/zshrc", "# zsh\n");
        let fifo = tree.home(".zshrc");
        mkfifo(&fifo);

        let stderr = refused(&tree, &one_symlink("shell/zshrc", "~/.zshrc"));
        assert!(
            stderr.contains("neither a regular file"),
            "the node found there was not described:\n{stderr}"
        );
        assert!(fifo.exists(), "the fifo was removed");
    }

    #[test]
    fn a_destination_holding_a_link_out_of_the_repository_is_refused() {
        // Someone else made this link, and where it points is not batfiles' to
        // decide.
        let tree = Tree::new();
        tree.repo_file("shell/zshrc", "# zsh\n");
        let elsewhere = tree.path("elsewhere");
        std::os::unix::fs::symlink(&elsewhere, tree.home(".zshrc")).expect("an unmanaged link");

        let stderr = refused(&tree, &one_symlink("shell/zshrc", "~/.zshrc"));
        // An absolute target resolves to itself, so it is named once and not
        // reported as though two paths were involved.
        let expected = format!(
            "a symlink to {}, which is outside the repository",
            display(&elsewhere)
        );
        for expected in [display(&tree.home(".zshrc")), expected] {
            assert!(stderr.contains(&expected), "no `{expected}` in:\n{stderr}");
        }
        assert_eq!(link_target(&tree.home(".zshrc")), elsewhere);
    }

    #[test]
    fn a_refused_link_is_named_as_written_and_as_it_resolves() {
        // The spelling is what the user will see from `ls`; the resolved path is
        // what the refusal was decided on. A relative target is where the two
        // differ, and neither alone explains the other.
        let tree = Tree::new();
        tree.repo_file("shell/zshrc", "# zsh\n");
        std::os::unix::fs::symlink("../elsewhere", tree.home(".zshrc")).expect("an unmanaged link");

        let stderr = refused(&tree, &one_symlink("shell/zshrc", "~/.zshrc"));
        for expected in ["../elsewhere".to_owned(), display(&tree.path("elsewhere"))] {
            assert!(stderr.contains(&expected), "no `{expected}` in:\n{stderr}");
        }
        assert_eq!(
            link_target(&tree.home(".zshrc")),
            PathBuf::from("../elsewhere")
        );
    }

    #[test]
    fn a_source_outside_the_repository_is_refused() {
        let tree = Tree::new();
        for source in ["../secrets", "shell/../../secrets", "/etc/hosts"] {
            let stderr = refused(&tree, &one_symlink(source, "~/.zshrc"));
            assert!(
                stderr.contains(source),
                "`{source}` was not named:\n{stderr}"
            );
            assert!(
                !tree.home(".zshrc").is_symlink(),
                "`{source}` was linked anyway"
            );
        }
    }

    #[test]
    fn a_source_the_repository_does_not_have_is_refused() {
        // A link to nothing is silent breakage, and the repository not containing
        // what it names is a mistake batfiles can see.
        let tree = Tree::new();
        let stderr = refused(&tree, &one_symlink("shell/zshrc", "~/.zshrc"));
        assert!(
            stderr.contains(&display(&tree.path("repo").join("shell/zshrc"))),
            "the source was not named:\n{stderr}"
        );
        assert!(
            !tree.home(".zshrc").is_symlink(),
            "a dangling link was made"
        );
    }
}

/// The other side of the gate above: what a `symlink` action does where
/// batfiles cannot make one. Nothing else in the suite reaches this path, and
/// no CI runner reaches this platform, so it is the whole of that coverage.
#[cfg(not(unix))]
#[test]
fn a_symlink_action_reports_that_the_platform_cannot_run_it() {
    let tree = Tree::new();
    tree.repo_file("shell/zshrc", "# zsh\n");
    tree.write_manifest(&one_symlink("shell/zshrc", "~/.zshrc"));

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("`symlink` actions are not supported"),
        "unexpected stderr:\n{stderr}"
    );
    assert!(
        !tree.home(".zshrc").exists(),
        "the destination was touched anyway"
    );
}

#[test]
fn an_option_sync_does_not_honor_yet_stops_it_before_it_writes() {
    let tree = Tree::new();
    tree.repo_file("shell/zshrc", "# zsh\n");
    tree.write_manifest(&one_symlink("shell/zshrc", "~/.zshrc"));

    // Status 2, not 1: nothing was attempted, so the invocation can be
    // corrected and retried freely.
    let assertion = tree
        .batfiles()
        .args(["sync", "--dry-run"])
        .assert()
        .failure()
        .code(2);
    let stderr = stderr_of(&assertion);
    for expected in ["--dry-run", "2.4"] {
        assert!(stderr.contains(expected), "no `{expected}` in:\n{stderr}");
    }
    assert!(
        !tree.home(".zshrc").exists(),
        "an unsupported option wrote anyway"
    );
}

#[test]
fn an_unsupported_option_is_reported_before_the_roots_are_resolved() {
    // A machine with no determinable home would otherwise fail with
    // `could not determine a home directory`, which explains nothing about the
    // option that was actually the problem.
    let assertion = batfiles()
        .env_remove("HOME")
        .env_remove("BATFILES_HOME")
        .env_remove("BATFILES_DIR")
        .env_remove("BATFILES_CONFIG_DIR")
        .env_remove("BATFILES_CACHE_DIR")
        .args(["sync", "--interactive"])
        .assert()
        .failure()
        .code(2);
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("--interactive"),
        "unexpected stderr:\n{stderr}"
    );
}

// Batfiles' own diagnostics, as opposed to the ones clap renders.

#[test]
fn an_error_is_labeled() {
    let tree = Tree::new();
    let assertion = tree
        .batfiles()
        .args(["--color", "never"])
        .args(a_stub())
        .assert()
        .failure()
        .code(2);
    let stderr = stderr_of(&assertion);
    assert_eq!(stderr, "error: `clone` is not implemented yet\n");
}

#[test]
fn color_always_colors_the_label_of_an_error_batfiles_raised() {
    let tree = Tree::new();
    let assertion = tree
        .batfiles()
        .args(["--color", "always"])
        .args(a_stub())
        .assert()
        .failure()
        .code(2);
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("\x1b[1;31merror:\x1b[0m `clone` is not implemented yet"),
        "expected a colored label:\n{stderr:?}"
    );
}

#[test]
fn an_invalid_batfiles_color_is_not_reported_when_the_option_settled_it() {
    // Precedence stops at the first input that answers, so a value the
    // resolution never consulted is never validated either.
    batfiles()
        .env("BATFILES_COLOR", "sometimes")
        .args(["--color", "never", "version"])
        .assert()
        .success()
        .stderr("");
}
