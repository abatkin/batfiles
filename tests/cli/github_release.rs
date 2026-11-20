//! CLI tests for `github-release` dynamic variables: the lookup against a loopback server
//! standing in for GitHub, its cache, its failures, and the `github-url` template.

use std::fs;

use jiff::{SignedDuration, Timestamp};

use crate::support::*;

/// The page a lookup of `o/r` asks for.
const LATEST: &str = "/o/r/releases/latest";

/// The redirect GitHub answers a repository's latest release with.
fn found(location: &str) -> Reply {
    Reply::Redirect {
        status: 302,
        location: location.to_owned(),
    }
}

/// A server whose `o/r` has the latest release `tag`, as its page's path spells it.
fn releasing(tag: &str) -> Server {
    Server::serving(vec![(
        LATEST.to_owned(),
        found(&format!("/o/r/releases/tag/{tag}")),
    )])
}

/// A leaf declaring `tag` as `o/r`'s latest release on `host`, with `extra` fields.
fn declaring(host: &str, extra: &str) -> Tree {
    let tree = Tree::new();
    tree.write_manifest(&format!(
        "[vars.tag]\ngithub-release = \"o/r\"\ngithub-url = \"{host}\"\n{extra}\n"
    ));
    tree
}

fn cache(tree: &Tree) -> std::path::PathBuf {
    tree.path("cache").join("dynamic-vars.toml")
}

fn cache_document(tree: &Tree) -> String {
    fs::read_to_string(cache(tree)).expect("dynamic-vars.toml should exist")
}

/// Put `tag`'s entry in the cache, captured `ago` before now, with `from` written as given:
/// an inline table, or nothing.
fn seed(tree: &Tree, value: &str, ago: SignedDuration, from: Option<&str>) {
    let captured = Timestamp::now() - ago;
    let from = from
        .map(|from| format!("from = {from}\n"))
        .unwrap_or_default();
    fs::create_dir_all(tree.path("cache")).expect("the cache root");
    fs::write(
        cache(tree),
        format!("[tag]\nvalue = \"{value}\"\ncaptured-at = \"{captured}\"\n{from}"),
    )
    .expect("a cache document");
}

/// The `from` a lookup of `o/r` on `host` records.
fn from_release(host: &str) -> String {
    format!("{{ github-release = \"o/r\", github-url = \"{host}\" }}")
}

fn two_days() -> SignedDuration {
    SignedDuration::from_hours(48)
}

fn an_hour() -> SignedDuration {
    SignedDuration::from_hours(1)
}

/// `vars list`'s standard output and standard error.
fn list(tree: &Tree, args: &[&str]) -> (String, String) {
    let assertion = tree
        .batfiles()
        .args(["vars", "list"])
        .args(args)
        .assert()
        .success();
    (stdout_of(&assertion), stderr_of(&assertion))
}

fn sync(tree: &Tree, args: &[&str]) -> String {
    let assertion = tree.batfiles().arg("sync").args(args).assert().success();
    stderr_of(&assertion)
}

#[test]
fn the_value_is_the_tag_the_latest_release_redirects_to() {
    for (path, tag) in [
        ("v1.2.3", "v1.2.3"),
        ("release%2F1.0%20rc", "release/1.0 rc"),
        ("team/v2", "team/v2"),
    ] {
        let server = releasing(path);
        let tree = declaring(server.address(), "");
        let (stdout, stderr) = list(&tree, &[]);
        assert_eq!(
            stdout,
            format!("tag = \"{tag}\" (batfiles.toml, github-release)\n"),
            "{stderr}"
        );
        assert_eq!(server.requests(), 1);
        let document = cache_document(&tree);
        assert!(
            document.contains(&format!("from = {}", from_release(server.address()))),
            "{document}"
        );
    }
}

#[test]
fn an_absolute_location_and_a_trailing_slash_on_the_host_are_understood() {
    let server = Server::serving(vec![(
        LATEST.to_owned(),
        Reply::Redirect {
            status: 302,
            location: "https://github.com/o/r/releases/tag/v3?ref=x".to_owned(),
        },
    )]);
    let tree = declaring(&format!("{}/", server.address()), "");
    let (stdout, _) = list(&tree, &[]);
    assert_eq!(stdout, "tag = \"v3\" (batfiles.toml, github-release)\n");
    assert!(
        cache_document(&tree).contains(&from_release(server.address())),
        "the trailing `/` was recorded"
    );
}

