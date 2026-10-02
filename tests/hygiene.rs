//! Repository checks for dead-code annotations, carry markers, filesystem ownership, and action
//! inventories. Text scans and TOML parsing check declared structure, not Rust semantics or
//! behavioral coverage.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

/// Roadmap defining valid step numbers and completion status.
const STEPS: &str = "docs/future/roadmap.md";

/// Unsupported-option declarations whose step literals are checked. Absent when all parsed
/// options are implemented.
const UNSUPPORTED: &str = "src/cli/unsupported.rs";

/// The enum that decides which action types a manifest may declare.
const ACTIONS: &str = "src/manifest/action.rs";

/// Documents listing implemented action types on an [`IMPLEMENTED`] line.
const ACTION_TYPE_DOCS: [&str; 1] = ["README.md"];

/// What that line starts with.
const IMPLEMENTED: &str = "Implemented so far:";

/// Fixture repositories checked for complete action-type coverage.
const FIXTURES: &str = "tests/fixtures";

/// This checker file, excluded from marker scans because it contains invalid-marker fixtures.
const CHECKER: &str = "tests/hygiene.rs";

/// Declared filesystem role for review. The scanner does not verify the role.
#[derive(Debug, Clone, Copy)]
enum Kind {
    /// Inspects the filesystem without writing.
    ReadOnly,
    /// Checks `RunMode` before performing action writes.
    ModeReader,
    /// Writes content only within nodes created by a mode-checking caller.
    Downstream,
    /// Maintains state or resolves inputs in both run modes.
    Bookkeeping,
    /// Performs writes for a standalone command without a dry-run mode.
    Standalone,
}

impl Kind {
    fn label(self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only, so it never writes",
            Self::ModeReader => "a mode reader",
            Self::Downstream => "downstream of a mode reader",
            Self::Bookkeeping => "bookkeeping, which runs in both modes",
            Self::Standalone => "a command of its own, with no run mode",
        }
    }
}

/// A module registered for filesystem access, with its declared role.
#[derive(Debug)]
struct Owner {
    path: &'static str,
    kind: Kind,
    reason: &'static str,
}

