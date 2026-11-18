//! CLI tests for [templates](../../docs/repoformat.md#templates): `${...}` in remote and
//! action fields, when they are parsed and evaluated, what they resolve against, and how
//! they fail.

use std::fs;

use crate::support::*;

/// Replace each `@key@` in `text` with its value, leaving `${...}` alone.
fn fill(text: &str, values: &[(&str, &str)]) -> String {
    values.iter().fold(text.to_owned(), |text, (key, value)| {
        text.replace(&format!("@{key}@"), value)
    })
}

/// A tree publishing `tool-<arch>-<profile>.txt` for this machine's architecture and the
/// `work` profile, with a manifest fetching it through a template; `extra` follows the
/// fetch's fields.
fn fetching_by_template(extra: &str) -> Tree {
    let tree = Tree::new();
    let published = tree.path("published");
    fs::create_dir_all(&published).expect("the published directory");
    fs::write(
        published.join(format!("tool-{}-work.txt", std::env::consts::ARCH)),
        "work tool\n",
    )
    .expect("the published file");
    tree.write_manifest(&fill(
        r#"[vars]
profile = "personal"

[[actions]]
type = "fetch-file"
source = "@published@/tool-${facts.arch}-${profile}.txt"
dest = "~/.local/${profile}/tool"
@extra@"#,
        &[("published", &file_url(&published)), ("extra", extra)],
    ));
    tree
}

/// The URL [`fetching_by_template`] resolves to under the `work` profile.
fn resolved_url(tree: &Tree) -> String {
    format!(
        "{}/tool-{}-work.txt",
        file_url(&tree.path("published")),
        std::env::consts::ARCH
    )
}

#[test]
fn a_fetch_interpolates_a_variable_its_override_and_a_fact() {
    let tree = fetching_by_template("");

    let assertion = tree
        .batfiles()
        .args(["sync", "--var", "profile=work"])
        .assert()
        .success();

    assert_eq!(
        fs::read_to_string(tree.home(".local/work/tool")).expect("the fetched file"),
        "work tool\n",
        "{}",
        stderr_of(&assertion)
    );
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains(&format!(
            "fetched {} from {}",
            display(&tree.home(".local/work/tool")),
            resolved_url(&tree)
        )),
        "the resolved values were not shown:\n{stderr}"
    );
}

#[test]
fn a_dry_run_evaluates_templates_and_reports_what_they_resolve_to() {
    let tree = fetching_by_template("");
    let before = snapshot(&tree.path("home"));

    let assertion = tree
        .batfiles()
        .args(["sync", "--dry-run", "--var", "profile=work"])
        .assert()
        .success();

    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains(&format!(
            "would fetch {} from {}",
            display(&tree.home(".local/work/tool")),
            resolved_url(&tree)
        )),
        "{stderr}"
    );
    assert_eq!(snapshot(&tree.path("home")), before);

    tree.write_manifest("[[actions]]\ntype = \"create-dir\"\ndest = \"~/${nope}\"\n");
    tree.batfiles()
        .args(["sync", "--dry-run"])
        .assert()
        .failure()
        .code(1);
}

#[test]
fn a_template_that_does_not_parse_fails_the_load_naming_its_record_and_field() {
    let tree = Tree::new();
    tree.write_manifest(
        r#"[[actions]]
type = "create-dir"
dest = "~/first"

[[actions]]
type = "fetch-file"
when = "false"
source = "https://example.com/${version"
dest = "~/tool"
"#,
    );

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);

    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains(&format!(
            "invalid configuration in {}: action 2: source \
             \"https://example.com/${{version\" is not a valid template: ",
            display(&tree.manifest())
        )),
        "{stderr}"
    );
    assert!(stderr.contains("at character 30"), "no position:\n{stderr}");
    assert!(!tree.home("first").exists(), "the load did not fail first");
}

/// A manifest whose first action succeeds and whose second resolves `dest` as written.
fn second_dest(dest: &str) -> String {
    format!(
        r#"[vars]
who = "other"

[[actions]]
type = "create-dir"
dest = "~/first"

[[actions]]
type = "create-dir"
dest = "{dest}"

[[actions]]
type = "create-dir"
dest = "~/third"
"#
    )
}