#[test]
fn a_fresh_tag_is_reused_and_a_stale_one_is_looked_up_again() {
    let server = releasing("v1");
    let tree = declaring(server.address(), "");
    sync(&tree, &[]);
    let stderr = sync(&tree, &["-vv"]);
    assert_eq!(server.requests(), 1, "a fresh tag was looked up again");
    assert!(
        stderr.contains(r#"tag = "v1" (batfiles.toml, cached just now)"#),
        "{stderr}"
    );

    sync(&tree, &["--refresh-vars"]);
    assert_eq!(server.requests(), 2, "--refresh-vars made no lookup");

    seed(
        &tree,
        "v0",
        two_days(),
        Some(&from_release(server.address())),
    );
    sync(&tree, &[]);
    assert_eq!(server.requests(), 3, "a stale tag was not looked up again");
    assert!(cache_document(&tree).contains("value = \"v1\""));

    seed(
        &tree,
        "v0",
        two_days(),
        Some(&from_release(server.address())),
    );
    let (stdout, _) = list(&tree, &["--no-refresh"]);
    assert_eq!(server.requests(), 3, "--no-refresh made a lookup");
    assert_eq!(
        stdout,
        "tag = \"v0\" (batfiles.toml, stale, cached 2d ago)\n"
    );
}

#[test]
fn a_cache_duration_decides_what_is_fresh() {
    let server = releasing("v1");
    let tree = declaring(server.address(), "cache = \"0s\"");
    sync(&tree, &[]);
    sync(&tree, &[]);
    assert_eq!(server.requests(), 2, "a zero duration is never fresh");
}

/// Each way a lookup fails, with the host that fails it and the reason it gives.
fn failures() -> Vec<(&'static str, Option<Server>, String, String)> {
    let server = |reply: Reply| Server::serving(vec![(LATEST.to_owned(), reply)]);
    let mut cases = Vec::new();
    for (name, server, reason) in [
        (
            "404",
            Server::new(&[]),
            "GitHub has no public repository `o/r`",
        ),
        (
            "no release",
            server(found("https://github.com/o/r/releases")),
            "`o/r` has no published release",
        ),
        (
            "renamed",
            server(Reply::Redirect {
                status: 301,
                location: "https://github.com/n/m/releases/latest".to_owned(),
            }),
            "`o/r` has moved to `https://github.com/n/m/releases/latest`; update \
             `github-release`",
        ),
        (
            "200",
            server(Reply::Body("a page\n")),
            "the release lookup was answered with status 200, not a redirect to a release",
        ),
    ] {
        let host = server.address().to_owned();
        cases.push((name, Some(server), host, reason.to_owned()));
    }
    cases.push((
        "refused",
        None,
        refusing_address(),
        "the release lookup failed".to_owned(),
    ));
    cases.push((
        "timeout",
        None,
        server_that_never_answers(),
        "the release lookup timed out after 300ms".to_owned(),
    ));
    cases
}

#[test]
fn a_failed_lookup_with_nothing_cached_reads_as_empty_and_says_why() {
    for (name, _server, host, reason) in failures() {
        let tree = declaring(&host, "timeout = \"300ms\"");
        let started = std::time::Instant::now();
        let (stdout, stderr) = list(&tree, &[]);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(10),
            "{name}"
        );
        assert_eq!(
            stdout, "tag = no value (batfiles.toml, github-release failed)\n",
            "{name}: {stderr}"
        );
        assert!(
            stderr.contains(&format!(
                "warning: dynamic variable `tag` could not be refreshed: {reason}"
            )),
            "{name}: {stderr}"
        );
        assert!(
            stderr.contains("it has no value, and reads as empty"),
            "{name}: {stderr}"
        );
        assert!(stderr.is_ascii(), "{name}: {stderr}");
        assert!(!cache(&tree).exists(), "{name}: a failure was cached");
    }
}

#[test]
fn a_failed_lookup_keeps_a_cached_tag_and_says_so() {
    for (name, _server, host, reason) in failures() {
        let tree = declaring(&host, "timeout = \"300ms\"");
        seed(&tree, "v0", two_days(), Some(&from_release(&host)));
        let (stdout, stderr) = list(&tree, &[]);
        assert_eq!(
            stdout, "tag = \"v0\" (batfiles.toml, github-release failed, cached 2d ago)\n",
            "{name}: {stderr}"
        );
        assert!(
            stderr.contains(&format!(
                "warning: dynamic variable `tag` could not be refreshed: {reason}"
            )),
            "{name}: {stderr}"
        );
        assert!(
            stderr.contains("using the value cached 2d ago"),
            "{name}: {stderr}"
        );
    }
}

