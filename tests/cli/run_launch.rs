//! CLI tests for how a `run` action launches its command on every platform: where a relative
//! program is found, and how a string reaches the platform shell.

use std::fs;

use crate::support::*;

#[test]
fn a_relative_program_with_a_directory_is_found_from_the_working_directory() {
    let tree = Tree::new();
    let program = format!("bin/tool{}", std::env::consts::EXE_SUFFIX);
    fs::create_dir_all(tree.path("repo/bin")).expect("a bin directory");
    fs::copy(
        assert_cmd::cargo::cargo_bin("batfiles"),
        tree.path(&format!("repo/{program}")),
    )
    .expect("a program in the repository");
    tree.write_manifest(&format!(
        "[[actions]]\ntype = \"run\"\ncommand = [\"./{program}\", \"version\"]\n"
    ));

    // The test runs batfiles from the package root, which holds no `bin/tool`.
    let assertion = tree.batfiles().arg("sync").assert().success();
    assert!(
        stdout_of(&assertion).starts_with("batfiles "),
        "{}",
        stdout_of(&assertion)
    );
}

#[cfg(windows)]
#[test]
fn a_string_command_reaches_cmd_with_its_quotes_intact() {
    let tree = Tree::new();
    tree.repo_file("file with spaces.txt", "quoted\r\n");
    tree.write_manifest(
        "[[actions]]\ntype = \"run\"\ncommand = 'type \"file with spaces.txt\" > copied.txt'\n",
    );
    tree.batfiles().arg("sync").assert().success();
    assert_eq!(
        fs::read_to_string(tree.path("repo/copied.txt")).expect("the copy"),
        "quoted\r\n"
    );
}
