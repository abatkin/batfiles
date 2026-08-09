//! End-to-end checks of the built `batfiles` binary.

use assert_cmd::Command;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn batfiles() -> Command {
    let mut command = Command::cargo_bin("batfiles").expect("the batfiles binary should be built");
    // The tests must not inherit the developer's own color environment.
    command.env_remove("BATFILES_COLOR").env_remove("NO_COLOR");
    command
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

#[test]
fn unimplemented_commands_fail_with_a_clear_message() {
    let assertion = batfiles().arg("sync").assert().failure().code(1);
    let stderr = String::from_utf8_lossy(&assertion.get_output().stderr).into_owned();
    assert!(
        stderr.contains("`sync` is not implemented yet"),
        "unexpected stderr:\n{stderr}"
    );
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
    let stderr = String::from_utf8_lossy(&assertion.get_output().stderr).into_owned();
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
        let stderr = String::from_utf8_lossy(&assertion.get_output().stderr).into_owned();
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
    let stderr = String::from_utf8_lossy(&assertion.get_output().stderr).into_owned();
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
    let stderr = String::from_utf8_lossy(&assertion.get_output().stderr).into_owned();
    assert!(
        !stderr.contains('\x1b'),
        "expected no escape sequences:\n{stderr}"
    );
}

// The persistent enable/disable commands. The config directory is the only root
// they read or write under, so each test gets one of its own and passes it
// explicitly.

fn config_dir() -> TempDir {
    tempfile::tempdir().expect("temp dir")
}

/// An invocation whose machine-local state lives in `config`.
fn in_config(config: &Path) -> Command {
    let mut command = batfiles();
    command.arg("--config-dir").arg(config);
    command
}

fn disabled_path(config: &Path) -> PathBuf {
    config.join("disabled.toml")
}

fn disabled_document(config: &Path) -> String {
    fs::read_to_string(disabled_path(config)).expect("disabled.toml should exist")
}

/// Put a `disabled.toml` in place verbatim, including shapes batfiles would
/// never write itself.
fn write_disabled(config: &Path, document: &str) {
    fs::write(disabled_path(config), document).expect("fixture");
}

fn stderr_of(assertion: &assert_cmd::assert::Assert) -> String {
    String::from_utf8_lossy(&assertion.get_output().stderr).into_owned()
}

#[test]
fn disabling_an_action_creates_the_document() {
    let config = config_dir();
    in_config(config.path())
        .args(["disable-action", "p10k"])
        .assert()
        .success();

    assert_eq!(
        disabled_document(config.path()),
        "actions = [\"p10k\"]\ngroups = []\n"
    );
}

#[test]
fn a_group_goes_in_the_other_list() {
    let config = config_dir();
    in_config(config.path())
        .args(["disable-group", "work"])
        .assert()
        .success();

    assert_eq!(
        disabled_document(config.path()),
        "actions = []\ngroups = [\"work\"]\n"
    );
}

#[test]
fn an_address_is_recorded_exactly_as_written() {
    // Qualified and deeper-than-resolvable addresses alike: these commands
    // validate syntax and resolve nothing, so `a.b.c.d.e` is simply an address
    // that names nothing yet.
    let config = config_dir();
    in_config(config.path())
        .args([
            "disable-action",
            "core.zshrc",
            "core.zshrc.plugin",
            "a.b.c.d.e",
        ])
        .assert()
        .success();

    assert_eq!(
        disabled_document(config.path()),
        "actions = [\"a.b.c.d.e\", \"core.zshrc\", \"core.zshrc.plugin\"]\ngroups = []\n"
    );
}

#[test]
fn enabling_removes_an_address_and_keeps_an_empty_document() {
    let config = config_dir();
    in_config(config.path())
        .args(["disable-action", "p10k", "core.zshrc"])
        .assert()
        .success();
    in_config(config.path())
        .args(["enable-action", "p10k"])
        .assert()
        .success();
    assert_eq!(
        disabled_document(config.path()),
        "actions = [\"core.zshrc\"]\ngroups = []\n"
    );

    // Emptying the last entry keeps a canonical document rather than deleting it.
    in_config(config.path())
        .args(["enable-action", "core.zshrc"])
        .assert()
        .success();
    assert_eq!(
        disabled_document(config.path()),
        "actions = []\ngroups = []\n"
    );
}

#[test]
fn each_outcome_line_says_whether_the_state_moved() {
    let config = config_dir();
    in_config(config.path())
        .args(["disable-action", "p10k"])
        .assert()
        .success()
        .stderr("disabled action `p10k`\n");

    let assertion = in_config(config.path())
        .args(["disable-action", "p10k"])
        .assert()
        .success();
    assert_eq!(
        stderr_of(&assertion),
        "action `p10k` was already disabled\n"
    );

    // The one that matters most: re-enabling something that really was off.
    let assertion = in_config(config.path())
        .args(["enable-action", "p10k"])
        .assert()
        .success();
    assert_eq!(
        stderr_of(&assertion),
        "enabled action `p10k` (was disabled)\n"
    );

    let assertion = in_config(config.path())
        .args(["enable-action", "p10k"])
        .assert()
        .success();
    assert_eq!(stderr_of(&assertion), "action `p10k` was already enabled\n");
}

#[test]
fn quiet_suppresses_the_outcome_lines_without_suppressing_the_work() {
    let config = config_dir();
    for args in [
        ["--quiet", "disable-action", "p10k"],
        ["--quiet", "enable-action", "p10k"],
        ["--quiet", "enable-action", "p10k"],
    ] {
        in_config(config.path())
            .args(args)
            .assert()
            .success()
            .stderr("");
    }

    assert_eq!(
        disabled_document(config.path()),
        "actions = []\ngroups = []\n"
    );
}

#[test]
fn a_repeated_address_warns_and_is_applied_once() {
    let config = config_dir();
    // Under `--quiet`, so the warning is the only thing that can appear.
    let assertion = in_config(config.path())
        .args(["--quiet", "disable-action", "p10k", "p10k"])
        .assert()
        .success();

    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("`p10k` was given more than once"),
        "unexpected stderr:\n{stderr}"
    );
    assert_eq!(
        disabled_document(config.path()),
        "actions = [\"p10k\"]\ngroups = []\n"
    );
}