#[test]
fn a_github_url_that_resolves_to_no_url_reads_as_empty_whatever_is_cached() {
    for (value, case) in [(None, "unset"), (Some("ftp://ghe.example"), "not http")] {
        let tree = declaring("${env.BATFILES_TEST_GHE}", "");
        seed(&tree, "v0", an_hour(), None);
        for args in [&[][..], &["--no-refresh"][..]] {
            let mut command = tree.batfiles();
            command.args(["vars", "list"]).args(args);
            match value {
                Some(value) => command.env("BATFILES_TEST_GHE", value),
                None => command.env_remove("BATFILES_TEST_GHE"),
            };
            let assertion = command.assert().success();
            let (stdout, stderr) = (stdout_of(&assertion), stderr_of(&assertion));
            assert_eq!(
                stdout, "tag = no value (batfiles.toml, github-release failed)\n",
                "{case}: {stderr}"
            );
            assert!(
                stderr.contains(
                    "dynamic variable `tag` could not be refreshed: `github-url` \
                     \"${env.BATFILES_TEST_GHE}\" resolves to a value that is not an http:// \
                     or https:// URL; it has no value, and reads as empty"
                ),
                "{case}: {stderr}"
            );
            assert!(!stderr.contains("ghe.example"), "{case}: {stderr}");
        }
    }
}

#[test]
fn a_github_url_reads_the_environment_and_host_facts() {
    let server = releasing("v1");
    let tree = declaring("${env.BATFILES_TEST_GHE}", "");
    let assertion = tree
        .batfiles()
        .args(["vars", "list"])
        .env("BATFILES_TEST_GHE", server.address())
        .assert()
        .success();
    assert_eq!(
        stdout_of(&assertion),
        "tag = \"v1\" (batfiles.toml, github-release)\n"
    );

    let os = std::env::consts::OS;
    let server = Server::serving(vec![(
        format!("/{os}{LATEST}"),
        found("/o/r/releases/tag/v2"),
    )]);
    let tree = declaring(&format!("{}/${{facts.os}}", server.address()), "");
    let (stdout, stderr) = list(&tree, &[]);
    assert_eq!(
        stdout, "tag = \"v2\" (batfiles.toml, github-release)\n",
        "{stderr}"
    );
}

#[test]
fn each_declaration_asks_its_own_host() {
    let (public, enterprise) = (releasing("v1"), releasing("v9"));
    let tree = Tree::new();
    tree.write_manifest(&format!(
        r#"[vars]
public = {{ github-release = "o/r", github-url = "{}" }}
internal = {{ github-release = "o/r", github-url = "{}" }}
"#,
        public.address(),
        enterprise.address()
    ));
    let (stdout, _) = list(&tree, &[]);
    assert_eq!(
        stdout,
        "internal = \"v9\" (batfiles.toml, github-release)\n\
         public   = \"v1\" (batfiles.toml, github-release)\n"
    );
    assert_eq!((public.requests(), enterprise.requests()), (1, 1));
}

#[test]
fn a_github_url_may_not_read_variables() {
    for (url, root) in [("${base}", "base"), ("${vars.base}/x", "vars")] {
        let stderr = rejected(&format!(
            "[vars]\nbase = \"https://ghe.example\"\n\n[vars.tag]\ngithub-release = \"o/r\"\n\
             github-url = \"{url}\"\n"
        ));
        assert!(
            stderr.contains(&format!(
                "variable `tag`: `github-url` reads `{root}`; `github-url` may read `env` and \
                 `facts`, not variables"
            )),
            "{stderr}"
        );
    }
}

#[test]
fn a_malformed_declaration_is_rejected_when_the_manifest_loads() {
    for (fields, expected) in [
        ("github-release = \"junegunn/fzf.git\"", "as `OWNER/REPO`"),
        (
            "github-release = \"https://github.com/junegunn/fzf\"",
            "as `OWNER/REPO`",
        ),
        (
            "github-release = \"o/r\"\ncommand = \"true\"",
            "`command` cannot be written beside `github-release`",
        ),
        (
            "command = \"true\"\ngithub-url = \"https://ghe.example\"",
            "`github-url` cannot be written beside `command`",
        ),
        (
            "command = \"true\"\ntimeout = \"1s\"",
            "`timeout` cannot be written beside `command`",
        ),
        (
            "github-release = \"o/r\"\ntimeout = \"0s\"",
            "`timeout` must be greater than zero",
        ),
        ("github-release = \"o/r\"\ntimeout = \"soon\"", "`soon`"),
        (
            "github-release = \"o/r\"\ngithb-url = \"https://ghe.example\"",
            "unknown field `githb-url`",
        ),
        (
            "github-release = \"o/r\"\ngithub-url = \"ftp://ghe.example\"",
            "variable `tag`: github-url `ftp://ghe.example` is not an http:// or https:// URL",
        ),
        (
            "github-release = \"o/r\"\ngithub-url = \"${env.\"",
            "variable `tag`: github-url",
        ),
    ] {
        let stderr = rejected(&format!("[vars.tag]\n{fields}\n"));
        assert!(stderr.contains(expected), "no `{expected}` in:\n{stderr}");
    }
}

