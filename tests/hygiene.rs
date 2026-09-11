//! Textual checks for dead-code annotations, scheduled markers, filesystem-owner
//! imports, and action inventories. These scans do not parse Rust or prove
//! read-only access, mode gating, call reachability, or behavioral coverage.
//! Review those properties in code and exercise them through CLI tests.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// The step list a `CARRY` marker or a withheld option is cleared by.
const STEPS: &str = "rewrite/steps.md";

/// Rule 12's list, and the only file whose step literals are checked.
const UNSUPPORTED: &str = "src/cli/unsupported.rs";

/// The enum that decides which action types a manifest may declare.
const ACTIONS: &str = "src/manifest/action.rs";

/// The document that answers "what can `sync` actually do", on a line beginning
/// [`IMPLEMENTED`]. Goals state intended scope and deliberately keep no
/// inventory.
const ACTION_TYPE_DOCS: [&str; 1] = ["README.md"];

/// What that line starts with.
const IMPLEMENTED: &str = "Implemented so far:";

/// The fixture repositories the CLI tests drive whole. Between them they
/// declare every action type, which is what makes "the suite covers them all"
/// true rather than apparent.
const FIXTURES: &str = "tests/fixtures";

/// This file, which the marker scan skips: its fixtures spell out the forms the
/// check rejects, so scanning it would report its own examples.
const CHECKER: &str = "tests/hygiene.rs";

/// Declared filesystem role for review. The scanner does not verify the role.
#[derive(Debug, Clone, Copy)]
enum Kind {
    /// Inspects and never writes, so there is no work for a mode to withhold.
    ReadOnly,
    /// Performs part of an action's work, and therefore consults `RunMode` itself.
    ModeReader,
    /// Produces content that only ever lands inside something a mode reader
    /// created, so it never sees `RunMode` and does not need to.
    Downstream,
    /// Batfiles' own bookkeeping, which runs in both modes: a state file is not
    /// part of the plan an action carries out.
    Bookkeeping,
}

impl Kind {
    fn label(self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only, so it never writes",
            Self::ModeReader => "a mode reader",
            Self::Downstream => "downstream of a mode reader",
            Self::Bookkeeping => "bookkeeping, which runs in both modes",
        }
    }
}

/// A module allowed to name the filesystem, and why.
#[derive(Debug)]
struct Owner {
    path: &'static str,
    kind: Kind,
    reason: &'static str,
}

/// The modules that own filesystem access. Every other module under `src/` is
/// forbidden from *naming* `std::fs`, a platform `fs` module, or
/// `std::process::Command`.
const FILESYSTEM_OWNERS: [Owner; 10] = [
    Owner {
        path: "src/clone_list.rs",
        kind: Kind::ReadOnly,
        reason: "reads the list a `git-clone-list` names, and never clones from it",
    },
    Owner {
        path: "src/paths.rs",
        kind: Kind::ReadOnly,
        reason: "what a path means, and what is already at one",
    },
    Owner {
        path: "src/action/copy.rs",
        kind: Kind::Downstream,
        reason: "reproduces a repository node into a staging tree install.rs made",
    },
    Owner {
        path: "src/fetch.rs",
        kind: Kind::Downstream,
        reason: "writes a download into a staging file install.rs opened, and widens its mode",
    },
    Owner {
        path: "src/archive.rs",
        kind: Kind::Downstream,
        reason: "unpacks a downloaded archive into a staging tree install.rs made",
    },
    Owner {
        path: "src/directory.rs",
        kind: Kind::ModeReader,
        reason: "creates installation containers and missing destination parents",
    },
    Owner {
        path: "src/install.rs",
        kind: Kind::ModeReader,
        reason: "creates, publishes, and cleans up seed staging nodes",
    },
    Owner {
        path: "src/git.rs",
        kind: Kind::ModeReader,
        reason: "runs git, and under DryRun runs none for any caller",
    },
    Owner {
        path: "src/action/symlink.rs",
        kind: Kind::ModeReader,
        reason: "the platform-specific call every symlink action makes",
    },
    Owner {
        path: "src/tomlfile.rs",
        kind: Kind::Bookkeeping,
        reason: "reads and atomically rewrites the documents batfiles owns",
    },
];

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

