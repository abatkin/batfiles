//! CLI tests for included clone-list source resolution, variable scope, and preparation before
//! action writes.

use crate::support::*;

/// A remote holding `manifest` as its `batfiles.toml` and `list` as
/// `plugins.txt`.
fn remote(manifest: &str, list: &str) -> BareRepo {
    let origin = BareRepo::new();
    origin.publish("plugins.txt", list, "a plugin list");
    origin.publish("batfiles.toml", manifest, "the remote's actions");
    origin
}

/// A leaf that declares `corporate`, then writes `before` (tables, such as
/// `[vars]`, and actions), includes the remote as `corp` with `fields` on the
/// inclusion, and writes the actions in `after`.
fn leaf(origin: &BareRepo, before: &str, fields: &str, after: &str) -> Tree {
    let tree = Tree::new();
    tree.write_manifest(&format!(
        r#"[remotes.corporate]
type = "git"
url = "{{origin}}"

{before}
[[actions]]
type = "include-remote"
id = "corp"
remote = "corporate"
{fields}
{after}"#
    ));
    tree.point_at_origin(origin);
    tree
}

/// A `create-dir` of `~/.<id>`, as a manifest writes one.
fn create_dir(id: &str) -> String {
    format!("[[actions]]\ntype = \"create-dir\"\nid = \"{id}\"\ndest = \"~/.{id}\"\n")
}

/// The `plugins` list a remote declares, with `fields` added to the record.
fn plugins(fields: &str) -> String {
    format!(
        "[[actions]]\ntype = \"git-clone-list\"\nid = \"plugins\"\nsource = \"plugins.txt\"\n\
         dest-dir = \"~/.plugins\"\n{fields}"
    )
}

#[test]
fn an_inclusions_actions_and_list_run_where_the_inclusion_is_written() {
    let upstream = BareRepo::new();
    let plugin = upstream.another("zsh-z");
    let origin = remote(
        &format!(
            "{}{}{}",
            create_dir("corp-first"),
            plugins(""),
            create_dir("corp-last")
        ),
        &format!(
            "{plugin}\n{plugin} dest-name=only-at-work when=\"work\"\n",
            plugin = display(&plugin)
        ),
    );
    let tree = leaf(
        &origin,
        &format!("[vars]\nwork = \"false\"\n\n{}", create_dir("first")),
        "",
        &create_dir("last"),
    );

    let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();
    let stderr = stderr_of(&assertion);

    let clone = display(&tree.home(".plugins/zsh-z"));
    let excluded = format!(
        "not cloning {} (plugins.txt line 2): when \"work\" is false",
        display(&plugin)
    );
    let order = [
        "create-dir first",
        "include-remote corp",
        "create-dir corp.corp-first",
        "git-clone-list corp.plugins",
        clone.as_str(),
        excluded.as_str(),
        "create-dir corp.corp-last",
        "create-dir last",
    ];
    for line in order {
        assert_eq!(
            stderr.matches(line).count(),
            1,
            "`{line}` was not reported exactly once:\n{stderr}"
        );
    }
    let at = |line: &str| stderr.find(line).expect("reported above");
    for pair in order.windows(2) {
        assert!(
            at(pair[0]) < at(pair[1]),
            "`{}` was not reported before `{}`:\n{stderr}",
            pair[0],
            pair[1]
        );
    }
    for installed in [
        ".first",
        ".corp-first",
        ".plugins/zsh-z",
        ".corp-last",
        ".last",
    ] {
        assert!(
            tree.home(installed).exists(),
            "`{installed}` was not installed"
        );
    }
    assert!(!tree.home(".plugins/only-at-work").exists());
}

#[test]
fn a_malformed_list_in_a_late_inclusion_stops_the_run_before_the_first_action() {
    let origin = remote(
        &plugins(""),
        "https://e.example/a.git\nhttps://e.example/b.git colour=blue\n",
    );
    let tree = leaf(&origin, &create_dir("first"), "", "");

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);
    let stderr = stderr_of(&assertion);

    assert!(
        !tree.home(".first").exists(),
        "the leaf's action ran before the included list was checked"
    );
    assert!(stderr.contains("plugins.txt"), "{stderr}");
    assert!(stderr.contains("line 2"), "{stderr}");
    assert!(stderr.contains("unknown key `colour`"), "{stderr}");
}

