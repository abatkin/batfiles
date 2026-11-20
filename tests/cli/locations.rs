//! Where a command decides to work: the five roots, their precedence, and
//! which commands resolve none.

use crate::support::*;

// Use verbose output to observe roots that the command does not otherwise read or write.

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
        ("state:      ", "state"),
    ] {
        let expected = format!("{label}{}", display(&tree.path(root)));
        assert!(stderr.contains(&expected), "no `{expected}` in:\n{stderr}");
    }
    let cache = stderr.find("cache:").expect("a cache line");
    let state = stderr.find("state:").expect("a state line");
    assert!(cache < state, "the state root is reported last:\n{stderr}");
}

#[test]
fn every_command_that_executes_actions_resolves_the_state_root() {
    let tree = Tree::new();
    // The apply commands report their roots before failing to find their targets.
    for args in [
        &["sync", "-v"][..],
        &["sync", "-v", "--dry-run"],
        &["apply-action", "-v", "--id", "zshrc"],
        &["apply-group", "-v", "--group", "unused"],
    ] {
        let assertion = tree.batfiles().args(args).assert();
        let stderr = stderr_of(&assertion);
        let expected = format!("state:      {}", display(&tree.path("state")));
        assert!(
            stderr.contains(&expected),
            "{args:?} did not report `{expected}`:\n{stderr}"
        );
    }
}

#[test]
fn a_run_with_no_run_action_creates_no_state_directory() {
    let tree = Tree::new();
    tree.batfiles().arg("sync").assert().success();
    assert!(
        !tree.path("state").exists(),
        "a state directory was created with nothing to keep in it"
    );
}

#[test]
fn the_state_option_outranks_its_variable() {
    let tree = Tree::new();
    let chosen = tree.path("from-the-option");
    let assertion = tree
        .batfiles()
        .args(["sync", "-v", "--state-dir"])
        .arg(&chosen)
        .assert()
        .success();
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains(&format!("state:      {}", display(&chosen))),
        "the option did not win:\n{stderr}"
    );
}

#[test]
fn the_state_root_falls_back_to_xdg_and_then_the_os_home() {
    let tree = Tree::new();
    let assertion = tree
        .batfiles()
        .env_remove("BATFILES_STATE_DIR")
        .env("XDG_STATE_HOME", tree.path("xdg-state"))
        .args(["sync", "-v"])
        .assert()
        .success();
    let stderr = stderr_of(&assertion);
    let expected = format!("state:      {}", display(&tree.path("xdg-state/batfiles")));
    assert!(stderr.contains(&expected), "no `{expected}` in:\n{stderr}");

    let os_home = tree.path("os-home");
    let assertion = tree
        .batfiles()
        .env_remove("BATFILES_STATE_DIR")
        .env("HOME", &os_home)
        .env("USERPROFILE", &os_home)
        .args(["sync", "-v"])
        .assert()
        .success();
    let stderr = stderr_of(&assertion);
    let expected = format!(
        "state:      {}",
        display(&tree.path("os-home/.local/state/batfiles"))
    );
    assert!(stderr.contains(&expected), "no `{expected}` in:\n{stderr}");
}

#[test]
fn commands_that_execute_no_actions_do_not_resolve_the_state_root() {
    let tree = Tree::new();
    for args in [
        &["vars", "list", "-v"][..],
        &["vars", "refresh", "-v"],
        &["vars", "get", "-v", "editor"],
        &["disable-action", "-v", "vim"],
    ] {
        let assertion = tree.batfiles().args(args).assert();
        let stderr = stderr_of(&assertion);
        assert!(
            !stderr.contains("state:"),
            "{args:?} resolved the state root:\n{stderr}"
        );
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
    let working = tree.path("working");
    std::fs::create_dir(&working).expect("a working directory");
    let assertion = tree
        .batfiles()
        .env_remove("BATFILES_DIR")
        .current_dir(working)
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
fn a_manifest_in_the_working_directory_selects_that_repository() {
    let tree = Tree::new();
    let working = tree.repository("working");
    let assertion = tree
        .batfiles()
        .env_remove("BATFILES_DIR")
        .current_dir(&working)
        .args(["sync", "-v"])
        .assert()
        .success();
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains(&format!("repository: {}", display(&working))),
        "the working directory was not selected:\n{stderr}"
    );
}

#[test]
fn the_repository_option_outranks_the_working_directory() {
    let tree = Tree::new();
    let working = tree.repository("working");
    let selected = tree.repository("selected");
    let assertion = tree
        .batfiles()
        .env_remove("BATFILES_DIR")
        .current_dir(working)
        .args(["sync", "-v", "--batfiles-dir"])
        .arg(&selected)
        .assert()
        .success();
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains(&format!("repository: {}", display(&selected))),
        "--batfiles-dir did not win:\n{stderr}"
    );
}

