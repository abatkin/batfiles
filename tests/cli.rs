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

/// Everything under a directory: names, types, symlink targets as written, and
/// file contents, sorted.
///
/// What a dry run is checked against, rather than the destinations a manifest
/// names — a `.batfiles-incomplete` staging node, a parent directory created on
/// the way, and a broken symlink cleared at an ancestor are none of them.
/// `Tree` gives the four roots as siblings, so this needs no exclusions.
#[cfg(unix)]
fn snapshot(root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    record_into(root, root, &mut found);
    found.sort();
    found
}

#[cfg(unix)]
fn record_into(root: &Path, dir: &Path, found: &mut Vec<String>) {
    for entry in fs::read_dir(dir).expect("a readable directory") {
        let entry = entry.expect("a directory entry");
        let path = entry.path();
        let name = display(path.strip_prefix(root).expect("a path under the root"));
        // Never followed: a symlink is a thing that is there, and what it
        // reaches is somebody else's part of the tree.
        let kind = entry.file_type().expect("a file type");
        if kind.is_symlink() {
            found.push(format!("{name} -> {}", display(&link_target(&path))));
        } else if kind.is_dir() {
            found.push(format!("{name}/"));
            record_into(root, &path, found);
        } else {
            let contents = fs::read(&path).expect("a readable file");
            found.push(format!("{name} = {}", String::from_utf8_lossy(&contents)));
        }
    }
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

/// A manifest declaring one `copy` and nothing else.
fn one_copy(source: &str, dest: &str) -> String {
    format!("[[actions]]\ntype = \"copy\"\nsource = \"{source}\"\ndest = \"{dest}\"\n")
}

/// A manifest declaring one `copy-dir` and nothing else.
fn one_copy_dir(source_dir: &str, dest_dir: &str, dot_prefix: bool) -> String {
    format!(
        "[[actions]]\n\
         type = \"copy-dir\"\n\
         source-dir = \"{source_dir}\"\n\
         dest-dir = \"{dest_dir}\"\n\
         dot-prefix = {dot_prefix}\n"
    )
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
fn dot_prefix_is_not_a_field_a_copy_has() {
    // What splitting `copy` from `copy-dir` buys. `dot-prefix` dots the names
    // of a directory's children, and a `copy` writes one name it was given in
    // full — so this is an unknown field caught while the manifest is read,
    // rather than a run-time complaint about a source that turned out to be a
    // file. The same holds for `symlink`, and always has.
    let stderr = rejected(
        "[[actions]]\n\
         type = \"copy\"\n\
         source = \"seed\"\n\
         dest = \"~/.config\"\n\
         dot-prefix = true\n",
    );
    assert!(
        stderr.contains("dot-prefix"),
        "the field was not named:\n{stderr}"
    );
}

#[test]
fn the_two_copy_types_do_not_share_a_field_set() {
    // As with the symlink pair: each record is closed, so a field belonging to
    // the other one is an error rather than something quietly ignored.
    let stderr = rejected(
        "[[actions]]\n\
         type = \"copy\"\n\
         source-dir = \"seed\"\n\
         dest = \"~\"\n",
    );
    assert!(
        stderr.contains("source-dir"),
        "the field was not named:\n{stderr}"
    );

    let stderr = rejected(
        "[[actions]]\n\
         type = \"copy-dir\"\n\
         source = \"seed/gitconfig\"\n\
         dest-dir = \"~\"\n",
    );
    assert!(
        stderr.contains("source"),
        "the field was not named:\n{stderr}"
    );
}

#[test]
fn a_filter_the_copy_types_do_not_have_yet_is_rejected() {
    // `include` and `exclude` are specified in `docs/future/repoformat.md` and
    // not built, on both of these as on `symlink-dir`. Ignoring one would copy
    // everything while looking as though it had copied a chosen few.
    for manifest in [
        "[[actions]]\n\
         type = \"copy\"\n\
         source = \"seed\"\n\
         dest = \"~/.config\"\n\
         include = \"*.toml\"\n",
        "[[actions]]\n\
         type = \"copy-dir\"\n\
         source-dir = \"seed\"\n\
         dest-dir = \"~\"\n\
         exclude = [\"private/*\"]\n",
    ] {
        let stderr = rejected(manifest);
        for expected in ["include", "exclude"] {
            if manifest.contains(expected) {
                assert!(
                    stderr.contains(expected),
                    "`{expected}` was not named:\n{stderr}"
                );
            }
        }
    }
}

#[test]
fn the_copy_types_paths_follow_the_same_rules_as_every_other() {
    for (manifest, expected) in [
        (
            one_copy("../secrets", "~/.x"),
            "resolves outside the repository",
        ),
        (one_copy(".", "~/.x"), "names the whole repository"),
        (one_copy("seed", ""), "dest is empty"),
        (one_copy("seed", "~other/x"), "another user's home"),
        (
            one_copy_dir("/etc", "~", false),
            "not relative to the repository root",
        ),
        (one_copy_dir("", "~", false), "source is empty"),
    ] {
        let stderr = rejected(&manifest);
        assert!(
            stderr.contains(expected),
            "no `{expected}` in:\n{stderr}\nfor manifest:\n{manifest}"
        );
    }
}

#[test]
fn an_action_type_that_has_not_landed_is_rejected() {
    let stderr = rejected(
        "[[actions]]\n\
         type = \"git-clone\"\n\
         source = \"https://example.invalid/repo.git\"\n\
         dest = \"~/repo\"\n",
    );
    assert!(
        stderr.contains("git-clone"),
        "the type was not named:\n{stderr}"
    );
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

// Executing `copy` and `copy-dir`. Portable like `create-dir`: nothing here
// makes a link, and the two tests that read a permission bit or build a symlink
// fixture are gated on their own.

/// A repository holding a seed directory with a file, a nested directory, and
/// something under that — the shape every case below reasons about.
fn with_seed(tree: &Tree) {
    tree.repo_file("seed/gitconfig", "[user]\n\temail = yours\n");
    tree.repo_file("seed/inputrc", "set editing-mode vi\n");
    tree.repo_file("seed/scripts/hello", "#!/bin/sh\necho hi\n");
}

#[test]
fn a_copy_action_seeds_a_file_and_says_so() {
    let tree = Tree::new();
    with_seed(&tree);
    tree.write_manifest(&one_copy("seed/gitconfig", "~/.config/git/config"));

    let assertion = tree
        .batfiles()
        .args(["--color", "never", "sync"])
        .assert()
        .success();
    assert_eq!(
        stderr_of(&assertion),
        format!(
            "copied {} from {}\n",
            display(&tree.home(".config/git/config")),
            display(&tree.path("repo/seed/gitconfig"))
        )
    );
    // Missing parents are created, as they are for a symlink's destination.
    assert_eq!(
        fs::read_to_string(tree.home(".config/git/config")).expect("the copy"),
        "[user]\n\temail = yours\n"
    );
    // A copy, not a link: it is the user's from here on.
    assert!(!tree.home(".config/git/config").is_symlink());
}

#[test]
fn a_copy_action_parses_with_every_field_it_accepts() {
    let tree = Tree::new();
    with_seed(&tree);
    tree.write_manifest(
        "[[actions]]\n\
         type = \"copy\"\n\
         id = \"gitconfig\"\n\
         group = \"git\"\n\
         source = \"seed/gitconfig\"\n\
         dest = \"~/.gitconfig\"\n",
    );

    tree.batfiles().arg("sync").assert().success();
    assert!(tree.home(".gitconfig").is_file());
}

#[test]
fn a_copy_action_leaves_an_occupied_destination_exactly_as_it_is() {
    // The whole point of a seed, and the one place batfiles finds something in
    // the way and does not fail: the file is the user's, they have edited it,
    // and a second `sync` must not undo that.
    let tree = Tree::new();
    with_seed(&tree);
    tree.write_manifest(&one_copy("seed/gitconfig", "~/.gitconfig"));
    fs::write(tree.home(".gitconfig"), "[user]\n\temail = mine\n").expect("their own file");

    tree.batfiles().arg("sync").assert().success().stderr("");
    assert_eq!(
        fs::read_to_string(tree.home(".gitconfig")).expect("the file"),
        "[user]\n\temail = mine\n"
    );

    let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();
    let expected = format!("kept {}", display(&tree.home(".gitconfig")));
    assert!(
        stderr_of(&assertion).contains(&expected),
        "no `{expected}` in:\n{}",
        stderr_of(&assertion)
    );
}

#[test]
fn a_copy_action_run_twice_changes_nothing() {
    let tree = Tree::new();
    with_seed(&tree);
    tree.write_manifest(&one_copy("seed/gitconfig", "~/.gitconfig"));
    tree.batfiles().arg("sync").assert().success();
    fs::write(tree.home(".gitconfig"), "edited by hand\n").expect("the user's edit");

    tree.batfiles().arg("sync").assert().success().stderr("");
    assert_eq!(
        fs::read_to_string(tree.home(".gitconfig")).expect("the file"),
        "edited by hand\n"
    );
}

#[test]
fn a_copy_action_installs_a_directory_whole() {
    let tree = Tree::new();
    with_seed(&tree);
    tree.write_manifest(&one_copy("seed", "~/.seed"));

    tree.batfiles().arg("sync").assert().success();
    assert_eq!(
        entries(&tree.home(".seed")),
        ["gitconfig", "inputrc", "scripts"]
    );
    // The tree is reproduced to the bottom, not one level deep.
    assert_eq!(
        fs::read_to_string(tree.home(".seed/scripts/hello")).expect("the nested copy"),
        "#!/bin/sh\necho hi\n"
    );
}

#[test]
fn a_copy_action_over_an_existing_directory_does_nothing_at_all() {
    // A directory source is one thing installed, so an occupied `dest` stops
    // the action rather than seeding into what is there. Someone who wants
    // their defaults filled in around an existing directory writes `copy-dir`,
    // which is exactly the difference between the two.
    let tree = Tree::new();
    with_seed(&tree);
    fs::create_dir(tree.home(".seed")).expect("a directory already there");
    tree.write_manifest(&one_copy("seed", "~/.seed"));

    tree.batfiles().arg("sync").assert().success().stderr("");
    assert_eq!(entries(&tree.home(".seed")), Vec::<String>::new());
}

#[test]
fn a_copy_dir_action_seeds_every_direct_child() {
    let tree = Tree::new();
    with_seed(&tree);
    tree.write_manifest(&one_copy_dir("seed", "~/installed", false));

    tree.batfiles().arg("sync").assert().success();
    assert_eq!(
        entries(&tree.home("installed")),
        ["gitconfig", "inputrc", "scripts"]
    );
    assert_eq!(
        fs::read_to_string(tree.home("installed/scripts/hello")).expect("the nested copy"),
        "#!/bin/sh\necho hi\n"
    );
}

#[test]
fn a_copy_dir_action_keeps_what_is_there_and_seeds_the_rest() {
    let tree = Tree::new();
    with_seed(&tree);
    fs::create_dir(tree.home("installed")).expect("an existing destination");
    fs::write(tree.home("installed/gitconfig"), "mine\n").expect("their own file");
    tree.write_manifest(&one_copy_dir("seed", "~/installed", false));

    tree.batfiles().arg("sync").assert().success();
    assert_eq!(
        fs::read_to_string(tree.home("installed/gitconfig")).expect("the file"),
        "mine\n"
    );
    assert_eq!(
        fs::read_to_string(tree.home("installed/inputrc")).expect("the seeded sibling"),
        "set editing-mode vi\n"
    );
}

#[test]
fn a_child_directory_that_is_already_there_is_kept_whole_rather_than_merged() {
    // One level, and the reason for it: seeding *into* a directory the user
    // already has interleaves two configurations that were never written to
    // combine, and nobody can tell afterwards which file came from where.
    let tree = Tree::new();
    with_seed(&tree);
    fs::create_dir_all(tree.home("installed/scripts")).expect("their own directory");
    fs::write(tree.home("installed/scripts/theirs"), "# theirs\n").expect("their own file");
    tree.write_manifest(&one_copy_dir("seed", "~/installed", false));

    tree.batfiles().arg("sync").assert().success();
    assert_eq!(entries(&tree.home("installed/scripts")), ["theirs"]);
    // Its siblings were still seeded: the child is kept, not the action.
    assert!(tree.home("installed/gitconfig").is_file());
}

#[test]
fn a_copy_dir_action_creates_its_destination_directory() {
    let tree = Tree::new();
    with_seed(&tree);
    tree.write_manifest(&one_copy_dir("seed", "~/installed", false));

    let assertion = tree
        .batfiles()
        .args(["--color", "never", "sync"])
        .assert()
        .success();
    let expected = format!("created {}", display(&tree.home("installed")));
    assert!(
        stderr_of(&assertion).contains(&expected),
        "no `{expected}` in:\n{}",
        stderr_of(&assertion)
    );
}

#[test]
fn a_copy_dir_with_nothing_in_it_creates_its_destination_and_says_so() {
    // The `symlink-dir` half of this is
    // `linking::an_empty_source_directory_still_makes_its_destination_and_links_nothing`,
    // and the
    // two say the same thing in the same order for the same reason: an empty
    // source is a repository mid-progress, and the destination is made anyway
    // because it is what the action was told to fill.
    let tree = Tree::new();
    fs::create_dir_all(tree.path("repo/seed")).expect("an empty source directory");
    tree.write_manifest(&one_copy_dir("seed", "~/installed", false));

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
        stderr_of(&assertion).contains("no children to copy"),
        "the empty directory was not reported:\n{}",
        stderr_of(&assertion)
    );
}

#[test]
fn copy_dir_dots_every_installed_name_and_nothing_else() {
    let tree = Tree::new();
    with_seed(&tree);
    tree.write_manifest(&one_copy_dir("seed", "~", true));

    tree.batfiles().arg("sync").assert().success();
    assert_eq!(
        entries(&tree.path("home")),
        [".gitconfig", ".inputrc", ".scripts"]
    );
    // Only the top level is dotted; what is inside keeps its own names.
    assert_eq!(entries(&tree.home(".scripts")), ["hello"]);
}

#[test]
fn a_copy_dir_child_that_is_already_a_dotfile_is_refused_under_dot_prefix() {
    let tree = Tree::new();
    with_seed(&tree);
    tree.repo_file("seed/.hidden", "# oops\n");
    tree.write_manifest(&one_copy_dir("seed", "~", true));

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);
    let stderr = stderr_of(&assertion);
    for expected in [".hidden", "..hidden"] {
        assert!(stderr.contains(expected), "no `{expected}` in:\n{stderr}");
    }
}

#[test]
fn a_copy_dirs_source_that_is_not_a_directory_is_refused() {
    let tree = Tree::new();
    with_seed(&tree);
    tree.write_manifest(&one_copy_dir("seed/gitconfig", "~/installed", false));

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);
    let stderr = stderr_of(&assertion);
    for expected in [
        "not a directory".to_owned(),
        display(&tree.path("repo/seed/gitconfig")),
    ] {
        assert!(stderr.contains(&expected), "no `{expected}` in:\n{stderr}");
    }
}

