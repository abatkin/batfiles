//! CLI tests for inclusion variable scopes and
//! [precedence](../../docs/environment.md#variable-precedence): remote defaults below leaf
//! values, inclusion overrides above them.

use crate::support::*;

/// Publish the supplied variables and two actions selected by complementary `profile`
/// conditions.
fn remote_declaring(vars: &str) -> BareRepo {
    let origin = BareRepo::new();
    origin.publish(
        "batfiles.toml",
        &format!(
            r#"{vars}[[actions]]
type = "create-dir"
id = "work-tools"
dest = "~/.cache/work-tools"
when = "profile == 'work'"

[[actions]]
type = "create-dir"
id = "personal-tools"
dest = "~/.cache/personal-tools"
unless = "profile == 'work'"
"#
        ),
        "two records one variable decides",
    );
    origin
}

/// The same remote, declaring no variables of its own.
fn remote() -> BareRepo {
    remote_declaring("")
}

/// A leaf including that remote, with `leaf_vars` in its own `[vars]` and
/// whatever the case writes on the inclusion. `remote_vars` is what the remote
/// declares for itself.
fn composed(remote_vars: &str, leaf_vars: &str, record: &str) -> (BareRepo, Tree) {
    let origin = remote_declaring(remote_vars);
    let tree = Tree::new();
    tree.write_manifest(&format!(
        r#"[remotes.corporate]
type = "git"
url = "{{origin}}"

{leaf_vars}[[actions]]
type = "include-remote"
id = "corp"
remote = "corporate"
{record}"#
    ));
    tree.point_at_origin(&origin);
    (origin, tree)
}

/// A leaf declaring `profile = "personal"` and including a remote that declares
/// nothing of its own.
fn including(record: &str) -> (BareRepo, Tree) {
    composed("", "[vars]\nprofile = \"personal\"\n\n", record)
}

/// Return whether stderr contains an exact heading line, ignoring surrounding whitespace.
fn headed_by(stderr: &str, heading: &str) -> bool {
    stderr.lines().any(|line| line.trim() == heading)
}

/// Infer the included records' profile from their installed destinations. Require exactly one
/// of the two destinations to exist.
fn included_profile(tree: &Tree) -> &'static str {
    let ran = |dest: &str| tree.home(dest).exists();
    match (ran(".cache/work-tools"), ran(".cache/personal-tools")) {
        (true, false) => "work",
        (false, true) => "personal",
        (work, personal) => {
            panic!("one record should have run: work-tools {work}, personal-tools {personal}")
        }
    }
}

#[test]
fn an_inclusion_writing_no_overrides_leaves_the_runs_variables_alone() {
    let (_origin, tree) = including("");

    tree.batfiles().arg("sync").assert().success();

    assert_eq!(included_profile(&tree), "personal");
}

#[test]
fn an_override_decides_a_contributed_records_condition() {
    let (_origin, tree) = including("vars = { profile = \"work\" }\n");

    let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();
    let stderr = stderr_of(&assertion);

    assert_eq!(
        included_profile(&tree),
        "work",
        "the override did not reach the record it was written for:\n{stderr}"
    );
}

#[test]
fn an_override_does_not_reach_the_leafs_own_records() {
    let origin = remote();
    let tree = Tree::new();
    tree.write_manifest(
        r#"[remotes.corporate]
type = "git"
url = "{origin}"

[vars]
profile = "personal"

[[actions]]
type = "create-dir"
id = "leaf-work"
dest = "~/.cache/leaf-work"
when = "profile == 'work'"

[[actions]]
type = "include-remote"
id = "corp"
remote = "corporate"
vars = { profile = "work" }
"#,
    );
    tree.point_at_origin(&origin);

    let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();
    let stderr = stderr_of(&assertion);

    assert_eq!(included_profile(&tree), "work");
    assert!(
        !tree.home(".cache/leaf-work").exists(),
        "an inclusion's override decided the leaf's own record:\n{stderr}"
    );
}