#[test]
fn an_invalid_address_fails_before_anything_is_written() {
    // Unsorted and duplicated on purpose: a save of any kind would canonicalize
    // it, so byte-identical content is the proof nothing was written.
    let config = config_dir();
    let original = "actions = [\"p10k\", \"core.zshrc\", \"p10k\"]\n";
    write_disabled(config.path(), original);

    // `vim` is perfectly valid and is still not applied: the invocation either
    // applies in full or changes nothing.
    let assertion = in_config(config.path())
        .args(["disable-action", "vim", "core..p10k"])
        .assert()
        .failure()
        .code(1);

    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("`core..p10k`"),
        "the error should name the offending address:\n{stderr}"
    );
    assert_eq!(disabled_document(config.path()), original);
}

#[test]
fn a_no_op_mutation_does_not_rewrite_the_document() {
    let config = config_dir();
    let original = "actions = [\"p10k\", \"core.zshrc\", \"p10k\"]\ngroups = []\n";
    write_disabled(config.path(), original);

    in_config(config.path())
        .args(["enable-action", "absent"])
        .assert()
        .success();
    assert_eq!(disabled_document(config.path()), original);

    in_config(config.path())
        .args(["disable-action", "p10k"])
        .assert()
        .success();
    assert_eq!(disabled_document(config.path()), original);
}

#[test]
fn a_no_op_against_a_missing_file_creates_nothing() {
    // A command that changed nothing does not leave a `disabled.toml` — or a
    // config directory — behind just because it ran.
    let config = config_dir();
    let absent = config.path().join("nested");

    in_config(&absent)
        .args(["enable-action", "p10k"])
        .assert()
        .success();

    assert!(
        !absent.exists(),
        "the config directory should not be created"
    );
}

