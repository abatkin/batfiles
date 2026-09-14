//! `[remotes]`: the repositories a manifest names, and the tree `sync` brings
//! them onto the machine in.
//!
//! The rules about what a record may say are settled as the manifest is read,
//! so those cases execute nothing. The rest clone from a local bare repository
//! (`guidance.md`, "Test environments"), as the `git-clone` tests do.

use std::fs;

use crate::support::*;

/// A manifest declaring one Git remote at `core`, plus whatever else the case
/// needs after it.
fn declaring(origin: &BareRepo, rest: &str) -> String {
    format!(
        "[remotes.core]\ntype = \"git\"\nurl = \"{}\"\n\n{rest}",
        display(&origin.origin())
    )
}

/// What the materialization of `core` holds at `name`.
fn materialized(tree: &Tree, name: &str) -> String {
    let path = tree.path("repo").join("remotes/core").join(name);
    fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", display(&path)))
}

#[test]
fn a_declared_remote_is_cloned_into_the_repository() {
    // Declaring one is what materializes it: nothing here names `core`, and
    // nothing can until an action can reach its content.
    let origin = BareRepo::new();
    let tree = Tree::new();
    tree.write_manifest(&declaring(&origin, &one_create_dir("~/.cache/zsh")));

    let assertion = tree.batfiles().arg("sync").assert().success();

    assert_eq!(materialized(&tree, "README.md"), "a plugin\n");
    assert!(
        tree.path("repo").join("remotes/core/.git").is_dir(),
        "the materialization is not a clone"
    );
    assert!(
        stderr_of(&assertion).contains(&format!(
            "cloned {} from {}",
            display(&tree.path("repo").join("remotes/core")),
            display(&origin.origin())
        )),
        "the clone was not reported:\n{}",
        stderr_of(&assertion)
    );
    // The actions still run, after it.
    assert!(tree.home(".cache/zsh").is_dir(), "the action did not run");
}

#[test]
fn a_later_sync_updates_the_materialization() {
    let origin = BareRepo::new();
    let tree = Tree::new();
    tree.write_manifest(&declaring(&origin, ""));
    tree.batfiles().arg("sync").assert().success();

    origin.publish("PLUGINS.md", "one more\n", "second");
    let assertion = tree.batfiles().arg("sync").assert().success();

    assert_eq!(materialized(&tree, "PLUGINS.md"), "one more\n");
    assert!(
        stderr_of(&assertion).contains(&format!(
            "updated {}",
            display(&tree.path("repo").join("remotes/core"))
        )),
        "the update was not reported:\n{}",
        stderr_of(&assertion)
    );
}

#[test]
fn a_declared_ref_is_what_the_materialization_follows() {
    // `ref` is `git-clone`'s field under another name, so it does what a cloned
    // action's does: the checkout stands on what the ref names, not on `main`.
    let origin = BareRepo::new();
    origin.publish_on("topic", "TOPIC.md", "on the branch\n", "topic work");
    let tree = Tree::new();
    tree.write_manifest(&format!(
        "[remotes.core]\ntype = \"git\"\nurl = \"{}\"\nref = \"topic\"\n",
        display(&origin.origin())
    ));

    tree.batfiles().arg("sync").assert().success();

    assert_eq!(materialized(&tree, "TOPIC.md"), "on the branch\n");
}

#[test]
fn a_materialization_is_named_and_reported_before_the_first_action() {
    // The heading an action gets at `-v`, for the work that comes before any of
    // them: the remote says which record its lines belong to, and the whole of
    // it precedes the first action's heading.
    let origin = BareRepo::new();
    let tree = Tree::new();
    tree.write_manifest(&declaring(&origin, &one_create_dir("~/.cache/zsh")));

    let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();
    let stderr = stderr_of(&assertion);

    let heading = stderr.find("remote core").expect("the remote heading");
    let cloned = stderr.find("cloned ").expect("the clone line");
    let action = stderr
        .find("create-dir action 1")
        .expect("the action heading");
    assert!(
        heading < cloned,
        "the heading came after its line:\n{stderr}"
    );
    assert!(
        cloned < action,
        "the remote was materialized after the actions:\n{stderr}"
    );
}

