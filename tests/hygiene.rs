//! Source-hygiene checks that keep `rewrite/guidance.md`'s dead-code rule
//! mechanical rather than honor-system.
//!
//! The scan is textual and line-oriented, so an attribute split across lines is
//! not seen. That is not worth an attribute parser.

use std::fs;
use std::path::{Path, PathBuf};

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