/// Sync `manifest`, which must load and then fail its second action with status 1. Return
/// the diagnostics.
fn failed_at_the_second_action(manifest: &str) -> String {
    let tree = Tree::new();
    tree.write_manifest(manifest);

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);

    let stderr = stderr_of(&assertion);
    assert!(tree.home("first").is_dir(), "the load failed:\n{stderr}");
    assert!(!tree.home("third").exists(), "the run went on:\n{stderr}");
    stderr
}

#[test]
fn an_undeclared_variable_fails_its_action() {
    let stderr = failed_at_the_second_action(&second_dest("~/${nope}"));
    assert!(
        stderr.contains(
            "create-dir action 2: dest \"~/${nope}\" cannot be evaluated at character 3: \
             `nope` is not declared"
        ),
        "{stderr}"
    );
}

#[test]
fn an_optional_variable_is_empty_and_an_empty_dest_is_refused() {
    let stderr = failed_at_the_second_action(&second_dest("${vars.nope}"));
    assert!(
        stderr.contains("dest \"${vars.nope}\" resolves to a value that is empty"),
        "{stderr}"
    );
}

#[test]
fn a_list_is_not_a_value_a_template_can_write() {
    let stderr = failed_at_the_second_action(&second_dest("~/${[1, 2]}"));
    assert!(
        stderr.contains("its value is not a string, number, or boolean"),
        "{stderr}"
    );
}

#[test]
fn a_doubled_dollar_writes_a_literal_template_opening() {
    let tree = Tree::new();
    tree.write_manifest("[[actions]]\ntype = \"create-dir\"\ndest = \"~/$${HOME}\"\n");

    tree.batfiles().arg("sync").assert().success();

    assert!(tree.home("${HOME}").is_dir());
}

#[test]
fn a_literal_value_is_checked_at_load_and_a_templated_one_when_it_resolves() {
    let stderr = rejected(&second_dest("~other/x"));
    assert!(
        stderr.contains("action 2: dest `~other/x` names another user's home"),
        "{stderr}"
    );

    let stderr = failed_at_the_second_action(&second_dest("~${who}/x"));
    assert!(
        stderr.contains(
            "create-dir action 2: dest \"~${who}/x\" resolves to a value that names another \
             user's home"
        ),
        "{stderr}"
    );
    assert!(!stderr.contains("~other"), "the value was shown:\n{stderr}");
}

#[test]
fn a_resolved_value_its_field_refuses_is_not_shown() {
    const SECRET: &str = "hunter2";
    let tree = Tree::new();
    tree.write_manifest(
        r#"[[actions]]
type = "fetch-file"
source = "http://127.0.0.1:9/tool"
dest = "~/tool"
sha256 = "${env.BATFILES_TEST_TEMPLATE_SECRET}"
"#,
    );

    let assertion = tree
        .batfiles()
        .arg("sync")
        .env("BATFILES_TEST_TEMPLATE_SECRET", SECRET)
        .assert()
        .failure()
        .code(1);

    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains(
            "fetch-file action 1: sha256 \"${env.BATFILES_TEST_TEMPLATE_SECRET}\" resolves to a \
             value that is not 64 hexadecimal digits"
        ),
        "{stderr}"
    );
    assert!(!stderr.contains(SECRET), "the value was shown:\n{stderr}");
}

/// The two versions of a file remote the server publishes.
fn versions() -> Server {
    Server::new(&[
        ("/pathogen-1.vim", Reply::Body("version one\n")),
        ("/pathogen-2.vim", Reply::Body("version two\n")),
    ])
}

#[test]
fn a_template_selects_a_quoted_url_from_a_dictionary() {
    let server = versions();
    let tree = Tree::new();
    tree.write_manifest(&fill(
        r#"[vars]
version = "2"

[[actions]]
type = "fetch-file"
source = "${{'1': '@server@/pathogen-1.vim', '2': '@server@/pathogen-2.vim'}[version]}"
dest = "~/pathogen.vim"
"#,
        &[("server", server.address())],
    ));

    tree.batfiles().arg("sync").assert().success();

    assert_eq!(
        fs::read_to_string(tree.home("pathogen.vim")).expect("the fetched file"),
        "version two\n"
    );
    assert_eq!(server.requests(), 1);
}