#[test]
fn a_malformed_address_in_the_file_fails_the_command() {
    // Enabling is deliberately not a repair path for a file that does not parse.
    let config = config_dir();
    let original = "actions = [\"core..p10k\"]\n";
    write_disabled(config.path(), original);

    for command in ["disable-action", "enable-action"] {
        let assertion = in_config(config.path())
            .args([command, "p10k"])
            .assert()
            .failure()
            .code(1);

        let stderr = stderr_of(&assertion);
        assert!(
            stderr.contains("`core..p10k`"),
            "`{command}` should name the offending entry:\n{stderr}"
        );
        assert_eq!(disabled_document(config.path()), original);
    }
}

// The machine-local `vars` commands. Like the toggles, the config directory is
// the only root they read or write under.

fn vars_path(config: &Path) -> PathBuf {
    config.join("vars.toml")
}

fn vars_document(config: &Path) -> String {
    fs::read_to_string(vars_path(config)).expect("vars.toml should exist")
}

/// Put a `vars.toml` in place verbatim, including shapes batfiles would never
/// write itself.
fn write_vars(config: &Path, document: &str) {
    fs::write(vars_path(config), document).expect("fixture");
}

fn stdout_of(assertion: &assert_cmd::assert::Assert) -> String {
    String::from_utf8_lossy(&assertion.get_output().stdout).into_owned()
}

#[test]
fn setting_a_variable_creates_the_document() {
    let config = config_dir();
    in_config(config.path())
        .args(["vars", "set", "editor", "nvim"])
        .assert()
        .success()
        .stderr("set `editor`\n");

    assert_eq!(vars_document(config.path()), "editor = \"nvim\"\n");
}

#[test]
fn each_vars_outcome_line_says_whether_the_state_moved() {
    let config = config_dir();
    let line = |args: [&str; 4]| {
        let assertion = in_config(config.path()).args(args).assert().success();
        stderr_of(&assertion)
    };

    assert_eq!(line(["vars", "set", "editor", "nvim"]), "set `editor`\n");
    assert_eq!(
        line(["vars", "set", "editor", "emacs"]),
        "changed `editor` (it had a different value)\n"
    );
    assert_eq!(
        line(["vars", "set", "editor", "emacs"]),
        "`editor` was already set to that value\n"
    );

    let assertion = in_config(config.path())
        .args(["vars", "unset", "editor"])
        .assert()
        .success();
    assert_eq!(stderr_of(&assertion), "unset `editor`\n");

    let assertion = in_config(config.path())
        .args(["vars", "unset", "editor"])
        .assert()
        .success();
    assert_eq!(stderr_of(&assertion), "`editor` was not set\n");
}

#[test]
fn setting_a_variable_never_echoes_its_value() {
    // A value may be a token or a machine-identifying path, so it stays out of
    // scrollback and out of a wrapper script's logs. `vars get` is the way to
    // read one back.
    let config = config_dir();
    for value in ["s3cr3t", "s3cr3t", "other"] {
        let assertion = in_config(config.path())
            .args(["vars", "set", "editor", value])
            .assert()
            .success();
        assert_eq!(stdout_of(&assertion), "");
        assert!(
            !stderr_of(&assertion).contains(value),
            "the value should not be reported:\n{}",
            stderr_of(&assertion)
        );
    }
}

#[test]
fn a_repeated_set_does_not_rewrite_the_document() {
    // Unsorted and quoted the other way on purpose: any save at all would
    // canonicalize it, so byte-identical content is the proof nothing was
    // written.
    let config = config_dir();
    let original = "profile = 'work'\neditor = 'nvim'\n";
    write_vars(config.path(), original);

    in_config(config.path())
        .args(["vars", "set", "editor", "nvim"])
        .assert()
        .success();

    assert_eq!(vars_document(config.path()), original);
}

#[test]
fn getting_a_variable_prints_the_value_on_standard_output_alone() {
    let config = config_dir();
    in_config(config.path())
        .args(["vars", "set", "editor", "nvim"])
        .assert()
        .success();

    in_config(config.path())
        .args(["vars", "get", "editor"])
        .assert()
        .success()
        .stdout("nvim\n")
        .stderr("");
}

