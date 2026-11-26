//! CLI tests for [`[vars]` values built from other values](../../docs/repoformat.md#variables):
//! templates in `[vars]` and in `include-remote` overrides, lazy per-scope evaluation, cycles,
//! and variables with no value.

use crate::support::*;

/// An environment variable the tests' templates read.
const SUFFIX: &str = "BATFILES_TEST_SUFFIX";

/// A tree whose manifest is `manifest`.
fn leaf(manifest: &str) -> Tree {
    let tree = Tree::new();
    tree.write_manifest(manifest);
    tree
}

/// Run `sync` with `args`, require success, and return its standard error.
fn sync(tree: &Tree, args: &[&str]) -> String {
    let assertion = tree.batfiles().arg("sync").args(args).assert().success();
    stderr_of(&assertion)
}

/// Run `vars list` with `args`, require success, and return its standard output.
fn list(tree: &Tree, args: &[&str]) -> String {
    let assertion = tree
        .batfiles()
        .args(["vars", "list"])
        .args(args)
        .assert()
        .success();
    stdout_of(&assertion)
}

/// A leaf creating `~/<greeting>`, where `greeting` is built from another variable, a host
/// fact, and the environment, and declared before the variable it reads.
const GREETING: &str = r#"[vars]
greeting = "${WHO}-${env.BATFILES_TEST_SUFFIX}-${facts.os}"
is_here = "${facts.os == facts.os}"
WHO = "me"

[[actions]]
type = "create-dir"
dest = "~/${greeting}"
when = "is_here && greeting != ''"
"#;

/// The directory [`GREETING`] creates for `WHO`, uppercase because Windows uppercases
/// environment names.
fn greeting_dir(who: &str) -> String {
    format!("{who}-x-{}", std::env::consts::OS)
}

#[test]
fn a_variable_built_from_a_variable_a_fact_and_the_environment_feeds_conditions_and_templates() {
    let tree = leaf(GREETING);
    let assertion = tree
        .batfiles()
        .arg("sync")
        .env(SUFFIX, "x")
        .assert()
        .success();
    assert!(
        tree.home(&greeting_dir("me")).is_dir(),
        "{}",
        stderr_of(&assertion)
    );
}

#[test]
fn a_higher_layer_over_a_variable_changes_what_is_built_from_it() {
    let tree = leaf(GREETING);
    let run = |args: &[&str], env: Option<(&str, &str)>| {
        let mut command = tree.batfiles();
        command.arg("sync").args(args).env(SUFFIX, "x");
        if let Some((key, value)) = env {
            command.env(key, value);
        }
        stderr_of(&command.assert().success())
    };

    let stderr = run(&["--var", "WHO=cli"], None);
    assert!(tree.home(&greeting_dir("cli")).is_dir(), "{stderr}");

    let stderr = run(&[], Some(("BATFILES_VAR_WHO", "environment")));
    assert!(tree.home(&greeting_dir("environment")).is_dir(), "{stderr}");

    tree.batfiles()
        .args(["vars", "set", "WHO", "machine"])
        .assert()
        .success();
    let stderr = run(&[], None);
    assert!(tree.home(&greeting_dir("machine")).is_dir(), "{stderr}");
}

#[test]
fn a_variable_a_higher_layer_overrides_never_evaluates_its_template() {
    let tree = leaf(
        r#"[vars]
derived = "${nope}${derived}"

[[actions]]
type = "create-dir"
dest = "~/${derived}"
"#,
    );
    let stderr = sync(&tree, &["--var", "derived=plain"]);
    assert!(tree.home("plain").is_dir(), "{stderr}");
    assert!(!stderr.contains("warning"), "{stderr}");
}

#[test]
fn an_escaped_template_is_literal_text() {
    let tree = leaf(
        r#"[vars]
literal = "a$${b}"

[[actions]]
type = "create-dir"
dest = "~/made"
when = "literal == 'a' + '$' + '{b}'"
"#,
    );
    let stderr = sync(&tree, &[]);
    assert!(tree.home("made").is_dir(), "{stderr}");
    assert_eq!(list(&tree, &[]), "literal = \"a${b}\" (batfiles.toml)\n");
}

#[test]
fn a_value_that_is_not_a_template_is_a_load_error_naming_the_variable() {
    let stderr = rejected("[vars]\nbroken = \"${\"\n");
    assert!(
        stderr.contains("variable `broken`: value \"${\" is not a valid template"),
        "{stderr}"
    );
}

