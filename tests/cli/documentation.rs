//! The user guides' TOML examples exercised through the binary on isolated roots.

use std::fs;

#[cfg(unix)]
use super::support::{Member, Reply, Server, tarball};
use super::support::{Tree, backup_of, stderr_of};

/// The document's TOML examples, in order. Tests select them by position, so a guide gaining or
/// losing an example fails here until its test is reviewed.
fn toml_blocks(document: &str, expected: usize) -> Vec<&str> {
    let blocks: Vec<&str> = document
        .split("```toml\n")
        .skip(1)
        .map(|rest| rest.split("```").next().expect("the block's contents"))
        .collect();
    assert_eq!(blocks.len(), expected, "the guide's TOML examples changed");
    blocks
}

#[test]
fn the_first_repository_tutorial_previews_installs_keeps_and_refreshes() {
    let guide = toml_blocks(include_str!("../../docs/getting-started.md"), 3);
    let tree = Tree::roots();
    fs::create_dir(tree.path("repo")).expect("a new tutorial checkout");
    tree.batfiles()
        .current_dir(tree.path("repo"))
        .env("HOME", tree.path("home"))
        .env("USERPROFILE", tree.path("home"))
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .arg("init")
        .assert()
        .success();
    let initial = guide[0];
    tree.repo_file("files/editor.toml", initial);
    tree.write_manifest(guide[1]);
    let dest = tree.home(".config/batfiles-demo/editor.toml");

    let preview = tree
        .batfiles()
        .args(["sync", "--dry-run"])
        .assert()
        .success();
    assert!(stderr_of(&preview).contains("would copy"));
    assert!(!dest.exists());
    tree.batfiles().arg("sync").assert().success();
    assert_eq!(
        fs::read_to_string(&dest).expect("installed content"),
        initial
    );
    let repeated = tree.batfiles().arg("sync").assert().success();
    assert!(stderr_of(&repeated).is_empty());

    let changed = "theme = \"light\"\n";
    tree.repo_file("files/editor.toml", changed);
    tree.batfiles().arg("sync").assert().success();
    assert_eq!(fs::read_to_string(&dest).expect("kept content"), initial);
    tree.batfiles()
        .args(["apply-action", "--id", "editor", "--refresh-content"])
        .assert()
        .success();
    assert_eq!(
        fs::read_to_string(&dest).expect("refreshed content"),
        changed
    );
    assert_eq!(
        fs::read_to_string(backup_of(&dest)).expect("backup"),
        initial
    );
}

#[test]
fn the_machine_guide_selects_work_and_adopts_bootstrap_choices() {
    let guide = toml_blocks(include_str!("../../docs/guides/machines.md"), 5);
    let tree = Tree::new();
    tree.write_manifest(&format!("{}\n{}", guide[0], guide[1]));
    tree.batfiles().arg("sync").assert().success();
    assert!(!tree.home("work-notes").exists());
    tree.batfiles()
        .args(["vars", "set", "work", "true"])
        .assert()
        .success();
    tree.batfiles()
        .args(["sync", "--var", "work=false"])
        .assert()
        .success();
    assert!(!tree.home("work-notes").exists());
    tree.batfiles().arg("sync").assert().success();
    assert!(tree.home("work-notes").is_dir());
    tree.batfiles()
        .args(["vars", "unset", "work"])
        .assert()
        .success();
    tree.batfiles().arg("sync").assert().success();
    assert!(tree.home("work-notes").is_dir());

    tree.write_manifest(&format!("{}\n{}", guide[0], guide[3]));
    tree.batfiles().arg("sync").assert().success();
    assert!(
        tree.home(&format!(".cache/my-tools-{}", std::env::consts::OS))
            .is_dir()
    );

    tree.write_manifest(&format!("{}\n{}\n{}", guide[0], guide[1], guide[4]));
    tree.batfiles()
        .args(["sync", "--bootstrap"])
        .assert()
        .success();
    assert!(tree.disabled_document().contains("work"));
    tree.batfiles()
        .args(["sync", "--bootstrap", "--enable-group", "work"])
        .assert()
        .success();
    assert!(!tree.disabled_document().contains("work"));
}