/// A `dead_code` annotation found in a source file.
#[derive(Debug)]
enum DeadCode {
    /// `allow(dead_code)`, which rule 1 permits nowhere under `src/`.
    Allowed { line: usize },
    /// `expect(dead_code)` with no `reason`, or an empty one.
    Unexplained { line: usize },
    /// `expect(dead_code, reason = "…")`, with every step-shaped token the
    /// reason names.
    Expected { line: usize, named: Vec<String> },
}

impl DeadCode {
    fn line(&self) -> usize {
        match self {
            Self::Allowed { line } | Self::Unexplained { line } | Self::Expected { line, .. } => {
                *line
            }
        }
    }
}

/// How far the scan looks either side of a `dead_code` for the attribute it
/// sits in. rustfmt's broken-up form puts the token on the line after the `#[`
/// and the reason on the line after that, so this is generous.
const ATTRIBUTE_LINES: usize = 6;

/// Every `dead_code` annotation in `source`.
fn dead_code_mentions(source: &str) -> Vec<DeadCode> {
    let lines: Vec<&str> = source.lines().collect();
    let mut found = Vec::new();
    for (index, text) in lines.iter().enumerate() {
        if !text.contains("dead_code") {
            continue;
        }
        // A `dead_code` outside an attribute is prose about one, not one.
        let Some(attribute) = attribute_around(&lines, index) else {
            continue;
        };
        // Whitespace is stripped so the check does not depend on how the
        // attribute happens to be spaced, or on where rustfmt broke it.
        let packed: String = attribute.chars().filter(|c| !c.is_whitespace()).collect();
        let line = index + 1;
        if packed.contains("allow(dead_code") {
            found.push(DeadCode::Allowed { line });
        }
        if packed.contains("expect(dead_code") {
            found.push(match reason_of(&attribute) {
                None => DeadCode::Unexplained { line },
                Some(reason) => DeadCode::Expected {
                    line,
                    named: reason_steps(reason),
                },
            });
        }
    }
    found
}

/// The whole attribute the `dead_code` on `lines[anchor]` belongs to, joined
/// into one string; `None` where the token is in no attribute at all.
fn attribute_around(lines: &[&str], anchor: usize) -> Option<String> {
    let start = (anchor.saturating_sub(ATTRIBUTE_LINES)..=anchor)
        .rev()
        .find(|&index| opens_attribute(lines[index]))?;
    let mut joined = String::new();
    let mut end = start;
    for (offset, line) in lines.iter().skip(start).take(ATTRIBUTE_LINES).enumerate() {
        joined.push_str(line);
        joined.push(' ');
        end = start + offset;
        if line.contains(")]") {
            break;
        }
    }
    // The attribute has to be the one the token is in, and not one that closed
    // a few lines above prose mentioning it.
    (anchor <= end).then_some(joined)
}

/// Whether a line opens an attribute, inner (`#![…]`) or outer (`#[…]`).
fn opens_attribute(line: &str) -> bool {
    line.contains("#[") || line.contains("#![")
}

/// The `reason = "…"` an attribute carries, if it carries a non-empty one.
fn reason_of(attribute: &str) -> Option<&str> {
    let (_, rest) = attribute.split_once("reason")?;
    let (_, rest) = rest.split_once('"')?;
    let (reason, _) = rest.split_once('"')?;
    (!reason.is_empty()).then_some(reason)
}

/// Every step-shaped token a `reason` names, in the order it names them.
fn reason_steps(reason: &str) -> Vec<String> {
    reason
        .split(|c: char| !c.is_ascii_digit() && c != '.')
        .map(|token| token.trim_matches('.'))
        .filter(|token| is_step(token))
        .map(str::to_string)
        .collect()
}