#[test]
fn a_copy_source_the_repository_does_not_have_is_refused() {
    let tree = Tree::new();
    tree.write_manifest(&one_copy("seed/gitconfig", "~/.gitconfig"));

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);
    assert!(
        stderr_of(&assertion).contains(&display(&tree.path("repo/seed/gitconfig"))),
        "the source was not named:\n{}",
        stderr_of(&assertion)
    );
    assert!(!tree.home(".gitconfig").exists(), "something was written");
}

/// A repository at `~/dotfiles`, which is where batfiles looks by default and
/// the layout in which a destination can reach the repository through `~`.
fn seeded_repository_in_the_home(tree: &Tree) -> PathBuf {
    let repo = tree.repository("home/dotfiles");
    fs::create_dir(repo.join("seed")).expect("a source directory");
    fs::write(repo.join("seed/a"), "x\n").expect("something to copy");
    repo
}

#[test]
fn a_copy_whose_destination_is_inside_its_source_is_refused() {
    // The destination would become a child of the source, so enumerating the
    // source finds it and the copy descends into what it is writing — until the
    // filesystem refuses a longer path, having written a deep tree into the
    // repository first. Nothing earlier refuses this: a `dest` may point
    // anywhere, the repository included, because the home is not a boundary.
    let tree = Tree::new();
    let repo = seeded_repository_in_the_home(&tree);
    fs::write(
        repo.join("batfiles.toml"),
        one_copy("seed", "~/dotfiles/seed/inner"),
    )
    .expect("a manifest");

    let assertion = tree
        .batfiles()
        .args(["--batfiles-dir", &display(&repo), "sync"])
        .assert()
        .failure()
        .code(1);
    assert!(
        stderr_of(&assertion).contains("which is inside it"),
        "unexpected stderr:\n{}",
        stderr_of(&assertion)
    );
    // And it found out before writing anything into the repository.
    assert_eq!(entries(&repo.join("seed")), ["a"]);
}