/// The modules that own filesystem access. Every other module under `src/` is
/// forbidden from *naming* `std::fs`, a platform `fs` module, or
/// `std::process::Command`.
const FILESYSTEM_OWNERS: [Owner; 14] = [
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
        reason: "writes a download or a file:// source into a staging file install.rs opened, and widens its mode",
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
        reason: "creates, publishes, compares, and cleans up seed staging nodes, and swaps rebuilt remote materializations into place",
    },
    Owner {
        path: "src/replace.rs",
        kind: Kind::ModeReader,
        reason: "renames a node in a destination's way to a backup or aside, and puts it back or removes it",
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
        path: "src/dynamic/run.rs",
        kind: Kind::Bookkeeping,
        reason: "runs dynamic-variable commands: arbitrary, unsandboxed subprocesses as the \
                 invoking user, in both modes and whatever they do",
    },
    Owner {
        path: "src/tomlfile.rs",
        kind: Kind::Bookkeeping,
        reason: "reads, atomically rewrites, and removes the documents batfiles owns",
    },
    Owner {
        path: "src/init.rs",
        kind: Kind::Standalone,
        reason: "lays the leaf-repository skeleton into the directory `init` was run in",
    },
    Owner {
        path: "src/update.rs",
        kind: Kind::Standalone,
        reason: "stages a release beside the running executable, runs its `version`, and renames \
                 it over that executable",
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

/// Maximum line distance searched around `dead_code` to find its enclosing attribute.
const ATTRIBUTE_LINES: usize = 6;

/// Every `dead_code` annotation in `source`.
fn dead_code_mentions(source: &str) -> Vec<DeadCode> {
    let lines: Vec<&str> = source.lines().collect();
    let mut found = Vec::new();
    for (index, text) in lines.iter().enumerate() {
        if !text.contains("dead_code") {
            continue;
        }
        // Ignore mentions outside attributes.
        let Some(attribute) = attribute_around(&lines, index) else {
            continue;
        };
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
    // Do not count an attribute that closed before the token.
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

/// Return the rule violation for a dead-code annotation, or `None` if valid.
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

/// Return recognized filesystem/process references with their line numbers.
fn filesystem_mentions(source: &str) -> Vec<(usize, &'static str)> {
    let mut found = Vec::new();
    for (index, line) in source.lines().enumerate() {
        let packed: String = line.chars().filter(|c| !c.is_whitespace()).collect();
        if packed.contains("std::fs") {
            found.push((index + 1, "std::fs"));
        }
        // Allow platform-specific modules that do not access the filesystem.
        if packed
            .split_once("std::os::")
            .is_some_and(|(_, rest)| rest.contains("::fs"))
        {
            found.push((index + 1, "a platform `fs` module"));
        }
        // Also recognize single-line braced imports.
        let braced = packed.split_once("process::{").is_some_and(|(_, rest)| {
            rest.split('}')
                .next()
                .unwrap_or_default()
                .split(',')
                .any(|name| name == "Command" || name.starts_with("Commandas"))
        });
        if packed.contains("process::Command") || braced {
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

/// Format the registered filesystem owners for a failure diagnostic.
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
    /// A malformed `CARRY` marker.
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

/// Why a referenced step cannot own outstanding work.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Spent {
    /// The roadmap defines no such step.
    Undefined,
    /// The step is marked complete.
    Done,
}

/// Return whether a step is undefined or complete, or `None` if still open.
fn step_state(step: &str, steps: &BTreeMap<String, bool>) -> Option<Spent> {
    match steps.get(step) {
        None => Some(Spent::Undefined),
        Some(true) => Some(Spent::Done),
        Some(false) => None,
    }
}

/// Return the carry-marker violation, or `None` for a valid marker naming an open step.
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
            // Read the contents of each quoted string.
            line.split('"')
                .skip(1)
                .step_by(2)
                .filter(|literal| is_step(literal))
                .map(move |literal| (index + 1, literal.to_string()))
        })
        .collect()
}

/// Return a diagnostic if an unsupported option names an undefined or completed step.
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
    // Extract names between backticks.
    let mut types: Vec<String> = named
        .split('`')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect();
    types.sort();
    Some(types)
}

/// Parse the distinct action types in the top-level `actions` array, sorted by name. Missing
/// actions yield an empty list; malformed TOML, non-array actions, and entries without string
/// types fail.
fn declared_action_types(manifest: &str) -> Result<Vec<String>, NotAnInventory> {
    let document: toml::Value = toml::from_str(manifest)?;
    let Some(actions) = document.get("actions") else {
        return Ok(Vec::new());
    };
    let actions = actions.as_array().ok_or(NotAnInventory::NotAList)?;

    let mut types: Vec<String> = Vec::new();
    for (index, action) in actions.iter().enumerate() {
        let position = index + 1;
        let kind = action
            .get("type")
            .and_then(toml::Value::as_str)
            .ok_or(NotAnInventory::Untyped { position })?;
        types.push(kind.to_owned());
    }
    types.sort();
    types.dedup();
    Ok(types)
}

/// Failures while reading a fixture's action-type inventory.
#[derive(Debug, Error)]
enum NotAnInventory {
    /// Invalid TOML, with the parser's diagnostic.
    #[error("is not TOML: {0}")]
    Toml(#[from] toml::de::Error),

    /// The `actions` value is not an array.
    #[error("writes an `actions` that is not an array of actions")]
    NotAList,

    /// An action entry is not a table with a string `type` field.
    #[error("writes an action at position {position} with no string `type`")]
    Untyped { position: usize },
}

/// Read the roadmap as a map from step numbers to completion status.
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

    // Without annotations, the check does not need a roadmap.
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
    assert_eq!(dead_code_reason(&mentions[0], &fixture_steps()), None);
}

#[test]
fn a_step_is_found_in_a_reason_however_it_is_punctuated() {
    // Recognize step references with punctuation, but ignore version numbers.
    assert_eq!(
        reason_steps("adopted at 8.3, by the bootstrap that reads it"),
        ["8.3"]
    );
    assert_eq!(reason_steps("the caller lands at 4.5."), ["4.5"]);
    assert!(reason_steps("wanted by proc-macro2 1.0.107").is_empty());
}

#[test]
fn a_dead_code_outside_an_attribute_is_not_an_annotation() {
    let prose = "//! The text `allow(dead_code)` in prose is not an annotation.\n";
    assert!(dead_code_mentions(prose).is_empty(), "{prose}");

    // Prose below a closed attribute must not count as a second annotation.
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
    // Catch inventory entries left behind after a file is renamed.
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
            "use std::process::{Child, Command, Stdio};",
            "std::process::Command",
        ),
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
        "use std::process::ExitCode;",
        "use std::process::{Child, ExitStatus};",
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

    // Without carry markers, the check does not need a roadmap.
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
    let source = match fs::read_to_string(crate_dir().join(UNSUPPORTED)) {
        Ok(source) => source,
        // A missing unsupported-options file means no options are withheld.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => panic!("{UNSUPPORTED} holds rule 12's list: {error}"),
    };
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
        let types = declared_action_types(&document).unwrap_or_else(|error| {
            panic!(
                "{} is a fixture manifest this check reads: it {error}",
                display(&manifest)
            )
        });
        declared.extend(types);
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

/// The inventory of a manifest this check expects to be able to read.
fn inventory(manifest: &str) -> Vec<String> {
    declared_action_types(manifest).expect("a manifest whose action types are readable")
}

/// Why a manifest has no inventory, for a case asserting that it has none.
fn not_an_inventory(manifest: &str) -> String {
    match declared_action_types(manifest) {
        Ok(types) => panic!("this manifest was read as declaring {types:?}"),
        Err(error) => error.to_string(),
    }
}

#[test]
fn a_manifests_action_types_are_read_once_each() {
    let manifest = r#"[[actions]]
type = "symlink"
source = "shell/zshrc"

[[actions]]
type = "copy"

[[actions]]
type = "symlink"
"#;
    assert_eq!(inventory(manifest), ["copy", "symlink"]);
}

#[test]
fn one_action_list_reads_the_same_however_its_toml_is_written() {
    for spelling in [
        "[[actions]]\ntype = \"symlink\"\n\n[[actions]]\ntype = \"copy\"\n",
        "[[actions]]\ntype = 'symlink'\n\n[[actions]]\ntype = 'copy'\n",
        "[[actions]]\ntype='symlink'\n\n[[actions]]\ntype    =   \"copy\"\n",
        "# what this repository installs\n\
         [[actions]]\n\
         type = \"symlink\" # the shell\n\n\
         [[actions]]\n\
         type = \"copy\"\n",
        "actions = [{ type = \"symlink\" }, { type = \"copy\" }]\n",
    ] {
        assert_eq!(
            inventory(spelling),
            ["copy", "symlink"],
            "this spelling was read as a different list:\n{spelling}"
        );
    }
}

#[test]
fn a_type_outside_the_action_list_is_not_an_action_type() {
    let manifest = r#"[remotes.corporate]
type = "git"
url = "https://example.invalid/corp.git"

[[actions]]
type = "include-remote"
remote = "corporate"
vars = { type = "not-an-action" }

[[actions]]
type = "symlink"

[actions.source]
type = "still-not-an-action"
"#;
    assert_eq!(inventory(manifest), ["include-remote", "symlink"]);
}

#[test]
fn a_manifest_declaring_no_actions_declares_no_action_types() {
    assert!(inventory("[vars]\nwork = \"false\"\n").is_empty());
    assert!(inventory("").is_empty());
    assert!(inventory("actions = []\n").is_empty());
}

#[test]
fn a_manifest_this_check_cannot_read_fails_rather_than_reading_nothing() {
    for (manifest, expected) in [
        ("[[actions]\ntype = \"symlink\"\n", "is not TOML"),
        ("actions = \"symlink\"\n", "not an array"),
        ("[[actions]]\nid = \"zshrc\"\n", "position 1"),
        (
            "[[actions]]\ntype = \"symlink\"\n\n[[actions]]\ntype = 7\n",
            "position 2",
        ),
        ("actions = [\"symlink\"]\n", "position 1"),
    ] {
        let reason = not_an_inventory(manifest);
        assert!(
            reason.contains(expected),
            "`{expected}` is not why this manifest has no inventory: {reason}"
        );
    }
}

/// Return a sample action enum with comments and attributes for scanner tests.
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
    // Step literals must be recognized in entries split across lines.
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