#[test]
fn applying_an_included_list_waives_its_condition_and_not_its_entries() {
    let upstream = BareRepo::new();
    let origin = remote(
        &plugins("when = \"work\"\n"),
        &format!(
            "{origin} dest-name=everywhere\n{origin} dest-name=only-at-work when=\"work\"\n",
            origin = display(&upstream.origin())
        ),
    );
    let tree = leaf(
        &origin,
        "[vars]\nwork = \"true\"\n",
        "vars = { work = \"false\" }\n",
        "",
    );

    tree.batfiles().arg("sync").assert().success();
    assert!(!tree.home(".plugins").exists());

    let assertion = tree
        .batfiles()
        .args(["apply-action", "--id", "corp.plugins", "-v"])
        .assert()
        .success();
    let stderr = stderr_of(&assertion);

    assert_eq!(entries(&tree.home(".plugins")), ["everywhere"], "{stderr}");
    assert!(
        stderr.contains(&format!(
            "not cloning {} (plugins.txt line 2): when \"work\" is false",
            display(&upstream.origin())
        )),
        "the entry's own gate should still close it:\n{stderr}"
    );
}

#[test]
fn an_included_list_with_nothing_to_clone_still_makes_its_directory() {
    let upstream = BareRepo::new();
    for list in [
        String::new(),
        format!("{} when=\"work\"\n", display(&upstream.origin())),
    ] {
        let origin = remote(&plugins(""), &list);
        let tree = leaf(&origin, "[vars]\nwork = \"false\"\n", "", "");
        tree.batfiles().arg("sync").assert().success();
        std::fs::remove_dir(tree.home(".plugins")).expect("the directory sync made");

        let assertion = tree
            .batfiles()
            .args(["apply-action", "--id", "corp.plugins"])
            .assert()
            .success();
        let stderr = stderr_of(&assertion);

        assert!(tree.home(".plugins").is_dir(), "{list:?}: {stderr}");
        assert!(
            !stderr.contains("nothing to apply"),
            "{list:?}: the list did not count as applied:\n{stderr}"
        );
    }
}

/// A remote whose `plugins` list names `zsh-z` and `p10k`, both with IDs, and
/// a leaf including it as `corp` with `fields`, synchronized once with
/// `plugins.p10k` disabled by its qualified address. Returns the repositories
/// the list names, the remote, and the leaf.
fn addressed(fields: &str) -> (BareRepo, BareRepo, Tree) {
    let upstream = BareRepo::new();
    let list = format!(
        "{} id=zsh-z\n{} id=p10k\n",
        display(&upstream.another("zsh-z")),
        display(&upstream.another("p10k")),
    );
    let origin = remote(&plugins(""), &list);
    let tree = leaf(&origin, "", fields, "");
    tree.batfiles()
        .args(["disable-action", "corp.plugins.p10k"])
        .assert()
        .success();
    tree.batfiles().arg("sync").assert().success();
    (upstream, origin, tree)
}

#[test]
fn an_entry_in_an_included_list_answers_to_its_full_address() {
    let (_upstream, _origin, tree) = addressed("");

    assert!(tree.home(".plugins/zsh-z").is_dir());
    assert!(
        !tree.home(".plugins/p10k").exists(),
        "the qualified disable did not reach the entry"
    );

    tree.batfiles()
        .args(["apply-action", "--id", "corp.plugins.p10k"])
        .assert()
        .success();
    assert!(tree.home(".plugins/p10k").is_dir());
}

#[test]
fn naming_an_entry_inside_an_excluded_inclusion_names_the_inclusion() {
    let (_upstream, _origin, tree) = addressed("");
    tree.batfiles()
        .args(["disable-action", "corp"])
        .assert()
        .success();

    let assertion = tree
        .batfiles()
        .args(["apply-action", "--id", "corp.plugins.p10k"])
        .assert()
        .failure();

    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains(
            "action `corp.plugins.p10k` would come from include-remote `corp`, which is \
             excluded: action `corp` is disabled"
        ),
        "{stderr}"
    );
    assert!(!tree.home(".plugins/p10k").exists());
}

#[test]
fn naming_an_entry_in_a_list_the_filters_left_out_names_the_list() {
    let (_upstream, _origin, tree) = addressed("exclude-actions = \"plugins\"");

    let assertion = tree
        .batfiles()
        .args(["apply-action", "--id", "corp.plugins.p10k"])
        .assert()
        .failure();

    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains(
            "entry `corp.plugins.p10k` would come from git-clone-list `corp.plugins`, which \
             is excluded: not selected by include-remote `corp`"
        ),
        "{stderr}"
    );
    assert!(!tree.home(".plugins").exists());
}