/// Why a `dead_code` annotation is not one rule 1 permits, if it is not.
fn dead_code_reason(mention: &DeadCode, steps: &BTreeMap<String, bool>) -> Option<String> {
    match mention {
        DeadCode::Allowed { .. } => Some(
            "`allow(dead_code)` is not permitted under `src/`: code with no caller reachable \
             from `main` does not get committed"
                .to_string(),
        ),
        DeadCode::Unexplained { .. } => Some(
            "`expect(dead_code)` must carry a `reason` naming the step that reads the item; \
             without one it is an `allow` that gets past this check"
                .to_string(),
        ),
        DeadCode::Expected { named, .. } if named.is_empty() => Some(format!(
            "this `reason` names no step, so nothing in {STEPS} clears it: name the step that \
             reads the item, as `reason = \"read at <step>\"`"
        )),
        DeadCode::Expected { named, .. } => {
            let undefined = named
                .iter()
                .filter(|step| matches!(step_state(step, steps), Some(Spent::Undefined)))
                .cloned()
                .collect::<Vec<String>>();
            if !undefined.is_empty() {
                return Some(format!(
                    "this `reason` names {}, which {STEPS} does not define, so nothing will \
                     ever clear the annotation",
                    undefined.join(" and ")
                ));
            }
            // Every step named is defined, so the annotation is live while any
            // one of them is still open to read the item.
            named
                .iter()
                .all(|step| step_state(step, steps).is_some())
                .then(|| {
                    format!(
                        "step {} is done and the item is still unread: the caller never \
                         arrived, so delete the item, or name the step that does read it",
                        named.join(" and ")
                    )
                })
        }
    }
}

/// Every way a line names the filesystem, with what it named.
fn filesystem_mentions(source: &str) -> Vec<(usize, &'static str)> {
    let mut found = Vec::new();
    for (index, line) in source.lines().enumerate() {
        let packed: String = line.chars().filter(|c| !c.is_whitespace()).collect();
        if packed.contains("std::fs") {
            found.push((index + 1, "std::fs"));
        }
        // Narrower than rejecting `std::os::` whole, so a future need for
        // `std::os::unix::ffi` is not caught by a filesystem check.
        if packed
            .split_once("std::os::")
            .is_some_and(|(_, rest)| rest.contains("::fs"))
        {
            found.push((index + 1, "a platform `fs` module"));
        }
        if packed.contains("process::Command") {
            found.push((index + 1, "std::process::Command"));
        }
    }
    found
}

/// The owner entry for a source path, if it has one.
fn owner_of(path: &Path) -> Option<&'static Owner> {
    FILESYSTEM_OWNERS
        .iter()
        .find(|owner| path.ends_with(Path::new(owner.path)))
}

/// The allowlist as a failure message renders it, so a violation is read
/// alongside what the permitted entries look like.
fn owners_as_written() -> String {
    FILESYSTEM_OWNERS
        .iter()
        .map(|owner| {
            format!(
                "  {} — {}: {}",
                owner.path,
                owner.kind.label(),
                owner.reason
            )
        })
        .collect::<Vec<String>>()
        .join("\n")
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

/// Why a step named by an annotation can no longer clear it.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Spent {
    /// [`STEPS`] defines no such step, so nothing will ever clear the note.
    Undefined,
    /// The step is marked ✅, so whatever it was going to do, it did.
    Done,
}

/// Why `step` is no longer an open step in [`STEPS`], if it is not.
fn step_state(step: &str, steps: &BTreeMap<String, bool>) -> Option<Spent> {
    match steps.get(step) {
        None => Some(Spent::Undefined),
        Some(true) => Some(Spent::Done),
        Some(false) => None,
    }
}

/// Why `mention` is not a live carry-forward note, if it is not one.
fn spent_reason(mention: &Mention, steps: &BTreeMap<String, bool>) -> Option<String> {
    match mention {
        Mention::Malformed { .. } => Some(
            "not a carry-forward marker: write `CARRY(<step>): <note>`, or nothing will ever \
             clear it"
                .to_string(),
        ),
        Mention::Marker { step, .. } => match step_state(step, steps)? {
            Spent::Undefined => Some(format!("`CARRY({step})` names no step in {STEPS}")),
            Spent::Done => Some(format!(
                "step {step} is done: route the note to whoever reads it next, or delete it"
            )),
        },
    }
}

