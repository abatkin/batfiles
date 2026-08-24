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
/// `sync` opens the repository root, so tests point at directories that exist
/// rather than at fixed absolute paths. A path that is never opened — an
/// alternative home, an `$XDG_*` base — can still be written inline.
struct Tree {
    dir: TempDir,
}

impl Tree {
    /// The four roots as sibling directories, with `repo/batfiles.toml` empty
    /// but present, which is what a command needs to get past reading it.
    fn new() -> Self {
        let tree = Self::roots();
        tree.repository("repo");
        tree
    }

    /// The same roots, with the leaf repository copied from
    /// `tests/fixtures/<name>` rather than holding a manifest written inline.
    ///
    /// One directory per repository shape, and copied rather than pointed at:
    /// the suite writes only inside the temporary tree, so a fixture is never
    /// mutated in place by a run.
    fn fixture(name: &str) -> Self {
        let tree = Self::roots();
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name);
        copy_tree(&source, &tree.path("repo"));
        tree
    }

    /// The three roots that are directories in their own right, with nothing in
    /// the repository yet.
    fn roots() -> Self {
        let dir = tempfile::tempdir().expect("a temporary directory");
        for name in ["home", "config", "cache"] {
            fs::create_dir(dir.path().join(name)).expect("a root directory");
        }
        Self { dir }
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

/// Copy a directory tree, creating `to` and everything beneath it.
fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("a destination directory");
    for entry in fs::read_dir(from).expect("a fixture directory") {
        let entry = entry.expect("a directory entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("a file type").is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).expect("a fixture file");
        }
    }
}

