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
/// The parity assertion covers all but the two of them that contend for one
/// destination. An action whose output is another's input diverges in substance
/// rather than tense, so it has to be excluded from the comparison rather than
/// tolerated by it; this count is what makes adding one say so.
const PARITY_ACTIONS: usize = 13;

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

    let said: Vec<String> = lines_apart_from_the_contended_pair(&tree, &dry)
        .iter()
        .map(|line| in_past_tense(line))
        .collect();
    assert_eq!(said, lines_apart_from_the_contended_pair(&tree, &real));
    // Guards the comparison itself: two runs that both said nothing
    // prospective would match line for line and prove nothing.
    for expected in ["would link", "would copy", "would create", "would keep"] {
        assert!(
            dry.contains(expected),
            "the dry run never said `{expected}`:\n{dry}"
        );
    }
}

/// One run's reported lines, minus everything said about the destination the
/// fixture's two seeds contend for.
///
/// **Not a loosening of the parity assertion.** Parity is a promise about
/// actions with distinct destinations, and that pair is deliberately not one:
/// the second seed finds what the first one left, which a dry run has not left,
/// so the two runs differ in substance rather than tense (`rewrite/guidance.md`,
/// "What a dry run says"). A comparison written to tolerate that difference
/// would restate the gap instead of checking the mode. The difference itself is
/// asserted in `the_runs_diverge_where_one_action_feeds_another`.
fn lines_apart_from_the_contended_pair(tree: &Tree, output: &str) -> Vec<String> {
    let contended = display(&tree.home(LEAF_ORDERED_PAIR.2));
    output
        .lines()
        .filter(|line| !line.contains(&contended))
        .map(str::to_owned)
        .collect()
}

/// What the excluded pair does in each mode, which is the whole of what parity
/// gives up by excluding it.
///
/// A dry run creates nothing, so the second seed finds the destination as empty
/// as the first one did and both say they would copy. A real run's second seed
/// finds the first one's work and keeps it. Nothing else in the tree produces
/// this, which is why it is worth writing down rather than merely filtering.
#[test]
fn the_runs_diverge_where_one_action_feeds_another() {
    let (winner, loser, dest) = LEAF_ORDERED_PAIR;
    let tree = Tree::fixture("leaf");
    let repo = tree.path("repo");
    let installed = display(&tree.home(dest));
    let from = |source: &str| format!("{installed} from {}", display(&repo.join(source)));

    let dry = stderr_of(
        &tree
            .batfiles()
            .args(["sync", "--dry-run", "-v"])
            .assert()
            .success(),
    );
    let real = stderr_of(&tree.batfiles().args(["sync", "-v"]).assert().success());

    assert_eq!(
        contended_lines(&tree, &dry),
        [
            format!("would copy {}", from(winner)),
            format!("would copy {}", from(loser)),
        ]
    );
    assert_eq!(
        contended_lines(&tree, &real),
        [
            format!("copied {}", from(winner)),
            format!("kept {installed}")
        ]
    );
}

/// The inverse of [`lines_apart_from_the_contended_pair`]: only what was said
/// about the contended destination.
fn contended_lines(tree: &Tree, output: &str) -> Vec<String> {
    let contended = display(&tree.home(LEAF_ORDERED_PAIR.2));
    output
        .lines()
        .filter(|line| line.contains(&contended))
        .map(str::to_owned)
        .collect()
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
