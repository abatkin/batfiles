//! CLI tests for the run lock under the cache directory: which commands take it, the refusal a
//! held lock produces, and a lock that cannot be created.

use crate::support::*;
use std::fs::{self, File, OpenOptions};
use std::path::PathBuf;

/// The lock file a tree's commands take.
fn lock_path(tree: &Tree) -> PathBuf {
    tree.path("cache").join("run.lock")
}

/// Hold the tree's run lock, as another batfiles run would, until the file is dropped.
fn hold_lock(tree: &Tree) -> File {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path(tree))
        .expect("the lock file");
    file.try_lock().expect("the lock, which no run holds yet");
    file
}

/// What a command refused by a held lock prints.
fn refusal(tree: &Tree) -> String {
    format!(
        "error: another batfiles run holds {}; try again once it finishes\n",
        display(&lock_path(tree))
    )
}

#[test]
fn a_held_lock_refuses_a_sync_until_it_is_released() {
    let tree = Tree::new();
    tree.write_manifest(&one_create_dir("~/.config"));

    let held = hold_lock(&tree);
    let assertion = tree.batfiles().arg("sync").assert().code(1);
    assert_eq!(stderr_of(&assertion), refusal(&tree));
    assert!(
        !tree.home(".config").exists(),
        "a refused run installs nothing"
    );

    drop(held);
    tree.batfiles().arg("sync").assert().success();
    assert!(tree.home(".config").is_dir());
    assert!(lock_path(&tree).exists(), "the lock file is left in place");
}

#[test]
fn every_state_writer_is_refused_before_it_reads_anything() {
    let tree = Tree::new();
    let _held = hold_lock(&tree);

    let commands: [&[&str]; 13] = [
        &["sync", "--dry-run"],
        &["apply-action", "--id", "anything"],
        &["apply-group", "--group", "anything"],
        &["disable-action", "zshrc"],
        &["enable-action", "zshrc"],
        &["disable-group", "work"],
        &["enable-group", "work"],
        &["vars", "set", "editor", "vim"],
        &["vars", "unset", "editor"],
        &["vars", "refresh"],
        &["vars", "list"],
        &["vars", "refresh", "anything"],
        &["vars", "eval", "${facts.os}"],
    ];
    for args in commands {
        let assertion = tree.batfiles().args(args).assert().code(1);
        assert_eq!(stderr_of(&assertion), refusal(&tree), "batfiles {args:?}");
    }
    assert!(!tree.disabled().exists());
    assert!(!tree.machine_vars().exists());
}

/// A bare origin whose manifest creates `~/.config`.
fn origin() -> BareRepo {
    let origin = BareRepo::new();
    origin.publish("batfiles.toml", &one_create_dir("~/.config"), "a manifest");
    origin
}

#[test]
fn a_clone_is_kept_but_not_synchronized_while_the_lock_is_held() {
    let tree = Tree::roots();
    let origin = origin();
    let _held = hold_lock(&tree);

    let assertion = tree
        .batfiles()
        .args(["clone", &display(&origin.origin())])
        .assert()
        .code(1);
    assert!(stderr_of(&assertion).ends_with(&refusal(&tree)));
    assert!(
        tree.path("repo/batfiles.toml").is_file(),
        "the clone is kept"
    );
    assert!(!tree.home(".config").exists(), "nothing is installed");
    assert!(!tree.disabled().exists(), "no bootstrap state is adopted");
}

#[test]
fn a_clone_succeeds_with_its_cache_inside_the_new_repository() {
    let tree = Tree::roots();
    let origin = origin();
    let cache = tree.path("repo/.cache");

    tree.batfiles()
        .env("BATFILES_CACHE_DIR", &cache)
        .args(["clone", &display(&origin.origin())])
        .assert()
        .success();
    assert!(tree.path("repo/batfiles.toml").is_file());
    assert!(tree.home(".config").is_dir());
    assert!(cache.join("run.lock").is_file());
}

#[test]
fn readers_run_while_the_lock_is_held() {
    let tree = Tree::new();
    tree.batfiles()
        .args(["vars", "set", "editor", "vim"])
        .assert()
        .success();
    let _held = hold_lock(&tree);

    let assertion = tree
        .batfiles()
        .args(["vars", "get", "editor"])
        .assert()
        .success();
    assert_eq!(stdout_of(&assertion), "vim\n");
    for args in [
        &["vars", "list", "--machine-only"][..],
        &["vars", "list", "--no-refresh"],
        &["vars", "eval", "--no-refresh", "${facts.os}"],
        &["vars", "eval", "--no-manifest-vars", "${editor}"],
        &["facts", "list"],
        &["version"],
    ] {
        tree.batfiles().args(args).assert().success();
    }
}

#[test]
fn a_missing_cache_directory_is_created_to_hold_the_lock() {
    let tree = Tree::new();
    let cache = tree.path("absent/cache");
    tree.batfiles()
        .env("BATFILES_CACHE_DIR", &cache)
        .args(["disable-action", "zshrc"])
        .assert()
        .success();

    let lock = fs::metadata(cache.join("run.lock")).expect("the lock file");
    assert_eq!(lock.len(), 0, "the lock file holds nothing");
}

#[test]
fn a_lock_that_cannot_be_created_fails_the_run() {
    let tree = Tree::new();
    let file = tree.home("not-a-directory");
    fs::write(&file, "").expect("a file");
    let cache = file.join("cache");

    let assertion = tree
        .batfiles()
        .env("BATFILES_CACHE_DIR", &cache)
        .args(["disable-action", "zshrc"])
        .assert()
        .code(1);
    let stderr = stderr_of(&assertion);
    let expected = format!(
        "error: could not take the run lock {}: ",
        display(&cache.join("run.lock"))
    );
    assert!(stderr.starts_with(&expected), "{stderr}");
    assert!(!tree.disabled().exists());
}

/// A `vars eval` that may refresh holds the lock until it has printed its value: while its
/// standard output, larger than a pipe holds, is still being written, another run is refused.
#[test]
#[cfg(unix)]
fn an_eval_that_may_refresh_holds_the_lock_until_it_has_printed() {
    use std::io::Read;
    use std::process::{Command, Stdio};

    let tree = Tree::new();
    let big = "x".repeat(100_000);
    let mut child = Command::new(assert_cmd::cargo::cargo_bin("batfiles"))
        .args(["vars", "eval", "${env.BATFILES_TEST_BIG}"])
        .env("BATFILES_TEST_BIG", &big)
        .env("BATFILES_HOME", tree.path("home"))
        .env("BATFILES_DIR", tree.path("repo"))
        .env("BATFILES_CONFIG_DIR", tree.path("config"))
        .env("BATFILES_CACHE_DIR", tree.path("cache"))
        .env("BATFILES_STATE_DIR", tree.path("state"))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("batfiles starts");
    let mut stdout = child.stdout.take().expect("a piped standard output");

    // The first byte means the value is being written, and the rest cannot fit in the pipe.
    let mut first = [0_u8; 1];
    stdout.read_exact(&mut first).expect("the value begins");
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(lock_path(&tree))
        .expect("the lock file");
    assert!(
        file.try_lock().is_err(),
        "the lock was released before the value was printed"
    );

    let mut rest = String::new();
    stdout
        .read_to_string(&mut rest)
        .expect("the rest of the value");
    assert!(child.wait().expect("batfiles exits").success());
    assert_eq!(
        rest.len(),
        big.len(),
        "the rest of the value, then its newline"
    );
}
