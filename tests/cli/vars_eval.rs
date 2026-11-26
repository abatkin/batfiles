//! CLI tests for `vars eval`, which evaluates a template in the leaf's scope, and `facts list`.

use crate::support::*;

/// An environment variable the tests' templates read.
const SUFFIX: &str = "BATFILES_TEST_SUFFIX";

/// A leaf whose `email` is built from `profile`.
const LEAF: &str = r#"[vars]
profile = "work"
email = "me@${profile}.example.com"
"#;

fn leaf(manifest: &str) -> Tree {
    let tree = Tree::new();
    tree.write_manifest(manifest);
    tree
}

/// Run `vars eval` with `args`, require success, and return its standard output and error.
fn eval(tree: &Tree, args: &[&str]) -> (String, String) {
    let assertion = tree
        .batfiles()
        .args(["vars", "eval"])
        .args(args)
        .assert()
        .success();
    (stdout_of(&assertion), stderr_of(&assertion))
}

/// Run `vars eval` with `args`, require status 1 and nothing on standard output, and return
/// its standard error.
fn refused(tree: &Tree, args: &[&str]) -> String {
    let assertion = tree
        .batfiles()
        .args(["vars", "eval"])
        .args(args)
        .assert()
        .failure()
        .code(1);
    assert_eq!(stdout_of(&assertion), "");
    stderr_of(&assertion)
}

#[test]
fn a_template_reads_the_leaf_s_variables_facts_and_the_environment() {
    let tree = leaf(LEAF);
    let assertion = tree
        .batfiles()
        .args([
            "vars",
            "eval",
            "${email}/${facts.os}/${env.BATFILES_TEST_SUFFIX}",
        ])
        .env(SUFFIX, "x")
        .assert()
        .success();
    assert_eq!(
        stdout_of(&assertion),
        format!("me@work.example.com/{}/x\n", std::env::consts::OS)
    );
}

#[test]
// Lowercase `BATFILES_VAR_*` names, which Windows uppercases at capture.
#[cfg(unix)]
fn every_layer_above_the_manifest_overrides_it() {
    let tree = leaf(LEAF);
    assert_eq!(eval(&tree, &["${email}"]).0, "me@work.example.com\n");

    tree.write_machine_vars("profile = \"lab\"\n");
    assert_eq!(eval(&tree, &["${email}"]).0, "me@lab.example.com\n");

    let from_environment = tree
        .batfiles()
        .args(["vars", "eval", "${email}"])
        .env("BATFILES_VAR_profile", "home")
        .assert()
        .success();
    assert_eq!(stdout_of(&from_environment), "me@home.example.com\n");

    let from_command_line = tree
        .batfiles()
        .args(["vars", "eval", "${email}", "--var", "profile=a", "--var"])
        .arg("profile=cli")
        .env("BATFILES_VAR_profile", "home")
        .assert()
        .success();
    assert_eq!(stdout_of(&from_command_line), "me@cli.example.com\n");
}

#[test]
fn the_value_is_followed_by_how_a_condition_would_take_it() {
    let tree = leaf(LEAF);
    let (stdout, stderr) = eval(&tree, &["${facts.os == facts.os}"]);
    assert_eq!(stdout, "true\n");
    assert!(
        stderr.contains("`when` includes and `unless` skips"),
        "{stderr}"
    );

    let (stdout, stderr) = eval(&tree, &["o${'ff'}"]);
    assert_eq!(stdout, "off\n");
    assert!(
        stderr.contains("`when` skips and `unless` includes"),
        "{stderr}"
    );

    let (stdout, stderr) = eval(&tree, &["${email}"]);
    assert_eq!(stdout, "me@work.example.com\n");
    assert!(stderr.contains("as a condition, this fails"), "{stderr}");

    let quiet = tree
        .batfiles()
        .args(["--quiet", "vars", "eval", "${profile}"])
        .assert()
        .success();
    assert_eq!(stdout_of(&quiet), "work\n");
    assert_eq!(stderr_of(&quiet), "");
}

#[test]
fn a_template_with_no_expression_is_its_own_text() {
    let tree = leaf(LEAF);
    assert_eq!(eval(&tree, &["profile"]).0, "profile\n");
    assert_eq!(eval(&tree, &["$${profile}"]).0, "${profile}\n");
}

