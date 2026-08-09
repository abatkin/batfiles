//! End-to-end checks of the built `batfiles` binary.

use assert_cmd::Command;
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
