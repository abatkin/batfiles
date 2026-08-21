//! Source-hygiene checks that keep three of `rewrite/guidance.md`'s rules
//! mechanical rather than honor-system: rule 1's dead-code annotations, the
//! `CARRY` markers of "Carrying work forward", and rule 12's list of options
//! that parse but are not honored yet.
//!
//! The first two scans are textual and line-oriented, so an attribute or a
//! marker split across lines is not seen. Neither is worth a parser.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// The step list a `CARRY` marker or a withheld option is cleared by.
const STEPS: &str = "rewrite/steps.md";

/// Rule 12's list, and the only file whose step literals are checked.
const UNSUPPORTED: &str = "src/cli/unsupported.rs";

/// This file, which the marker scan skips: its fixtures spell out the forms the
/// check rejects, so scanning it would report its own examples.
const CHECKER: &str = "tests/hygiene.rs";

fn crate_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// Every `.rs` file under `relative`, sorted so failures list in a stable order.
fn rust_sources(relative: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    collect(&crate_dir().join(relative), &mut found);
    found.sort();
    found
}

fn collect(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("a source directory") {
        let path = entry.expect("a directory entry").path();
        if path.is_dir() {
            collect(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
}

/// A source path as written in a failure message, relative to the crate root.
fn display(path: &Path) -> String {
    path.strip_prefix(crate_dir())
        .unwrap_or(path)
        .display()
        .to_string()
}

/// Rule 1, as line numbers and messages: `allow(dead_code)` never appears under
/// `src/`, and every `expect(dead_code)` names the step that reads the field.
fn dead_code_violations(source: &str) -> Vec<(usize, &'static str)> {
    let mut violations = Vec::new();
    for (index, line) in source.lines().enumerate() {
        // Whitespace is stripped so the check does not depend on how the
        // attribute happens to be spaced.
        let packed: String = line.chars().filter(|c| !c.is_whitespace()).collect();
        if packed.contains("allow(dead_code") {
            violations.push((
                index + 1,
                "`allow(dead_code)` is not permitted under `src/`: code with no caller \
                 reachable from `main` does not get committed",
            ));
        }
        if packed.contains("expect(dead_code") && !reason_is_filled(&packed) {
            violations.push((
                index + 1,
                "`expect(dead_code)` must carry a `reason` naming the step that reads the \
                 field; without one it is an `allow` that gets past this check",
            ));
        }
    }
    violations
}

/// Whether a whitespace-stripped attribute carries a non-empty `reason = "…"`.
fn reason_is_filled(packed: &str) -> bool {
    packed
        .split_once("reason=\"")
        .is_some_and(|(_, rest)| !rest.starts_with('"'))
}

/// A `CARRY` note found in a source file.
#[derive(Debug)]
enum Mention {
    /// A well-formed `// CARRY(1.3): note`.
    Marker { line: usize, step: String },
    /// A `CARRY` written some other way, so a typo cannot outlive its step
    /// unnoticed.
    Malformed { line: usize },
}

impl Mention {
    fn line(&self) -> usize {
        match self {
            Self::Marker { line, .. } | Self::Malformed { line } => *line,
        }
    }
}

/// Every `CARRY` mention in `source`, well-formed or not.
fn carry_mentions(source: &str) -> Vec<Mention> {
    source
        .lines()
        .enumerate()
        .filter(|(_, text)| text.contains("CARRY"))
        .map(|(index, text)| {
            let line = index + 1;
            match marker_step(text) {
                Some(step) => Mention::Marker { line, step },
                None => Mention::Malformed { line },
            }
        })
        .collect()
}

/// The step a marker names, if the line is written `CARRY(<step>): <note>`.
fn marker_step(line: &str) -> Option<String> {
    let (_, rest) = line.split_once("CARRY(")?;
    let (step, rest) = rest.split_once(')')?;
    let note = rest.strip_prefix(':')?.trim();
    (is_step(step) && !note.is_empty()).then(|| step.to_string())
}

/// Whether `candidate` is a step number: digits, a dot, digits.
fn is_step(candidate: &str) -> bool {
    match candidate.split_once('.') {
        Some((slice, step)) => {
            !slice.is_empty()
                && !step.is_empty()
                && slice
                    .chars()
                    .chain(step.chars())
                    .all(|c| c.is_ascii_digit())
        }
        None => false,
    }
}

/// Every step [`STEPS`] defines, and whether it is marked ✅.
fn step_status(steps: &str) -> BTreeMap<String, bool> {
    steps
        .lines()
        .filter_map(|line| {
            let (step, rest) = line.strip_prefix("- **")?.split_once("**")?;
            is_step(step).then(|| (step.to_string(), rest.trim_start().starts_with('✅')))
        })
        .collect()
}

/// Why `mention` is not a live carry-forward note, if it is not one.
fn spent_reason(mention: &Mention, steps: &BTreeMap<String, bool>) -> Option<String> {
    match mention {
        Mention::Malformed { .. } => Some(
            "not a carry-forward marker: write `CARRY(<step>): <note>`, or nothing will ever \
             clear it"
                .to_string(),
        ),
        Mention::Marker { step, .. } => match steps.get(step) {
            None => Some(format!("`CARRY({step})` names no step in {STEPS}")),
            Some(true) => Some(format!(
                "step {step} is done: route the note to whoever reads it next, or delete it"
            )),
            Some(false) => None,
        },
    }
}

/// Every step-shaped string literal in `source`, with its line number.
///
/// Literal-oriented rather than line-oriented, because rustfmt wraps a long
/// entry across several lines and an option need not sit beside its step.
fn step_literals(source: &str) -> Vec<(usize, String)> {
    source
        .lines()
        .enumerate()
        .flat_map(|(index, line)| {
            // Between the first and second quote, the third and fourth, and so
            // on: the contents of each string literal on the line.
            line.split('"')
                .skip(1)
                .step_by(2)
                .filter(|literal| is_step(literal))
                .map(move |literal| (index + 1, literal.to_string()))
        })
        .collect()
}

/// Why a step named by [`UNSUPPORTED`] is no longer one to withhold an option
/// for, if it is not.
fn live_step_reason(step: &str, steps: &BTreeMap<String, bool>) -> Option<String> {
    match steps.get(step) {
        None => Some(format!("`{step}` names no step in {STEPS}")),
        Some(true) => Some(format!(
            "step {step} is done, so the option it withholds is live: delete the entry, or \
             the option is refused after it works"
        )),
        Some(false) => None,
    }
}

/// [`STEPS`] as the step-to-done map both checks are cleared by.
fn recorded_steps() -> BTreeMap<String, bool> {
    let steps = fs::read_to_string(crate_dir().join(STEPS))
        .unwrap_or_else(|error| panic!("{STEPS} is what clears these notes: {error}"));
    step_status(&steps)
}

#[test]
fn src_dead_code_annotations_follow_rule_one() {
    let mut failures = Vec::new();
    for path in rust_sources("src") {
        let source = fs::read_to_string(&path).expect("a readable source file");
        for (line, message) in dead_code_violations(&source) {
            failures.push(format!("{}:{line}: {message}", display(&path)));
        }
    }
    assert!(failures.is_empty(), "\n{}\n", failures.join("\n"));
}

#[test]
fn allow_dead_code_is_rejected_however_it_is_spelled() {
    for source in [
        "#[allow(dead_code)]",
        "#![allow(dead_code)]",
        "#[allow( dead_code )]",
        "#[allow(dead_code, unused)]",
        "#[cfg_attr(test, allow(dead_code))]",
    ] {
        assert_eq!(dead_code_violations(source).len(), 1, "{source}");
    }
}

#[test]
fn expect_dead_code_is_accepted_only_with_a_filled_reason() {
    let live = r#"#[expect(dead_code, reason = "3.2 selects by group")]"#;
    assert!(dead_code_violations(live).is_empty(), "{live}");

    for source in [
        "#[expect(dead_code)]",
        r#"#[expect(dead_code, reason = "")]"#,
    ] {
        assert_eq!(dead_code_violations(source).len(), 1, "{source}");
    }
}

#[test]
fn carry_markers_name_a_step_that_is_still_open() {
    let mut found = Vec::new();
    for path in rust_sources("src").into_iter().chain(rust_sources("tests")) {
        if path.ends_with(CHECKER) {
            continue;
        }
        let source = fs::read_to_string(&path).expect("a readable source file");
        for mention in carry_mentions(&source) {
            found.push((path.clone(), mention));
        }
    }

    // With no markers there is nothing to clear, which is also what lets this
    // check outlive `rewrite/` — see that directory's README.
    if found.is_empty() {
        return;
    }

    let steps = recorded_steps();
    let failures: Vec<String> = found
        .iter()
        .filter_map(|(path, mention)| {
            let reason = spent_reason(mention, &steps)?;
            Some(format!("{}:{}: {reason}", display(path), mention.line()))
        })
        .collect();
    assert!(failures.is_empty(), "\n{}\n", failures.join("\n"));
}

#[test]
fn withheld_options_name_steps_that_are_still_open() {
    let source = fs::read_to_string(crate_dir().join(UNSUPPORTED))
        .unwrap_or_else(|error| panic!("{UNSUPPORTED} holds rule 12's list: {error}"));
    let steps = recorded_steps();
    let failures: Vec<String> = step_literals(&source)
        .into_iter()
        .filter_map(|(line, step)| {
            let reason = live_step_reason(&step, &steps)?;
            Some(format!("{UNSUPPORTED}:{line}: {reason}"))
        })
        .collect();
    assert!(failures.is_empty(), "\n{}\n", failures.join("\n"));
}

/// Stands in for [`STEPS`]: one step done, one still open.
fn fixture_steps() -> BTreeMap<String, bool> {
    step_status(
        "## Slice 0 — Walking skeleton\n\
         \n\
         - **0.13** ✅ Reject a stale marker whose step is done.\n\
         - **1.3** Extract only what all three variants genuinely share.\n\
         - a bullet that names no step\n",
    )
}

#[test]
fn a_marker_is_live_while_its_step_is_open() {
    let mentions = carry_mentions("    // CARRY(1.3): written for `symlink` alone\n");
    assert!(
        matches!(&mentions[..], [Mention::Marker { line: 1, step }] if step == "1.3"),
        "{mentions:?}"
    );
    assert_eq!(spent_reason(&mentions[0], &fixture_steps()), None);
}

#[test]
fn a_marker_is_spent_once_its_step_is_done() {
    let mentions = carry_mentions("// CARRY(0.13): the fixture marks this step done\n");
    assert!(
        spent_reason(&mentions[0], &fixture_steps()).is_some(),
        "{mentions:?}"
    );
}

#[test]
fn a_marker_naming_no_step_is_rejected() {
    // 0.10 absorbed 0.9, so no bullet defines it and nothing would ever clear
    // a note pointing at it.
    let mentions = carry_mentions("// CARRY(0.9): a step that no longer exists\n");
    assert!(
        spent_reason(&mentions[0], &fixture_steps()).is_some(),
        "{mentions:?}"
    );
}

#[test]
fn an_entry_is_found_however_rustfmt_wrapped_it() {
    let wrapped = "        (\n\
                   \x20           !options.skip_actions.is_empty(),\n\
                   \x20           \"--skip-action\",\n\
                   \x20           \"3.4\",\n\
                   \x20       ),\n";
    assert_eq!(step_literals(wrapped), [(4, "3.4".to_string())]);
}

#[test]
fn an_entry_is_live_while_the_step_that_frees_its_option_is_open() {
    let steps = fixture_steps();
    assert_eq!(live_step_reason("1.3", &steps), None);
    // 0.13 is done in the fixture, and 0.9 was absorbed and defines no bullet.
    assert!(live_step_reason("0.13", &steps).is_some());
    assert!(live_step_reason("0.9", &steps).is_some());
}

#[test]
fn a_carry_written_any_other_way_is_rejected() {
    for line in [
        "// CARRY 1.3: no parentheses",
        "// CARRY(1.3) no colon",
        "// CARRY(1.3):",
        "// CARRY(slice one): not a step number",
    ] {
        let mentions = carry_mentions(line);
        assert_eq!(mentions.len(), 1, "{line}");
        assert!(
            spent_reason(&mentions[0], &fixture_steps()).is_some(),
            "{line}"
        );
    }
}