#[test]
fn an_override_reaches_only_the_inclusion_that_wrote_it() {
    // The inclusions share destinations, so use their reports to distinguish which ran.
    let origin = remote();
    let tree = Tree::new();
    tree.write_manifest(
        r#"[remotes.corporate]
type = "git"
url = "{origin}"

[vars]
profile = "personal"

[[actions]]
type = "include-remote"
id = "corp"
remote = "corporate"
vars = { profile = "work" }

[[actions]]
type = "include-remote"
id = "lab"
remote = "corporate"
"#,
    );
    tree.point_at_origin(&origin);

    let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();
    let stderr = stderr_of(&assertion);

    for expected in [
        "create-dir corp.personal-tools - skipped:",
        "create-dir lab.work-tools - skipped:",
    ] {
        assert!(
            stderr.contains(expected),
            "`{expected}` was not reported:\n{stderr}"
        );
    }
    for unexpected in [
        "create-dir corp.work-tools - skipped:",
        "create-dir lab.personal-tools - skipped:",
    ] {
        assert!(
            !stderr.contains(unexpected),
            "one inclusion's override decided the other's records:\n{stderr}"
        );
    }
}

#[test]
// Lowercase `BATFILES_VAR_*` names, which Windows uppercases at capture.
#[cfg(unix)]
fn the_machine_the_environment_and_the_command_line_all_beat_an_override() {
    for (what, machine, env, args) in [
        ("vars.toml", "profile = \"personal\"\n", None, Vec::new()),
        ("the environment", "", Some("personal"), Vec::new()),
        (
            "the command line",
            "",
            None,
            vec!["--var", "profile=personal"],
        ),
    ] {
        let (_origin, tree) = including("vars = { profile = \"work\" }\n");
        if !machine.is_empty() {
            tree.write_machine_vars(machine);
        }
        let mut command = tree.batfiles();
        command.arg("sync").args(&args);
        if let Some(value) = env {
            command.env("BATFILES_VAR_profile", value);
        }
        command.assert().success();

        assert_eq!(
            included_profile(&tree),
            "personal",
            "an override beat {what}"
        );
    }
}

#[test]
fn an_override_beats_the_leafs_own_declaration_of_the_same_name() {
    let (_origin, tree) = including("vars = { profile = \"work\" }\n");

    let assertion = tree.batfiles().args(["sync", "-vv"]).assert().success();
    let stderr = stderr_of(&assertion);

    assert_eq!(included_profile(&tree), "work");
    assert!(
        stderr.contains("profile = \"personal\" (batfiles.toml)"),
        "the run's own set stopped reading the leaf's value:\n{stderr}"
    );
}

#[test]
fn an_inclusions_own_condition_is_decided_without_its_overrides() {
    let (_origin, tree) =
        including("when = \"profile == 'work'\"\nvars = { profile = \"work\" }\n");

    let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();
    let stderr = stderr_of(&assertion);

    assert!(
        stderr.contains("include-remote corp - skipped: when \"profile == 'work'\" is false"),
        "the inclusion's own condition read its overrides:\n{stderr}"
    );
    for dest in [".cache/work-tools", ".cache/personal-tools"] {
        assert!(
            !tree.home(dest).exists(),
            "a closed inclusion contributed `{dest}`:\n{stderr}"
        );
    }
}

#[test]
fn a_remotes_own_condition_is_decided_without_an_inclusions_overrides() {
    let origin = remote();
    let tree = Tree::new();
    tree.write_manifest(
        r#"[remotes.corporate]
type = "git"
url = "{origin}"
when = "profile == 'work'"

[vars]
profile = "personal"

[[actions]]
type = "include-remote"
id = "corp"
remote = "corporate"
vars = { profile = "work" }
"#,
    );
    tree.point_at_origin(&origin);

    let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();
    let stderr = stderr_of(&assertion);

    assert!(
        stderr.contains("remote `corporate` is excluded here"),
        "the remote's condition read the inclusion's overrides:\n{stderr}"
    );
    assert!(
        !tree.path("repo/remotes/corporate").exists(),
        "an excluded remote was materialized anyway:\n{stderr}"
    );
}