/// Every step-shaped string literal in `source`, with its line number.
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
    match step_state(step, steps)? {
        Spent::Undefined => Some(format!("`{step}` names no step in {STEPS}")),
        Spent::Done => Some(format!(
            "step {step} is done, so the option it withholds is live: delete the entry, or \
             the option is refused after it works"
        )),
    }
}

/// The action types a manifest may declare: every `Action` variant, spelled the
/// way that enum's `rename_all` makes serde write it.
fn implemented_action_types(source: &str) -> Vec<String> {
    let after = source
        .split_once("enum Action {")
        .map(|(_, rest)| rest)
        .unwrap_or_else(|| panic!("{ACTIONS} no longer declares `enum Action`"));
    let body = after
        .split_once('}')
        .map(|(body, _)| body)
        .unwrap_or_else(|| panic!("`enum Action` in {ACTIONS} is not closed"));

    let mut types: Vec<String> = body
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("//") && !line.starts_with('#'))
        .map(|line| kebab_case(variant_name(line)))
        .collect();
    types.sort();
    types
}

/// The variant an enum body's line declares, without its payload.
fn variant_name(line: &str) -> &str {
    line.split(|c: char| c == '(' || c == '{' || c == ',' || c.is_whitespace())
        .next()
        .unwrap_or(line)
}

/// A variant name as `rename_all = "kebab-case"` writes it.
fn kebab_case(variant: &str) -> String {
    let mut kebab = String::new();
    for (index, character) in variant.char_indices() {
        if character.is_uppercase() && index != 0 {
            kebab.push('-');
        }
        kebab.extend(character.to_lowercase());
    }
    kebab
}

/// The action types a document claims are built, from its [`IMPLEMENTED`] line.
fn documented_action_types(document: &str) -> Option<Vec<String>> {
    let (_, named) = document
        .lines()
        .find_map(|line| line.split_once(IMPLEMENTED))?;
    // Between the first and second backtick, the third and fourth, and so on.
    let mut types: Vec<String> = named
        .split('`')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect();
    types.sort();
    Some(types)
}

/// The action types a manifest declares, sorted and without repeats.
fn declared_action_types(manifest: &str) -> Vec<String> {
    let mut types: Vec<String> = manifest
        .lines()
        .filter_map(|line| line.trim().strip_prefix("type = \""))
        .filter_map(|rest| rest.split('"').next())
        .map(str::to_string)
        .collect();
    types.sort();
    types.dedup();
    types
}

/// [`STEPS`] as the step-to-done map both checks are cleared by.
fn recorded_steps() -> BTreeMap<String, bool> {
    let steps = fs::read_to_string(crate_dir().join(STEPS))
        .unwrap_or_else(|error| panic!("{STEPS} is what clears these notes: {error}"));
    step_status(&steps)
}

#[test]
fn src_dead_code_annotations_follow_rule_one() {
    let mut found = Vec::new();
    for path in rust_sources("src") {
        let source = fs::read_to_string(&path).expect("a readable source file");
        for mention in dead_code_mentions(&source) {
            found.push((path.clone(), mention));
        }
    }

    // With no annotations there is no step to judge, which is also what lets
    // this check outlive `rewrite/` — see that directory's README.
    if found.is_empty() {
        return;
    }

    let steps = recorded_steps();
    let failures: Vec<String> = found
        .iter()
        .filter_map(|(path, mention)| {
            let reason = dead_code_reason(mention, &steps)?;
            Some(format!("{}:{}: {reason}", display(path), mention.line()))
        })
        .collect();
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
        "#[allow(\n    dead_code\n)]",
    ] {
        let mentions = dead_code_mentions(source);
        assert!(
            matches!(&mentions[..], [DeadCode::Allowed { .. }]),
            "{source}: {mentions:?}"
        );
        assert!(
            dead_code_reason(&mentions[0], &fixture_steps()).is_some(),
            "{source}"
        );
    }
}

