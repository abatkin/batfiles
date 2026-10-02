//! What the PowerShell scripts must share with each other and with the release scripts, read as
//! text.

use std::fs;
use std::path::Path;

fn read(relative: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(relative))
        .unwrap_or_else(|error| panic!("{relative}: {error}"))
}

/// The value of the one line in `script` that assigns `prefix`, quotes included.
fn assignment<'a>(script: &'a str, prefix: &str) -> &'a str {
    let mut found = script
        .lines()
        .filter_map(|line| line.trim_start().strip_prefix(prefix));
    let value = found.next().unwrap_or_else(|| panic!("no `{prefix}` line"));
    assert!(found.next().is_none(), "more than one `{prefix}` line");
    value
}

/// The definition of function `name` in `script`, from its `function` line to the line that
/// closes it at the same indentation.
fn function<'a>(script: &'a str, name: &str) -> &'a str {
    let start = ["(", " {"]
        .iter()
        .find_map(|after| script.find(&format!("function {name}{after}")))
        .unwrap_or_else(|| panic!("no function {name}"));
    let line_start = script[..start].rfind('\n').map_or(0, |at| at + 1);
    let indent = &script[line_start..start];
    let close = format!("\n{indent}}}\n");
    let end = script[start..]
        .find(&close)
        .unwrap_or_else(|| panic!("function {name} does not close"));
    &script[start..start + end + close.len()]
}

#[test]
fn the_installer_follows_the_release_grammar() {
    assert_eq!(
        assignment(&read("dist/install.ps1"), "$VersionPattern = "),
        assignment(&read("dist/version.sh"), "version_pattern=")
    );
}

#[test]
fn the_stub_shares_its_version_and_candidate_text_with_the_installer() {
    let installer = read("dist/install.ps1");
    let stub = read("src/stub.ps1");
    assert_eq!(
        assignment(&stub, "$VersionPattern = "),
        assignment(&installer, "$VersionPattern = ")
    );
    for name in ["Test-Version", "Get-BatfilesVersion", "Get-CandidateList"] {
        assert_eq!(function(&stub, name), function(&installer, name), "{name}");
    }
}

#[test]
fn the_installer_is_ascii_with_one_line_to_stamp() {
    let installer = read("dist/install.ps1");
    // Windows PowerShell reads a file without a byte-order mark in the system code page.
    assert!(installer.is_ascii());
    assert!(!installer.contains('\r'));
    assert_eq!(
        installer
            .lines()
            .filter(|line| line.starts_with("$BatfilesStampedBase = "))
            .collect::<Vec<_>>(),
        ["$BatfilesStampedBase = 'unstamped'"]
    );
    // The whole body is a function called on the last line, so a truncated download runs
    // nothing.
    assert_eq!(
        installer.lines().last(),
        Some("Invoke-BatfilesInstaller @args")
    );
}
