//! CLI tests for action IDs and groups in progress headings.

use crate::support::*;

/// Declare actions with both ID and group, group only, ID only, and neither.
fn four_shapes(tree: &Tree) {
    tree.repo_file("seed/profile", "# profile\n");
    tree.write_manifest(
        r#"[[actions]]
type = "create-dir"
id = "zsh-cache"
group = "shell"
dest = "~/.cache/zsh"

[[actions]]
type = "create-dir"
group = "shell"
dest = "~/.cache/other"

[[actions]]
type = "copy"
id = "profile"
source = "seed/profile"
dest = "~/.profile"

[[actions]]
type = "copy-dir"
source-dir = "seed"
dest-dir = "~/.config/zsh"
"#,
    );
}

/// Expected headings for [`four_shapes`], in declaration order.
const FOUR_HEADINGS: [&str; 4] = [
    "create-dir zsh-cache (group shell)",
    "create-dir action 2 (group shell)",
    "copy profile",
    "copy-dir action 4",
];

#[test]
fn a_verbose_run_names_each_record_before_reporting_what_it_did() {
    let tree = Tree::new();
    four_shapes(&tree);

    let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();
    let stderr = stderr_of(&assertion);

    let mut searched_from = 0;
    for heading in FOUR_HEADINGS {
        let found = stderr[searched_from..]
            .find(heading)
            .unwrap_or_else(|| panic!("no `{heading}` after the one before it in:\n{stderr}"));
        searched_from += found + heading.len();
    }
}

#[test]
fn the_record_headings_are_detail_and_not_ordinary_output() {
    let tree = Tree::new();
    four_shapes(&tree);

    let assertion = tree.batfiles().arg("sync").assert().success();
    let stderr = stderr_of(&assertion);
    for heading in FOUR_HEADINGS {
        assert!(!stderr.contains(heading), "`{heading}` in:\n{stderr}");
    }
    assert!(
        stderr.contains(&display(&tree.home(".profile"))),
        "the run reported none of its work:\n{stderr}"
    );
}

/// Actions with the same group belong to it regardless of declaration position.
#[test]
fn a_group_is_made_by_the_actions_that_name_it_wherever_they_sit() {
    let tree = Tree::new();
    tree.write_manifest(
        r#"[[actions]]
type = "create-dir"
id = "one"
group = "shared"
dest = "~/one"

[[actions]]
type = "create-dir"
id = "two"
group = "alone"
dest = "~/two"

[[actions]]
type = "create-dir"
id = "three"
group = "shared"
dest = "~/three"
"#,
    );

    let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();
    let stderr = stderr_of(&assertion);
    for expected in [
        "create-dir one (group shared)",
        "create-dir two (group alone)",
        "create-dir three (group shared)",
    ] {
        assert!(stderr.contains(expected), "no `{expected}` in:\n{stderr}");
    }
    assert_eq!(entries(&tree.path("home")), ["one", "three", "two"]);
}