#[test]
fn a_copy_dir_whose_destination_is_inside_its_source_is_refused() {
    // The same hazard one level up: the destination directory is created before
    // the children are enumerated, so it would be among them.
    let tree = Tree::new();
    let repo = seeded_repository_in_the_home(&tree);
    fs::write(
        repo.join("batfiles.toml"),
        one_copy_dir("seed", "~/dotfiles/seed/inner", false),
    )
    .expect("a manifest");

    let assertion = tree
        .batfiles()
        .args(["--batfiles-dir", &display(&repo), "sync"])
        .assert()
        .failure()
        .code(1);
    assert!(
        stderr_of(&assertion).contains("which is inside it"),
        "unexpected stderr:\n{}",
        stderr_of(&assertion)
    );
    assert_eq!(entries(&repo.join("seed")), ["a"]);
}

#[test]
fn a_copy_dir_checks_its_source_before_it_checks_its_destination() {
    // Both rules are broken at once: the source is a file, and the destination
    // lands inside it. The source is what the run reports, because a
    // `source-dir` that is not a directory is a mistake in the record itself,
    // and there is no reading of the rest of the action until it is fixed.
    let tree = Tree::new();
    let repo = seeded_repository_in_the_home(&tree);
    fs::write(
        repo.join("batfiles.toml"),
        one_copy_dir("seed/a", "~/dotfiles/seed/a/inner", false),
    )
    .expect("a manifest");

    let assertion = tree
        .batfiles()
        .args(["--batfiles-dir", &display(&repo), "sync"])
        .assert()
        .failure()
        .code(1);
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("not a directory"),
        "the source was not what was reported:\n{stderr}"
    );
    assert!(
        !stderr.contains("which is inside it"),
        "the destination was reported ahead of the source:\n{stderr}"
    );
}

/// A copy stopped partway. Gated because a symlink is the cheapest way to make
/// one fail after it has already written something; what it asserts is not
/// platform-specific.
#[cfg(unix)]
#[test]
fn a_copy_that_fails_partway_leaves_nothing_at_its_destination() {
    // A half-made copy is the one way a seed can converge on a broken state:
    // the next run finds the destination occupied, keeps it, and reports
    // success over a seed that never finished. So a failure takes back what it
    // made, and the run after it says the same thing as the first.
    let tree = Tree::new();
    tree.repo_file("seed/a-file", "good\n");
    // Sorted order puts `a-file` first, so one file is already written when the
    // link stops the copy.
    std::os::unix::fs::symlink("/nowhere", tree.path("repo/seed/b-link")).expect("a link");
    tree.write_manifest(&one_copy("seed", "~/.seed"));

    tree.batfiles().arg("sync").assert().failure().code(1);
    assert!(
        fs::symlink_metadata(tree.home(".seed")).is_err(),
        "a partial copy was left where the next run would keep it"
    );

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);
    assert!(
        stderr_of(&assertion).contains("it is a symlink"),
        "the second run did not report the same problem:\n{}",
        stderr_of(&assertion)
    );
}