#[test]
fn the_repository_variable_outranks_the_working_directory() {
    let tree = Tree::new();
    let working = tree.repository("working");
    let selected = tree.path("repo");
    let assertion = tree
        .batfiles()
        .current_dir(working)
        .args(["sync", "-v"])
        .assert()
        .success();
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains(&format!("repository: {}", display(&selected))),
        "BATFILES_DIR did not win:\n{stderr}"
    );
}

#[test]
fn an_invalid_working_directory_manifest_does_not_fall_through() {
    let tree = Tree::new();
    tree.repository("home/dotfiles");
    let working = tree.repository("working");
    std::fs::write(working.join("batfiles.toml"), "[").expect("an invalid manifest");
    let assertion = tree
        .batfiles()
        .env_remove("BATFILES_DIR")
        .current_dir(&working)
        .arg("sync")
        .assert()
        .failure();
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains(&display(&working.join("batfiles.toml")))
            && stderr.contains("invalid TOML"),
        "the working repository did not fail by name:\n{stderr}"
    );
}

#[cfg(unix)]
#[test]
fn a_command_that_does_not_use_the_repository_works_from_a_deleted_directory() {
    let tree = Tree::new();
    let working = tree.path("deleted-working-directory");
    std::fs::create_dir(&working).expect("a working directory");
    let assertion = assert_cmd::Command::new("sh")
        .args([
            "-c",
            "cd \"$1\" && rmdir \"$1\" && exec \"$2\" disable-action vim",
            "batfiles-deleted-cwd",
        ])
        .arg(&working)
        .arg(env!("CARGO_BIN_EXE_batfiles"))
        .env_remove("BATFILES_COLOR")
        .env_remove("NO_COLOR")
        .env("BATFILES_HOME", tree.path("home"))
        .env_remove("BATFILES_DIR")
        .env("BATFILES_CONFIG_DIR", tree.path("config"))
        .env("BATFILES_CACHE_DIR", tree.path("cache"))
        .assert()
        .success();
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("disabled action `vim`"),
        "the state-only command did not run:\n{stderr}"
    );
}

#[test]
fn a_command_that_does_not_use_the_repository_resolves_only_its_own_state() {
    let tree = Tree::new();
    let working = tree.repository("working");
    let assertion = tree
        .batfiles()
        .env_remove("BATFILES_DIR")
        .current_dir(working)
        .args(["disable-action", "vim", "-v"])
        .assert()
        .success();
    let stderr = stderr_of(&assertion);
    assert!(
        !stderr.contains("repository:"),
        "an unused repository was reported:\n{stderr}"
    );
    assert!(
        !stderr.contains("home:"),
        "a destination home was reported by a command that installs nothing:\n{stderr}"
    );
    assert!(
        stderr.contains(&format!("config:     {}", display(&tree.path("config")))),
        "the command's config root was not reported:\n{stderr}"
    );
}

#[test]
// Unix paths, which Windows would join with `\\`.
#[cfg(unix)]
fn config_and_cache_do_not_follow_the_selected_home() {
    let tree = Tree::new();
    let assertion = tree
        .batfiles()
        .env_remove("BATFILES_CONFIG_DIR")
        .env_remove("BATFILES_CACHE_DIR")
        .env("XDG_CONFIG_HOME", tree.path("xdg-config"))
        .env("XDG_CACHE_HOME", tree.path("xdg-cache"))
        .args(["sync", "-v", "--home-dir", "/elsewhere"])
        .assert()
        .success();
    let stderr = stderr_of(&assertion);
    for expected in [
        "home:       /elsewhere".to_owned(),
        format!("config:     {}", display(&tree.path("xdg-config/batfiles"))),
        format!("cache:      {}", display(&tree.path("xdg-cache/batfiles"))),
    ] {
        assert!(stderr.contains(&expected), "no `{expected}` in:\n{stderr}");
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
    let tree = Tree::roots();
    let working = tree.path("working");
    std::fs::create_dir(&working).expect("a working directory");

    let assertion = tree
        .batfiles()
        .current_dir(&working)
        .args(["init", "--no-git-init", "-v"])
        .assert()
        .success();

    let stderr = stderr_of(&assertion);
    for label in ["repository:", "home:", "config:", "cache:", "state:"] {
        assert!(
            !stderr.contains(label),
            "`init` resolved the {label} root:\n{stderr}"
        );
    }
    assert!(working.join("batfiles.toml").is_file());
}
