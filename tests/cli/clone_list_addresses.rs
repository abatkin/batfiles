//! CLI tests for addressing single clone-list entries as `<list>.<entry>`: disables, run-only
//! skips, and `apply-action`, using local bare repositories.

use crate::support::*;

/// The list's entries in line order, by the directory each clones into. `fzf` has no `id`,
/// and `work-tools` is gated on `work`, which the manifest leaves false.
const ENTRIES: [&str; 4] = ["zsh-z", "p10k", "fzf", "work-tools"];

/// A leaf whose one action, `plugins` in group `shell`, clones `plugins.txt`
/// into `~/.plugins`, with `fields` added to the record and `before` written
/// ahead of the actions. Returns the repositories it names, by entry.
fn listed(before: &str, fields: &str) -> (BareRepo, Tree) {
    let origin = BareRepo::new();
    let repository = |name| display(&origin.another(name));
    let list = format!(
        "{} id=zsh-z\n{} id=p10k\n{}\n{} id=work-tools when=\"work\"\n",
        repository("zsh-z"),
        repository("p10k"),
        repository("fzf"),
        repository("work-tools"),
    );
    let tree = Tree::new();
    tree.write_manifest(&format!(
        r#"{before}
[vars]
work = "false"

[[actions]]
type = "git-clone-list"
id = "plugins"
group = "shell"
source = "plugins.txt"
dest-dir = "~/.plugins"
{fields}
"#
    ));
    tree.repo_file("plugins.txt", &list);
    (origin, tree)
}

/// Which entries have been cloned, in list order.
fn cloned(tree: &Tree) -> Vec<&'static str> {
    ENTRIES
        .into_iter()
        .filter(|name| tree.home(&format!(".plugins/{name}")).is_dir())
        .collect()
}

/// The bare repository a `listed` entry clones, as its list writes it.
fn repository(origin: &BareRepo, name: &str) -> String {
    written(&origin.origin().with_file_name(format!("{name}.git")))
}

#[test]
fn a_disabled_entry_is_left_out_until_it_is_enabled() {
    let (origin, tree) = listed("", "");
    tree.batfiles()
        .args(["disable-action", "plugins.p10k"])
        .assert()
        .success();

    let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();

    assert_eq!(cloned(&tree), ["zsh-z", "fzf"]);
    let stderr = stderr_of(&assertion);
    let expected = format!(
        "not cloning {} (id=p10k, plugins.txt line 2): action `plugins.p10k` is disabled",
        repository(&origin, "p10k")
    );
    assert!(stderr.contains(&expected), "{stderr}");

    tree.batfiles()
        .args(["enable-action", "plugins.p10k"])
        .assert()
        .success();
    tree.batfiles().arg("sync").assert().success();
    assert_eq!(cloned(&tree), ["zsh-z", "p10k", "fzf"]);
}

#[test]
fn a_run_only_skip_leaves_out_one_entry_for_one_run() {
    let (_origin, tree) = listed("", "");

    let assertion = tree
        .batfiles()
        .args(["sync", "-v", "--skip-action", "plugins.p10k"])
        .env("BATFILES_SKIP_ACTIONS", "plugins.zsh-z")
        .assert()
        .success();

    assert_eq!(cloned(&tree), ["fzf"]);
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("`plugins.p10k` from --skip-action"),
        "{stderr}"
    );
    assert!(
        stderr.contains("`plugins.zsh-z` from BATFILES_SKIP_ACTIONS"),
        "{stderr}"
    );
    assert!(!stderr.contains("matched no"), "{stderr}");
}

#[test]
fn an_entry_left_out_by_a_skip_is_not_asked_its_condition() {
    let (_origin, tree) = listed("", "");
    let list = tree.path("repo/plugins.txt");
    let text = std::fs::read_to_string(&list).expect("the list");
    tree.repo_file(
        "plugins.txt",
        &text.replace("when=\"work\"", "when=\"nowhere\""),
    );

    let skipped = tree
        .batfiles()
        .args(["sync", "--skip-action", "plugins.work-tools"])
        .assert()
        .success();
    assert!(
        !stderr_of(&skipped).contains("cannot be evaluated"),
        "{}",
        stderr_of(&skipped)
    );

    let asked = tree.batfiles().arg("sync").assert().success();
    assert!(
        stderr_of(&asked).contains("when \"nowhere\" cannot be evaluated"),
        "{}",
        stderr_of(&asked)
    );
}

#[test]
fn a_skip_naming_no_entry_of_a_list_that_was_read_warns() {
    let (_origin, tree) = listed("", "");

    let assertion = tree
        .batfiles()
        .args(["sync", "--skip-action", "plugins.nowhere"])
        .args(["--skip-action", "plugins.fzf"])
        .assert()
        .success();

    let stderr = stderr_of(&assertion);
    for name in ["plugins.nowhere", "plugins.fzf"] {
        assert!(
            stderr.contains(&format!("--skip-action `{name}` matched no action")),
            "{stderr}"
        );
    }
    // An entry without an `id` answers to nothing, so the skip left it in.
    assert_eq!(cloned(&tree), ["zsh-z", "p10k", "fzf"]);
}