#[test]
fn a_seeded_file_is_not_written_at_its_destination() {
    // A file is built beside its destination and moved in, for the reason a
    // directory is: a run that is interrupted rather than failed returns no
    // error and runs no cleanup, so a file written in place is left truncated
    // at the path the next run reads as finished. The copy being somewhere else
    // until it is whole is what makes that impossible, and it is observable —
    // during the copy the destination does not exist and the incomplete copy
    // sits beside it.
    let tree = Tree::new();
    with_seed(&tree);
    tree.write_manifest(&one_copy("seed/gitconfig", "~/.gitconfig"));
    tree.batfiles().arg("sync").assert().success();

    // Nothing beside it once the run is done: the copy was moved in, not left.
    assert_eq!(entries(&tree.path("home")), [".gitconfig"]);
    assert_eq!(
        fs::read_to_string(tree.home(".gitconfig")).expect("the copy"),
        "[user]\n\temail = yours\n"
    );
}

#[test]
fn something_at_the_staging_path_is_named_rather_than_removed() {
    // A copy is built beside its destination before being moved in, and that
    // path is as much somebody's as any other. Cleaning it up on the assumption
    // that batfiles put it there is how a copy comes to delete data it never
    // created — `remove_dir_all` on a directory this run did not make takes the
    // tree with it. So the action stops and names the path instead, and
    // clearing it is the user's call.
    let tree = Tree::new();
    with_seed(&tree);
    let in_the_way = tree.home(".seed.batfiles-incomplete");
    fs::create_dir(&in_the_way).expect("something already at the staging path");
    fs::write(in_the_way.join("irreplaceable"), "mine\n").expect("data inside it");
    tree.write_manifest(&one_copy("seed", "~/.seed"));

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);
    assert!(
        stderr_of(&assertion).contains(&display(&in_the_way)),
        "the path in the way was not named:\n{}",
        stderr_of(&assertion)
    );
    assert_eq!(
        fs::read_to_string(in_the_way.join("irreplaceable")).expect("the data survives"),
        "mine\n"
    );
    assert!(
        fs::symlink_metadata(tree.home(".seed")).is_err(),
        "the destination was installed over a failure"
    );
}

/// The permissions a copy has *while* it is being made, which are not the ones
/// it ends with. Gated for the mode; the exposure is not platform-specific.
#[cfg(unix)]
#[test]
fn a_copy_is_never_readable_by_more_people_than_its_source() {
    use std::os::unix::fs::PermissionsExt;

    // A copy takes the source's permissions once it is whole, which cannot
    // happen up front — a read-only source directory would lock batfiles out of
    // the copy it is still filling. Created with the default in the meantime, a
    // `0700` source would be staged at `0755` with its contents readable by
    // anyone for as long as the copy ran. An interrupted copy is left where it
    // is on purpose, so that would outlast the run.
    let tree = Tree::new();
    let private = tree.path("repo/seed");
    tree.repo_file("seed/held/token", "secret\n");
    // Read-only, so the copy of it cannot be removed and the incomplete copy
    // outlives the failure below. That is what makes the mode observable after
    // the run, and it is also the case in which the exposure lasts.
    let held = private.join("held");
    fs::set_permissions(&held, fs::Permissions::from_mode(0o555)).expect("a read-only source");
    // Sorted after `held`, so the copy stops with the secret already written.
    std::os::unix::fs::symlink("/nowhere", private.join("z-link")).expect("a link");
    fs::set_permissions(&private, fs::Permissions::from_mode(0o700)).expect("a private source");
    tree.write_manifest(&one_copy("seed", "~/.private"));

    tree.batfiles().arg("sync").assert().failure().code(1);

    // What the failed run left behind is what another user could have reached
    // while it ran, and it is still there now.
    let leftover = tree.home(".private.batfiles-incomplete");
    let mode = fs::metadata(&leftover)
        .expect("the leftover")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(
        format!("{mode:o}"),
        "700",
        "an incomplete copy of a private directory was left reachable by anyone"
    );

    // So the temporary tree can be removed when it drops.
    for path in [&held, &leftover.join("held")] {
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).expect("writable again");
    }
}

/// Gated because the destination has to be a symlink for the question to
/// arise; the rule it covers is not platform-specific.
#[cfg(unix)]
#[test]
fn a_destination_resolving_into_the_source_is_kept_rather_than_refused() {
    // The containment refusal is about a copy descending into what it writes,
    // so it is only a question when there is going to be a copy. Asked ahead of
    // the occupied check it fired on a destination that was merely taken — a
    // link of the user's own resolving into the source — turning a `kept` into
    // an error that stops the whole run, every run.
    let tree = Tree::new();
    let repo = seeded_repository_in_the_home(&tree);
    fs::create_dir(repo.join("seed/inner")).expect("somewhere inside the source");
    std::os::unix::fs::symlink(repo.join("seed/inner"), tree.home(".seed")).expect("their link");
    fs::write(
        repo.join("batfiles.toml"),
        format!(
            "{}{}",
            one_copy("seed", "~/.seed"),
            one_create_dir("~/later-action-ran")
        ),
    )
    .expect("a manifest");

    tree.batfiles()
        .args(["--batfiles-dir", &display(&repo), "sync"])
        .assert()
        .success();
    assert!(
        tree.home(".seed").is_symlink(),
        "the destination was not left alone"
    );
    // And the actions after it still ran, which is what an error would have
    // cost beyond the wrong answer.
    assert!(tree.home("later-action-ran").is_dir());
}

