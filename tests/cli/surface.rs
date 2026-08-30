//! The command-line surface: what runs, what refuses, and how either is
//! reported. Nothing here executes an action.

use crate::support::*;

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