#[test]
fn a_file_remote_is_fetched_again_only_when_its_resolved_url_changes() {
    let server = versions();
    let tree = Tree::new();
    tree.write_manifest(&fill(
        r#"[vars]
version = "1"

[remotes.pathogen]
type = "file"
url = "@server@/pathogen-${version}.vim"
"#,
        &[("server", server.address())],
    ));
    let materialized = tree.path("repo").join("remotes").join("pathogen");
    let stamp = tree
        .path("repo")
        .join("remotes")
        .join("pathogen.batfiles-source");

    tree.batfiles().arg("sync").assert().success();
    assert_eq!(
        fs::read_to_string(&materialized).expect("the remote"),
        "version one\n"
    );
    let recorded = fs::read_to_string(&stamp).expect("the stamp");
    assert!(
        recorded.contains(&format!("{}/pathogen-1.vim", server.address())),
        "the stamp did not record the resolved URL:\n{recorded}"
    );

    tree.batfiles().arg("sync").assert().success();
    assert_eq!(server.requests(), 1, "an unchanged URL was fetched again");

    let assertion = tree
        .batfiles()
        .args(["sync", "--var", "version=2"])
        .assert()
        .success();
    assert_eq!(server.requests(), 2);
    assert_eq!(
        fs::read_to_string(&materialized).expect("the remote"),
        "version two\n",
        "{}",
        stderr_of(&assertion)
    );
}

#[test]
fn a_remote_template_fails_the_run_before_any_action() {
    let tree = Tree::new();
    tree.write_manifest(
        r#"[remotes.pathogen]
type = "file"
url = "https://e.example/${nope}.vim"

[[actions]]
type = "create-dir"
dest = "~/first"
"#,
    );

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);

    let stderr = stderr_of(&assertion);
    assert!(
        stderr
            .contains("remote pathogen: url \"https://e.example/${nope}.vim\" cannot be evaluated"),
        "{stderr}"
    );
    assert!(!tree.home("first").exists());
}

#[test]
fn an_apply_command_does_not_evaluate_a_remotes_templates() {
    let tree = Tree::new();
    tree.write_manifest(
        r#"[remotes.pathogen]
type = "file"
url = "https://e.example/${nope}.vim"

[[actions]]
type = "create-dir"
id = "first"
dest = "~/first"
"#,
    );

    tree.batfiles()
        .args(["apply-action", "--id", "first"])
        .assert()
        .success();

    assert!(tree.home("first").is_dir());
}

#[test]
fn an_excluded_record_never_evaluates_its_templates() {
    let tree = Tree::new();
    tree.write_manifest(
        r#"[remotes.pathogen]
type = "file"
url = "https://e.example/${nope}.vim"
when = "false"

[[actions]]
type = "create-dir"
dest = "~/${nope}"
when = "false"

[[actions]]
type = "create-dir"
dest = "~/ran"
"#,
    );

    let assertion = tree.batfiles().arg("sync").assert().success();

    assert!(tree.home("ran").is_dir(), "{}", stderr_of(&assertion));
}

/// A remote publishing one record whose `dest` is `dest`, and a leaf including it with the
/// leaf's own record `leaf_dest` and `remote_fields` on its remote declaration.
fn including(dest: &str, leaf_dest: &str, remote_fields: &str) -> (BareRepo, Tree) {
    let origin = BareRepo::new();
    origin.publish(
        "batfiles.toml",
        &fill(
            "[[actions]]\ntype = \"create-dir\"\nid = \"cache\"\ndest = \"@dest@\"\n",
            &[("dest", dest)],
        ),
        "one templated record",
    );
    let tree = Tree::new();
    tree.write_manifest(&fill(
        r#"[vars]
profile = "personal"

[remotes.corporate]
type = "git"
url = "{origin}"
@remote_fields@

[[actions]]
type = "create-dir"
dest = "@leaf_dest@"

[[actions]]
type = "include-remote"
id = "corp"
remote = "corporate"
vars = { profile = "work" }
"#,
        &[("leaf_dest", leaf_dest), ("remote_fields", remote_fields)],
    ));
    tree.point_at_origin(&origin);
    (origin, tree)
}

