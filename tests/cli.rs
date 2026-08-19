//! End-to-end checks of the built `batfiles` binary.

use assert_cmd::Command;

fn batfiles() -> Command {
    let mut command = Command::cargo_bin("batfiles").expect("the batfiles binary should be built");
    // The tests must not inherit the developer's own color environment.
    command.env_remove("BATFILES_COLOR").env_remove("NO_COLOR");
    // Every root is selected, so no test depends on the machine the suite runs
    // on. None of these directories exists, which is safe because no command
    // reads or writes one yet. A test that wants a default back removes the
    // variable that covers it.
    command
        .env("BATFILES_HOME", "/selected-home")
        .env("BATFILES_DIR", "/selected-repo")
        .env("BATFILES_CONFIG_DIR", "/selected-config")
        .env("BATFILES_CACHE_DIR", "/selected-cache")
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("XDG_CACHE_HOME");
    command
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

#[test]
fn unimplemented_commands_fail_with_a_clear_message() {
    let assertion = batfiles().arg("sync").assert().failure().code(2);
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("`sync` is not implemented yet"),
        "unexpected stderr:\n{stderr}"
    );
}

#[test]
fn an_unimplemented_subcommand_is_named_in_full() {
    let assertion = batfiles()
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
    batfiles().arg("sync").assert().failure().stdout("");
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

// Location resolution. Nothing reads or writes a resolved root yet, so `-v` is
// how a root is observed from outside the binary.

#[test]
fn verbose_reports_every_resolved_root() {
    let assertion = batfiles().args(["sync", "-v"]).assert().failure().code(2);
    let stderr = stderr_of(&assertion);
    for expected in [
        "repository: /selected-repo",
        "home:       /selected-home",
        "config:     /selected-config",
        "cache:      /selected-cache",
    ] {
        assert!(stderr.contains(expected), "no `{expected}` in:\n{stderr}");
    }
}

#[test]
fn the_roots_are_reported_only_when_asked_for() {
    let assertion = batfiles().arg("sync").assert().failure().code(2);
    let stderr = stderr_of(&assertion);
    assert!(
        !stderr.contains("/selected-repo"),
        "unexpected detail:\n{stderr}"
    );
}

#[test]
fn a_location_option_outranks_its_variable() {
    let assertion = batfiles()
        .args(["sync", "-v", "--batfiles-dir", "/from-the-option"])
        .assert()
        .failure()
        .code(2);
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("repository: /from-the-option"),
        "the option did not win:\n{stderr}"
    );
    assert!(
        stderr.contains("home:       /selected-home"),
        "an unselected root changed:\n{stderr}"
    );
}

#[test]
fn the_leaf_repository_defaults_under_the_selected_home() {
    let assertion = batfiles()
        .env_remove("BATFILES_DIR")
        .args(["sync", "-v"])
        .assert()
        .failure()
        .code(2);
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("repository: /selected-home/dotfiles"),
        "unexpected default:\n{stderr}"
    );
}

#[test]
fn config_and_cache_do_not_follow_the_selected_home() {
    // Batfiles' own state belongs to the invoking user, not to whichever home
    // is being installed into, so `--home-dir` must not move it.
    let assertion = batfiles()
        .env_remove("BATFILES_CONFIG_DIR")
        .env_remove("BATFILES_CACHE_DIR")
        .env("XDG_CONFIG_HOME", "/xdg-config")
        .env("XDG_CACHE_HOME", "/xdg-cache")
        .args(["sync", "-v", "--home-dir", "/elsewhere"])
        .assert()
        .failure()
        .code(2);
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

// Batfiles' own diagnostics, as opposed to the ones clap renders.

#[test]
fn an_error_is_labeled() {
    let assertion = batfiles()
        .args(["--color", "never", "sync"])
        .assert()
        .failure()
        .code(2);
    let stderr = stderr_of(&assertion);
    assert_eq!(stderr, "error: `sync` is not implemented yet\n");
}

#[test]
fn color_always_colors_the_label_of_an_error_batfiles_raised() {
    let assertion = batfiles()
        .args(["--color", "always", "sync"])
        .assert()
        .failure()
        .code(2);
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("\x1b[1;31merror:\x1b[0m `sync` is not implemented yet"),
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