#[test]
fn the_content_guide_examples_preview_without_network_access() {
    let guide = toml_blocks(include_str!("../../docs/guides/content.md"), 5);
    for example in guide {
        let tree = Tree::new();
        tree.repo_file("files/editor.toml", "theme = \"dark\"\n");
        tree.write_manifest(example);
        tree.batfiles()
            .args(["sync", "--dry-run"])
            .assert()
            .success();
    }
}

#[test]
fn the_composition_guide_uses_remote_sources_and_reports_missing_inclusions() {
    let guide = toml_blocks(include_str!("../../docs/guides/composition.md"), 4);
    let tree = Tree::new();
    tree.repo_file("remotes/shared/files/editor.toml", "theme = \"dark\"\n");
    tree.write_manifest(&format!("{}\n{}", guide[0], guide[1]));
    tree.batfiles()
        .args(["sync", "--dry-run"])
        .assert()
        .success();

    let tree = Tree::new();
    tree.write_manifest(&format!("{}\n{}", guide[0], guide[2]));
    let partial = tree
        .batfiles()
        .args(["sync", "--dry-run"])
        .assert()
        .success();
    assert!(stderr_of(&partial).contains("not materialized"));
    tree.write_manifest(guide[3]);
    let gated = tree
        .batfiles()
        .args(["sync", "--dry-run"])
        .assert()
        .success();
    assert!(!stderr_of(&gated).contains("not materialized"));
}

/// The releases guide's example, run against a loopback server standing in for github.com, through
/// a first install, a sync with nothing new, and an upgrade. Its symlink needs Unix.
#[cfg(unix)]
#[test]
fn the_releases_guide_installs_a_tool_and_follows_its_releases() {
    let guide = toml_blocks(include_str!("../../docs/guides/releases.md"), 2);
    let latest = "/junegunn/fzf/releases/latest";
    let release = |tag: &str| Reply::Redirect {
        status: 302,
        location: format!("/junegunn/fzf/releases/tag/{tag}"),
    };
    let build = |version: &str| {
        let script = match version {
            "0.65.2" => "#!/bin/sh\necho '# fzf 0.65.2 integration'\n",
            _ => "#!/bin/sh\necho '# fzf 0.66.0 integration'\n",
        };
        Reply::Bytes(tarball(&[Member::File("fzf", 0o755, script)]))
    };
    let mut routes = vec![(latest.to_owned(), release("v0.65.2"))];
    for (version, arch) in [
        ("0.65.2", "amd64"),
        ("0.65.2", "arm64"),
        ("0.66.0", "amd64"),
        ("0.66.0", "arm64"),
    ] {
        routes.push((
            format!("/junegunn/fzf/releases/download/v{version}/fzf-{version}-linux_{arch}.tar.gz"),
            build(version),
        ));
    }
    let server = Server::serving(routes);
    let declaration = "github-release = \"junegunn/fzf\"";
    assert!(guide[0].contains(declaration));
    let manifest = guide[0]
        .replace("https://github.com", server.address())
        .replace(
            declaration,
            &format!("{declaration}\ngithub-url = \"{}\"", server.address()),
        );

    let tree = Tree::new();
    tree.write_manifest(&manifest);
    let sync = |args: &[&str]| {
        let assertion = tree
            .batfiles()
            .env("HOME", tree.path("home"))
            .arg("sync")
            .args(args)
            .assert()
            .success();
        stderr_of(&assertion)
    };
    let integration = tree.home(".config/fzf/init.zsh");
    let stderr = sync(&[]);
    assert!(tree.home(".local/bin/fzf").is_symlink(), "{stderr}");
    assert_eq!(
        fs::read_to_string(&integration).expect("the shell integration"),
        "# fzf 0.65.2 integration\n"
    );

    fs::remove_file(&integration).expect("the shell integration");
    sync(&["--refresh-vars"]);
    assert!(!integration.exists(), "fzf-shell ran with nothing new");

    server.answer(latest, release("v0.66.0"));
    sync(&["--refresh-vars"]);
    assert_eq!(
        fs::read_to_string(&integration).expect("the shell integration"),
        "# fzf 0.66.0 integration\n"
    );

    let tree = Tree::new();
    tree.write_manifest(guide[1]);
    tree.batfiles()
        .args(["sync", "--dry-run"])
        .assert()
        .success();
}