#[test]
fn a_template_that_cannot_be_evaluated_fails_naming_why() {
    let tree = leaf(LEAF);
    let stderr = refused(&tree, &["${nope}"]);
    assert!(
        stderr.contains("vars eval: template \"${nope}\""),
        "{stderr}"
    );
    assert!(stderr.contains("`nope` is not declared"), "{stderr}");
    assert!(!stderr.contains("as a condition"), "{stderr}");

    assert_eq!(eval(&tree, &["[${vars.nope}]"]).0, "[]\n");
}

#[test]
fn a_cycle_among_the_variables_read_fails() {
    let tree = leaf("[vars]\na = \"${b}\"\nb = \"${a}\"\n");
    let stderr = refused(&tree, &["${a}"]);
    assert!(
        stderr.contains("variables refer to each other in a cycle: a -> b -> a"),
        "{stderr}"
    );
}

/// `[vars]` lines where `v1` reads `v2` and so on, through `templates` templates, ending at a
/// literal.
fn chain_of(templates: usize) -> String {
    let mut vars: Vec<String> = (1..=templates)
        .map(|index| format!("v{index} = \"${{v{}}}\"", index + 1))
        .collect();
    vars.push(format!("v{} = \"end\"", templates + 1));
    vars.join("\n")
}

#[test]
fn the_template_counts_toward_the_nesting_limit_as_a_vars_value_would() {
    let tree = leaf(&format!(
        "[vars]\npreview = \"${{v1}}\"\n{}\n",
        chain_of(32)
    ));
    let message = |first: &str| {
        let chain: Vec<String> = (1..=32).map(|index| format!("v{index}")).collect();
        format!(
            "variables read each other too deeply: {first} -> {}; a [vars] value can read \
             through at most 32 templated variables in a row",
            chain.join(" -> ")
        )
    };

    let stderr = refused(&tree, &["${v1}"]);
    assert!(stderr.contains(&message("the template")), "{stderr}");
    let listing = tree
        .batfiles()
        .args(["vars", "list"])
        .assert()
        .failure()
        .code(1);
    let stderr = stderr_of(&listing);
    assert!(stderr.contains(&message("preview")), "{stderr}");

    assert_eq!(eval(&tree, &["${v2}"]).0, "end\n");
}

#[test]
fn a_template_that_does_not_parse_fails_before_the_manifest_is_read() {
    let tree = leaf("this is not a manifest");
    let stderr = refused(&tree, &["${"]);
    assert!(
        stderr.contains("vars eval: template \"${\" is not a valid template"),
        "{stderr}"
    );
}

#[test]
fn without_manifest_vars_no_repository_is_read() {
    let tree = leaf("this is not a manifest");
    tree.write_machine_vars("profile = \"lab\"\n");
    let (stdout, _) = eval(&tree, &["--no-manifest-vars", "${profile}/${vars.email}"]);
    assert_eq!(stdout, "lab/\n");

    let assertion = tree
        .batfiles()
        .args(["vars", "eval", "--no-manifest-vars", "${profile}-${RANK}"])
        .args(["--var", "profile=cli"])
        .env("BATFILES_VAR_RANK", "9")
        .env("BATFILES_DIR", tree.path("absent"))
        .assert()
        .success();
    assert_eq!(stdout_of(&assertion), "cli-9\n");

    let stderr = refused(&tree, &["--no-manifest-vars", "${email}"]);
    assert!(stderr.contains("`email` is not declared"), "{stderr}");
}

#[test]
fn facts_list_prints_every_fact_without_a_repository() {
    let tree = Tree::roots();
    let assertion = tree.batfiles().args(["facts", "list"]).assert().success();
    let stdout = stdout_of(&assertion);
    let keys: Vec<&str> = stdout
        .lines()
        .map(|line| line.split_whitespace().next().unwrap_or_default())
        .collect();
    assert_eq!(keys, ["arch", "family", "hostname", "os"], "{stdout}");
    assert!(
        stdout.contains(&format!("os       = \"{}\"\n", std::env::consts::OS)),
        "{stdout}"
    );
    assert!(
        stdout.contains(&format!("family   = \"{}\"\n", std::env::consts::FAMILY)),
        "{stdout}"
    );
}

