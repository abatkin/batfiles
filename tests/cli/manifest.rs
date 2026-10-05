//! Reading `batfiles.toml`. Every case here is refused before anything is
//! executed, so all of them run on every platform.

use std::fs;

use crate::support::*;

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

// Variable schema validation; condition evaluation is covered in `conditions.rs`.

#[test]
fn a_vars_section_alone_changes_nothing() {
    let tree = Tree::new();
    tree.write_manifest(&format!(
        r#"[vars]
work = "false"
profile = "personal"
_rank = "3"
empty = ""

{}"#,
        one_create_dir("~/.config")
    ));

    tree.batfiles().arg("sync").assert().success();

    assert!(
        tree.home(".config").is_dir(),
        "the run did not install what the manifest asked for"
    );
}

#[test]
fn a_variable_name_follows_its_own_rule_rather_than_the_id_rule() {
    for name in ["oh-my-zsh", "1up", "has.dot"] {
        let stderr = rejected(&format!("[vars]\n\"{name}\" = \"x\"\n"));
        for expected in [name, "a variable name must start with"] {
            assert!(stderr.contains(expected), "no `{expected}` in:\n{stderr}");
        }
    }
}

#[test]
fn a_name_the_expression_language_owns_is_rejected() {
    for reserved in ["facts", "env", "vars", "true", "false"] {
        let stderr = rejected(&format!("[vars]\n{reserved} = \"x\"\n"));
        assert!(
            stderr.contains("reserved by the expression language"),
            "`{reserved}` was accepted:\n{stderr}"
        );
    }
}

#[test]
fn a_value_that_is_not_a_string_is_rejected_where_it_is_written() {
    for (document, line) in [
        ("[vars]\nwork = true\n", "line 2"),
        ("[vars]\nrank = 3\n", "line 2"),
        ("[vars]\nnames = [\"a\"]\n", "line 2"),
    ] {
        let stderr = rejected(document);
        for expected in ["expected a string", line] {
            assert!(stderr.contains(expected), "no `{expected}` in:\n{stderr}");
        }
    }
}

#[test]
fn a_malformed_dynamic_variable_declaration_is_rejected_by_line() {
    for (document, expected) in [
        (
            "[vars.has_op]\ncommand = \"true\"\nchache = \"1h\"\n",
            "unknown field `chache`",
        ),
        (
            "[vars]\nemail = { cache = \"1h\" }\n",
            "missing field `command`",
        ),
        ("[vars]\nemail = { command = [] }\n", "has nothing to run"),
        (
            "[vars]\nemail = { command = \"true\", cache = \"-1h\" }\n",
            "cannot be negative",
        ),
        (
            "[vars]\nemail = { command = \"true\", command-timeout = \"0s\" }\n",
            "must be greater than zero",
        ),
        (
            "[vars]\nemail = { command = \"true\", capture = \"stderr\" }\n",
            "unknown variant `stderr`",
        ),
    ] {
        let stderr = rejected(document);
        assert!(stderr.contains(expected), "no `{expected}` in:\n{stderr}");
        assert!(stderr.contains("line"), "no line in:\n{stderr}");
    }
}

// Default-disabled schema validation; adoption is covered in `bootstrap.rs`.

/// The two lists, written the way the format spells them.
const CANDIDATES: &str = r#"[[default-disabled.actions]]
id = "p10k"

[[default-disabled.groups]]
group = "gui"
"#;

#[test]
fn a_default_disabled_section_is_accepted_and_changes_nothing() {
    let tree = Tree::new();
    tree.write_manifest(&format!("{CANDIDATES}\n{}", one_create_dir("~/.config")));

    tree.batfiles().arg("sync").assert().success();

    assert!(
        tree.home(".config").is_dir(),
        "the run did not install what the manifest asked for"
    );
    assert!(
        !tree.disabled().exists(),
        "the candidates were adopted, which is `clone`'s to do"
    );
}

#[test]
fn a_default_disabled_candidate_does_not_disable_anything_under_sync() {
    let tree = Tree::new();
    tree.write_manifest(
        r#"[[default-disabled.actions]]
id = "config"

[[actions]]
type = "create-dir"
id = "config"
dest = "~/.config"
"#,
    );

    tree.batfiles().arg("sync").assert().success();

    assert!(
        tree.home(".config").is_dir(),
        "the candidate disabled the action it names"
    );
}