#[test]
fn a_template_naming_an_undeclared_variable_has_no_value() {
    let tree = leaf(
        r#"[vars]
derived = "${nope}"

[[actions]]
type = "create-dir"
dest = "~/when"
when = "derived == ''"

[[actions]]
type = "create-dir"
dest = "~/optional"
when = "vars.derived == ''"
"#,
    );
    let stderr = sync(&tree, &[]);
    assert!(!tree.home("when").exists(), "{stderr}");
    assert!(
        stderr.contains(
            "when \"derived == ''\" cannot be evaluated, so it is not installed: `derived` has \
             no value (`nope` is not declared); write `vars.derived` to read it as empty"
        ),
        "{stderr}"
    );
    assert!(tree.home("optional").is_dir(), "{stderr}");
    assert_eq!(
        list(&tree, &[]),
        "derived = no value (batfiles.toml, template \"${nope}\", `nope` is not declared)\n"
    );
}

#[test]
fn an_action_template_reading_a_variable_with_no_value_fails_its_record() {
    let tree = leaf(
        r#"[vars]
derived = "${nope}"

[[actions]]
type = "create-dir"
dest = "~/${derived}"
"#,
    );
    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains(
            "dest \"~/${derived}\" cannot be evaluated at character 3: `derived` has no value \
             (`nope` is not declared)"
        ),
        "{stderr}"
    );
}

#[test]
fn errors_and_listings_never_show_a_value_a_variable_was_built_from() {
    let tree = leaf(
        r#"[vars]
secret = "${env.BATFILES_TEST_SUFFIX}"
derived = "${secret}-tail"
missing = "${secret}${nope}"

[[actions]]
type = "create-dir"
dest = "~/never"
when = "derived"

[[actions]]
type = "create-dir"
dest = "~/never-either"
when = "missing"
"#,
    );
    let assertion = tree
        .batfiles()
        .args(["sync", "-vv"])
        .env(SUFFIX, "s3cr3t")
        .assert()
        .success();
    let stderr = stderr_of(&assertion);
    assert!(!tree.home("never").exists(), "{stderr}");
    let warnings: Vec<&str> = stderr
        .lines()
        .filter(|line| line.contains("cannot be evaluated"))
        .collect();
    assert_eq!(warnings.len(), 2, "{stderr}");
    for warning in warnings {
        assert!(!warning.contains("s3cr3t"), "{warning}");
    }
    assert!(
        stderr.contains("missing = no value (batfiles.toml, template \"${secret}${nope}\""),
        "{stderr}"
    );
}

// Cycles

/// A leaf with one directory created first, `condition` gating a second, and a third after
/// both.
fn gated(vars: &str, condition: &str) -> Tree {
    leaf(&format!(
        r#"[vars]
{vars}

[[actions]]
type = "create-dir"
dest = "~/first"

[[actions]]
type = "create-dir"
dest = "~/second"
{condition}

[[actions]]
type = "create-dir"
dest = "~/later"
"#
    ))
}

/// Run `sync`, require it to fail with the cycle `chain`, and require that no action ran.
fn fails_on_cycle(tree: &Tree, args: &[&str], chain: &str) {
    fails_reading(
        tree,
        args,
        &format!("variables refer to each other in a cycle: {chain};"),
    );
}

/// Run `sync`, require it to fail with `message`, and require that no action ran.
fn fails_reading(tree: &Tree, args: &[&str], message: &str) {
    let assertion = tree
        .batfiles()
        .arg("sync")
        .args(args)
        .assert()
        .failure()
        .code(1);
    let stderr = stderr_of(&assertion);
    assert!(stderr.contains(message), "{stderr}");
    for dest in ["first", "second", "later"] {
        assert!(!tree.home(dest).exists(), "`{dest}` ran:\n{stderr}");
    }
}

#[test]
fn a_self_reference_a_when_reads_fails_the_command() {
    let tree = gated("path = \"${path}:x\"", "when = \"path != ''\"");
    fails_on_cycle(&tree, &[], "path -> path");
}

#[test]
fn a_cycle_an_unless_reads_through_vars_fails_the_command() {
    let tree = gated(
        "a = \"${b}\"\nb = \"${vars.a}\"",
        "unless = \"vars.a == 'x'\"",
    );
    fails_on_cycle(&tree, &[], "a -> b -> a");
}