#[test]
fn quiet_never_suppresses_requested_data() {
    // `--quiet` suppresses what a command did, not what it was asked for.
    let config = config_dir();
    in_config(config.path())
        .args(["--quiet", "vars", "set", "editor", "nvim"])
        .assert()
        .success()
        .stderr("");

    in_config(config.path())
        .args(["--quiet", "vars", "get", "editor"])
        .assert()
        .success()
        .stdout("nvim\n")
        .stderr("");
}

#[test]
fn getting_an_absent_variable_fails_without_printing_anything() {
    // The alternative — an empty line and a zero status — is indistinguishable
    // from a key stored as the empty string, which `vars set` permits.
    let config = config_dir();
    in_config(config.path())
        .args(["vars", "set", "empty", ""])
        .assert()
        .success();

    in_config(config.path())
        .args(["vars", "get", "empty"])
        .assert()
        .success()
        .stdout("\n");

    let assertion = in_config(config.path())
        .args(["vars", "get", "editor"])
        .assert()
        .failure()
        .code(1);

    assert_eq!(stdout_of(&assertion), "");
    assert!(
        stderr_of(&assertion).contains("`editor`"),
        "the error should name the key:\n{}",
        stderr_of(&assertion)
    );
}

#[test]
fn getting_a_variable_reads_no_other_file() {
    // Neither the leaf repository nor the dynamic cache is consulted, so roots
    // pointing at paths that do not exist change nothing.
    let config = config_dir();
    let absent = config.path().join("absent");
    in_config(config.path())
        .args(["vars", "set", "editor", "nvim"])
        .assert()
        .success();

    in_config(config.path())
        .arg("--batfiles-dir")
        .arg(&absent)
        .arg("--cache-dir")
        .arg(&absent)
        .args(["vars", "get", "editor"])
        .assert()
        .success()
        .stdout("nvim\n");
    assert!(!absent.exists(), "nothing else should have been touched");
}

#[test]
fn unsetting_the_last_variable_keeps_an_empty_document() {
    let config = config_dir();
    in_config(config.path())
        .args(["vars", "set", "editor", "nvim"])
        .assert()
        .success();
    in_config(config.path())
        .args(["vars", "unset", "editor"])
        .assert()
        .success();

    assert_eq!(vars_document(config.path()), "");
}

#[test]
fn unsetting_an_absent_variable_against_a_missing_file_creates_nothing() {
    let config = config_dir();
    let absent = config.path().join("nested");

    in_config(&absent)
        .args(["vars", "unset", "editor"])
        .assert()
        .success();

    assert!(
        !absent.exists(),
        "the config directory should not be created"
    );
}

#[test]
fn an_invalid_key_fails_before_anything_is_written() {
    let config = config_dir();
    let original = "profile = 'work'\n";
    write_vars(config.path(), original);

    for args in [
        vec!["vars", "set", "1up", "x"],
        vec!["vars", "get", "1up"],
        vec!["vars", "unset", "1up"],
    ] {
        let assertion = in_config(config.path())
            .args(&args)
            .assert()
            .failure()
            .code(1);

        let stderr = stderr_of(&assertion);
        assert!(
            stderr.contains("`1up`") && stderr.contains("must start with"),
            "unexpected stderr for {args:?}:\n{stderr}"
        );
        assert_eq!(vars_document(config.path()), original);
    }
}

#[test]
fn a_malformed_vars_file_fails_the_command() {
    // Setting a key is deliberately not a repair path for a file that does not
    // parse.
    let config = config_dir();
    let original = "has-dash = 'x'\n";
    write_vars(config.path(), original);

    for args in [
        vec!["vars", "set", "editor", "nvim"],
        vec!["vars", "get", "editor"],
        vec!["vars", "unset", "editor"],
    ] {
        let assertion = in_config(config.path())
            .args(&args)
            .assert()
            .failure()
            .code(1);

        let stderr = stderr_of(&assertion);
        assert!(
            stderr.contains("vars.toml") && stderr.contains("a variable name must"),
            "unexpected stderr for {args:?}:\n{stderr}"
        );
        assert_eq!(vars_document(config.path()), original);
    }
}