#[test]
fn a_contributed_record_resolves_its_inclusions_variables() {
    let (_origin, tree) = including("~/.cache/${profile}", "~/.cache/leaf-${profile}", "");

    let assertion = tree.batfiles().arg("sync").assert().success();

    let stderr = stderr_of(&assertion);
    assert!(tree.home(".cache/work").is_dir(), "{stderr}");
    assert!(tree.home(".cache/leaf-personal").is_dir(), "{stderr}");
}

#[test]
fn a_contributed_template_reads_env_only_if_its_remote_allows_it() {
    const VARIABLE: &str = "BATFILES_TEST_TEMPLATE_TOKEN";
    let (_origin, tree) = including(
        "~/.cache/${env.BATFILES_TEST_TEMPLATE_TOKEN}",
        "~/.cache/leaf-${env.BATFILES_TEST_TEMPLATE_TOKEN}",
        "",
    );

    let assertion = tree
        .batfiles()
        .arg("sync")
        .env(VARIABLE, "token")
        .assert()
        .failure()
        .code(1);

    let stderr = stderr_of(&assertion);
    assert!(
        tree.home(".cache/leaf-token").is_dir(),
        "the leaf needs no permission:\n{stderr}"
    );
    assert!(
        stderr.contains("create-dir corp.cache: dest")
            && stderr.contains("cannot be evaluated at character 10: reads `env`")
            && stderr.contains("`[remotes.corporate]` sets `allow-env-templates = true`"),
        "{stderr}"
    );
    assert!(
        !stderr.contains("token\""),
        "the value was shown:\n{stderr}"
    );

    let (_origin, tree) = including(
        "~/.cache/${env.BATFILES_TEST_TEMPLATE_TOKEN}",
        "~/.cache/leaf",
        "allow-env-templates = true",
    );
    let assertion = tree
        .batfiles()
        .arg("sync")
        .env(VARIABLE, "token")
        .assert()
        .success();
    assert!(
        tree.home(".cache/token").is_dir(),
        "{}",
        stderr_of(&assertion)
    );
}

#[test]
fn a_missing_computed_key_is_reported_without_the_key() {
    const PROFILE: &str = "client-acme-7f3k";
    let tree = Tree::new();
    tree.write_manifest(
        r#"[[actions]]
type = "create-dir"
dest = "~/${{'home': 'h', 'work': 'w'}[env.BATFILES_TEST_TEMPLATE_PROFILE]}"
"#,
    );

    let assertion = tree
        .batfiles()
        .arg("sync")
        .env("BATFILES_TEST_TEMPLATE_PROFILE", PROFILE)
        .assert()
        .failure()
        .code(1);

    let stderr = stderr_of(&assertion);
    for expected in [
        "create-dir action 1: dest ",
        "cannot be evaluated at character 3: \
         a dictionary has no entry for the key looked up in it",
    ] {
        assert!(stderr.contains(expected), "no `{expected}` in:\n{stderr}");
    }
    assert!(!stderr.contains(PROFILE), "the key was shown:\n{stderr}");
}

#[test]
fn a_failure_never_shows_a_variables_value() {
    const SECRET: &str = "hunter2";
    for dest in [
        "~/${secret * 2}",
        "~/${[secret]}",
        "~/${env.BATFILES_TEST_TEMPLATE_SECRET * 2}",
        "~/${secret}/${missing}",
        "~/${{'home': 'h'}[secret]}",
        "~/${{'home': 'h'}[env.BATFILES_TEST_TEMPLATE_SECRET]}",
        "~/${['a'][secret == 'guess']}",
    ] {
        let tree = Tree::new();
        tree.write_manifest(&fill(
            r#"[vars]
secret = "@secret@"

[[actions]]
type = "create-dir"
dest = "@dest@"
"#,
            &[("secret", SECRET), ("dest", dest)],
        ));

        let assertion = tree
            .batfiles()
            .arg("sync")
            .env("BATFILES_TEST_TEMPLATE_SECRET", SECRET)
            .assert()
            .failure()
            .code(1);

        let stderr = stderr_of(&assertion);
        assert!(stderr.contains("cannot be evaluated"), "{dest}: {stderr}");
        assert!(
            !stderr.contains(SECRET),
            "{dest} showed the value:\n{stderr}"
        );
    }
}
