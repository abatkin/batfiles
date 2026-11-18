//! A Git remote's permissions apply only to a checkout of the repository the leaf declares. See
//! `docs/safety.md#permissions-follow-the-origin`.

use std::fs;
use std::path::PathBuf;

use crate::support::*;

/// A leaf permission and what a remote does with it.
#[derive(Clone, Copy, Debug)]
enum Permission {
    DynamicVars,
    EnvTemplates,
}

const BOTH: [Permission; 2] = [Permission::DynamicVars, Permission::EnvTemplates];

/// The environment variable the env-reading remote writes into a path.
const TOKEN: &str = "BATFILES_TEST_ORIGIN_TOKEN";

impl Permission {
    /// The `[remotes]` field granting it.
    fn field(self) -> &'static str {
        match self {
            Self::DynamicVars => "allow-dynamic-vars = true",
            Self::EnvTemplates => "allow-env-templates = true",
        }
    }

    /// A remote whose manifest uses the permission.
    fn remote(self) -> BareRepo {
        let manifest = match self {
            // The command runs in the checkout, two levels below the leaf root.
            Self::DynamicVars => {
                r#"[vars]
team = { command = "echo ran >> ../../runs; printf platform" }

[[actions]]
type = "create-dir"
id = "used"
dest = "~/team"
when = "team == 'platform'"
"#
            }
            Self::EnvTemplates => {
                r#"[[actions]]
type = "create-dir"
id = "used"
dest = "~/token-${env.BATFILES_TEST_ORIGIN_TOKEN}"
"#
            }
        };
        let origin = BareRepo::new();
        origin.publish("batfiles.toml", manifest, "a remote using a permission");
        origin
    }

    /// Whether the remote's contributed work happened: its command ran, or its template read the
    /// environment into a directory name.
    fn used(self, tree: &Tree) -> bool {
        match self {
            Self::DynamicVars => tree.path("repo/runs").exists(),
            Self::EnvTemplates => tree.home("token-secret").exists(),
        }
    }

    /// Undo what [`used`](Self::used) looks for, and drop cached captures so a run that reached
    /// the remote's command would run it again.
    fn forget(self, tree: &Tree) {
        let _ = fs::remove_file(tree.path("repo/runs"));
        let _ = fs::remove_file(tree.path("cache/dynamic-vars.toml"));
        let _ = fs::remove_dir(tree.home("token-secret"));
    }
}

/// Declare `corporate` at `url` with `fields`, included once as `corp`.
fn declare(tree: &Tree, url: &str, fields: &str) {
    tree.write_manifest(&format!(
        r#"[remotes.corporate]
type = "git"
url = "{url}"
{fields}

[[actions]]
type = "include-remote"
id = "corp"
remote = "corporate"
"#
    ));
}

fn run(tree: &Tree, args: &[&str]) -> assert_cmd::assert::Assert {
    tree.batfiles().args(args).env(TOKEN, "secret").assert()
}

fn checkout(tree: &Tree) -> PathBuf {
    tree.path("repo/remotes/corporate")
}

/// Run `args`, expect the checkout to be refused for `reason`, and return the diagnostics.
fn refused(tree: &Tree, args: &[&str], reason: &str) -> String {
    let stderr = stderr_of(&run(tree, args).failure().code(1));
    for expected in [
        format!(
            "include-remote `corp`: cannot use the checkout of remote `corporate` at {}: {reason}",
            display(&checkout(tree))
        ),
        "move the checkout aside and run `batfiles sync`".to_owned(),
    ] {
        assert!(
            stderr.contains(&expected),
            "{args:?}: no `{expected}` in:\n{stderr}"
        );
    }
    stderr
}

#[test]
fn a_checkout_of_the_declared_repository_is_used_with_either_permission() {
    for permission in BOTH {
        let origin = permission.remote();
        let tree = Tree::new();
        declare(&tree, &display(&origin.origin()), permission.field());

        run(&tree, &["sync"]).success();
        assert!(
            permission.used(&tree),
            "{permission:?}: the clone was not used"
        );

        // The second run verifies the checkout the first one cloned.
        permission.forget(&tree);
        run(&tree, &["sync"]).success();
        assert!(
            permission.used(&tree),
            "{permission:?}: the checkout was not used"
        );
    }
}