#[test]
fn a_candidate_takes_a_condition_and_still_changes_nothing_under_sync() {
    let tree = Tree::new();
    tree.write_manifest(
        r#"[[default-disabled.actions]]
id = "config"
when = "work"

[[default-disabled.groups]]
group = "gui"
unless = "facts.os == 'macos'"

[[actions]]
type = "create-dir"
id = "config"
dest = "~/.config"
"#,
    );

    tree.batfiles().arg("sync").assert().success();

    assert!(
        tree.home(".config").is_dir(),
        "the candidate's condition took effect on the action it names"
    );
}

#[test]
fn a_candidate_writing_both_conditions_is_rejected() {
    for entry in [
        r#"[[default-disabled.actions]]
id = "p10k"
when = "work"
unless = "work"
"#,
        r#"[[default-disabled.groups]]
group = "gui"
when = "work"
unless = "work"
"#,
    ] {
        let stderr = rejected(entry);
        assert!(
            stderr.contains("writes both `when` and `unless`"),
            "the entry was accepted:\n{stderr}"
        );
    }
}

#[test]
fn a_candidates_condition_is_parsed_where_it_is_written() {
    let stderr = rejected(
        r#"[[default-disabled.actions]]
id = "p10k"
when = "work &&"
"#,
    );
    assert!(
        stderr.contains("is not a valid condition"),
        "the condition was accepted:\n{stderr}"
    );
}

#[test]
fn a_candidate_may_name_a_qualified_address() {
    let tree = Tree::new();
    fs::write(
        tree.manifest(),
        r#"[[default-disabled.actions]]
id = "core.p10k"

[[default-disabled.groups]]
group = "core.gui"
"#,
    )
    .expect("a manifest");

    tree.batfiles().arg("sync").assert().success();
}

#[test]
fn a_candidate_naming_a_malformed_address_is_rejected() {
    for entry in [
        "[[default-disabled.actions]]\nid = \"core..p10k\"\n",
        "[[default-disabled.groups]]\ngroup = \"my group\"\n",
    ] {
        let stderr = rejected(entry);
        assert!(
            stderr.contains("not a valid address"),
            "the name was accepted:\n{stderr}"
        );
    }
}

#[test]
fn a_candidate_without_the_field_that_names_it_is_rejected() {
    for (entry, field) in [
        ("[[default-disabled.actions]]\ngroup = \"gui\"\n", "id"),
        ("[[default-disabled.groups]]\nid = \"p10k\"\n", "group"),
    ] {
        let stderr = rejected(entry);
        assert!(stderr.contains(field), "`{field}` was not named:\n{stderr}");
    }
}

#[test]
fn the_default_disabled_records_are_closed_like_every_other() {
    for (document, field) in [
        ("[default-disabled]\nremotes = []\n", "remotes"),
        (
            "[[default-disabled.actions]]\nid = \"p10k\"\nreason = \"slow\"\n",
            "reason",
        ),
    ] {
        let stderr = rejected(document);
        assert!(stderr.contains(field), "`{field}` was not named:\n{stderr}");
    }
}

#[test]
fn the_two_symlink_types_do_not_share_a_field_set() {
    let stderr = rejected(
        r#"[[actions]]
type = "symlink"
source-dir = "files"
dest = "~"
"#,
    );
    assert!(
        stderr.contains("source-dir"),
        "the field was not named:\n{stderr}"
    );

    let stderr = rejected(
        r#"[[actions]]
type = "symlink-dir"
source = "files/zshrc"
dest-dir = "~"
"#,
    );
    assert!(
        stderr.contains("source"),
        "the field was not named:\n{stderr}"
    );
}

#[test]
fn a_symlink_dir_filter_matches_child_names_and_so_refuses_a_slash() {
    let stderr = rejected(
        r#"[[actions]]
type = "symlink-dir"
source-dir = "files"
dest-dir = "~"
exclude = ["README.md", "config/private"]
"#,
    );
    assert!(
        stderr.contains("exclude pattern `config/private` contains `/`"),
        "the pattern was not named:\n{stderr}"
    );
}

#[test]
fn a_filter_pattern_that_is_not_a_glob_is_rejected() {
    for (pattern, expected) in [
        ("[abc", "is not a glob"),
        ("", "a pattern is empty"),
        ("/zshrc", "starts with `/`"),
    ] {
        let stderr = rejected(&format!(
            "[[actions]]\ntype = \"symlink-dir\"\nsource-dir = \"files\"\ndest-dir = \"~\"\n\
             include = \"{pattern}\"\n"
        ));
        assert!(
            stderr.contains(expected),
            "no `{expected}` for `{pattern}`:\n{stderr}"
        );
    }
}

