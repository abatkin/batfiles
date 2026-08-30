//! Source-hygiene checks that keep five of `rewrite/guidance.md`'s rules
//! mechanical rather than honor-system: rule 1's dead-code annotations, the
//! `CARRY` markers of "Carrying work forward", rule 12's list of options that
//! parse but are not honored yet, "Two lists, and why they are not the same
//! one" — which modules may touch the filesystem at all — and the definition of
//! done's requirement that the documents keep saying what the binary actually
//! does.
//!
//! The scans are textual and line-oriented, so an attribute or a marker split
//! across lines is not seen. None of it is worth a parser.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// The step list a `CARRY` marker or a withheld option is cleared by.
const STEPS: &str = "rewrite/steps.md";

/// Rule 12's list, and the only file whose step literals are checked.
const UNSUPPORTED: &str = "src/cli/unsupported.rs";

/// The enum that decides which action types a manifest may declare.
const ACTIONS: &str = "src/manifest/action.rs";

/// The documents that answer "what can `sync` actually do", each on a line
/// beginning [`IMPLEMENTED`].
const ACTION_TYPE_DOCS: [&str; 2] = ["README.md", "docs/goals.md"];

/// What that line starts with, in both documents.
const IMPLEMENTED: &str = "Implemented so far:";

/// The fixture the dry-run tests drive over a whole repository. One test covers
/// every action type through it, which holds only as long as it declares every
/// action type.
const LEAF_MANIFEST: &str = "tests/fixtures/leaf/batfiles.toml";

/// This file, which the marker scan skips: its fixtures spell out the forms the
/// check rejects, so scanning it would report its own examples.
const CHECKER: &str = "tests/hygiene.rs";

/// Why a module is allowed to name the filesystem (`guidance.md`, "Two lists,
/// and why they are not the same one"). An addition that is none of these is
/// the bug this check exists to catch.
#[derive(Debug, Clone, Copy)]
enum Kind {
    /// Inspects and never writes, so there is no work for a mode to withhold.
    /// The safest kind, and the one to reach for first.
    ReadOnly,
    /// Performs part of an action's work, and therefore consults `RunMode` itself.
    ModeReader,
    /// Produces content that only ever lands inside something a mode reader
    /// created, so it never sees `RunMode` and does not need to.
    #[expect(
        dead_code,
        reason = "4.1 adds the first: action/copy.rs takes install.rs's fillers"
    )]
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
///
/// The list is meant to grow. Growing it means editing this file, which is what
/// makes saying which [`Kind`] you are adding unavoidable.
const FILESYSTEM_OWNERS: [Owner; 5] = [
    Owner {
        path: "src/paths.rs",
        kind: Kind::ReadOnly,
        reason: "what a path means, and what is already at one",
    },
    Owner {
        path: "src/directory.rs",
        kind: Kind::ModeReader,
        reason: "the only place a directory is made",
    },
    Owner {
        path: "src/install.rs",
        kind: Kind::ModeReader,
        reason: "rule 15's staging, publication, and discard",
    },
    Owner {
        path: "src/action/symlink.rs",
        kind: Kind::ModeReader,
        reason: "the one platform-specific call in the crate",
    },
    Owner {
        path: "src/tomlfile.rs",
        kind: Kind::Bookkeeping,
        reason: "reads the documents batfiles parses; 3.3 adds the writer",
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
///
/// `None` where the document has no such line at all, which is a failure rather
/// than an empty answer: a document that stops naming them stops being checked.
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
    let live = r#"#[expect(dead_code, reason = "5.6 gates on conditions")]"#;
    assert!(dead_code_violations(live).is_empty(), "{live}");

    for source in [
        "#[expect(dead_code)]",
        r#"#[expect(dead_code, reason = "")]"#,
    ] {
        assert_eq!(dead_code_violations(source).len(), 1, "{source}");
    }
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
         consult `Mode` or sit downstream of something that does. If this file genuinely \
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
fn the_filesystem_is_found_however_it_is_named() {
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
fn the_documents_name_every_action_type_that_exists() {
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
fn the_leaf_fixture_declares_every_action_type_that_exists() {
    let source = fs::read_to_string(crate_dir().join(ACTIONS))
        .unwrap_or_else(|error| panic!("{ACTIONS} declares the action types: {error}"));
    let manifest = fs::read_to_string(crate_dir().join(LEAF_MANIFEST))
        .unwrap_or_else(|error| panic!("{LEAF_MANIFEST} is the fixture that covers them: {error}"));
    assert_eq!(
        declared_action_types(&manifest),
        implemented_action_types(&source),
        "{LEAF_MANIFEST} does not declare every action type in {ACTIONS}, so the dry-run \
         tests cover fewer of them than they appear to"
    );
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
    // The shape both documents use today, and what slice 1 owes them.
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
