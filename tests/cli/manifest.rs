//! Reading `batfiles.toml`. Every case here is refused before anything is
//! executed, so all of them run on every platform.

use std::fs;

use crate::support::*;

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

// The rejections below never get as far as executing anything, so they run
// everywhere. Their positive counterpart — a record using every field it
// accepts, which has to be executed to be worth asserting — is
// `linking::a_symlink_action_parses_with_every_field_it_accepts`.

// `[vars]`: the values conditions are decided against. The section is accepted
// and checked as the manifest is read, so the tests below are about what the
// document will and will not take; what a condition makes of a value is
// `conditions.rs`.

#[test]
fn a_vars_section_alone_changes_nothing() {
    // Variables feed conditions and nothing else: no field of any action
    // interpolates one. So a run over a manifest whose records carry no
    // condition installs exactly what it would have installed without them.
    let tree = Tree::new();
    tree.write_manifest(&format!(
        "[vars]\nwork = \"false\"\nprofile = \"personal\"\n_rank = \"3\"\nempty = \"\"\n\n{}",
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
    // The two rules genuinely differ, and a manifest author holding the ID rule
    // in mind is the one who needs telling: `oh-my-zsh` is a fine action ID and
    // not a variable name.
    for name in ["oh-my-zsh", "1up", "has.dot"] {
        let stderr = rejected(&format!("[vars]\n\"{name}\" = \"x\"\n"));
        for expected in [name, "a variable name must start with"] {
            assert!(stderr.contains(expected), "no `{expected}` in:\n{stderr}");
        }
    }
}

#[test]
fn a_name_the_expression_language_owns_is_rejected() {
    // Reserved names keep namespace lookup unambiguous. Reject them while
    // loading the manifest, rather than accepting names evaluation cannot use.
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
    // Every variable value is a string, so the types a TOML author reaches for
    // are refused rather than converted. The diagnostic is serde's, and what
    // makes it enough is the position: it underlines the value itself.
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

// CARRY(9.1): dynamic variables are declared as a table under `[vars]`, so this
// is the assertion that step replaces.
#[test]
fn a_dynamic_variable_declaration_is_rejected_in_both_spellings() {
    // A table under `[vars]` is a dynamic-variable declaration, which arrives at
    // 9.1. Until then it is refused as the non-string value it is, rather than
    // parsed into a record nothing would ever run.
    for document in [
        "[vars.has_op]\ncommand = [\"sh\", \"-c\", \"command -v op\"]\ncache = \"1h\"\n",
        "[vars]\nhas_op = { command = [\"sh\", \"-c\", \"command -v op\"] }\n",
    ] {
        let stderr = rejected(document);
        assert!(
            stderr.contains("expected a string"),
            "the declaration was accepted:\n{stderr}"
        );
    }
}

// `[default-disabled]`: candidates a later bootstrap adopts. The section is
// accepted and checked as the manifest is read, and nothing reads it yet, so
// every test below is about what the document will and will not take.

/// The two lists, written the way the format spells them.
const CANDIDATES: &str = "[[default-disabled.actions]]\n\
                          id = \"p10k\"\n\
                          \n\
                          [[default-disabled.groups]]\n\
                          group = \"gui\"\n";

#[test]
fn a_default_disabled_section_is_accepted_and_changes_nothing() {
    // The closed document accepts the section. Nothing adopts the
    // candidates until 8.3, so the run installs what it would have installed
    // and leaves the machine-local lists alone — including by not creating the
    // `disabled.toml` that adoption would have to write.
    let tree = Tree::new();
    tree.write_manifest(&format!("{CANDIDATES}\n{}", one_create_dir("~/.config")));

    tree.batfiles().arg("sync").assert().success();

    assert!(
        tree.home(".config").is_dir(),
        "the run did not install what the manifest asked for"
    );
    assert!(
        !tree.disabled().exists(),
        "the candidates were adopted, which is 8.3's job"
    );
}

#[test]
fn a_default_disabled_candidate_does_not_disable_anything_yet() {
    // The candidate names the action, and the action still runs. A section that
    // quietly took effect would be the worse of the two failures, and this is
    // the assertion 8.3 has to change on purpose.
    let tree = Tree::new();
    tree.write_manifest(
        "[[default-disabled.actions]]\n\
         id = \"config\"\n\
         \n\
         [[actions]]\n\
         type = \"create-dir\"\n\
         id = \"config\"\n\
         dest = \"~/.config\"\n",
    );

    tree.batfiles().arg("sync").assert().success();

    assert!(
        tree.home(".config").is_dir(),
        "the candidate disabled the action it names"
    );
}

#[test]
fn a_candidate_takes_a_condition_and_still_changes_nothing() {
    // A candidate's condition is parsed and checked as the manifest is read and
    // evaluated nowhere: which candidates a fresh machine adopts is the
    // bootstrap's question, and it cannot ask one yet. So a condition here
    // decides nothing about the run, including the action the entry names.
    let tree = Tree::new();
    tree.write_manifest(
        "[[default-disabled.actions]]\n\
         id = \"config\"\n\
         when = \"work\"\n\
         \n\
         [[default-disabled.groups]]\n\
         group = \"gui\"\n\
         unless = \"facts.os == 'macos'\"\n\
         \n\
         [[actions]]\n\
         type = \"create-dir\"\n\
         id = \"config\"\n\
         dest = \"~/.config\"\n",
    );

    tree.batfiles().arg("sync").assert().success();

    assert!(
        tree.home(".config").is_dir(),
        "the candidate's condition took effect on the action it names"
    );
}

#[test]
fn a_candidate_writing_both_conditions_is_rejected() {
    // The rule every record carrying a condition follows, checked here even
    // though nothing evaluates these two: a candidate that could never mean one
    // thing is caught on the machine that writes it.
    for entry in [
        "[[default-disabled.actions]]\nid = \"p10k\"\nwhen = \"work\"\nunless = \"work\"\n",
        "[[default-disabled.groups]]\ngroup = \"gui\"\nwhen = \"work\"\nunless = \"work\"\n",
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
    let stderr = rejected("[[default-disabled.actions]]\nid = \"p10k\"\nwhen = \"work &&\"\n");
    assert!(
        stderr.contains("is not a valid condition"),
        "the condition was accepted:\n{stderr}"
    );
}

#[test]
fn a_candidate_may_name_a_qualified_address() {
    // What an entry names is never looked up, so a candidate can name an action
    // an included remote will contribute for the same reason it can name one a
    // later branch will introduce: there is nothing to resolve it against
    // either way.
    let tree = Tree::new();
    fs::write(
        tree.manifest(),
        "[[default-disabled.actions]]\nid = \"core.p10k\"\n\n\
         [[default-disabled.groups]]\ngroup = \"core.gui\"\n",
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
    // An entry that names nothing is a mistake rather than a candidate, and it
    // is one the format can catch on the machine that writes it.
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
fn a_type_the_format_does_not_specify_is_not_an_action_at_all() {
    // Every action type the format specifies now exists, so the only unknown
    // type left is one nothing ever specified. It fails as the document is
    // read, because there is no record behind the tag to check.
    let stderr = rejected(
        "[[actions]]\n\
         type = \"rsync\"\n\
         source = \"a\"\n",
    );
    assert!(
        stderr.contains("rsync"),
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
fn a_path_within_a_remote_may_not_start_the_reference_over() {
    // `@` introduces a remote and nothing else, so a second one cannot open a
    // path inside the first. Only the leading character is reserved;
    // `files/@work/zshrc` is an ordinary path, which is a rule about spelling
    // and is tested where the spelling is.
    let stderr = rejected(&format!(
        "[remotes.core]\ntype = \"git\"\nurl = \"https://git.example/core.git\"\n\n{}",
        one_symlink("@core/@work/zshrc", "~/.zshrc")
    ));
    for expected in ["@core/@work/zshrc", "starts with `@`"] {
        assert!(stderr.contains(expected), "no `{expected}` in:\n{stderr}");
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
    // one that never looks at it is unaffected by a broken one. `vars set`
    // writes machine-local state and does its whole job here.
    let tree = Tree::new();
    fs::write(tree.manifest(), "[[actions]\n").expect("a malformed manifest");

    tree.batfiles()
        .args(["vars", "set", "profile", "work"])
        .assert()
        .success();
}