#[test]
fn a_symlink_dir_missing_a_required_field_is_rejected() {
    let stderr = rejected(
        r#"[[actions]]
type = "symlink-dir"
source-dir = "files"
"#,
    );
    assert!(
        stderr.contains("dest-dir"),
        "the field was not named:\n{stderr}"
    );
}

#[test]
fn a_symlink_dirs_paths_follow_the_same_rules_as_a_symlinks() {
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
    let stderr = rejected(
        r#"[[actions]]
type = "create-dir"
source = "files"
dest = "~/.config"
"#,
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
    let stderr = rejected(
        r#"[[actions]]
type = "copy"
source = "seed"
dest = "~/.config"
dot-prefix = true
"#,
    );
    assert!(
        stderr.contains("dot-prefix"),
        "the field was not named:\n{stderr}"
    );
}

#[test]
fn the_two_copy_types_do_not_share_a_field_set() {
    let stderr = rejected(
        r#"[[actions]]
type = "copy"
source-dir = "seed"
dest = "~"
"#,
    );
    assert!(
        stderr.contains("source-dir"),
        "the field was not named:\n{stderr}"
    );

    let stderr = rejected(
        r#"[[actions]]
type = "copy-dir"
source = "seed/gitconfig"
dest-dir = "~"
"#,
    );
    assert!(
        stderr.contains("source"),
        "the field was not named:\n{stderr}"
    );
}

#[test]
fn the_copy_types_take_filters_that_may_reach_below_a_child() {
    let tree = Tree::new();
    tree.repo_file("seed/a/b", "b\n");
    tree.write_manifest(
        r#"[[actions]]
type = "copy"
source = "seed"
dest = "~/one"
include = "a/*"

[[actions]]
type = "copy-dir"
source-dir = "seed"
dest-dir = "~/two"
exclude = ["a/c", "**/*.bak"]
"#,
    );
    tree.batfiles().arg("sync").assert().success();
    assert!(tree.home("one/a/b").is_file());
    assert!(tree.home("two/a/b").is_file());
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
fn a_type_the_format_does_not_specify_is_not_an_action_at_all() {
    let stderr = rejected(
        r#"[[actions]]
type = "rsync"
source = "a"
"#,
    );
    assert!(
        stderr.contains("rsync"),
        "the type was not named:\n{stderr}"
    );
}

#[test]
fn an_action_missing_a_required_field_is_rejected() {
    let stderr = rejected(
        r#"[[actions]]
type = "symlink"
source = "files/zshrc"
"#,
    );
    assert!(
        stderr.contains("dest"),
        "the field was not named:\n{stderr}"
    );
}

#[test]
fn an_id_that_breaks_the_id_rule_is_rejected() {
    let stderr = rejected(
        r#"[[actions]]
type = "symlink"
id = "core.zshrc"
source = "files/zshrc"
dest = "~/.zshrc"
"#,
    );
    for expected in ["core.zshrc", "not a valid ID"] {
        assert!(stderr.contains(expected), "no `{expected}` in:\n{stderr}");
    }
}

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
fn a_path_within_a_remote_may_not_start_the_reference_over() {
    let stderr = rejected(&format!(
        r#"[remotes.core]
type = "git"
url = "https://git.example/core.git"

{}"#,
        one_symlink("@core/@work/zshrc", "~/.zshrc")
    ));
    for expected in ["@core/@work/zshrc", "starts with `@`"] {
        assert!(stderr.contains(expected), "no `{expected}` in:\n{stderr}");
    }
}

#[test]
fn an_empty_dest_is_rejected_in_favor_of_writing_the_home_out() {
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
    // Keep the first action valid to check that the error identifies the second.
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
    let stderr = rejected(
        r#"[[actions]]
type = "symlink"
id = "zshrc"
source = "files/zshrc"
dest = "~/.zshrc"

[[actions]]
type = "symlink"
id = "zshrc"
source = "files/zshrc.local"
dest = "~/.zshrc.local"
"#,
    );
    for expected in ["zshrc", "action 2", "action 1"] {
        assert!(stderr.contains(expected), "no `{expected}` in:\n{stderr}");
    }
}

#[test]
fn a_command_that_does_not_need_the_manifest_does_not_read_it() {
    let tree = Tree::new();
    fs::write(tree.manifest(), "[[actions]\n").expect("a malformed manifest");

    tree.batfiles()
        .args(["vars", "set", "profile", "work"])
        .assert()
        .success();
}