#[test]
fn a_cycle_an_action_template_reads_fails_the_command() {
    let tree = leaf(
        r#"[vars]
a = "${b}"
b = "${c}"
c = "${a}"

[[actions]]
type = "create-dir"
dest = "~/${b}"

[[actions]]
type = "create-dir"
dest = "~/later"
"#,
    );
    fails_on_cycle(&tree, &[], "b -> c -> a -> b");
}

#[test]
fn an_unused_cycle_fails_only_what_displays_it() {
    let tree = gated("a = \"${b}\"\nb = \"${a}\"", "");
    let stderr = sync(&tree, &[]);
    assert!(tree.home("later").is_dir(), "{stderr}");

    let assertion = tree
        .batfiles()
        .args(["vars", "list"])
        .assert()
        .failure()
        .code(1);
    let stderr = stderr_of(&assertion);
    assert!(stderr.contains("cycle: a -> b -> a;"), "{stderr}");
    assert!(stdout_of(&assertion).is_empty());

    let fresh = gated("a = \"${b}\"\nb = \"${a}\"", "");
    fails_on_cycle(&fresh, &["-vv"], "a -> b -> a");
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
fn a_condition_reading_through_too_many_variables_fails_the_command() {
    let within = gated(&chain_of(32), "when = \"v1 == 'end'\"");
    let stderr = sync(&within, &[]);
    assert!(within.home("second").is_dir(), "{stderr}");

    let beyond = gated(&chain_of(33), "when = \"v1 == 'end'\"");
    let chain: Vec<String> = (1..=33).map(|index| format!("v{index}")).collect();
    fails_reading(
        &beyond,
        &[],
        &format!(
            "variables read each other too deeply: {}; a [vars] value can read through at \
             most 32 templated variables in a row",
            chain.join(" -> ")
        ),
    );
}

// Dynamic variables

/// A leaf whose `fzf_ver` is built from a captured `fzf_tag`.
#[cfg(unix)]
fn built_from_a_capture(command: &str) -> Tree {
    leaf(&format!(
        r#"[vars]
fzf_ver = "${{fzf_tag.substring(1)}}"
fzf_tag = {{ command = "{command}" }}

[[actions]]
type = "create-dir"
dest = "~/fzf-${{fzf_ver}}"
"#
    ))
}

#[test]
#[cfg(unix)]
fn a_variable_built_from_a_captured_value_reads_what_was_captured() {
    let tree = built_from_a_capture("printf v0.65.2");
    let stderr = sync(&tree, &[]);
    assert!(tree.home("fzf-0.65.2").is_dir(), "{stderr}");
    // The cache's age depends on the time between the two commands.
    let listing = list(&tree, &[]);
    let lines: Vec<&str> = listing.lines().collect();
    assert_eq!(lines.len(), 2, "{listing}");
    assert!(
        lines[0].starts_with("fzf_tag = \"v0.65.2\" (batfiles.toml, cached "),
        "{listing}"
    );
    assert_eq!(
        lines[1],
        "fzf_ver = \"0.65.2\" (batfiles.toml, template \"${fzf_tag.substring(1)}\")"
    );
}

#[test]
#[cfg(unix)]
fn a_listing_without_refresh_shows_a_template_over_an_uncached_value_as_having_none() {
    let tree = built_from_a_capture("printf v0.65.2");
    assert_eq!(
        list(&tree, &["--no-refresh"]),
        "fzf_tag = no value (batfiles.toml, not cached)\n\
         fzf_ver = no value (batfiles.toml, template \"${fzf_tag.substring(1)}\", reads \
         `fzf_tag`: not cached)\n"
    );
}

#[test]
#[cfg(unix)]
fn a_failed_capture_with_nothing_cached_closes_unless() {
    let tree = leaf(
        r#"[vars]
has_op = { command = "exit 1" }

[[actions]]
type = "create-dir"
dest = "~/without-op"
unless = "has_op == 'yes'"
"#,
    );
    let stderr = sync(&tree, &[]);
    assert!(!tree.home("without-op").exists(), "{stderr}");
    assert!(
        stderr.contains("`has_op` has no value (command failed)"),
        "{stderr}"
    );
}

// Inclusions

/// A remote declaring `remote_vars` and creating `~/<dest>` under `condition`.
fn remote(remote_vars: &str, dest: &str, condition: &str) -> BareRepo {
    let origin = BareRepo::new();
    origin.publish(
        "batfiles.toml",
        &format!(
            r#"[vars]
{remote_vars}

[[actions]]
type = "create-dir"
dest = "~/{dest}"
{condition}
"#
        ),
        "a remote",
    );
    origin
}

/// A leaf declaring `leaf_vars`, creating `~/leaf-${email}`, and including `origin` with
/// `fields` on the inclusion and `permissions` on the remote.
fn including(origin: &BareRepo, leaf_vars: &str, fields: &str, permissions: &str) -> Tree {
    let tree = Tree::new();
    tree.write_manifest(&format!(
        r#"[remotes.corporate]
type = "git"
url = "{{origin}}"
{permissions}

[vars]
{leaf_vars}

[[actions]]
type = "include-remote"
id = "corp"
remote = "corporate"
{fields}
"#
    ));
    tree.point_at_origin(origin);
    tree
}

#[test]
fn a_leaf_template_evaluated_in_an_inclusion_reads_its_override() {
    let origin = remote("", "${email}", "");
    let tree = including(
        &origin,
        "email = \"me@${profile}\"\nprofile = \"personal\"",
        "vars = { profile = \"work\" }",
        "",
    );
    let stderr = sync(&tree, &[]);
    assert!(tree.home("me@work").is_dir(), "{stderr}");
}

#[test]
fn an_override_template_reads_the_leafs_value_of_its_own_name_and_the_environment() {
    let origin = remote("", "${profile}", "");
    let tree = including(
        &origin,
        "profile = \"personal\"",
        "vars = { profile = \"${profile}-corp-${env.BATFILES_TEST_SUFFIX}\" }",
        "",
    );
    let assertion = tree
        .batfiles()
        .args(["sync", "-vv"])
        .env(SUFFIX, "x")
        .assert()
        .success();
    let stderr = stderr_of(&assertion);
    assert!(tree.home("personal-corp-x").is_dir(), "{stderr}");
    assert!(
        stderr.contains(
            "profile = \"personal-corp-x\" (include-remote `corp`, template \
             \"${profile}-corp-${env.BATFILES_TEST_SUFFIX}\"; over batfiles.toml)"
        ),
        "{stderr}"
    );
}

#[test]
fn an_override_a_higher_layer_overrides_is_never_evaluated() {
    let origin = remote("", "${pick}", "");
    let tree = including(
        &origin,
        "a = \"${b}\"\nb = \"${a}\"",
        "vars = { pick = \"${a}\" }",
        "",
    );
    let stderr = sync(&tree, &["--var", "pick=valid"]);
    assert!(tree.home("valid").is_dir(), "{stderr}");

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);
    let stderr = stderr_of(&assertion);
    assert!(stderr.contains("cycle: a -> b -> a;"), "{stderr}");
}