// `init`. Every case runs in a temporary working directory, with the home
// pointed at a *different* temporary directory so the home-directory check is
// deterministic and never sees the developer's own home.

/// One `init` scenario: the directory being initialized, and the unrelated
/// directory that stands in for the invoking user's home.
struct Init {
    dir: TempDir,
    home: TempDir,
}

impl Init {
    fn new() -> Self {
        Self {
            dir: tempfile::tempdir().expect("temp dir"),
            home: tempfile::tempdir().expect("temp dir"),
        }
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    /// `init` in the working directory, with the home isolated.
    fn command(&self) -> Command {
        let mut command = with_home(batfiles(), self.home.path());
        command.current_dir(self.dir.path()).arg("init");
        command
    }

    /// The common case: no Git, so most of the suite neither shells out nor
    /// depends on `git` being installed.
    fn no_git(&self) -> Command {
        let mut command = self.command();
        command.arg("--no-git-init");
        command
    }

    fn join(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn read(&self, name: &str) -> String {
        fs::read_to_string(self.join(name)).unwrap_or_else(|_| panic!("{name} should be readable"))
    }
}

/// Point the invoking user's OS home at `path`.
///
/// Both variables, because the platform home input differs: `$HOME` on Unix and
/// `%USERPROFILE%` on Windows. Setting the pair keeps the home-directory check
/// deterministic wherever the suite runs, rather than only where `$HOME` happens
/// to be the one that counts.
fn with_home(mut command: Command, path: &Path) -> Command {
    command.env("HOME", path).env("USERPROFILE", path);
    command
}

/// Leave the invocation with no home variable at all, on either platform.
fn without_home(mut command: Command) -> Command {
    command.env_remove("HOME").env_remove("USERPROFILE");
    command
}

/// The sorted names directly under `dir`, so a test can assert that a refused
/// run created nothing.
fn entries(dir: &Path) -> Vec<OsString> {
    let mut names: Vec<OsString> = fs::read_dir(dir)
        .expect("read dir")
        .map(|entry| entry.expect("entry").file_name())
        .collect();
    names.sort();
    names
}

#[test]
fn init_lays_out_the_conventional_skeleton() {
    let init = Init::new();
    let assertion = init.no_git().assert().success();

    for name in ["batfiles.toml", "install.sh", ".gitignore"] {
        assert!(init.join(name).is_file(), "{name} should be a file");
    }
    for name in ["bin", "files", "local-files"] {
        assert!(init.join(name).is_dir(), "{name}/ should be a directory");
    }

    // `remotes/` is generated materialization data, so it appears only once
    // something materializes a remote — and is excluded from history instead.
    assert!(!init.join("remotes").exists());
    assert!(init.read(".gitignore").contains("remotes/"));
    // `--no-git-init` means exactly that.
    assert!(!init.join(".git").exists());

    // The starter manifest is a valid document that installs nothing.
    toml::from_str::<toml::Table>(&init.read("batfiles.toml"))
        .expect("the starter manifest should be valid TOML");

    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("initialized batfiles repository in"),
        "unexpected stderr:\n{stderr}"
    );
    let created = "created batfiles.toml, install.sh, .gitignore, bin/, files/, local-files/";
    assert!(stderr.contains(created), "unexpected stderr:\n{stderr}");
    assert!(
        stderr.contains("add files under files/ or local-files/"),
        "unexpected stderr:\n{stderr}"
    );
}

#[cfg(unix)]
#[test]
fn init_creates_an_executable_bootstrap_script() {
    use std::os::unix::fs::PermissionsExt as _;

    let init = Init::new();
    init.no_git().assert().success();

    let mode = fs::metadata(init.join("install.sh"))
        .expect("metadata")
        .permissions()
        .mode();
    assert!(mode & 0o111 != 0, "unexpected mode: {mode:o}");
}

#[test]
fn existing_paths_of_the_right_kind_survive_untouched() {
    let init = Init::new();
    fs::create_dir(init.join("files")).expect("fixture");
    fs::write(init.join("files/zshrc"), "mine\n").expect("fixture");
    fs::write(init.join("install.sh"), "#!/bin/sh\necho mine\n").expect("fixture");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(init.join("install.sh"), fs::Permissions::from_mode(0o644))
            .expect("fixture");
    }

