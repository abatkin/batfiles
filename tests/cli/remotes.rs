//! `[remotes]`: the repositories a manifest names. Declaring one is read and
//! checked as the manifest is read; nothing materializes one yet, so every case
//! here is settled before anything is executed and all of them run on every
//! platform.

use crate::support::*;

/// A complete Git remote, using every field the record accepts.
const COMPLETE: &str = "[remotes.core]\n\
     type = \"git\"\n\
     url = \"https://e.example/core.git\"\n\
     ref = \"main\"\n\
     when = \"work\"\n\n";

#[test]
fn a_declared_remote_is_read_and_changes_no_run() {
    // The `[default-disabled]` bargain, for the section that arrives before
    // anything reads it: a manifest declaring a remote installs exactly what
    // the same manifest installs without one, and materializes nothing.
    let tree = Tree::new();
    tree.write_manifest(&format!(
        "{COMPLETE}[vars]\nwork = \"true\"\n\n{}",
        one_create_dir("~/.cache/zsh")
    ));

    tree.batfiles().arg("sync").assert().success();

    assert!(tree.home(".cache/zsh").is_dir(), "the action did not run");
    assert_eq!(
        entries(&tree.path("repo")),
        ["batfiles.toml"],
        "something was materialized into the repository"
    );
}

#[test]
fn a_url_is_whatever_git_accepts_but_never_nothing() {
    // The remote is named by the key it was declared under, which unlike an
    // action it always has.
    let stderr = rejected("[remotes.core]\ntype = \"git\"\nurl = \"\"\n");
    assert!(
        stderr.contains("remote `core`"),
        "the remote was not named:\n{stderr}"
    );
    assert!(stderr.contains("url is empty"), "{stderr}");
}

#[test]
fn a_ref_written_with_nothing_in_it_is_refused() {
    let stderr =
        rejected("[remotes.core]\ntype = \"git\"\nurl = \"https://e.example/a.git\"\nref = \"\"\n");
    assert!(stderr.contains("remote `core`"), "{stderr}");
    assert!(stderr.contains("ref is empty"), "{stderr}");
}

#[test]
fn a_remote_writes_one_condition_or_none() {
    // The rule every record carrying a condition follows, reported the way it
    // is for an action and a bootstrap candidate.
    let stderr = rejected(
        "[remotes.core]\n\
         type = \"git\"\n\
         url = \"https://e.example/a.git\"\n\
         when = \"work\"\n\
         unless = \"gui\"\n",
    );
    assert!(stderr.contains("remote `core`"), "{stderr}");
    assert!(
        stderr.contains("writes both `when` and `unless`"),
        "{stderr}"
    );
}

#[test]
fn a_remotes_condition_is_parsed_where_the_manifest_is_read() {
    // Parsed with the document that holds it, like every other condition, and
    // evaluated nowhere until 6.5 gives it something to decide.
    let stderr = rejected(
        "[remotes.core]\ntype = \"git\"\nurl = \"https://e.example/a.git\"\nwhen = \"work &&\"\n",
    );
    assert!(stderr.contains("not a valid condition"), "{stderr}");
}

#[test]
fn a_remote_type_that_is_reserved_and_unbuilt_names_its_step() {
    // A reader of the future schema has reason to expect these two to work, so
    // each says which step builds it rather than that it is not a type.
    for (kind, document) in [
        (
            "file",
            "[remotes.pathogen]\ntype = \"file\"\nurl = \"https://e.example/pathogen.vim\"\n",
        ),
        (
            "archive",
            "[remotes.pathogen]\ntype = \"archive\"\nurl = \"https://e.example/fzf.tar.gz\"\n",
        ),
    ] {
        let stderr = rejected(document);
        assert!(stderr.contains("remote `pathogen`"), "{stderr}");
        assert!(
            stderr.contains(&format!("type `{kind}`")),
            "the type was not named:\n{stderr}"
        );
        assert!(stderr.contains("9.3"), "the step was not named:\n{stderr}");
    }
}

#[test]
fn a_git_remote_is_closed_over_the_fields_it_accepts() {
    for (document, unknown) in [
        // The spelling `docs/future/repoformat.md` used for the ref field.
        // Batfiles reads it under `git-clone`'s name, with `git-clone`'s rules.
        ("branch = \"main\"\n", "branch"),
        // Arrives with the dynamic variables it would permit, at 9.1.
        ("allow-dynamic-vars = true\n", "allow-dynamic-vars"),
        // A field belonging to a remote type that is not this one.
        ("archive-root = \"*\"\n", "archive-root"),
    ] {
        let stderr = rejected(&format!(
            "[remotes.core]\ntype = \"git\"\nurl = \"https://e.example/a.git\"\n{document}"
        ));
        assert!(
            stderr.contains(&format!("unknown field `{unknown}`")),
            "`{unknown}` was accepted:\n{stderr}"
        );
    }
}

#[test]
fn a_remote_key_is_an_id() {
    // The key is the remote's ID, so it follows the ID rule rather than TOML's
    // rule for a bare key -- a dot would compose an address, and a leading
    // underscore is a variable name's spelling rather than an ID's.
    for key in ["\"core.extra\"", "_hidden", "\"two words\""] {
        let stderr = rejected(&format!(
            "[remotes.{key}]\ntype = \"git\"\nurl = \"https://e.example/a.git\"\n"
        ));
        assert!(
            stderr.contains("is not a valid ID"),
            "`{key}` was accepted:\n{stderr}"
        );
    }
}

#[test]
fn a_remote_needs_a_type_to_be_one() {
    let stderr = rejected("[remotes.core]\nurl = \"https://e.example/a.git\"\n");
    assert!(stderr.contains("type"), "{stderr}");
}