#[test]
fn an_annotation_is_seen_however_rustfmt_broke_it_up() {
    // rustfmt may put the token and reason on separate lines.
    let wrapped = "    #[expect(\n\
                   \x20       dead_code,\n\
                   \x20       reason = \"walked at 1.3, to reject an entry setting both \
                   conditions\"\n\
                   \x20   )]\n";
    let mentions = dead_code_mentions(wrapped);
    assert!(
        matches!(&mentions[..], [DeadCode::Expected { line: 2, named }] if *named == ["1.3"]),
        "{mentions:?}"
    );
    assert_eq!(dead_code_reason(&mentions[0], &fixture_steps()), None);
}

#[test]
fn expect_dead_code_is_accepted_only_with_a_filled_reason() {
    for source in [
        "#[expect(dead_code)]",
        r#"#[expect(dead_code, reason = "")]"#,
        "#[expect(\n    dead_code\n)]",
    ] {
        let mentions = dead_code_mentions(source);
        assert!(
            matches!(&mentions[..], [DeadCode::Unexplained { .. }]),
            "{source}: {mentions:?}"
        );
        assert!(
            dead_code_reason(&mentions[0], &fixture_steps()).is_some(),
            "{source}"
        );
    }
}

#[test]
fn an_expectation_is_live_while_the_step_that_reads_the_item_is_open() {
    let live = r#"#[expect(dead_code, reason = "read at 1.3, by the caller it is written for")]"#;
    let mentions = dead_code_mentions(live);
    assert_eq!(dead_code_reason(&mentions[0], &fixture_steps()), None);
}

#[test]
fn an_expectation_whose_step_is_done_is_rejected() {
    // The gap the by-the-end-of-the-slice reading opens: the compiler deletes
    // the note when the caller lands and says nothing when it never does.
    let stale = r#"#[expect(dead_code, reason = "read at 0.13, which the fixture marks done")]"#;
    let mentions = dead_code_mentions(stale);
    assert!(
        dead_code_reason(&mentions[0], &fixture_steps()).is_some(),
        "{mentions:?}"
    );
}

#[test]
fn an_expectation_naming_no_step_at_all_is_rejected() {
    let prose = r#"#[expect(dead_code, reason = "the bootstrap will want this one day")]"#;
    let mentions = dead_code_mentions(prose);
    assert!(
        matches!(&mentions[..], [DeadCode::Expected { named, .. }] if named.is_empty()),
        "{mentions:?}"
    );
    assert!(dead_code_reason(&mentions[0], &fixture_steps()).is_some());
}

#[test]
fn an_expectation_naming_a_step_that_does_not_exist_is_rejected() {
    // The fixture has no step 0.9.
    let orphan = r#"#[expect(dead_code, reason = "read at 0.9, an undefined step")]"#;
    let mentions = dead_code_mentions(orphan);
    assert!(
        dead_code_reason(&mentions[0], &fixture_steps()).is_some(),
        "{mentions:?}"
    );
}

#[test]
fn an_expectation_naming_two_steps_is_live_while_either_is_open() {
    let both = r#"#[expect(dead_code, reason = "read at 0.13 and again at 1.3")]"#;
    let mentions = dead_code_mentions(both);
    assert!(
        matches!(&mentions[..], [DeadCode::Expected { named, .. }] if *named == ["0.13", "1.3"]),
        "{mentions:?}"
    );
    // 1.3 is still open, so a step is still coming that reads the item.
    assert_eq!(dead_code_reason(&mentions[0], &fixture_steps()), None);
}

#[test]
fn a_step_is_found_in_a_reason_however_it_is_punctuated() {
    // The form in the tree today, a step ending a sentence, and a version
    // number, which is not step-shaped.
    assert_eq!(
        reason_steps("adopted at 8.3, by the bootstrap that reads it"),
        ["8.3"]
    );
    assert_eq!(reason_steps("the caller lands at 4.5."), ["4.5"]);
    assert!(reason_steps("wanted by proc-macro2 1.0.107").is_empty());
}