#[test]
fn an_included_clone_lists_entries_are_decided_in_the_inclusions_scope() {
    let origin = BareRepo::new();
    let plugin = origin.another("zsh-z");
    origin.publish(
        "plugins.txt",
        &format!("{} when=\"profile == 'work'\"\n", display(&plugin)),
        "a list one variable decides",
    );
    origin.publish(
        "batfiles.toml",
        r#"[[actions]]
type = "git-clone-list"
id = "plugins"
source = "plugins.txt"
dest-dir = "~/.plugins"
"#,
        "clone what the list names",
    );

    for (record, cloned) in [("vars = { profile = \"work\" }\n", true), ("", false)] {
        let tree = Tree::new();
        tree.write_manifest(&format!(
            r#"[remotes.corporate]
type = "git"
url = "{{origin}}"

[vars]
profile = "personal"

[[actions]]
type = "include-remote"
id = "corp"
remote = "corporate"
{record}"#
        ));
        tree.point_at_origin(&origin);

        let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();
        let stderr = stderr_of(&assertion);

        assert_eq!(
            tree.home(".plugins/zsh-z").is_dir(),
            cloned,
            "the entry's condition did not read the inclusion's scope:\n{stderr}"
        );
    }
}

#[test]
fn an_inclusions_overrides_are_reported_at_the_second_verbose_level() {
    let (_origin, tree) = including("vars = { profile = \"work\" }\n");

    let assertion = tree.batfiles().args(["sync", "-vv"]).assert().success();
    let stderr = stderr_of(&assertion);

    assert!(
        headed_by(&stderr, "include-remote `corp` variables:"),
        "the block was not reported:\n{stderr}"
    );
    assert!(
        stderr.contains("profile = \"work\" (include-remote `corp`; over batfiles.toml)"),
        "the line did not name the inclusion and what it overrode:\n{stderr}"
    );
}

#[test]
fn an_override_the_machine_beat_is_reported_with_the_layer_that_won() {
    let (_origin, tree) = including("vars = { profile = \"work\" }\n");
    tree.write_machine_vars("profile = \"lab\"\n");

    let assertion = tree.batfiles().args(["sync", "-vv"]).assert().success();
    let stderr = stderr_of(&assertion);

    assert!(
        stderr.contains("profile = \"lab\" (vars.toml; over include-remote `corp`, batfiles.toml)"),
        "the block did not say the override lost:\n{stderr}"
    );
}

#[test]
fn an_inclusion_with_no_id_is_headed_by_where_it_was_written() {
    let origin = remote();
    let tree = Tree::new();
    tree.write_manifest(
        r#"[remotes.corporate]
type = "git"
url = "{origin}"

[vars]
profile = "personal"

[[actions]]
type = "include-remote"
remote = "corporate"
vars = { profile = "work" }
"#,
    );
    tree.point_at_origin(&origin);

    let assertion = tree.batfiles().args(["sync", "-vv"]).assert().success();
    let stderr = stderr_of(&assertion);

    assert!(
        headed_by(
            &stderr,
            "include-remote action 1 of remote `corporate` variables:"
        ),
        "the block did not name the inclusion the only way it can be named:\n{stderr}"
    );
}

#[test]
fn an_inclusion_writing_no_overrides_reports_no_block_of_its_own() {
    let (_origin, tree) = including("");

    let assertion = tree.batfiles().args(["sync", "-vv"]).assert().success();
    let stderr = stderr_of(&assertion);

    assert!(
        stderr.contains("variables:"),
        "the run's own variables were not reported:\n{stderr}"
    );
    assert!(
        !stderr.contains("include-remote `corp` variables:"),
        "an inclusion that overrode nothing reported a block:\n{stderr}"
    );
}

#[test]
fn an_unopened_inclusion_reports_nothing_about_its_overrides() {
    let origin = remote();
    let tree = Tree::new();
    tree.write_manifest(
        r#"[remotes.corporate]
type = "git"
url = "{origin}"

[vars]
profile = "personal"

[[actions]]
type = "create-dir"
id = "cache"
dest = "~/.cache/leaf"

[[actions]]
type = "include-remote"
id = "corp"
remote = "corporate"
vars = { profile = "work" }
"#,
    );
    tree.point_at_origin(&origin);
    tree.batfiles().arg("sync").assert().success();

    let assertion = tree
        .batfiles()
        .args(["apply-action", "-vv", "--id", "cache"])
        .assert()
        .success();
    let stderr = stderr_of(&assertion);

    assert!(
        !stderr.contains("include-remote `corp` variables:"),
        "an inclusion this command never opened reported its overrides:\n{stderr}"
    );
}

