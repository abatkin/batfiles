//! Groups, and the run that reports them.
//!
//! A group is nothing but the `group` field: no section declares one, and
//! nothing selects by one yet. What reads it today is the line a run prints at
//! `-v` before each action, naming the record about to act — which is also the
//! only place an action's own identity reaches the output at all.

use crate::support::*;

/// A manifest declaring the four shapes a record can name itself in: with an
/// `id` and a group, with a group alone, with an `id` alone, and with neither.
///
/// Every action here needs no symlink, so this runs on every platform.
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

/// The headings [`four_shapes`] produces, in order.
///
/// A record with an `id` is named by it; one without is named by its one-based
/// position in the list, which is how a load error names it too. The group
/// follows when there is one, and nothing follows when there is not.
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

    // In order, and each one ahead of the next: a heading that named the right
    // record in the wrong place would be no use to a reader scanning for the
    // action that produced a line.
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
    // Guards the assertion above: a run that reported nothing at all would
    // satisfy it without the headings being the reason.
    assert!(
        stderr.contains(&display(&tree.home(".profile"))),
        "the run reported none of its work:\n{stderr}"
    );
}

/// A group is made by the actions that name it. Nothing declares one, so the
/// field is a property of the action rather than a reference to be resolved,
/// and membership implies nothing about where in the list a record sits.
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
    // Membership is not contiguity: the two actions in `shared` are separated
    // by one in another group, and declaration order is untouched by either.
    assert_eq!(entries(&tree.path("home")), ["one", "three", "two"]);
}