#[test]
fn a_remote_that_cannot_be_materialized_stops_the_run() {
    // A materialization failure is an action failure: the run stops, and the
    // records after it do not run.
    let tree = Tree::new();
    tree.write_manifest(&format!(
        "[remotes.core]\ntype = \"git\"\nurl = \"{}\"\n\n{}",
        display(&tree.path("nowhere.git")),
        one_create_dir("~/.cache/zsh")
    ));

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);

    assert!(
        stderr_of(&assertion).contains("git clone failed"),
        "{}",
        stderr_of(&assertion)
    );
    assert!(
        !tree.home(".cache/zsh").exists(),
        "the run continued past the failed remote"
    );
}

#[test]
fn an_apply_command_materializes_nothing() {
    // `sync` is what brings the declared remotes up to date. Applying one
    // record is aimed at that record, and putting the network in front of it
    // would make the narrow command the slow one.
    let origin = BareRepo::new();
    let tree = Tree::new();
    tree.write_manifest(&declaring(
        &origin,
        "[[actions]]\ntype = \"create-dir\"\nid = \"cache\"\ndest = \"~/.cache/zsh\"\n",
    ));

    tree.batfiles()
        .args(["apply-action", "--id", "cache"])
        .assert()
        .success();

    assert!(tree.home(".cache/zsh").is_dir(), "the action did not run");
    assert_eq!(
        entries(&tree.path("repo")),
        ["batfiles.toml"],
        "an apply command materialized a remote"
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
fn two_remote_keys_may_not_differ_only_in_case() {
    // Two map keys and one directory, wherever the filesystem folds case: the
    // second remote would find the first one's clone and update that, since a
    // clone keeps the remote it was made with. Refused here rather than on
    // macOS, because the manifest is the same repository on every machine.
    let stderr = rejected(
        "[remotes.core]\n\
         type = \"git\"\n\
         url = \"https://e.example/a.git\"\n\n\
         [remotes.Core]\n\
         type = \"git\"\n\
         url = \"https://e.example/b.git\"\n",
    );
    assert!(
        stderr.contains("`core`") && stderr.contains("`Core`"),
        "{stderr}"
    );
    assert!(stderr.contains("differ only in case"), "{stderr}");
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

// Installing from a materialization. Each of the five fields that reads a
// repository path may name a remote, so what these cover is the resolution
// rather than the action: an action installing from `@core/...` behaves
// exactly as it does installing from the leaf repository, and the tests of
// that behavior are with the action.

/// A remote holding what an action can install from: files, a directory of
/// them, and a list of repositories to clone.
fn stocked() -> BareRepo {
    let origin = BareRepo::new();
    origin.publish("files/zshrc", "# zsh, from core\n", "add zshrc");
    origin.publish("seed/gitconfig", "[user]\n", "add a seed");
    origin
}

#[cfg(unix)]
#[test]
fn a_symlink_may_point_into_a_materialization() {
    let origin = stocked();
    let tree = Tree::new();
    tree.write_manifest(&declaring(
        &origin,
        &one_symlink("@core/files/zshrc", "~/.zshrc"),
    ));

    tree.batfiles().arg("sync").assert().success();

    // The link points at the materialization, which is the only place the
    // content is: nothing copies a remote's file into the leaf repository.
    assert_eq!(
        link_target(&tree.home(".zshrc")),
        tree.path("repo").join("remotes/core/files/zshrc")
    );
    assert_eq!(
        fs::read_to_string(tree.home(".zshrc")).expect("the link should resolve"),
        "# zsh, from core\n"
    );
}

#[cfg(unix)]
#[test]
fn both_spellings_of_a_reference_reach_the_same_file() {
    // The structured form is the shorthand written out, so a manifest may use
    // either and get the same link.
    let origin = stocked();
    let tree = Tree::new();
    tree.write_manifest(&declaring(
        &origin,
        "[[actions]]\n\
         type = \"symlink\"\n\
         source = { remote = \"core\", path = \"files/zshrc\" }\n\
         dest = \"~/.zshrc\"\n",
    ));

    tree.batfiles().arg("sync").assert().success();

    assert_eq!(
        link_target(&tree.home(".zshrc")),
        tree.path("repo").join("remotes/core/files/zshrc")
    );
}

#[test]
fn a_copy_seeds_from_a_materialization() {
    let origin = stocked();
    let tree = Tree::new();
    tree.write_manifest(&declaring(
        &origin,
        &one_copy("@core/seed/gitconfig", "~/.gitconfig"),
    ));

    tree.batfiles().arg("sync").assert().success();

    // A detached copy, as a copy from the leaf repository is: the file is the
    // seed's contents and nothing points back at the remote.
    assert_eq!(
        fs::read_to_string(tree.home(".gitconfig")).expect("the seed should be installed"),
        "[user]\n"
    );
    assert!(!tree.home(".gitconfig").is_symlink());
}

#[test]
fn a_directory_action_reads_its_children_from_a_materialization() {
    let origin = stocked();
    origin.publish("seed/npmrc", "loglevel=warn\n", "add another seed");
    let tree = Tree::new();
    tree.write_manifest(&declaring(
        &origin,
        &one_copy_dir("@core/seed", "~/.config/seeds", false),
    ));

    tree.batfiles().arg("sync").assert().success();

    assert_eq!(
        entries(&tree.home(".config/seeds")),
        ["gitconfig", "npmrc"],
        "the children came from somewhere other than the remote"
    );
}

#[test]
fn a_clone_list_may_live_in_a_materialization() {
    // The list is repository content like any other, so it can be held by a
    // remote. What the list names is a repository to clone, which was never a
    // repository path and is unchanged by where the list itself came from.
    let origin = stocked();
    let plugin = origin.another("plugin");
    origin.publish(
        "plugins.txt",
        &format!("{}\n", display(&plugin)),
        "add a plugin list",
    );
    let tree = Tree::new();
    tree.write_manifest(&declaring(
        &origin,
        "[[actions]]\n\
         type = \"git-clone-list\"\n\
         source = \"@core/plugins.txt\"\n\
         dest-dir = \"~/.plugins\"\n",
    ));

    tree.batfiles().arg("sync").assert().success();

    assert_eq!(
        fs::read_to_string(tree.home(".plugins/plugin/README.md")).expect("a cloned plugin"),
        "a plugin\n"
    );
}

#[test]
fn a_list_held_by_a_remote_is_named_the_way_the_manifest_wrote_it() {
    // A warning about an entry names the list it came from. That is the path as
    // written, including the remote, rather than the materialization it was
    // read out of: the reader's copy of the list is the one in the remote.
    let origin = stocked();
    let tree = Tree::new();
    origin.publish(
        "plugins.txt",
        &format!("{}\n", display(&tree.path("nowhere.git"))),
        "add a list naming nothing",
    );
    tree.write_manifest(&declaring(
        &origin,
        "[[actions]]\n\
         type = \"git-clone-list\"\n\
         source = \"@core/plugins.txt\"\n\
         dest-dir = \"~/.plugins\"\n",
    ));

    let assertion = tree.batfiles().arg("sync").assert().success();

    assert!(
        stderr_of(&assertion).contains("@core/plugins.txt line 1"),
        "the list was not named as written:\n{}",
        stderr_of(&assertion)
    );
}

#[test]
fn a_source_naming_an_undeclared_remote_is_refused_when_the_manifest_is_read() {
    // The one source rule that spans two records, so it is the manifest that
    // answers it, before anything runs. This is also what a repository really
    // holding a directory called `@work` is told: `@` introduces a remote
    // wherever a repository path starts with it, and there is no escape.
    let stderr = rejected(&one_symlink("@work/zshrc", "~/.zshrc"));
    for expected in ["@work/zshrc", "does not declare", "[remotes.work]"] {
        assert!(stderr.contains(expected), "no `{expected}` in:\n{stderr}");
    }
}

#[test]
fn a_source_in_a_remote_this_machine_has_not_cloned_says_to_sync() {
    // An apply command materializes nothing, so it is the command that can
    // reach a declared remote that is not on the machine. The refusal names the
    // remote rather than reporting a missing file under a directory nobody
    // made.
    let origin = stocked();
    let tree = Tree::new();
    tree.write_manifest(&declaring(
        &origin,
        "[[actions]]\n\
         type = \"copy\"\n\
         id = \"gitconfig\"\n\
         source = \"@core/seed/gitconfig\"\n\
         dest = \"~/.gitconfig\"\n",
    ));

    let assertion = tree
        .batfiles()
        .args(["apply-action", "--id", "gitconfig"])
        .assert()
        .failure()
        .code(1);

    for expected in ["remote `core` is not materialized", "batfiles sync"] {
        assert!(
            stderr_of(&assertion).contains(expected),
            "no `{expected}` in:\n{}",
            stderr_of(&assertion)
        );
    }
    assert!(!tree.home(".gitconfig").exists(), "the action installed");
}