/// The names directly inside a directory, sorted, for asserting that a run
/// installed everything it should have and nothing else.
fn entries(dir: &Path) -> Vec<String> {
    let mut found: Vec<String> = fs::read_dir(dir)
        .expect("a readable directory")
        .map(|entry| {
            entry
                .expect("a directory entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    found.sort();
    found
}

/// A manifest declaring one symlink and nothing else.
fn one_symlink(source: &str, dest: &str) -> String {
    format!("[[actions]]\ntype = \"symlink\"\nsource = \"{source}\"\ndest = \"{dest}\"\n")
}

/// A manifest declaring one `symlink-dir` and nothing else.
fn one_symlink_dir(source_dir: &str, dest_dir: &str, dot_prefix: bool) -> String {
    format!(
        "[[actions]]\n\
         type = \"symlink-dir\"\n\
         source-dir = \"{source_dir}\"\n\
         dest-dir = \"{dest_dir}\"\n\
         dot-prefix = {dot_prefix}\n"
    )
}

/// A manifest declaring one `create-dir` and nothing else.
fn one_create_dir(dest: &str) -> String {
    format!("[[actions]]\ntype = \"create-dir\"\ndest = \"{dest}\"\n")
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

// The rejections below never get as far as executing anything, so they run
// everywhere. Their positive counterpart — a record using every field it
// accepts, which has to be executed to be worth asserting — is
// `linking::a_symlink_action_parses_with_every_field_it_accepts`.

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
fn the_two_symlink_types_do_not_share_a_field_set() {
    // `symlink` links one path and `symlink-dir` links a directory's children.
    // Each record is closed, so a field belonging to the other type is an
    // error rather than something quietly ignored — which is what borrowing
    // one field from the wrong type would otherwise be.
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

    let stderr = rejected(
        "[[actions]]\n\
         type = \"symlink-dir\"\n\
         source = \"files/zshrc\"\n\
         dest-dir = \"~\"\n",
    );
    assert!(
        stderr.contains("source"),
        "the field was not named:\n{stderr}"
    );
}

#[test]
fn a_filter_symlink_dir_does_not_have_yet_is_rejected() {
    // `include` and `exclude` are specified in `docs/future/repoformat.md` and
    // not built. Ignoring one would link every child while looking as though
    // it had linked a chosen few, which is the worse of the two failures.
    let stderr = rejected(
        "[[actions]]\n\
         type = \"symlink-dir\"\n\
         source-dir = \"files\"\n\
         dest-dir = \"~\"\n\
         exclude = \"README.md\"\n",
    );
    assert!(
        stderr.contains("exclude"),
        "the field was not named:\n{stderr}"
    );
}

#[test]
fn a_symlink_dir_missing_a_required_field_is_rejected() {
    let stderr = rejected(
        "[[actions]]\n\
         type = \"symlink-dir\"\n\
         source-dir = \"files\"\n",
    );
    assert!(
        stderr.contains("dest-dir"),
        "the field was not named:\n{stderr}"
    );
}

#[test]
fn a_symlink_dirs_paths_follow_the_same_rules_as_a_symlinks() {
    // `source-dir` is a source and `dest-dir` is a destination, so both are
    // decided from the manifest alone by the same two checkers. The
    // repository-root case matters more here than it does for `symlink`:
    // it would link `batfiles.toml` and `.git` into the home rather than
    // install one of them.
    for (source_dir, dest_dir, expected) in [
        ("../secrets", "~", "resolves outside the repository"),
        ("/etc", "~", "not relative to the repository root"),
        (".", "~", "names the whole repository"),
        ("", "~", "source is empty"),
        ("files", "", "dest is empty"),
        ("files", "~other/x", "another user's home"),
    ] {
        let stderr = rejected(&one_symlink_dir(source_dir, dest_dir, false));
        assert!(
            stderr.contains(expected),
            "no `{expected}` for `{source_dir}` -> `{dest_dir}` in:\n{stderr}"
        );
    }
}

#[test]
fn a_create_dir_has_nothing_to_install_and_so_takes_no_source() {
    // The record is closed like every other, and this is the field someone
    // reaches for by habit. There is no source because nothing is installed —
    // linking a directory's contents is what the two symlink types are for.
    let stderr = rejected(
        "[[actions]]\n\
         type = \"create-dir\"\n\
         source = \"files\"\n\
         dest = \"~/.config\"\n",
    );
    assert!(
        stderr.contains("source"),
        "the field was not named:\n{stderr}"
    );
}

#[test]
fn a_create_dir_without_a_destination_is_rejected() {
    let stderr = rejected("[[actions]]\ntype = \"create-dir\"\n");
    assert!(
        stderr.contains("dest"),
        "the field was not named:\n{stderr}"
    );
}

#[test]
fn a_create_dirs_destination_follows_the_same_rules_as_a_symlinks() {
    // Its `dest` is a destination like any other, decided from the manifest
    // alone by the same checker.
    for (dest, expected) in [
        ("", "dest is empty"),
        ("~other/.config", "another user's home"),
    ] {
        let stderr = rejected(&one_create_dir(dest));
        assert!(
            stderr.contains(expected),
            "no `{expected}` for dest `{dest}` in:\n{stderr}"
        );
    }
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

// What a `source` and a `dest` may say is decided from the manifest alone, so
// these are rejections like the ones above rather than actions that failed:
// nothing is resolved, nothing is opened, and they run on every platform.

#[test]
fn a_source_outside_the_repository_is_rejected() {
    for source in ["../secrets", "shell/../../secrets", "/etc/hosts"] {
        let stderr = rejected(&one_symlink(source, "~/.zshrc"));
        assert!(
            stderr.contains(source),
            "`{source}` was not named:\n{stderr}"
        );
    }
}

#[test]
fn a_source_naming_the_whole_repository_is_rejected() {
    // Containment alone lets the root through, and installing it would put
    // `batfiles.toml` and `.git` in the home. The spelling decides which
    // diagnostic it gets, not whether it is refused.
    for (source, expected) in [
        ("", "source is empty"),
        (".", "names the whole repository"),
        ("shell/..", "names the whole repository"),
    ] {
        let stderr = rejected(&one_symlink(source, "~/.zshrc"));
        assert!(
            stderr.contains(expected),
            "no `{expected}` for source `{source}` in:\n{stderr}"
        );
    }
}

#[test]
fn an_empty_dest_is_rejected_in_favor_of_writing_the_home_out() {
    // `~` already means the home directory itself, so the diagnostic points at
    // that spelling rather than only refusing.
    let stderr = rejected(&one_symlink("shell/zshrc", ""));
    for expected in ["dest is empty", "write `~`"] {
        assert!(stderr.contains(expected), "no `{expected}` in:\n{stderr}");
    }
}

#[test]
fn a_dest_naming_another_users_home_is_rejected() {
    let stderr = rejected(&one_symlink("shell/zshrc", "~other/.zshrc"));
    assert!(
        stderr.contains("~other/.zshrc"),
        "the destination was not named:\n{stderr}"
    );
}

#[test]
fn a_rejected_path_names_the_action_it_was_written_on() {
    // One-based, so it reads against the file. Action 1 here is valid, which
    // is what makes the number worth checking.
    let stderr = rejected(&format!(
        "{}{}",
        one_symlink("shell/zshrc", "~/.zshrc"),
        one_symlink("", "~/.zshenv")
    ));
    assert!(
        stderr.contains("action 2"),
        "the offending action was not located:\n{stderr}"
    );
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

// Executing `create-dir`, the first action type that is not platform-specific:
// every platform makes directories, so these run everywhere rather than inside
// `mod linking`. The one below that builds its fixture with a symlink is gated
// on its own, because what it asserts is not platform-specific either.

#[test]
fn a_create_dir_action_makes_the_directory_and_says_so() {
    let tree = Tree::new();
    tree.write_manifest(&one_create_dir("~/.local/share/zsh-plugins"));

    let assertion = tree
        .batfiles()
        .args(["--color", "never", "sync"])
        .assert()
        .success();
    assert_eq!(
        stderr_of(&assertion),
        format!(
            "created {}\n",
            display(&tree.home(".local/share/zsh-plugins"))
        )
    );
    // Missing parents come with it, as they do for a symlink's destination.
    assert!(tree.home(".local/share/zsh-plugins").is_dir());
}

#[test]
fn a_create_dir_action_parses_with_every_field_it_accepts() {
    let tree = Tree::new();
    tree.write_manifest(
        "[[actions]]\n\
         type = \"create-dir\"\n\
         id = \"plugin-root\"\n\
         group = \"shell\"\n\
         dest = \"~/.config\"\n",
    );

    tree.batfiles().arg("sync").assert().success();
    assert!(tree.home(".config").is_dir());
}

#[test]
fn a_create_dir_action_run_twice_changes_nothing() {
    let tree = Tree::new();
    tree.write_manifest(&one_create_dir("~/.config"));
    tree.batfiles().arg("sync").assert().success();
    // Something else put a file in it, which the second run has no business
    // touching: the action creates a directory, it does not own one.
    fs::write(tree.home(".config/theirs"), "mine\n").expect("a file inside it");

    tree.batfiles().arg("sync").assert().success().stderr("");
    let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();
    let expected = format!("unchanged {}", display(&tree.home(".config")));
    assert!(
        stderr_of(&assertion).contains(&expected),
        "no `{expected}` in:\n{}",
        stderr_of(&assertion)
    );
    assert_eq!(entries(&tree.home(".config")), ["theirs"]);
}

#[test]
fn a_create_dir_action_over_a_file_is_refused() {
    // Rule 13: someone's data is in the way, and until there is a backup policy
    // there is nothing to do with it but name it.
    let tree = Tree::new();
    fs::write(tree.home(".config"), "mine\n").expect("an existing file");
    tree.write_manifest(&one_create_dir("~/.config"));

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);
    let stderr = stderr_of(&assertion);
    for expected in [display(&tree.home(".config")), "a regular file".to_owned()] {
        assert!(stderr.contains(&expected), "no `{expected}` in:\n{stderr}");
    }
    assert_eq!(
        fs::read_to_string(tree.home(".config")).expect("the file"),
        "mine\n"
    );
}

/// A destination that is already a directory by another route. Gated only
/// because the fixture needs a symlink to build; the rule it covers is not
/// platform-specific, which is why it is not in `mod linking`.
#[cfg(unix)]
#[test]
fn a_create_dir_destination_symlinked_elsewhere_is_satisfied_by_what_it_reaches() {
    // `create-dir` replaces nothing, so its destination is a container like
    // `symlink-dir`'s `dest-dir` rather than a node to judge: someone whose
    // `~/.config` lives on another volume put that link there deliberately, and
    // the directory they asked for is already at the far end of it.
    let tree = Tree::new();
    let elsewhere = tree.path("elsewhere");
    fs::create_dir(&elsewhere).expect("a directory on another volume");
    std::os::unix::fs::symlink(&elsewhere, tree.home(".config")).expect("a deliberate link");
    tree.write_manifest(&one_create_dir("~/.config"));

    tree.batfiles().arg("sync").assert().success().stderr("");
    assert!(tree.home(".config").is_symlink(), "the link was replaced");
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
    fn a_symlink_action_parses_with_every_field_it_accepts() {
        // The counterpart to the rejections outside this module: a record
        // spelling out every field `symlink` takes is accepted and carried out.
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

    // `symlink-dir`: one link per direct child of a directory, all of them in
    // one destination directory.

    /// A repository holding `files/{ackrc,zshrc}` and a directory child
    /// `files/config/` with something inside it, which is the shape every case
    /// below reasons about.
    fn with_children(tree: &Tree) {
        tree.repo_file("files/zshrc", "# zsh\n");
        tree.repo_file("files/ackrc", "--smart-case\n");
        tree.repo_file("files/config/starship.toml", "# prompt\n");
    }

    #[test]
    fn a_symlink_dir_action_links_every_direct_child() {
        let tree = Tree::new();
        with_children(&tree);
        tree.write_manifest(&one_symlink_dir("files", "~/installed", false));

        tree.batfiles().arg("sync").assert().success();

        // The destination directory did not exist, and holds exactly the three
        // children — no more, and nothing renamed.
        assert_eq!(
            entries(&tree.home("installed")),
            ["ackrc", "config", "zshrc"]
        );
        for child in ["ackrc", "config", "zshrc"] {
            assert_eq!(
                link_target(&tree.home(&format!("installed/{child}"))),
                tree.path("repo").join("files").join(child),
                "`{child}` does not point into the repository"
            );
        }
    }

    #[test]
    fn a_directory_child_is_one_link_with_its_contents_reached_through_it() {
        // Decision 3: every direct child becomes exactly one symlink, whatever
        // it is. Nothing descends, so a file added under `files/config/` later
        // appears without the manifest or a further sync mentioning it.
        let tree = Tree::new();
        with_children(&tree);
        tree.write_manifest(&one_symlink_dir("files", "~/installed", false));

        tree.batfiles().arg("sync").assert().success();
        assert!(tree.home("installed/config").is_symlink());
        assert_eq!(
            fs::read_to_string(tree.home("installed/config/starship.toml"))
                .expect("the directory link resolves"),
            "# prompt\n"
        );

        // Added after the sync, and reachable with no second run.
        tree.repo_file("files/config/added-later.toml", "# later\n");
        assert!(tree.home("installed/config/added-later.toml").exists());
    }

    #[test]
    fn dot_prefix_dots_every_installed_name_and_nothing_else() {
        let tree = Tree::new();
        with_children(&tree);
        tree.write_manifest(&one_symlink_dir("files", "~", true));

        tree.batfiles().arg("sync").assert().success();
        assert_eq!(entries(&tree.path("home")), [".ackrc", ".config", ".zshrc"]);
        // The name in the repository is undotted, which is the point: a
        // repository reads better without a tree of dot-files in it.
        assert_eq!(
            link_target(&tree.home(".zshrc")),
            tree.path("repo").join("files/zshrc")
        );
    }

    #[test]
    fn the_children_are_linked_in_a_stable_order() {
        // `read_dir` yields whatever order the filesystem holds. An action that
        // reports its work differently on every machine is one nobody can diff.
        let tree = Tree::new();
        for name in ["zshrc", "ackrc", "inputrc", "curlrc"] {
            tree.repo_file(&format!("files/{name}"), "# rc\n");
        }
        tree.write_manifest(&one_symlink_dir("files", "~", true));

        let assertion = tree
            .batfiles()
            .args(["--color", "never", "sync"])
            .assert()
            .success();
        let reported: Vec<String> = stderr_of(&assertion)
            .lines()
            .filter_map(|line| Some(line.strip_prefix("linked ")?.split(' ').next()?.to_owned()))
            .collect();
        let expected: Vec<String> = [".ackrc", ".curlrc", ".inputrc", ".zshrc"]
            .iter()
            .map(|name| display(&tree.home(name)))
            .collect();
        assert_eq!(reported, expected);
    }

    #[test]
    fn an_existing_destination_directory_is_used_rather_than_refused() {
        // `dest-dir` is the container the links go in, not a node the action
        // installs, so finding one already there is the ordinary case — and it
        // may hold things batfiles did not put there.
        let tree = Tree::new();
        with_children(&tree);
        fs::create_dir(tree.home("bin")).expect("an existing directory");
        fs::write(tree.home("bin/theirs"), "mine\n").expect("a file inside it");
        tree.write_manifest(&one_symlink_dir("files", "~/bin", false));

        tree.batfiles().arg("sync").assert().success();
        assert_eq!(
            entries(&tree.home("bin")),
            ["ackrc", "config", "theirs", "zshrc"]
        );
        assert_eq!(
            fs::read_to_string(tree.home("bin/theirs")).expect("the file"),
            "mine\n"
        );
    }

    #[test]
    fn a_destination_directory_symlinked_elsewhere_is_followed() {
        // Unlike a destination, which is judged without following a final
        // link, `dest-dir` is resolved: someone whose `~/.config` lives on
        // another volume put that link there deliberately.
        let tree = Tree::new();
        with_children(&tree);
        let elsewhere = tree.path("elsewhere");
        fs::create_dir(&elsewhere).expect("a directory on another volume");
        std::os::unix::fs::symlink(&elsewhere, tree.home("bin")).expect("a deliberate link");
        tree.write_manifest(&one_symlink_dir("files", "~/bin", false));

        tree.batfiles().arg("sync").assert().success();
        assert_eq!(entries(&elsewhere), ["ackrc", "config", "zshrc"]);
        assert!(tree.home("bin").is_symlink(), "the link was replaced");
    }

    #[test]
    fn a_symlink_dir_run_twice_changes_nothing() {
        let tree = Tree::new();
        with_children(&tree);
        tree.write_manifest(&one_symlink_dir("files", "~", true));
        tree.batfiles().arg("sync").assert().success();

        tree.batfiles().arg("sync").assert().success().stderr("");
        let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();
        let stderr = stderr_of(&assertion);
        for name in [".ackrc", ".config", ".zshrc"] {
            let expected = format!("unchanged {}", display(&tree.home(name)));
            assert!(stderr.contains(&expected), "no `{expected}` in:\n{stderr}");
        }
    }

    #[test]
    fn an_empty_source_directory_still_makes_its_destination_and_links_nothing() {
        // Not an error: a directory that is empty today is a repository in
        // progress, not a manifest that cannot be honored. The destination is
        // made anyway — it is what the action was told to fill, and `create-dir`
        // makes exactly that directory when a manifest asks for it outright —
        // and saying so is what keeps a run that changed the home from being
        // silent about it.
        let tree = Tree::new();
        fs::create_dir_all(tree.path("repo/files")).expect("an empty source directory");
        tree.write_manifest(&one_symlink_dir("files", "~/installed", false));

        let assertion = tree
            .batfiles()
            .args(["--color", "never", "sync"])
            .assert()
            .success();
        assert_eq!(
            stderr_of(&assertion),
            format!("created {}\n", display(&tree.home("installed")))
        );
        assert_eq!(entries(&tree.home("installed")), Vec::<String>::new());

        let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();
        assert!(
            stderr_of(&assertion).contains("no children to link"),
            "the empty directory was not reported:\n{}",
            stderr_of(&assertion)
        );
    }

    #[test]
    fn a_child_that_is_already_a_dotfile_is_refused_under_dot_prefix() {
        // `..hidden` is a legal file name and never the one that was meant, so
        // the mistake is named rather than installed.
        let tree = Tree::new();
        with_children(&tree);
        tree.repo_file("files/.hidden", "# oops\n");

        let stderr = refused(&tree, &one_symlink_dir("files", "~", true));
        for expected in [".hidden", "..hidden"] {
            assert!(stderr.contains(expected), "no `{expected}` in:\n{stderr}");
        }
        assert!(
            !tree.home("..hidden").exists(),
            "the doubly-dotted name was installed anyway"
        );
    }

    #[test]
    fn a_source_directory_that_is_not_a_directory_is_refused() {
        // There are no children to link, and linking the file itself is what a
        // `symlink` action is for.
        let tree = Tree::new();
        with_children(&tree);

        let stderr = refused(&tree, &one_symlink_dir("files/zshrc", "~/installed", false));
        for expected in [
            "not a directory".to_owned(),
            display(&tree.path("repo/files/zshrc")),
        ] {
            assert!(stderr.contains(&expected), "no `{expected}` in:\n{stderr}");
        }
    }

    #[test]
    fn a_source_directory_the_repository_does_not_have_is_refused() {
        let tree = Tree::new();
        let stderr = refused(&tree, &one_symlink_dir("files", "~/installed", false));
        assert!(
            stderr.contains(&display(&tree.path("repo/files"))),
            "the source was not named:\n{stderr}"
        );
    }

    #[test]
    fn a_destination_directory_holding_a_file_is_refused() {
        let tree = Tree::new();
        with_children(&tree);
        fs::write(tree.home("bin"), "mine\n").expect("an existing file");

        let stderr = refused(&tree, &one_symlink_dir("files", "~/bin", false));
        for expected in [display(&tree.home("bin")), "a regular file".to_owned()] {
            assert!(stderr.contains(&expected), "no `{expected}` in:\n{stderr}");
        }
        assert_eq!(
            fs::read_to_string(tree.home("bin")).expect("the file"),
            "mine\n"
        );
    }

    #[test]
    fn a_destination_directory_that_is_a_dangling_link_is_named_rather_than_hit() {
        // Nothing resolves there, so the directory looks absent — and creating
        // it fails with a bare `EEXIST` naming nothing unless the link is
        // identified first.
        let tree = Tree::new();
        with_children(&tree);
        let nowhere = tree.path("nowhere");
        std::os::unix::fs::symlink(&nowhere, tree.home("bin")).expect("a dangling link");

        let stderr = refused(&tree, &one_symlink_dir("files", "~/bin", false));
        for expected in [
            display(&tree.home("bin")),
            format!("a symlink to {}, which is not there", display(&nowhere)),
        ] {
            assert!(stderr.contains(&expected), "no `{expected}` in:\n{stderr}");
        }
        assert!(tree.home("bin").is_symlink(), "the link was removed");
    }

    #[test]
    fn a_dangling_destination_directory_is_not_reported_as_pointing_outside() {
        // A dangling link is refused because its target is missing, which is
        // true wherever it points. Describing it as an unowned link would
        // claim it points outside the repository — demonstrably false for
        // this one, which points inside.
        let tree = Tree::new();
        with_children(&tree);
        let inside = tree.path("repo/missing");
        std::os::unix::fs::symlink(&inside, tree.home("bin")).expect("a dangling link");

        let stderr = refused(&tree, &one_symlink_dir("files", "~/bin", false));
        assert!(
            stderr.contains(&format!(
                "a symlink to {}, which is not there",
                display(&inside)
            )),
            "unexpected description:\n{stderr}"
        );
        assert!(
            !stderr.contains("outside the repository"),
            "a link into the repository was called outside it:\n{stderr}"
        );
    }

    #[test]
    fn an_occupied_child_destination_stops_the_action_where_it_stands() {
        // Rule 13 inside one action. Partial application within a
        // `symlink-dir` is the same story as partial application across a
        // manifest: earlier children stay, later ones are not attempted.
        let tree = Tree::new();
        with_children(&tree);
        // `config` sorts between `ackrc` and `zshrc`, so one child is installed
        // before the refusal and one is never reached.
        fs::create_dir(tree.home("installed")).expect("the destination directory");
        fs::write(tree.home("installed/config"), "mine\n").expect("an occupied child");

        let stderr = refused(&tree, &one_symlink_dir("files", "~/installed", false));
        assert!(
            stderr.contains(&display(&tree.home("installed/config"))),
            "the child was not named:\n{stderr}"
        );
        assert!(
            tree.home("installed/ackrc").is_symlink(),
            "the child before the refusal was rolled back"
        );
        assert!(
            fs::symlink_metadata(tree.home("installed/zshrc")).is_err(),
            "a child after the refusal was installed"
        );
        assert_eq!(
            fs::read_to_string(tree.home("installed/config")).expect("the file"),
            "mine\n"
        );
    }

    #[test]
    fn a_child_link_batfiles_owns_is_repaired() {
        // The same rule `symlink` follows, reached through the same code: a
        // link pointing elsewhere in the repository holds no content of its
        // own, so repointing it loses nothing.
        let tree = Tree::new();
        with_children(&tree);
        let stale = tree.repo_file("files/zshrc.old", "# old\n");
        fs::create_dir(tree.home("installed")).expect("the destination directory");
        std::os::unix::fs::symlink(&stale, tree.home("installed/zshrc")).expect("a stale link");

        tree.write_manifest(&one_symlink_dir("files", "~/installed", false));
        tree.batfiles().arg("sync").assert().success();

        assert_eq!(
            link_target(&tree.home("installed/zshrc")),
            tree.path("repo").join("files/zshrc")
        );
        assert!(stale.exists(), "the old source was removed");
    }

    // A destination reached through a symlinked parent. `~/bin -> ~/.local/bin`
    // is an ordinary arrangement, and a relative link sitting in it is read by
    // the operating system from the directory it is *physically* in. Composing
    // that answer from the written path instead classifies the link against a
    // directory it is not in, and every case below is a way for that to go
    // wrong (`guidance.md`, rule 14).

    /// A home whose `~/bin` is a symlink to `~/.local/bin`, with the repository
    /// at `~/dotfiles` holding `bin/tool`, and one existing link already at the
    /// destination, spelled as given.
    ///
    /// Returns the manifest to run and the physical path of that existing link.
    fn through_an_aliased_parent(tree: &Tree, existing: &str) -> (String, PathBuf) {
        let repo = tree.home("dotfiles");
        fs::create_dir_all(repo.join("bin")).expect("a repository");
        fs::write(repo.join("bin/tool"), "#!/bin/sh\n# ours\n").expect("the source");

        // What a relative link in `~/.local/bin` reaches by climbing out of it,
        // which is not what the same spelling reaches from `~/bin`.
        fs::create_dir_all(tree.home(".local/dotfiles/bin")).expect("a neighbour");
        fs::write(tree.home(".local/dotfiles/bin/tool"), "# theirs\n").expect("their file");
        fs::write(tree.home(".local/dotfiles/bin/other"), "# theirs\n").expect("their file");

        fs::create_dir_all(tree.home(".local/bin")).expect("the real directory");
        std::os::unix::fs::symlink(".local/bin", tree.home("bin")).expect("the alias");

        let link = tree.home(".local/bin/tool");
        std::os::unix::fs::symlink(existing, &link).expect("the existing link");

        fs::write(
            repo.join("batfiles.toml"),
            one_symlink_dir("bin", "~/bin", false),
        )
        .expect("a manifest");
        (display(&repo), link)
    }

    /// `sync` against a repository that is not the tree's default one.
    fn sync_against(tree: &Tree, repo: &str) -> assert_cmd::assert::Assert {
        tree.batfiles()
            .args(["--color", "never", "--batfiles-dir", repo, "sync"])
            .assert()
    }

    #[test]
    fn a_link_reached_through_an_aliased_parent_is_not_called_ours() {
        // Read from `~/bin`, `../dotfiles/bin/tool` looks like `~/dotfiles`.
        // Read from `~/.local/bin`, where the link actually is, it is
        // `~/.local/dotfiles` — someone else's. Judging it lexically reported
        // the repository as installed while `~/bin/tool` ran the wrong program,
        // and said nothing at all, which is the worst way to be wrong.
        let tree = Tree::new();
        let (repo, link) = through_an_aliased_parent(&tree, "../dotfiles/bin/tool");

        let assertion = sync_against(&tree, &repo).failure().code(1);
        let stderr = stderr_of(&assertion);
        assert!(
            stderr.contains(&display(&tree.home(".local/dotfiles/bin/tool"))),
            "the refusal did not say where the link really points:\n{stderr}"
        );
        assert_eq!(
            link_target(&link),
            PathBuf::from("../dotfiles/bin/tool"),
            "the unmanaged link was touched"
        );
    }

    #[test]
    fn a_link_reached_through_an_aliased_parent_is_not_deleted_as_ours() {
        // The same misreading, one step further: here the lexical answer is
        // inside the repository but not what the action wants, so the link was
        // deleted and replaced rather than merely mistaken for correct.
        let tree = Tree::new();
        let (repo, link) = through_an_aliased_parent(&tree, "../dotfiles/bin/other");

        let assertion = sync_against(&tree, &repo).failure().code(1);
        let stderr = stderr_of(&assertion);
        assert!(
            stderr.contains(&display(&tree.home(".local/dotfiles/bin/other"))),
            "the refusal did not say where the link really points:\n{stderr}"
        );
        assert_eq!(
            link_target(&link),
            PathBuf::from("../dotfiles/bin/other"),
            "an unmanaged link was destroyed"
        );
    }

    #[test]
    fn a_correct_link_reached_through_an_aliased_parent_is_left_alone() {
        // The other direction of the same misreading, and the one a user hits
        // by doing everything right: this link resolves to exactly what the
        // action installs, and was refused as pointing outside the repository.
        let tree = Tree::new();
        let (repo, link) = through_an_aliased_parent(&tree, "../../dotfiles/bin/tool");
        assert_eq!(
            fs::canonicalize(&link).expect("the link resolves"),
            fs::canonicalize(tree.home("dotfiles/bin/tool")).expect("the source"),
            "the fixture is wrong: this link should already be correct"
        );

        sync_against(&tree, &repo).success().stderr("");
        assert_eq!(
            link_target(&link),
            PathBuf::from("../../dotfiles/bin/tool"),
            "a correct link was rewritten"
        );
    }

    #[test]
    fn a_repository_reached_through_a_symlink_still_converges() {
        // The other half of resolving what is already there: the repository
        // root has to be compared in the same space. Selected through a
        // symlink, its written form and its resolved form are the same place by
        // two names — and comparing across the two calls every freshly written
        // link stale, relinking the whole repository on every run and never
        // reaching a quiet one. A `/home` that is a symlink is enough to do it.
        let tree = Tree::new();
        let (_, _) = through_an_aliased_parent(&tree, "../dotfiles/bin/tool");
        fs::remove_file(tree.home(".local/bin/tool")).expect("start from nothing");

        std::os::unix::fs::symlink(tree.path("home"), tree.path("by-another-name"))
            .expect("a symlinked route to the home");
        let aliased = display(&tree.path("by-another-name/dotfiles"));

        sync_against(&tree, &aliased).success();
        // Quiet: everything it just wrote is recognised as already right.
        sync_against(&tree, &aliased).success().stderr("");
    }

    // The `leaf` fixture: several actions over a directory tree, as opposed to
    // the manifests above, which are written inline to isolate one rule each.

    /// Every link `tests/fixtures/leaf` installs, in the order it installs
    /// them.
    ///
    /// Written out rather than read back from the manifest: a test that derives
    /// its expectations from the file under test asserts nothing. The last
    /// three are the one `symlink-dir` action expanded — one entry per child,
    /// dotted and in sorted order, because that is what the run produces.
    const LEAF_ACTIONS: [(&str, &str); 10] = [
        ("shell/zshrc", ".zshrc"),
        ("shell/zshenv", ".zshenv"),
        ("shell/aliases.zsh", ".config/zsh/aliases.zsh"),
        ("git/gitconfig", ".gitconfig"),
        ("git/gitignore", ".config/git/ignore"),
        ("editor/nvim", ".config/nvim"),
        ("bin/batgrep", ".local/bin/batgrep"),
        ("files/ackrc", ".ackrc"),
        ("files/curlrc", ".curlrc"),
        ("files/inputrc", ".inputrc"),
    ];

    #[test]
    fn syncing_a_real_repository_installs_every_action_and_nothing_else() {
        let tree = Tree::fixture("leaf");
        tree.batfiles().arg("sync").assert().success();

        for (source, dest) in LEAF_ACTIONS {
            assert_eq!(
                link_target(&tree.home(dest)),
                tree.path("repo").join(source),
                "`{dest}` does not point at `{source}`"
            );
        }

        // A link to a directory is only worth making if what is under it reads
        // back, and `lua/plugins.lua` is reachable no other way.
        assert!(
            fs::read_to_string(tree.home(".config/nvim/lua/plugins.lua"))
                .expect("the directory link resolves")
                .contains("vim-fugitive")
        );

        // The link is only a usable command if what it reaches is executable,
        // which is a property of the repository rather than of batfiles — so
        // this is here to keep the fixture honest about being one someone
        // keeps, and it fails if the mode is lost getting the fixture in place.
        use std::os::unix::fs::PermissionsExt;
        let installed = fs::metadata(tree.home(".local/bin/batgrep")).expect("the link resolves");
        assert!(
            installed.permissions().mode() & 0o111 != 0,
            "`bin/batgrep` installed as a file nobody can run"
        );

        // Only `batfiles.toml` has intrinsic meaning: `README.md` is a file it
        // never names, so nothing of it reaches the home on its own. Every file
        // the installed configuration refers to does — `.zshrc` sources both of
        // the other two shell files, and a fixture whose shell would fail to
        // start is not one anybody would keep.
        assert_eq!(
            entries(&tree.path("home")),
            [
                ".ackrc",
                ".config",
                ".curlrc",
                ".gitconfig",
                ".inputrc",
                ".local",
                ".zshenv",
                ".zshrc"
            ]
        );
    }

    #[test]
    fn an_occupied_destination_stops_the_run_where_it_stands() {
        // Rule 13 at repository scale. What one action's worth of it cannot
        // show is what happens to the rest of the list: the run stops, so the
        // actions after the refusal are not attempted. Which of two actions
        // runs first, where one depends on the other, is 3.1's to pin.
        let tree = Tree::fixture("leaf");
        let occupied = "[user]\n\temail = mine\n";
        fs::write(tree.home(".gitconfig"), occupied).expect("an existing file");
        // Found rather than counted, so an action added to the fixture ahead of
        // this one does not silently move the two halves of the assertion.
        let refused = LEAF_ACTIONS
            .iter()
            .position(|(_, dest)| *dest == ".gitconfig")
            .expect("the occupied destination is one the fixture declares");

        let assertion = tree.batfiles().arg("sync").assert().failure().code(1);
        let stderr = stderr_of(&assertion);
        for expected in [
            display(&tree.home(".gitconfig")),
            "a regular file".to_owned(),
        ] {
            assert!(stderr.contains(&expected), "no `{expected}` in:\n{stderr}");
        }
        assert_eq!(
            fs::read_to_string(tree.home(".gitconfig")).expect("the file"),
            occupied
        );

        for (_, dest) in &LEAF_ACTIONS[..refused] {
            assert!(tree.home(dest).is_symlink(), "`{dest}` was not installed");
        }
        for (_, dest) in &LEAF_ACTIONS[refused + 1..] {
            // Not `exists`, which follows the link and would call a dangling
            // one absent.
            assert!(
                fs::symlink_metadata(tree.home(dest)).is_err(),
                "`{dest}` was installed after the refusal"
            );
        }
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
fn an_option_sync_does_not_honor_yet_stops_a_whole_repository() {
    // The same refusal against the `leaf` fixture, where "before it writes"
    // means a whole repository's worth of nothing rather than one link's.
    let tree = Tree::fixture("leaf");

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
        entries(&tree.path("home")).is_empty(),
        "an unsupported option wrote into the home"
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

#[test]
fn a_stub_command_names_the_option_before_it_names_itself() {
    let tree = Tree::new();
    let assertion = tree
        .batfiles()
        .args([
            "clone",
            "https://example.invalid/dotfiles.git",
            "--interactive",
        ])
        .assert()
        .failure()
        .code(2);
    let stderr = stderr_of(&assertion);
    for expected in ["--interactive", "9.4"] {
        assert!(stderr.contains(expected), "no `{expected}` in:\n{stderr}");
    }
    assert!(
        !stderr.contains("`clone` is not implemented yet"),
        "the command's own message preempted the option's:\n{stderr}"
    );
}

#[test]
fn each_command_withholds_the_options_it_does_not_honor_yet() {
    let tree = Tree::new();
    for (args, option, step) in [
        (
            &[
                "clone",
                "https://example.invalid/d.git",
                "--enable-action",
                "shell",
            ][..],
            "--enable-action",
            "8.3",
        ),
        (
            &["apply-action", "--id", "vim", "--dry-run"],
            "--dry-run",
            "2.4",
        ),
        (
            &["apply-group", "--group", "gui", "--var", "profile=work"],
            "--var",
            "5.3",
        ),
        (&["vars", "list", "--no-refresh"], "--no-refresh", "9.1"),
    ] {
        let assertion = tree.batfiles().args(args).assert().failure().code(2);
        let stderr = stderr_of(&assertion);
        for expected in [option, step] {
            assert!(
                stderr.contains(expected),
                "no `{expected}` for `{args:?}` in:\n{stderr}"
            );
        }
    }
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