/// A copy stopped partway that cannot be cleaned up afterwards. Gated for its
/// fixture's sake; what it asserts is not platform-specific.
#[cfg(unix)]
#[test]
fn a_copy_that_fails_leaves_no_destination_even_when_it_cannot_clean_up() {
    use std::os::unix::fs::PermissionsExt;

    // A repository holding a read-only directory makes a copy batfiles cannot
    // remove: taking `a-dir/inner` back out needs write permission on `a-dir`,
    // which the copy has just taken away by carrying the source's mode across.
    // So cleanup is not something correctness can rest on, and the copy is
    // built beside the destination instead of at it — the destination is
    // published only once it is whole.
    let tree = Tree::new();
    tree.repo_file("seed/a-dir/inner", "x\n");
    // Sorted after `a-dir`, so the read-only copy already exists when this
    // stops the run.
    std::os::unix::fs::symlink("/nowhere", tree.path("repo/seed/b-link")).expect("a link");
    let read_only = tree.path("repo/seed/a-dir");
    fs::set_permissions(&read_only, fs::Permissions::from_mode(0o555)).expect("a read-only source");
    tree.write_manifest(&one_copy("seed", "~/.seed"));

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);
    assert!(
        fs::symlink_metadata(tree.home(".seed")).is_err(),
        "a partial copy was left where the next run would keep it"
    );
    // The copy that could not be removed is named, so the user knows what is
    // there and where.
    assert!(
        stderr_of(&assertion).contains("could not remove the incomplete copy"),
        "the leftover was not reported:\n{}",
        stderr_of(&assertion)
    );

    // The second run does not report success either. It stops on the leftover
    // rather than on the symlink, because batfiles will not clear a path it did
    // not create — which is the whole reason the first run left it.
    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);
    assert!(
        stderr_of(&assertion).contains("something is already there"),
        "the second run did not name what was in the way:\n{}",
        stderr_of(&assertion)
    );
    assert!(
        fs::symlink_metadata(tree.home(".seed")).is_err(),
        "the destination appeared on the second run"
    );

    // So the temporary directory can be cleaned up when this test's tree drops.
    fs::set_permissions(&read_only, fs::Permissions::from_mode(0o755)).expect("the source back");
    let leftover = tree.home(".seed.batfiles-incomplete/a-dir");
    fs::set_permissions(&leftover, fs::Permissions::from_mode(0o755)).expect("the leftover back");
}

/// The destination is claimed by creating it exclusively rather than by looking
/// first and writing after. The window that closes is a race, which no test can
/// open on purpose; what is checkable is that the claim never follows a final
/// link, so a link at the destination is kept and its target left alone.
#[cfg(unix)]
#[test]
fn a_seed_does_not_write_through_a_symlink_at_its_destination() {
    let tree = Tree::new();
    with_seed(&tree);
    let theirs = tree.path("theirs");
    fs::write(&theirs, "theirs\n").expect("their file");
    std::os::unix::fs::symlink(&theirs, tree.home(".gitconfig")).expect("their link");
    tree.write_manifest(&one_copy("seed/gitconfig", "~/.gitconfig"));

    tree.batfiles().arg("sync").assert().success().stderr("");
    assert!(
        tree.home(".gitconfig").is_symlink(),
        "the link was replaced"
    );
    assert_eq!(
        fs::read_to_string(&theirs).expect("their file"),
        "theirs\n",
        "the seed was written through the link"
    );
}

/// The permission half of copying, which needs a mode to look at.
#[cfg(unix)]
#[test]
fn a_copy_carries_the_permissions_of_what_it_copied() {
    use std::os::unix::fs::PermissionsExt;

    let tree = Tree::new();
    with_seed(&tree);
    let script = tree.path("repo/seed/scripts/hello");
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).expect("an executable source");
    let private = tree.path("repo/seed/private");
    fs::create_dir(&private).expect("a directory nobody else may read");
    fs::write(private.join("token"), "secret\n").expect("something in it");
    fs::set_permissions(&private, fs::Permissions::from_mode(0o700)).expect("its mode");

    tree.write_manifest(&one_copy("seed", "~/.seed"));
    tree.batfiles().arg("sync").assert().success();

    // An executable arrives executable, or the copy is not a usable command.
    let installed = fs::metadata(tree.home(".seed/scripts/hello")).expect("the copy");
    assert!(
        installed.permissions().mode() & 0o111 != 0,
        "an executable source installed as a file nobody can run"
    );
    // And a directory the repository kept private does not arrive readable —
    // which also proves the mode is set after the directory is filled, since
    // batfiles had to write into it first.
    let directory = fs::metadata(tree.home(".seed/private")).expect("the copied directory");
    assert_eq!(
        directory.permissions().mode() & 0o777,
        0o700,
        "a private source directory was installed more broadly readable"
    );
    assert_eq!(
        fs::read_to_string(tree.home(".seed/private/token")).expect("its contents"),
        "secret\n"
    );
}

/// Refusing a symlink found inside something being copied. Gated only because
/// the fixture needs a symlink to build; the rule is not platform-specific.
#[cfg(unix)]
#[test]
fn a_symlink_inside_a_copied_tree_is_refused_rather_than_flattened() {
    // Following it would turn a link the repository chose into a detached file
    // and say nothing about it; recreating it would re-read a relative target
    // from a directory it is no longer in. Refusing is the answer that can be
    // revisited without changing what a working manifest already does.
    let tree = Tree::new();
    with_seed(&tree);
    let link = tree.path("repo/seed/link-to-gitconfig");
    std::os::unix::fs::symlink("gitconfig", &link).expect("a link in the repository");
    tree.write_manifest(&one_copy("seed", "~/.seed"));

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);
    let stderr = stderr_of(&assertion);
    for expected in [display(&link), "it is a symlink".to_owned()] {
        assert!(stderr.contains(&expected), "no `{expected}` in:\n{stderr}");
    }
}

/// The other side of that rule: the path the *manifest* named is resolved like
/// every other action's source, following a final link.
#[cfg(unix)]
#[test]
fn a_source_the_manifest_named_through_a_link_is_followed() {
    let tree = Tree::new();
    with_seed(&tree);
    std::os::unix::fs::symlink("gitconfig", tree.path("repo/seed/aliased"))
        .expect("a link the repository stores");
    tree.write_manifest(&one_copy("seed/aliased", "~/.gitconfig"));

    tree.batfiles().arg("sync").assert().success();
    assert!(
        !tree.home(".gitconfig").is_symlink(),
        "the link was reproduced instead of what it names"
    );
    assert_eq!(
        fs::read_to_string(tree.home(".gitconfig")).expect("the copy"),
        "[user]\n\temail = yours\n"
    );
}

// The portable half of the `leaf` fixture: the actions that need no symlink,
// which the manifest declares first so that a platform which cannot make one
// still runs them before the refusal stops the list.

/// Every directory `tests/fixtures/leaf` creates outright, as opposed to the
/// ones made on the way to a destination.
const LEAF_DIRS: [&str; 1] = [".cache/zsh"];