#[test]
fn a_machine_value_or_var_overrides_the_tag() {
    let server = releasing("v1");
    let tree = declaring(server.address(), "");
    tree.write_machine_vars("tag = \"v0.9\"\n");
    let (stdout, _) = list(&tree, &[]);
    assert_eq!(stdout, "tag = \"v0.9\" (vars.toml; over batfiles.toml)\n");
    assert_eq!(server.requests(), 0, "a shadowed declaration was looked up");

    fs::remove_file(tree.machine_vars()).expect("vars.toml");
    let stderr = sync(&tree, &["-vv", "--var", "tag=v0.8"]);
    assert!(
        stderr.contains(r#"tag = "v0.8" (--var; over batfiles.toml)"#),
        "{stderr}"
    );
}

#[test]
fn a_remote_built_from_the_tag_follows_it() {
    let server = Server::serving(vec![
        (LATEST.to_owned(), found("/o/r/releases/tag/v1")),
        ("/dl/v1/tool".to_owned(), Reply::Body("one\n")),
        ("/dl/v2/tool".to_owned(), Reply::Body("two\n")),
        ("/dl/v0/tool".to_owned(), Reply::Body("zero\n")),
    ]);
    let tree = declaring(server.address(), "");
    let manifest = fs::read_to_string(tree.manifest()).expect("the manifest");
    tree.write_manifest(&format!(
        "{manifest}\n[remotes.tool]\ntype = \"file\"\nurl = \"{}/dl/${{tag}}/tool\"\n",
        server.address()
    ));
    let fetched = || fs::read_to_string(tree.path("repo/remotes/tool")).expect("the remote");

    sync(&tree, &[]);
    assert_eq!(fetched(), "one\n");
    assert_eq!(server.requests(), 2);

    sync(&tree, &["--refresh-vars"]);
    assert_eq!(fetched(), "one\n");
    assert_eq!(server.requests(), 3, "an unchanged tag downloaded again");

    server.answer(LATEST, found("/o/r/releases/tag/v2"));
    sync(&tree, &["--refresh-vars"]);
    assert_eq!(fetched(), "two\n");
    assert_eq!(server.requests(), 5, "a new tag did not download");

    sync(&tree, &["--var", "tag=v0"]);
    assert_eq!(fetched(), "zero\n", "--var did not pin the version");
}

#[test]
fn a_dry_run_looks_the_tag_up_and_caches_it() {
    let server = releasing("v1");
    let tree = declaring(server.address(), "");
    sync(&tree, &["--dry-run"]);
    assert_eq!(server.requests(), 1);
    assert!(cache_document(&tree).contains("value = \"v1\""));
}

// Provenance

#[test]
fn a_tag_another_declaration_cached_is_looked_up_again() {
    let server = releasing("v1");
    for from in [
        "{ github-release = \"o/other\", github-url = \"{host}\" }".to_owned(),
        from_release("http://old.example"),
        "{ command = \"printf v0\", capture = \"stdout\" }".to_owned(),
    ] {
        let tree = declaring(server.address(), "");
        let before = server.requests();
        seed(
            &tree,
            "v0",
            an_hour(),
            Some(&from.replace("{host}", server.address())),
        );
        let (stdout, _) = list(&tree, &[]);
        assert_eq!(stdout, "tag = \"v1\" (batfiles.toml, github-release)\n");
        assert_eq!(server.requests(), before + 1, "{from}");
        assert!(cache_document(&tree).contains(&from_release(server.address())));
    }
}

#[test]
fn a_failed_lookup_never_falls_back_on_another_declarations_tag() {
    let server = Server::new(&[]);
    let tree = declaring(server.address(), "");
    seed(
        &tree,
        "v0",
        an_hour(),
        Some(&from_release("http://old.example")),
    );
    let (stdout, stderr) = list(&tree, &[]);
    assert_eq!(
        stdout, "tag = no value (batfiles.toml, github-release failed)\n",
        "{stderr}"
    );
    assert!(
        stderr.contains("it has no value, and reads as empty"),
        "{stderr}"
    );
}

#[test]
fn an_entry_recording_no_declaration_is_reused() {
    let server = releasing("v1");
    let tree = declaring(server.address(), "");
    seed(&tree, "v0", an_hour(), None);
    let (stdout, _) = list(&tree, &[]);
    assert_eq!(stdout, "tag = \"v0\" (batfiles.toml, cached 1h ago)\n");
    assert_eq!(server.requests(), 0);
}

#[test]
fn no_refresh_reports_an_entry_another_declaration_cached() {
    let server = releasing("v1");
    let tree = declaring(server.address(), "");
    seed(
        &tree,
        "v0",
        an_hour(),
        Some(&from_release("http://old.example")),
    );
    let (stdout, _) = list(&tree, &["--no-refresh"]);
    assert_eq!(
        stdout,
        "tag = no value (batfiles.toml, cached for a different declaration)\n"
    );
    assert_eq!(server.requests(), 0);
}

// Included manifests

/// A remote whose manifest declares `tag` from `host`, written as given, and the command
/// `team`, each deciding one directory.
fn remote(host: &str) -> BareRepo {
    let origin = BareRepo::new();
    origin.publish(
        "batfiles.toml",
        &format!(
            r#"[vars]
tag = {{ github-release = "o/r", github-url = "{host}" }}
team = {{ command = "printf platform" }}

[[actions]]
type = "create-dir"
id = "released"
dest = "~/released"
when = "tag == 'v1'"
"#
        ),
        "a remote declaring a release variable",
    );
    origin
}

/// A leaf including that remote, with `fields` on its `[remotes]` entry.
fn including(host: &str, fields: &str) -> (BareRepo, Tree) {
    let origin = remote(host);
    let tree = Tree::new();
    tree.write_manifest(&format!(
        r#"[remotes.corporate]
type = "git"
url = "{{origin}}"
{fields}
[[actions]]
type = "include-remote"
id = "corporate"
remote = "corporate"
"#
    ));
    tree.point_at_origin(&origin);
    (origin, tree)
}

#[test]
fn a_remote_without_permissions_still_looks_its_releases_up() {
    let server = releasing("v1");
    let (_origin, tree) = including(server.address(), "");
    let stderr = sync(&tree, &["-v"]);
    assert!(tree.home("released").is_dir(), "{stderr}");
    assert_eq!(server.requests(), 1);
    assert!(
        stderr.contains(
            "remote `corporate` is not allowed to run dynamic-variable commands, so it does \
             not declare `team`"
        ),
        "{stderr}"
    );
    assert!(
        cache_document(&tree).contains("[\"remote:corporate.tag\"]"),
        "{}",
        cache_document(&tree)
    );
}

#[test]
fn an_included_github_url_reads_env_only_if_the_leaf_allows_it() {
    let server = releasing("v1");
    for (fields, allowed) in [("", false), ("allow-env-templates = true\n", true)] {
        let (_origin, tree) = including("${env.BATFILES_TEST_GHE}", fields);
        let assertion = tree
            .batfiles()
            .arg("sync")
            .env("BATFILES_TEST_GHE", server.address())
            .assert()
            .success();
        let stderr = stderr_of(&assertion);
        assert_eq!(tree.home("released").is_dir(), allowed, "{stderr}");
        assert_eq!(
            stderr.contains(
                "dynamic variable `corporate.tag` could not be refreshed: `github-url` \
                 \"${env.BATFILES_TEST_GHE}\" cannot be evaluated"
            ),
            !allowed,
            "{stderr}"
        );
        assert_eq!(
            stderr.contains("`[remotes.corporate]` sets `allow-env-templates = true`"),
            !allowed,
            "{stderr}"
        );
    }
}

/// `vars refresh` refreshes what a remote without permissions may capture.
#[test]
fn vars_refresh_captures_a_remotes_release_and_refuses_its_command() {
    let server = releasing("v1");
    let (_origin, tree) = including(server.address(), "");
    sync(&tree, &[]);
    let assertion = tree
        .batfiles()
        .args(["vars", "refresh", "corporate.tag"])
        .assert()
        .success();
    assert_eq!(stderr_of(&assertion), "refreshed `corporate.tag`\n");
    assert_eq!(server.requests(), 2);

    let assertion = tree
        .batfiles()
        .args(["vars", "refresh", "corporate.team"])
        .assert()
        .failure()
        .code(1);
    assert!(
        stderr_of(&assertion).contains(
            "cannot refresh `corporate.team`: remote `corporate` is not allowed to run \
             dynamic-variable commands"
        ),
        "{}",
        stderr_of(&assertion)
    );

    let assertion = tree.batfiles().args(["vars", "refresh"]).assert().success();
    assert_eq!(stderr_of(&assertion), "refreshed `corporate.tag`\n");
}