/// The `[vars]` a remote declares in its own manifest, for the cases below.
const REMOTE_VARS: &str = "[vars]\nprofile = \"work\"\n\n";

#[test]
fn a_remotes_own_vars_decide_its_records_where_nothing_overrides_them() {
    let (_origin, tree) = composed(REMOTE_VARS, "", "");

    let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();
    let stderr = stderr_of(&assertion);

    assert_eq!(
        included_profile(&tree),
        "work",
        "the remote's own declaration did not reach its records:\n{stderr}"
    );
}

#[test]
// Lowercase `BATFILES_VAR_*` names, which Windows uppercases at capture.
#[cfg(unix)]
fn the_leaf_the_inclusion_and_this_machine_all_beat_a_remotes_own_vars() {
    for (what, leaf_vars, record, machine, env, args) in [
        (
            "the leaf's own [vars]",
            "[vars]\nprofile = \"personal\"\n\n",
            "",
            "",
            None,
            Vec::new(),
        ),
        (
            "the inclusion's overrides",
            "",
            "vars = { profile = \"personal\" }\n",
            "",
            None,
            Vec::new(),
        ),
        (
            "vars.toml",
            "",
            "",
            "profile = \"personal\"\n",
            None,
            vec![],
        ),
        ("the environment", "", "", "", Some("personal"), Vec::new()),
        (
            "the command line",
            "",
            "",
            "",
            None,
            vec!["--var", "profile=personal"],
        ),
    ] {
        let (_origin, tree) = composed(REMOTE_VARS, leaf_vars, record);
        if !machine.is_empty() {
            tree.write_machine_vars(machine);
        }
        let mut command = tree.batfiles();
        command.arg("sync").args(&args);
        if let Some(value) = env {
            command.env("BATFILES_VAR_profile", value);
        }
        command.assert().success();

        assert_eq!(
            included_profile(&tree),
            "personal",
            "a remote's own declaration beat {what}"
        );
    }
}

#[test]
fn a_remotes_own_vars_do_not_reach_the_leafs_own_records() {
    let origin = remote_declaring(REMOTE_VARS);
    let tree = Tree::new();
    tree.write_manifest(
        r#"[remotes.corporate]
type = "git"
url = "{origin}"

[[actions]]
type = "create-dir"
id = "leaf-work"
dest = "~/.cache/leaf-work"
when = "vars.profile == 'work'"

[[actions]]
type = "include-remote"
id = "corp"
remote = "corporate"
"#,
    );
    tree.point_at_origin(&origin);

    let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();
    let stderr = stderr_of(&assertion);

    assert_eq!(included_profile(&tree), "work");
    assert!(
        !tree.home(".cache/leaf-work").exists(),
        "a remote's own declaration decided the leaf's record:\n{stderr}"
    );
}

#[test]
fn a_remotes_own_vars_do_not_reach_another_inclusions_records() {
    let work = remote_declaring(REMOTE_VARS);
    let lab = remote_declaring("[vars]\nprofile = \"personal\"\n\n");
    let tree = Tree::new();
    tree.write_manifest(&format!(
        r#"[remotes.corporate]
type = "git"
url = "{}"

[remotes.laboratory]
type = "git"
url = "{}"

[[actions]]
type = "include-remote"
id = "corp"
remote = "corporate"

[[actions]]
type = "include-remote"
id = "lab"
remote = "laboratory"
"#,
        display(&work.origin()),
        display(&lab.origin())
    ));

    let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();
    let stderr = stderr_of(&assertion);

    for expected in [
        "create-dir corp.personal-tools - skipped:",
        "create-dir lab.work-tools - skipped:",
    ] {
        assert!(
            stderr.contains(expected),
            "`{expected}` was not reported:\n{stderr}"
        );
    }
    for unexpected in [
        "create-dir corp.work-tools - skipped:",
        "create-dir lab.personal-tools - skipped:",
    ] {
        assert!(
            !stderr.contains(unexpected),
            "one remote's own declaration decided the other's records:\n{stderr}"
        );
    }
}