#[test]
fn a_checkout_of_a_repository_the_leaf_no_longer_names_is_refused_before_it_is_used() {
    for permission in BOTH {
        let (a, b) = (permission.remote(), permission.remote());
        let tree = Tree::new();
        declare(&tree, &display(&a.origin()), permission.field());
        run(&tree, &["sync"]).success();

        declare(&tree, &display(&b.origin()), permission.field());
        let mut commands = vec![
            vec!["sync"],
            vec!["sync", "--dry-run"],
            vec!["apply-action", "--id", "corp.used"],
            vec!["apply-action", "--id", "corp.used", "--dry-run"],
            vec!["vars", "refresh"],
        ];
        if let Permission::DynamicVars = permission {
            commands.push(vec!["vars", "refresh", "corporate.team"]);
        }
        for args in commands {
            permission.forget(&tree);
            let stderr = refused(&tree, &args, "its origin is a different repository");
            assert!(
                stderr.contains("allow-"),
                "{permission:?} {args:?}: the permission was not named:\n{stderr}"
            );
            for url in [a.origin(), b.origin()] {
                assert!(
                    !stderr.contains(&display(&url)),
                    "{permission:?} {args:?}: a URL was shown:\n{stderr}"
                );
            }
            assert!(
                !permission.used(&tree),
                "{permission:?} {args:?}: the checkout was used:\n{stderr}"
            );
        }
    }
}

#[test]
fn a_checkout_with_no_origin_is_refused() {
    for permission in BOTH {
        let origin = permission.remote();
        let tree = Tree::new();
        declare(&tree, &display(&origin.origin()), permission.field());
        run(&tree, &["sync"]).success();
        git(&checkout(&tree), &["remote", "remove", "origin"]);

        for args in [
            vec!["apply-action", "--id", "corp.used"],
            vec!["sync", "--dry-run"],
        ] {
            permission.forget(&tree);
            refused(&tree, &args, "it has no origin");
            assert!(!permission.used(&tree), "{permission:?} {args:?}");
        }
    }
}

#[test]
fn a_checkout_without_its_own_git_directory_is_not_read_through_the_leaf() {
    // The leaf root is itself a Git repository, whose origin a lookup from the checkout could
    // otherwise find.
    let permission = Permission::EnvTemplates;
    let origin = permission.remote();
    let tree = Tree::new();
    declare(&tree, &display(&origin.origin()), permission.field());
    run(&tree, &["sync"]).success();
    git(&tree.path("repo"), &["init", "-q"]);
    git(
        &tree.path("repo"),
        &["remote", "add", "origin", &display(&origin.origin())],
    );
    fs::remove_dir_all(checkout(&tree).join(".git")).expect("remove the checkout's .git");

    permission.forget(&tree);
    refused(
        &tree,
        &["apply-action", "--id", "corp.used"],
        "it has no .git directory of its own",
    );
    assert!(!permission.used(&tree));
}

#[test]
fn a_relative_local_clone_source_matches_the_absolute_path_git_recorded() {
    for permission in BOTH {
        let origin = permission.remote();
        let beside = origin.origin().parent().expect("a parent").to_path_buf();
        let tree = Tree::new();
        // Git records `<working directory>/./work/../origin.git` as written.
        declare(&tree, "./work/../origin.git", permission.field());

        for _ in 0..2 {
            permission.forget(&tree);
            let assertion = tree
                .batfiles()
                .arg("sync")
                .env(TOKEN, "secret")
                .current_dir(&beside)
                .assert()
                .success();
            assert!(
                permission.used(&tree),
                "{permission:?}: {}",
                stderr_of(&assertion)
            );
        }
    }
}

#[test]
fn an_inclusion_granted_neither_permission_is_not_verified() {
    let plain = |name: &str| {
        let origin = BareRepo::new();
        origin.publish(
            "batfiles.toml",
            &format!("[[actions]]\ntype = \"create-dir\"\ndest = \"~/{name}\"\n"),
            "a remote needing no permission",
        );
        origin
    };
    let (a, b) = (plain("a"), plain("b"));
    let tree = Tree::new();
    declare(&tree, &display(&a.origin()), "");
    run(&tree, &["sync"]).success();

    declare(&tree, &display(&b.origin()), "");
    fs::remove_dir(tree.home("a")).expect("remove what the first sync made");
    run(&tree, &["sync"]).success();
    assert!(
        tree.home("a").is_dir(),
        "the existing checkout was not used"
    );
}