#[test]
fn a_skip_inside_a_list_that_was_not_read_is_not_reported() {
    let (_origin, tree) = listed("", "");

    let assertion = tree
        .batfiles()
        .args(["sync", "--skip-action", "plugins"])
        .args(["--skip-action", "plugins.nowhere"])
        .assert()
        .success();

    assert!(
        !stderr_of(&assertion).contains("matched no"),
        "{}",
        stderr_of(&assertion)
    );
    assert!(!tree.home(".plugins").exists());
}

#[test]
fn naming_an_entry_applies_it_alone_whatever_excludes_it() {
    let (_origin, tree) = listed("", "");
    tree.batfiles()
        .args(["disable-action", "plugins.work-tools"])
        .assert()
        .success();

    tree.batfiles()
        .args(["apply-action", "--id", "plugins.work-tools"])
        .assert()
        .success();

    // Both its disable and its `when` are waived; nothing else in the list is requested.
    assert_eq!(cloned(&tree), ["work-tools"]);
}

#[test]
fn naming_an_entry_does_not_waive_its_lists_exclusions() {
    struct Case {
        fields: &'static str,
        disable: Option<&'static str>,
        reason: &'static str,
    }
    let cases = [
        Case {
            fields: "",
            disable: Some("plugins"),
            reason: "action `plugins` is disabled",
        },
        Case {
            fields: "",
            disable: Some("shell"),
            reason: "group `shell` is disabled",
        },
        Case {
            fields: "when = \"work\"",
            disable: None,
            reason: "when \"work\" is false",
        },
    ];
    for case in cases {
        let (_origin, tree) = listed("", case.fields);
        if let Some(name) = case.disable {
            let command = if name == "shell" {
                "disable-group"
            } else {
                "disable-action"
            };
            tree.batfiles().args([command, name]).assert().success();
        }

        let assertion = tree
            .batfiles()
            .args(["apply-action", "--id", "plugins.p10k"])
            .assert()
            .failure();

        let expected = format!(
            "entry `plugins.p10k` would come from git-clone-list `plugins`, which is \
             excluded: {}",
            case.reason
        );
        assert!(
            stderr_of(&assertion).contains(&expected),
            "{}",
            stderr_of(&assertion)
        );
        assert!(!tree.home(".plugins").exists(), "{}", case.reason);
    }
}

#[test]
fn naming_an_entry_the_list_does_not_declare_fails_having_written_nothing() {
    let (_origin, tree) = listed("", "");

    for missing in ["plugins.nowhere", "plugins.fzf"] {
        let assertion = tree
            .batfiles()
            .args(["apply-action", "--id", missing])
            .assert()
            .failure();

        let stderr = stderr_of(&assertion);
        assert!(
            stderr.contains(&format!("has the id `{missing}`")),
            "{stderr}"
        );
        assert!(!tree.home(".plugins").exists(), "{stderr}");
    }
}

#[test]
fn naming_the_list_still_honors_its_entries_disables() {
    let (_origin, tree) = listed("", "");
    tree.batfiles()
        .args(["disable-action", "plugins.p10k", "plugins"])
        .assert()
        .success();

    tree.batfiles()
        .args(["apply-action", "--id", "plugins"])
        .assert()
        .success();

    assert_eq!(cloned(&tree), ["zsh-z", "fzf"]);
}

#[test]
fn a_group_run_honors_a_skip_naming_an_entry() {
    let (_origin, tree) = listed("", "");

    tree.batfiles()
        .args(["apply-group", "--group", "shell"])
        .args(["--skip-action", "plugins.zsh-z"])
        .assert()
        .success();

    assert_eq!(cloned(&tree), ["p10k", "fzf"]);
}

#[test]
fn a_bootstrap_adopts_a_default_disabled_entry() {
    let (_origin, tree) = listed("[[default-disabled.actions]]\nid = \"plugins.p10k\"\n", "");

    tree.batfiles()
        .args(["sync", "--bootstrap"])
        .assert()
        .success();

    assert_eq!(cloned(&tree), ["zsh-z", "fzf"]);
    assert!(
        tree.disabled_document().contains("plugins.p10k"),
        "{}",
        tree.disabled_document()
    );
}

#[test]
fn naming_an_entry_under_dry_run_writes_nothing() {
    let (_origin, tree) = listed("", "");

    tree.batfiles()
        .args(["apply-action", "--id", "plugins.p10k", "--dry-run"])
        .assert()
        .success();

    assert!(!tree.home(".plugins").exists());
}