#[test]
fn an_inclusions_own_condition_is_decided_without_its_remotes_vars() {
    let (_origin, tree) = composed(REMOTE_VARS, "", "when = \"profile == 'work'\"\n");

    let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();
    let stderr = stderr_of(&assertion);

    assert!(
        stderr.contains("include-remote corp: when \"profile == 'work'\" cannot be evaluated")
            && stderr.contains("`profile` is not declared"),
        "the inclusion's own condition read its remote's variables:\n{stderr}"
    );
    for dest in [".cache/work-tools", ".cache/personal-tools"] {
        assert!(
            !tree.home(dest).exists(),
            "a closed inclusion contributed `{dest}`:\n{stderr}"
        );
    }
}

#[test]
fn an_included_clone_lists_entries_read_the_remotes_own_vars() {
    let origin = BareRepo::new();
    let plugin = origin.another("zsh-z");
    origin.publish(
        "plugins.txt",
        &format!("{} when=\"profile == 'work'\"\n", display(&plugin)),
        "a list one variable decides",
    );
    origin.publish(
        "batfiles.toml",
        r#"[vars]
profile = "work"

[[actions]]
type = "git-clone-list"
id = "plugins"
source = "plugins.txt"
dest-dir = "~/.plugins"
"#,
        "clone what the list names",
    );

    let tree = Tree::new();
    tree.write_manifest(
        r#"[remotes.corporate]
type = "git"
url = "{origin}"

[[actions]]
type = "include-remote"
id = "corp"
remote = "corporate"
"#,
    );
    tree.point_at_origin(&origin);

    let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();
    let stderr = stderr_of(&assertion);

    assert!(
        tree.home(".plugins/zsh-z").is_dir(),
        "the entry's condition did not read the remote's own variables:\n{stderr}"
    );
}

#[test]
fn a_remotes_own_vars_are_reported_under_the_inclusion_that_opened_them() {
    let (_origin, tree) = composed(REMOTE_VARS, "", "");

    let assertion = tree.batfiles().args(["sync", "-vv"]).assert().success();
    let stderr = stderr_of(&assertion);

    assert!(
        headed_by(&stderr, "include-remote `corp` variables:"),
        "the block was not reported:\n{stderr}"
    );
    assert!(
        stderr.contains("profile = \"work\" (batfiles.toml of include-remote `corp`)"),
        "the line did not name the manifest the value came from:\n{stderr}"
    );
}

#[test]
fn a_remotes_declaration_the_leaf_overrode_is_reported_as_having_lost() {
    let (_origin, tree) = composed(REMOTE_VARS, "[vars]\nprofile = \"personal\"\n\n", "");

    let assertion = tree.batfiles().args(["sync", "-vv"]).assert().success();
    let stderr = stderr_of(&assertion);

    assert_eq!(included_profile(&tree), "personal");
    assert!(
        stderr.contains(
            "profile = \"personal\" (batfiles.toml; over batfiles.toml of include-remote `corp`)"
        ),
        "the block did not say the remote's declaration lost:\n{stderr}"
    );
}

#[test]
fn a_remote_declaring_nothing_leaves_an_inclusion_with_no_block() {
    let (_origin, tree) = composed("", "", "");

    let assertion = tree.batfiles().args(["sync", "-vv"]).assert().success();
    let stderr = stderr_of(&assertion);

    assert!(
        !stderr.contains("include-remote `corp` variables:"),
        "an inclusion with nothing of its own reported a block:\n{stderr}"
    );
}

#[test]
fn vars_list_does_not_list_what_an_included_remote_declared() {
    let (_origin, tree) = composed(REMOTE_VARS, "[vars]\neditor = \"vi\"\n\n", "");
    tree.batfiles().arg("sync").assert().success();

    let assertion = tree.batfiles().args(["vars", "list"]).assert().success();
    let stdout = stdout_of(&assertion);

    assert!(
        stdout.contains("editor = \"vi\" (batfiles.toml)"),
        "the leaf's own variables were not listed:\n{stdout}"
    );
    assert!(
        !stdout.contains("profile"),
        "the listing reached into an inclusion's scope:\n{stdout}"
    );
}