#[test]
fn an_override_that_is_not_a_template_is_a_load_error_naming_it() {
    let stderr = rejected(
        r#"[remotes.corporate]
type = "git"
url = "https://git.example.com/dotfiles.git"

[[actions]]
type = "include-remote"
remote = "corporate"
vars = { profile = "${" }
"#,
    );
    assert!(
        stderr.contains("action 1: `vars` value for `profile` \"${\" is not a valid template"),
        "{stderr}"
    );
}

/// A remote whose `[vars]` template reads `env`, deciding one directory.
fn remote_reading_env() -> BareRepo {
    remote(
        "team = \"${env.BATFILES_TEST_SUFFIX}\"",
        "team",
        "when = \"team == 'x'\"",
    )
}

#[test]
fn a_remotes_template_reads_env_only_with_permission() {
    let origin = remote_reading_env();
    let tree = including(&origin, "", "", "");
    let assertion = tree
        .batfiles()
        .arg("sync")
        .env(SUFFIX, "x")
        .assert()
        .success();
    let stderr = stderr_of(&assertion);
    assert!(!tree.home("team").exists(), "{stderr}");
    assert!(
        stderr.contains("`team` has no value (reads `env`, which a template in an included"),
        "{stderr}"
    );

    let tree = including(&origin, "", "", "allow-env-templates = true");
    let assertion = tree
        .batfiles()
        .arg("sync")
        .env(SUFFIX, "x")
        .assert()
        .success();
    assert!(tree.home("team").is_dir(), "{}", stderr_of(&assertion));
}

#[test]
fn a_leaf_template_reading_env_works_inside_an_inclusion_without_permission() {
    let origin = remote("", "team", "when = \"team == 'x'\"");
    let tree = including(&origin, "team = \"${env.BATFILES_TEST_SUFFIX}\"", "", "");
    let assertion = tree
        .batfiles()
        .arg("sync")
        .env(SUFFIX, "x")
        .assert()
        .success();
    assert!(tree.home("team").is_dir(), "{}", stderr_of(&assertion));
}