    let assertion = init.no_git().assert().success();

    assert_eq!(init.read("files/zshrc"), "mine\n");
    assert_eq!(init.read("install.sh"), "#!/bin/sh\necho mine\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = fs::metadata(init.join("install.sh"))
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o644, "the existing mode should be preserved");
    }

    // Only what `init` actually created is reported.
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("created batfiles.toml, .gitignore, bin/, local-files/"),
        "unexpected stderr:\n{stderr}"
    );
}

#[test]
fn a_gitignore_that_does_not_cover_remotes_warns_without_being_rewritten() {
    let init = Init::new();
    let original = "*.swp\ntarget\n";
    fs::write(init.join(".gitignore"), original).expect("fixture");

    // Under `--quiet`, so the warning is the only thing that can appear. It
    // names the file relative to the repository root, where a `.gitignore`
    // always lives.
    let assertion = init.no_git().arg("--quiet").assert().success();

    assert_eq!(
        stderr_of(&assertion),
        "warning: .gitignore does not ignore the tool-owned `remotes/` tree; \
         consider adding `/remotes/` to it\n"
    );
    assert_eq!(init.read(".gitignore"), original);
}

#[test]
fn an_existing_manifest_refuses_the_command_and_creates_nothing() {
    // Any node by that name counts, a directory included: its presence, not its
    // kind, is what `init` refuses.
    for fixture in ["file", "directory"] {
        let init = Init::new();
        let manifest = init.join("batfiles.toml");
        if fixture == "file" {
            fs::write(&manifest, "# mine\n").expect("fixture");
        } else {
            fs::create_dir(&manifest).expect("fixture");
        }

        let assertion = init.no_git().assert().failure().code(1);

        let stderr = stderr_of(&assertion);
        assert!(
            stderr.contains("batfiles.toml") && stderr.contains("already"),
            "unexpected stderr for a {fixture}:\n{stderr}"
        );
        assert_eq!(entries(init.path()), ["batfiles.toml"]);
        if fixture == "file" {
            assert_eq!(init.read("batfiles.toml"), "# mine\n");
        }
    }
}

#[test]
fn a_skeleton_path_of_the_wrong_kind_refuses_the_command_and_creates_nothing() {
    let init = Init::new();
    fs::write(init.join("files"), "not a directory\n").expect("fixture");

    let assertion = init.no_git().assert().failure().code(1);

    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("files") && stderr.contains("directory"),
        "unexpected stderr:\n{stderr}"
    );
    assert_eq!(entries(init.path()), ["files"]);
}

#[test]
fn the_home_directory_itself_is_refused() {
    let init = Init::new();
    let assertion = with_home(init.no_git(), init.path())
        .assert()
        .failure()
        .code(1);

    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("home directory"),
        "unexpected stderr:\n{stderr}"
    );
    assert!(entries(init.path()).is_empty());
}

#[test]
fn a_home_that_cannot_be_found_does_not_stop_init() {
    // `init` needs a home only to refuse one particular directory, so not having
    // one is not fatal the way it is for a command that resolves roots.
    let init = Init::new();
    without_home(init.no_git()).assert().success();

    let other = Init::new();
    with_home(other.no_git(), &other.home.path().join("absent"))
        .assert()
        .success();
}

