//! Repository checks for `dead_code` suppression, filesystem ownership, and action inventories.
//! Text scans and TOML parsing check declared structure, not Rust semantics or behavioral coverage.

use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

/// The enum that decides which action types a manifest may declare.
const ACTIONS: &str = "src/manifest/action.rs";

/// Documents listing implemented action types on an [`IMPLEMENTED`] line.
const ACTION_TYPE_DOCS: [&str; 1] = ["README.md"];

/// What that line starts with.
const IMPLEMENTED: &str = "Supported actions:";

/// Fixture repositories checked for complete action-type coverage.
const FIXTURES: &str = "tests/fixtures";

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
const FILESYSTEM_OWNERS: [Owner; 15] = [
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
        reason: "writes a download or a file:// source, or what it decompresses to, into a staging file install.rs opened, and widens its mode",
    },
    Owner {
        path: "src/archive/mod.rs",
        kind: Kind::Downstream,
        reason: "unpacks a downloaded archive into a staging tree, or decompresses a download into a staging file, that install.rs made",
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
        path: "src/run_lock.rs",
        kind: Kind::Bookkeeping,
        reason: "creates and locks the empty run lock under the cache directory, in both modes",
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

#[test]
fn no_source_suppresses_dead_code() {
    let checker = crate_dir().join(file!());
    let mut failures = Vec::new();
    for path in rust_sources("src").into_iter().chain(rust_sources("tests")) {
        if path == checker {
            continue;
        }
        let source = fs::read_to_string(&path).expect("a readable source file");
        for (index, line) in source.lines().enumerate() {
            if line.contains("dead_code") {
                failures.push(format!("{}:{}: {}", display(&path), index + 1, line.trim()));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "\n{}\n\nRemove the unused code rather than suppressing `dead_code` with `allow` or \
         `expect`; see rule 1 in docs/contributing/architecture.md.\n",
        failures.join("\n")
    );
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
    let behind = "**Supported actions: `symlink`.**\n";
    let current = "Supported actions: `symlink`, `create-dir`, and `copy`.\n";
    assert_ne!(documented_action_types(behind).as_ref(), Some(&built));
    assert_eq!(documented_action_types(current), Some(built));
}

#[test]
fn a_document_that_stopped_naming_them_is_not_silently_passed() {
    assert_eq!(documented_action_types("# Batfiles\n"), None);
}