#[test]
fn facts_list_agrees_with_what_a_template_reads() {
    let tree = leaf(LEAF);
    let listed = stdout_of(&tree.batfiles().args(["facts", "list"]).assert().success());
    let hostname = eval(&tree, &["${facts.hostname}"]).0;
    assert!(
        listed.contains(&format!("hostname = \"{}\"\n", hostname.trim_end())),
        "{listed}"
    );
}

/// Dynamic variables, captured through `sh`.
#[cfg(unix)]
mod dynamic {
    use std::fs;

    use jiff::{SignedDuration, Timestamp};

    use super::*;

    /// A leaf whose `ver` is built from `tag`, captured by a command that records its run, and
    /// whose `other` is captured by a command no test reads.
    const DYNAMIC: &str = r#"[vars]
tag = { command = "echo ran >> runs; printf v1.2" }
ver = "${tag.substring(1)}"
other = { command = "printf other" }
"#;

    fn runs(tree: &Tree) -> usize {
        fs::read_to_string(tree.path("repo/runs"))
            .map(|text| text.lines().count())
            .unwrap_or_default()
    }

    fn cache(tree: &Tree) -> std::path::PathBuf {
        tree.path("cache").join("dynamic-vars.toml")
    }

    /// Put `tag` in the cache with `value`, captured two days ago.
    fn seed_stale(tree: &Tree, value: &str) {
        let captured = Timestamp::now() - SignedDuration::from_hours(48);
        fs::write(
            cache(tree),
            format!("[tag]\nvalue = \"{value}\"\ncaptured-at = \"{captured}\"\n"),
        )
        .expect("a cache document");
    }

    #[test]
    fn a_dynamic_variable_is_captured_and_cached_as_a_listing_would() {
        let tree = leaf(DYNAMIC);
        let (stdout, stderr) = eval(&tree, &["${ver}"]);
        assert_eq!(stdout, "1.2\n");
        assert!(!stderr.contains("warning"), "{stderr}");
        assert_eq!(runs(&tree), 1);
        let document = fs::read_to_string(cache(&tree)).expect("a cache");
        assert!(document.contains("[tag]"), "{document}");

        eval(&tree, &["${ver}"]);
        assert_eq!(runs(&tree), 1, "a fresh value is reused");
    }

    #[test]
    fn no_refresh_warns_about_a_stale_value_read_through_another_variable() {
        let tree = leaf(DYNAMIC);
        seed_stale(&tree, "v0.9");
        let before = fs::read_to_string(cache(&tree)).expect("a cache");
        let (stdout, stderr) = eval(&tree, &["--no-refresh", "${ver}"]);
        assert_eq!(stdout, "0.9\n");
        assert!(
            stderr.contains(
                "warning: the template reads `tag`, which --no-refresh did not capture: \
                 stale, cached 2d ago"
            ),
            "{stderr}"
        );
        assert!(!stderr.contains("`other`"), "{stderr}");
        assert_eq!(runs(&tree), 0);
        assert_eq!(fs::read_to_string(cache(&tree)).expect("a cache"), before);
    }

    #[test]
    fn no_refresh_warns_about_a_value_never_cached_even_when_evaluation_fails() {
        let tree = leaf(DYNAMIC);
        let (stdout, stderr) = eval(&tree, &["--no-refresh", "[${vars.tag}]"]);
        assert_eq!(stdout, "[]\n");
        assert!(
            stderr.contains(
                "the template reads `tag`, which --no-refresh did not capture: not cached"
            ),
            "{stderr}"
        );

        let stderr = refused(&tree, &["--no-refresh", "${ver}"]);
        assert!(
            stderr.contains("the template reads `tag`, which --no-refresh did not capture"),
            "{stderr}"
        );
        assert!(stderr.contains("`ver` has no value"), "{stderr}");
        assert_eq!(runs(&tree), 0);
        assert!(!cache(&tree).exists());
    }

    #[test]
    fn a_value_above_a_dynamic_declaration_needs_no_capture() {
        let tree = leaf(DYNAMIC);
        let (stdout, stderr) = eval(&tree, &["--no-refresh", "${ver}", "--var", "tag=v2"]);
        assert_eq!(stdout, "2\n");
        assert!(!stderr.contains("warning"), "{stderr}");
    }
}
