//! What `--dry-run` promises, in the two halves it promises it: the run does
//! none of the work, and it says the same things the real run then says.
//!
//! Gated with `linking` because the fixture it drives declares symlink actions.

use std::fs;

use crate::support::*;

/// A home already holding some of what the fixture installs: a seed's
/// destination and a directory an action would otherwise make.
///
/// Both dry-run tests start here rather than from an empty home, so the
/// comparison has content to be wrong about and the run reports a `kept`
/// and an `unchanged` as well as the rest.
fn with_existing_content(tree: &Tree) {
    fs::create_dir_all(tree.home(".config/git")).expect("a config directory");
    fs::write(tree.home(".config/git/local"), "[user]\n\tname = me\n").expect("a seeded file");
    fs::create_dir_all(tree.home(".local/bin")).expect("a bin directory");
}

#[test]
fn a_dry_run_writes_nothing_at_all_into_the_home() {
    let tree = Tree::fixture("leaf");
    with_existing_content(&tree);
    let before = snapshot(&tree.path("home"));

    tree.batfiles()
        .args(["sync", "--dry-run"])
        .assert()
        .success();

    // Byte for byte: no destination, no parent directory made on the way,
    // and no `.batfiles-incomplete` staging node, which is the one 2.1's
    // prohibition exists for.
    assert_eq!(snapshot(&tree.path("home")), before);
}

/// How many action records the `leaf` fixture declares.
///
/// The parity assertion covers all of them, which is sound only because
/// their destinations are distinct. An action whose output is another's
/// input diverges in substance rather than tense, so it has to be excluded
/// from the comparison rather than tolerated by it; this count is what
/// makes adding one say so.
const PARITY_ACTIONS: usize = 11;

#[test]
fn a_dry_runs_lines_are_the_real_runs_lines_in_another_tense() {
    let tree = Tree::fixture("leaf");
    with_existing_content(&tree);
    let manifest = fs::read_to_string(tree.manifest()).expect("the fixture manifest");
    assert_eq!(
        manifest.matches("[[actions]]").count(),
        PARITY_ACTIONS,
        "the fixture's actions changed; see `PARITY_ACTIONS`"
    );

    // `-v` so the lines that only appear at detail — `unchanged`, and the
    // `kept` a seed reports — are compared too.
    let dry = stderr_of(
        &tree
            .batfiles()
            .args(["sync", "--dry-run", "-v"])
            .assert()
            .success(),
    );
    // The same tree, which the dry run has left exactly as it found it.
    let real = stderr_of(&tree.batfiles().args(["sync", "-v"]).assert().success());

    let said: Vec<String> = dry.lines().map(in_past_tense).collect();
    assert_eq!(said, real.lines().collect::<Vec<&str>>());
    // Guards the comparison itself: two runs that both said nothing
    // prospective would match line for line and prove nothing.
    for expected in ["would link", "would copy", "would create", "would keep"] {
        assert!(
            dry.contains(expected),
            "the dry run never said `{expected}`:\n{dry}"
        );
    }
}

/// One reported line as the real run would have written it.
///
/// The inverse of `Verb::say`, and deliberately spelled out here rather
/// than imported: a test that shared the table with the code under test
/// would agree with it however wrong both were.
fn in_past_tense(line: &str) -> String {
    for (prospective, past) in [
        ("would relink ", "relinked "),
        ("would link ", "linked "),
        ("would copy ", "copied "),
        ("would create ", "created "),
        ("would remove ", "removed "),
        ("would keep ", "kept "),
    ] {
        if let Some(rest) = line.strip_prefix(prospective) {
            return format!("{past}{rest}");
        }
    }
    line.to_owned()
}