/// Every file it seeds, and the repository file each one is a copy of, in the
/// order it seeds them.
///
/// Written out rather than read back from the manifest: a test that derives its
/// expectations from the file under test asserts nothing. The last two are the
/// one `copy-dir` action expanded — one entry per child, in sorted order,
/// because that is what the run produces.
const LEAF_SEEDS: [(&str, &str); 3] = [
    ("templates/gitconfig.local", ".config/git/local"),
    ("zsh-local/env.zsh", ".config/zsh/local/env.zsh"),
    ("zsh-local/prompt.zsh", ".config/zsh/local/prompt.zsh"),
];

/// Assert that every action in [`LEAF_DIRS`] and [`LEAF_SEEDS`] has been
/// carried out, which every platform can do.
fn assert_leaf_portable_actions(tree: &Tree) {
    for dest in LEAF_DIRS {
        assert!(
            tree.home(dest).is_dir(),
            "`{dest}` is not a directory that exists"
        );
    }
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

/// Syncing the whole fixture where symlinks cannot be made: the actions ahead
/// of the first `symlink` record are carried out, and the refusal stops the run
/// there.
///
/// The unix side of this is `linking::syncing_a_real_repository_installs_every_
/// action_and_nothing_else`, which asserts the same half and the links besides.
#[cfg(not(unix))]
#[test]
fn the_actions_that_need_no_symlink_run_where_symlinks_cannot_be_made() {
    let tree = Tree::fixture("leaf");

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("`symlink` actions are not supported"),
        "unexpected stderr:\n{stderr}"
    );

    assert_leaf_portable_actions(&tree);
    // The first symlink the manifest declares, and the one the run stopped at.
    assert!(
        !tree.home(".zshrc").exists(),
        "a symlink action ran on a platform that cannot make one"
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
        // The target has to exist: a link reaching nothing is replaceable
        // whatever it names, so a missing one would prove the wrong thing.
        fs::write(tree.path("outside"), "someone else's\n").expect("the target");
        std::os::unix::fs::symlink(&escaping, tree.home(".zshrc")).expect("an escaping link");

        let stderr = refused(&tree, &one_symlink("shell/zshrc", "~/.zshrc"));
        assert!(
            stderr.contains(&display(&tree.home(".zshrc"))),
            "the destination was not named:\n{stderr}"
        );
        assert_eq!(link_target(&tree.home(".zshrc")), escaping);
    }

    #[test]
    fn a_broken_link_at_a_destination_is_replaced_wherever_it_pointed() {
        // Rule 13 protects data, and a link reaching nothing gives access to
        // none — so unlike a link that leaves the repository and lands on
        // something, this one is batfiles' to repoint.
        let tree = Tree::new();
        tree.repo_file("shell/zshrc", "# zsh\n");
        let nowhere = tree.path("nowhere");
        std::os::unix::fs::symlink(&nowhere, tree.home(".zshrc")).expect("a broken link");
        tree.write_manifest(&one_symlink("shell/zshrc", "~/.zshrc"));

        let assertion = tree.batfiles().arg("sync").assert().success();
        let stderr = stderr_of(&assertion);
        assert!(
            stderr.contains(&format!("(was {})", display(&nowhere))),
            "the replaced target was not named:\n{stderr}"
        );
        assert_eq!(
            link_target(&tree.home(".zshrc")),
            tree.path("repo/shell/zshrc")
        );
        assert!(
            !nowhere.exists(),
            "the far end of the broken link was created"
        );
    }

    #[test]
    fn a_link_at_a_deliberately_broken_source_is_still_left_alone() {
        // A repository may name a source that is itself a broken link — it is
        // there, and linking at it is what was asked for. The destination then
        // reaches nothing either, so "already right" has to be decided before
        // "broken", or every run relinks a link that is correct.
        let tree = Tree::new();
        fs::create_dir(tree.path("repo/shell")).expect("a source directory");
        std::os::unix::fs::symlink("nowhere", tree.path("repo/shell/zshrc"))
            .expect("a broken source");
        tree.write_manifest(&one_symlink("shell/zshrc", "~/.zshrc"));
        tree.batfiles().arg("sync").assert().success();

        tree.batfiles().arg("sync").assert().success().stderr("");
    }

    #[test]
    fn a_symlink_inside_its_source_is_refused_even_over_a_link_it_could_repair() {
        // The refusal has to cover repairing as well as creating. A destination
        // inside the source that already holds a replaceable link takes the
        // repair arm, and a check that only guards the vacant one lets exactly
        // this case through — writing into the repository after removing what
        // was there.
        let tree = Tree::new();
        let repo = seeded_repository_in_the_home(&tree);
        // Broken, so it is batfiles' to replace and the repair arm is reached.
        std::os::unix::fs::symlink(tree.path("nowhere"), repo.join("seed/inner"))
            .expect("a replaceable link");
        fs::write(
            repo.join("batfiles.toml"),
            one_symlink("seed", "~/dotfiles/seed/inner"),
        )
        .expect("a manifest");

        let assertion = tree
            .batfiles()
            .args(["--batfiles-dir", &display(&repo), "sync"])
            .assert()
            .failure()
            .code(1);
        assert!(
            stderr_of(&assertion).contains("which is inside it"),
            "unexpected stderr:\n{}",
            stderr_of(&assertion)
        );
        // Refused before the old link was removed, so nothing was destroyed on
        // the way to failing.
        assert_eq!(
            link_target(&repo.join("seed/inner")),
            tree.path("nowhere"),
            "the link was replaced despite the refusal"
        );
    }

    #[test]
    fn a_symlink_whose_destination_is_inside_its_source_is_refused() {
        // Linking a directory into itself means nothing, and making the link
        // would write into the repository — which is the one place `sync` never
        // writes. Refused on the same terms as the copy actions, by where the
        // two paths resolve rather than by how they are spelled.
        let tree = Tree::new();
        let repo = seeded_repository_in_the_home(&tree);
        fs::write(
            repo.join("batfiles.toml"),
            one_symlink("seed", "~/dotfiles/seed/inner"),
        )
        .expect("a manifest");

        let assertion = tree
            .batfiles()
            .args(["--batfiles-dir", &display(&repo), "sync"])
            .assert()
            .failure()
            .code(1);
        assert!(
            stderr_of(&assertion).contains("which is inside it"),
            "unexpected stderr:\n{}",
            stderr_of(&assertion)
        );
        assert_eq!(entries(&repo.join("seed")), ["a"]);
    }

    #[test]
    fn a_symlink_dir_whose_destination_is_inside_its_source_is_refused() {
        // The same hazard one level up, and worse than for a single link: the
        // destination directory is created before the children are enumerated,
        // so it would be among them and would be linked into itself.
        let tree = Tree::new();
        let repo = seeded_repository_in_the_home(&tree);
        fs::write(
            repo.join("batfiles.toml"),
            one_symlink_dir("seed", "~/dotfiles/seed/inner", false),
        )
        .expect("a manifest");

        let assertion = tree
            .batfiles()
            .args(["--batfiles-dir", &display(&repo), "sync"])
            .assert()
            .failure()
            .code(1);
        assert!(
            stderr_of(&assertion).contains("which is inside it"),
            "unexpected stderr:\n{}",
            stderr_of(&assertion)
        );
        // And it found out before creating the destination directory, which is
        // what would have put it among the children.
        assert_eq!(entries(&repo.join("seed")), ["a"]);
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

    #[test]
    fn a_file_in_the_way_of_a_parent_is_named_for_what_it_is() {
        // A destination under a regular file reads as vacant — nothing is at
        // it — so the refusal falls to whoever makes the parents, which is the
        // step that can say *which* component is the problem. Reporting the
        // kernel's answer where it arose would name the path below the file.
        let tree = Tree::new();
        tree.repo_file("shell/zshrc", "# zsh\n");
        fs::write(tree.home(".config"), "not a directory\n").expect("a file in the way");

        let stderr = refused(&tree, &one_symlink("shell/zshrc", "~/.config/zsh/zshrc"));
        for expected in [display(&tree.home(".config")), "a regular file".to_owned()] {
            assert!(stderr.contains(&expected), "no `{expected}` in:\n{stderr}");
        }
        assert_eq!(
            fs::read_to_string(tree.home(".config")).expect("the file"),
            "not a directory\n"
        );
    }

    #[test]
    fn a_broken_link_in_the_way_of_a_parent_is_cleared_and_the_removal_reported() {
        // The parent is made, because the link that was there reached nothing.
        // Silently is the one way it must not happen: a run that removes a node
        // says so, even one it is entitled to remove.
        let tree = Tree::new();
        let source = tree.repo_file("nvim/init.lua", "-- nvim\n");
        fs::create_dir(tree.home(".config")).expect("a config directory");
        let nowhere = tree.path("nowhere");
        std::os::unix::fs::symlink(&nowhere, tree.home(".config/nvim")).expect("a broken link");
        tree.write_manifest(&one_symlink("nvim/init.lua", "~/.config/nvim/init.lua"));

        let assertion = tree.batfiles().arg("sync").assert().success();
        let stderr = stderr_of(&assertion);
        assert!(
            stderr.contains(&format!(
                "removed a broken symlink to {} to make {}",
                display(&nowhere),
                display(&tree.home(".config/nvim"))
            )),
            "the removal was not reported:\n{stderr}"
        );
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
        // The target has to exist: a link reaching nothing is replaceable
        // whatever it names, so a missing one would prove the wrong thing.
        fs::write(&elsewhere, "someone else's\n").expect("the target");
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
        // Reachable, so that what is under test is how the refusal names the
        // link rather than whether it is refused at all.
        fs::write(tree.path("elsewhere"), "someone else's\n").expect("the target");
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
    fn a_destination_directory_that_is_a_broken_link_is_replaced_and_said_so() {
        // Nothing resolves there, so the directory looks absent — and creating
        // it fails with a bare `EEXIST` naming nothing unless the link is
        // cleared first. It holds nothing, so clearing it destroys nothing.
        let tree = Tree::new();
        with_children(&tree);
        let nowhere = tree.path("nowhere");
        std::os::unix::fs::symlink(&nowhere, tree.home("bin")).expect("a broken link");
        tree.write_manifest(&one_symlink_dir("files", "~/bin", false));

        let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();
        let stderr = stderr_of(&assertion);
        // The removal is its own line, and comes first: it is the part the user
        // may need to act on, and it is true of a path they did not name.
        for expected in [
            format!(
                "removed a broken symlink to {} to make {}",
                display(&nowhere),
                display(&tree.home("bin"))
            ),
            format!("created {}", display(&tree.home("bin"))),
        ] {
            assert!(stderr.contains(&expected), "no `{expected}` in:\n{stderr}");
        }
        assert!(tree.home("bin").is_dir(), "the directory was not created");
        assert_eq!(entries(&tree.home("bin")), ["ackrc", "config", "zshrc"]);
    }

    #[test]
    fn a_broken_destination_directory_is_replaced_wherever_it_pointed() {
        // Where a broken link points decides nothing: it reaches no content
        // either way, so one naming a path inside the repository is cleared on
        // the same terms as one naming a path outside it.
        let tree = Tree::new();
        with_children(&tree);
        let inside = tree.path("repo/missing");
        std::os::unix::fs::symlink(&inside, tree.home("bin")).expect("a broken link");
        tree.write_manifest(&one_symlink_dir("files", "~/bin", false));

        tree.batfiles().arg("sync").assert().success();
        assert!(tree.home("bin").is_dir(), "the directory was not created");
        assert!(
            !tree.path("repo/missing").exists(),
            "the far end of the link was created"
        );
    }

    #[test]
    fn a_broken_link_above_the_directory_being_made_is_cleared_too() {
        // The link is at an ancestor nobody named, so a single `mkdir -p` hits
        // it and reports `EEXIST` against the path that does *not* exist. Each
        // level is asked the same question the named directory is, so the link
        // is found where it actually is and the removal names that path.
        let tree = Tree::new();
        with_children(&tree);
        let nowhere = tree.path("nowhere");
        fs::create_dir(tree.home("a")).expect("an existing directory");
        std::os::unix::fs::symlink(&nowhere, tree.home("a/broken")).expect("a broken link");
        tree.write_manifest(&one_create_dir("~/a/broken/b/c"));

        let assertion = tree.batfiles().arg("sync").assert().success();
        let stderr = stderr_of(&assertion);
        assert!(
            stderr.contains(&format!(
                "removed a broken symlink to {} to make {}",
                display(&nowhere),
                display(&tree.home("a/broken"))
            )),
            "the ancestor removal was not reported:\n{stderr}"
        );
        assert!(tree.home("a/broken/b/c").is_dir(), "nothing was created");
    }

    #[test]
    fn a_link_reaching_nothing_through_a_file_is_replaceable_too() {
        // `<some-file>/child` resolves nowhere, but the kernel says so with
        // `ENOTDIR` rather than `ENOENT`. Reading only the second calls this
        // link someone else's data and refuses a destination holding nothing.
        let tree = Tree::new();
        tree.repo_file("shell/zshrc", "# zsh\n");
        let through_a_file = tree.path("afile").join("nope");
        fs::write(tree.path("afile"), "not a directory\n").expect("a file");
        std::os::unix::fs::symlink(&through_a_file, tree.home(".zshrc")).expect("a broken link");
        tree.write_manifest(&one_symlink("shell/zshrc", "~/.zshrc"));

        tree.batfiles().arg("sync").assert().success();
        assert_eq!(
            link_target(&tree.home(".zshrc")),
            tree.path("repo/shell/zshrc")
        );
        assert_eq!(
            fs::read_to_string(tree.path("afile")).expect("the file"),
            "not a directory\n",
            "the file the link resolved through was touched"
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
    // Only its links are in here; the half that needs no symlink is outside,
    // with the tests that can run anywhere.

    /// Every link `tests/fixtures/leaf` installs, in the order it installs
    /// them, all of them after everything in [`LEAF_DIRS`] and [`LEAF_SEEDS`].
    ///
    /// Written out rather than read back from the manifest: a test that derives
    /// its expectations from the file under test asserts nothing. The last
    /// three are the one `symlink-dir` action expanded — one entry per child,
    /// dotted and in sorted order, because that is what the run produces.
    const LEAF_LINKS: [(&str, &str); 10] = [
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

        assert_leaf_portable_actions(&tree);

        for (source, dest) in LEAF_LINKS {
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
        // the installed configuration refers to does — `.zshrc` sources the
        // other two shell files, everything the `copy-dir` seeds, and a history
        // file in the directory the `create-dir` makes, and a fixture whose
        // shell would fail to start is not one anybody would keep.
        assert_eq!(
            entries(&tree.path("home")),
            [
                ".ackrc",
                ".cache",
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
        let refused = LEAF_LINKS
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

        // Everything ahead of the refusal, links and seeds alike: the whole
        // portable half is declared before the first link, so it is all of it
        // before this one.
        assert_leaf_portable_actions(&tree);
        for (_, dest) in &LEAF_LINKS[..refused] {
            assert!(tree.home(dest).is_symlink(), "`{dest}` was not installed");
        }
        for (_, dest) in &LEAF_LINKS[refused + 1..] {
            // Not `exists`, which follows the link and would call a dangling
            // one absent.
            assert!(
                fs::symlink_metadata(tree.home(dest)).is_err(),
                "`{dest}` was installed after the refusal"
            );
        }
    }

    // A dry run over the whole fixture, in the two halves it promises: it does
    // none of the work, and it says the same things the real run then says.

    /// A home already holding some of what the fixture installs: a seed's
    /// destination and a directory an action would otherwise make.
    ///
    /// Both dry-run tests start here rather than from an empty home, so the
    /// comparison has content to be wrong about and the run reports a `kept`
    /// and an `unchanged` as well as the rest.
    fn with_existing_content(tree: &Tree) {
        fs::create_dir_all(tree.home(".config/git")).expect("a config directory");
        fs::write(tree.home(".config/git/local"), "[user]\n\tname = me\n").expect("a seeded file");
        fs::create_dir_all(tree.home(".local/bin")).expect("a bin directory");
    }

    #[test]
    fn a_dry_run_writes_nothing_at_all_into_the_home() {
        let tree = Tree::fixture("leaf");
        with_existing_content(&tree);
        let before = snapshot(&tree.path("home"));

        tree.batfiles()
            .args(["sync", "--dry-run"])
            .assert()
            .success();

        // Byte for byte: no destination, no parent directory made on the way,
        // and no `.batfiles-incomplete` staging node, which is the one 2.1's
        // prohibition exists for.
        assert_eq!(snapshot(&tree.path("home")), before);
    }

    /// How many action records the `leaf` fixture declares.
    ///
    /// The parity assertion covers all of them, which is sound only because
    /// their destinations are distinct. An action whose output is another's
    /// input diverges in substance rather than tense, so it has to be excluded
    /// from the comparison rather than tolerated by it; this count is what
    /// makes adding one say so.
    const PARITY_ACTIONS: usize = 11;

    #[test]
    fn a_dry_runs_lines_are_the_real_runs_lines_in_another_tense() {
        let tree = Tree::fixture("leaf");
        with_existing_content(&tree);
        let manifest = fs::read_to_string(tree.manifest()).expect("the fixture manifest");
        assert_eq!(
            manifest.matches("[[actions]]").count(),
            PARITY_ACTIONS,
            "the fixture's actions changed; see `PARITY_ACTIONS`"
        );

        // `-v` so the lines that only appear at detail — `unchanged`, and the
        // `kept` a seed reports — are compared too.
        let dry = stderr_of(
            &tree
                .batfiles()
                .args(["sync", "--dry-run", "-v"])
                .assert()
                .success(),
        );
        // The same tree, which the dry run has left exactly as it found it.
        let real = stderr_of(&tree.batfiles().args(["sync", "-v"]).assert().success());

        let said: Vec<String> = dry.lines().map(in_past_tense).collect();
        assert_eq!(said, real.lines().collect::<Vec<&str>>());
        // Guards the comparison itself: two runs that both said nothing
        // prospective would match line for line and prove nothing.
        for expected in ["would link", "would copy", "would create", "would keep"] {
            assert!(
                dry.contains(expected),
                "the dry run never said `{expected}`:\n{dry}"
            );
        }
    }

    /// One reported line as the real run would have written it.
    ///
    /// The inverse of `Verb::say`, and deliberately spelled out here rather
    /// than imported: a test that shared the table with the code under test
    /// would agree with it however wrong both were.
    fn in_past_tense(line: &str) -> String {
        for (prospective, past) in [
            ("would relink ", "relinked "),
            ("would link ", "linked "),
            ("would copy ", "copied "),
            ("would create ", "created "),
            ("would remove ", "removed "),
            ("would keep ", "kept "),
        ] {
            if let Some(rest) = line.strip_prefix(prospective) {
                return format!("{past}{rest}");
            }
        }
        line.to_owned()
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
        .args(["sync", "--refresh-content"])
        .assert()
        .failure()
        .code(2);
    let stderr = stderr_of(&assertion);
    for expected in ["--refresh-content", "9.4"] {
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
        .args(["sync", "--refresh-content"])
        .assert()
        .failure()
        .code(2);
    let stderr = stderr_of(&assertion);
    for expected in ["--refresh-content", "9.4"] {
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
            &["apply-action", "--id", "vim", "--no-overwrite"],
            "--no-overwrite",
            "9.4",
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