#[test]
fn quiet_suppresses_the_informational_lines_but_never_an_error() {
    let init = Init::new();
    init.no_git().arg("--quiet").assert().success().stderr("");
    assert!(init.join("batfiles.toml").is_file());

    // The same run again: now it refuses, and the refusal still speaks.
    let assertion = init.no_git().arg("--quiet").assert().failure().code(1);
    assert!(
        stderr_of(&assertion).contains("batfiles.toml"),
        "an error must survive --quiet"
    );
}

// The Git behavior. These shell out, so they are the only `init` cases that
// depend on `git` being installed.

#[test]
fn init_creates_a_git_repository() {
    let init = Init::new();
    let assertion = init.command().assert().success();

    assert!(init.join(".git").exists());
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("initialized a Git repository"),
        "unexpected stderr:\n{stderr}"
    );
}

#[test]
fn init_inside_an_existing_repository_skips_git_init() {
    let init = Init::new();
    std::process::Command::new("git")
        .arg("init")
        .current_dir(init.path())
        .output()
        .expect("git should be available");

    let nested = init.join("dotfiles");
    fs::create_dir(&nested).expect("fixture");
    let mut command = with_home(batfiles(), init.home.path());
    let assertion = command.current_dir(&nested).arg("init").assert().success();

    assert!(
        !nested.join(".git").exists(),
        "a repository already covers this directory"
    );
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("already covers"),
        "unexpected stderr:\n{stderr}"
    );
}

/// The two Git failures need a `git` that fails in a chosen way, which a shim
/// first on `PATH` provides. Emptying `PATH` alone cannot tell them apart:
/// detection runs first and would fail for the same reason `git init` does.
#[cfg(unix)]
mod git_failures {
    use super::*;
    use std::os::unix::fs::PermissionsExt as _;

    /// A directory holding a `git` that answers `rev-parse` with `false` and
    /// fails `init`.
    fn shim() -> TempDir {
        let bin = tempfile::tempdir().expect("temp dir");
        let script = "#!/bin/sh\n\
             case \"$1\" in\n\
             rev-parse) echo false ;;\n\
             *) echo 'fatal: the shim refuses' >&2; exit 1 ;;\n\
             esac\n";
        let path = bin.path().join("git");
        fs::write(&path, script).expect("fixture");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("fixture");
        bin
    }

    #[test]
    fn a_failing_git_init_fails_the_command() {
        let bin = shim();
        let init = Init::new();
        let assertion = init
            .command()
            .env("PATH", bin.path())
            .assert()
            .failure()
            .code(1);

        let stderr = stderr_of(&assertion);
        assert!(
            stderr.contains("the shim refuses"),
            "git's own diagnostic should reach the user:\n{stderr}"
        );
        assert!(
            stderr.contains("--no-git-init"),
            "unexpected stderr:\n{stderr}"
        );
        // The skeleton created before Git ran stays; creation does not unwind.
        assert!(init.join("batfiles.toml").is_file());
    }

    #[test]
    fn a_git_that_cannot_be_run_fails_the_command() {
        let empty = tempfile::tempdir().expect("temp dir");
        let init = Init::new();
        let assertion = init
            .command()
            .env("PATH", empty.path())
            .assert()
            .failure()
            .code(1);

        let stderr = stderr_of(&assertion);
        assert!(stderr.contains("`git`"), "unexpected stderr:\n{stderr}");
        assert!(
            stderr.contains("--no-git-init"),
            "unexpected stderr:\n{stderr}"
        );
    }
}

#[test]
fn the_leaf_repository_is_never_loaded() {
    let config = config_dir();
    let repo = config_dir();
    fs::write(repo.path().join("batfiles.toml"), "actions = \n").expect("fixture");

    // A `batfiles.toml` that cannot parse, and then no leaf repository at all.
    in_config(config.path())
        .arg("--batfiles-dir")
        .arg(repo.path())
        .args(["disable-action", "p10k"])
        .assert()
        .success();
    in_config(config.path())
        .arg("--batfiles-dir")
        .arg(repo.path().join("absent"))
        .args(["enable-action", "p10k"])
        .assert()
        .success();

    assert_eq!(
        disabled_document(config.path()),
        "actions = []\ngroups = []\n"
    );
}
