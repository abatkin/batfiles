//! CLI tests for dry-run filesystem preservation and progress output, using the leaf fixture.

use std::fs;

use crate::support::*;

/// Create an existing seed destination and a directory used by the leaf fixture.
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

    assert_eq!(snapshot(&tree.path("home")), before);
}

/// Total action count in the leaf fixture. Output parity excludes the two seeds sharing a
/// destination.
const PARITY_ACTIONS: usize = 14;

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

    // Include kept and unchanged actions in the output comparison.
    let dry = stderr_of(
        &tree
            .batfiles()
            .args(["sync", "--dry-run", "-v"])
            .assert()
            .success(),
    );
    let real = stderr_of(&tree.batfiles().args(["sync", "-v"]).assert().success());

    let said: Vec<String> = lines_apart_from_the_contended_pair(&tree, &dry)
        .iter()
        .map(|line| in_past_tense(line))
        .collect();
    assert_eq!(said, lines_apart_from_the_contended_pair(&tree, &real));
    // Require some actions so the parity check cannot pass on two empty outputs.
    for expected in ["would link", "would copy", "would create", "would keep"] {
        assert!(
            dry.contains(expected),
            "the dry run never said `{expected}`:\n{dry}"
        );
    }
}

/// Return output lines excluding the fixture's shared seed destination. Those dependent actions
/// are checked separately because dry runs do not simulate earlier writes.
fn lines_apart_from_the_contended_pair(tree: &Tree, output: &str) -> Vec<String> {
    let contended = display(&tree.home(LEAF_ORDERED_PAIR.2));
    output
        .lines()
        .filter(|line| !line.contains(&contended))
        .map(str::to_owned)
        .collect()
}

/// Both dry-run seeds report a copy; the real run's second seed keeps the first seed's content.
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

/// Group application in dry-run mode reports planned work without writing.
#[test]
fn applying_a_group_under_dry_run_reports_and_writes_nothing() {
    let tree = Tree::fixture("leaf");
    with_existing_content(&tree);
    let before = snapshot(&tree.path("home"));

    let dry = stderr_of(
        &tree
            .batfiles()
            .args(["apply-group", "--group", "git", "--dry-run", "-v"])
            .assert()
            .success(),
    );
    assert_eq!(snapshot(&tree.path("home")), before);

    let real = stderr_of(
        &tree
            .batfiles()
            .args(["apply-group", "--group", "git", "-v"])
            .assert()
            .success(),
    );
    let said: Vec<String> = dry.lines().map(in_past_tense).collect();
    assert_eq!(said, real.lines().collect::<Vec<_>>());
    // Require some actions so the parity check cannot pass on two empty outputs.
    for expected in ["would link", "would keep"] {
        assert!(
            dry.contains(expected),
            "the dry run never said `{expected}`:\n{dry}"
        );
    }
}

#[test]
fn applying_one_action_under_dry_run_writes_nothing() {
    let tree = Tree::fixture("leaf");
    with_existing_content(&tree);
    let before = snapshot(&tree.path("home"));

    let dry = stderr_of(
        &tree
            .batfiles()
            .args(["apply-action", "--id", "nvim", "--dry-run"])
            .assert()
            .success(),
    );
    assert_eq!(snapshot(&tree.path("home")), before);

    let real = stderr_of(
        &tree
            .batfiles()
            .args(["apply-action", "--id", "nvim"])
            .assert()
            .success(),
    );
    assert_eq!(
        dry.lines().map(in_past_tense).collect::<Vec<_>>(),
        real.lines().collect::<Vec<_>>()
    );
    assert!(
        dry.contains("would link"),
        "the dry run never said what it would do:\n{dry}"
    );
}

/// Convert dry-run progress wording to the corresponding past tense for comparison.
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