#[test]
fn a_dead_code_outside_an_attribute_is_not_an_annotation() {
    // Rule 1 is discussed in prose in the modules it governs, and a doc comment
    // saying `dead_code` is not one.
    let prose = "//! The text `allow(dead_code)` in prose is not an annotation.\n";
    assert!(dead_code_mentions(prose).is_empty(), "{prose}");

    // The same prose below an annotation that has already closed, which the
    // backward search would otherwise reach and count twice.
    let below = "#[expect(dead_code, reason = \"read at 1.3\")]\n\
                 pub struct Entry;\n\
                 /// Not to be confused with allow(dead_code).\n";
    assert_eq!(dead_code_mentions(below).len(), 1, "{below}");
}

#[test]
fn only_the_modules_that_own_filesystem_access_name_it() {
    let mut failures = Vec::new();
    for path in rust_sources("src") {
        if owner_of(&path).is_some() {
            continue;
        }
        let source = fs::read_to_string(&path).expect("a readable source file");
        for (line, named) in filesystem_mentions(&source) {
            failures.push(format!(
                "{}:{line}: names `{named}`, and does not own filesystem access",
                display(&path)
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "\n{}\n\nA module that writes carries out part of an action's work, so it has to \
         consult `RunMode` or sit downstream of something that does. If this file genuinely \
         owns an operation, add it to `FILESYSTEM_OWNERS` with the kind of owner it is:\n{}\n",
        failures.join("\n"),
        owners_as_written()
    );
}

#[test]
fn every_module_that_owns_filesystem_access_still_exists() {
    // A renamed owner would otherwise leave an entry permitting nothing.
    for owner in &FILESYSTEM_OWNERS {
        let path = crate_dir().join(owner.path);
        assert!(
            path.is_file(),
            "{} is on the filesystem-owner list and is not there",
            owner.path
        );
    }
}

#[test]
fn supported_filesystem_import_spellings_are_detected() {
    for (source, expected) in [
        ("use std::fs;", "std::fs"),
        ("    let found = std::fs::metadata(path)?;", "std::fs"),
        ("use std :: fs :: File;", "std::fs"),
        ("use std::os::unix::fs::symlink;", "a platform `fs` module"),
        (
            "use std::os::windows::fs::symlink_file;",
            "a platform `fs` module",
        ),
        ("use std::process::Command;", "std::process::Command"),
        (
            "    process::Command::new(\"git\")",
            "std::process::Command",
        ),
    ] {
        assert_eq!(filesystem_mentions(source), [(1, expected)], "{source}");
    }
}

#[test]
fn a_name_that_is_not_the_filesystem_is_left_alone() {
    for source in [
        // The exit status every command returns, which `app.rs` needs.
        "use std::process::ExitCode;",
        // Platform-specific and not the filesystem.
        "use std::os::unix::ffi::OsStrExt;",
        "let contents = read_to_string(path)?;",
    ] {
        assert!(filesystem_mentions(source).is_empty(), "{source}");
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

#[test]
fn the_readme_names_every_action_type_that_exists() {
    let source = fs::read_to_string(crate_dir().join(ACTIONS))
        .unwrap_or_else(|error| panic!("{ACTIONS} declares the action types: {error}"));
    assert!(
        source.contains(r#"rename_all = "kebab-case""#),
        "{ACTIONS} no longer renames its variants kebab-case, so this check is comparing \
         the wrong spelling"
    );
    let built = implemented_action_types(&source);

    for relative in ACTION_TYPE_DOCS {
        let document = fs::read_to_string(crate_dir().join(relative))
            .unwrap_or_else(|error| panic!("{relative} says what `sync` can do: {error}"));
        let documented = documented_action_types(&document).unwrap_or_else(|| {
            panic!("{relative} has no `{IMPLEMENTED}` line, so nothing keeps it honest")
        });
        assert_eq!(
            documented, built,
            "{relative} has fallen behind {ACTIONS}: its `{IMPLEMENTED}` line names the \
             wrong action types"
        );
    }
}

#[test]
fn the_fixture_repositories_declare_every_action_type_that_exists() {
    let source = fs::read_to_string(crate_dir().join(ACTIONS))
        .unwrap_or_else(|error| panic!("{ACTIONS} declares the action types: {error}"));

    let mut declared: Vec<String> = Vec::new();
    for manifest in fixture_manifests() {
        let document = fs::read_to_string(&manifest).unwrap_or_else(|error| {
            panic!("{} is a fixture manifest: {error}", display(&manifest))
        });
        declared.extend(declared_action_types(&document));
    }
    declared.sort();
    declared.dedup();

    assert_eq!(
        declared,
        implemented_action_types(&source),
        "the fixtures under {FIXTURES} do not declare every action type in {ACTIONS} between \
         them, so the CLI tests cover fewer of them than they appear to"
    );
}

/// Every fixture repository's manifest, sorted.
fn fixture_manifests() -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = fs::read_dir(crate_dir().join(FIXTURES))
        .expect("the fixture directory")
        .map(|entry| entry.expect("a fixture entry").path().join("batfiles.toml"))
        .filter(|manifest| manifest.is_file())
        .collect();
    assert!(
        !found.is_empty(),
        "no fixture repository under {FIXTURES} has a manifest, so this check reads nothing"
    );
    found.sort();
    found
}

#[test]
fn a_manifests_action_types_are_read_once_each() {
    let manifest = "[[actions]]\n\
                    type = \"symlink\"\n\
                    source = \"shell/zshrc\"\n\
                    \n\
                    [[actions]]\n\
                    type = \"copy\"\n\
                    \n\
                    [[actions]]\n\
                    type = \"symlink\"\n";
    assert_eq!(declared_action_types(manifest), ["copy", "symlink"]);
}

/// Stands in for [`ACTIONS`]: an enum shaped like the real one, with the
/// comment and attribute lines it carries. Deliberately not a copy of it —
/// what is under test is the reading, so a fixture that tracked the real
/// variants would only prove they equal themselves.
fn fixture_actions() -> &'static str {
    "#[derive(Debug, Deserialize)]\n\
     #[serde(tag = \"type\", rename_all = \"kebab-case\")]\n\
     pub(crate) enum Action {\n\
     \x20   Symlink(SymlinkAction),\n\
     \x20   /// A directory, created when it is missing.\n\
     \x20   CreateDir(CreateDirAction),\n\
     \x20   Copy(CopyAction),\n\
     }\n"
}

#[test]
fn an_action_type_is_named_the_way_a_manifest_writes_it() {
    assert_eq!(
        implemented_action_types(fixture_actions()),
        ["copy", "create-dir", "symlink"]
    );
}

#[test]
fn a_document_that_has_fallen_behind_the_enum_is_caught() {
    let built = implemented_action_types(fixture_actions());
    // The shape the README uses today, and what slice 1 owes it.
    let behind = "**Implemented so far: `symlink`.**\n";
    let current = "Implemented so far: `symlink`, `create-dir`, and `copy`.\n";
    assert_ne!(documented_action_types(behind).as_ref(), Some(&built));
    assert_eq!(documented_action_types(current), Some(built));
}

#[test]
fn a_document_that_stopped_naming_them_is_not_silently_passed() {
    assert_eq!(documented_action_types("# Batfiles\n"), None);
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
    // The fixture has no step 0.9.
    let mentions = carry_mentions("// CARRY(0.9): an undefined step\n");
    assert!(
        spent_reason(&mentions[0], &fixture_steps()).is_some(),
        "{mentions:?}"
    );
}

#[test]
fn an_entry_is_found_however_rustfmt_wrapped_it() {
    // Copied from a real entry long enough for rustfmt to break it up, and kept
    // pointing at a step that is still open so that grepping for a done one
    // does not land here.
    let wrapped = "        (\n\
                   \x20           !options.disable_actions.is_empty(),\n\
                   \x20           \"--disable-action\",\n\
                   \x20           \"8.3\",\n\
                   \x20       ),\n";
    assert_eq!(step_literals(wrapped), [(4, "8.3".to_string())]);
}

#[test]
fn an_entry_is_live_while_the_step_that_frees_its_option_is_open() {
    let steps = fixture_steps();
    assert_eq!(live_step_reason("1.3", &steps), None);
    // The fixture completes 0.13 and does not define 0.9.
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
