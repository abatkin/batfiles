use std::collections::BTreeMap;

use super::*;
use crate::manifest::item::ItemKind;

/// The record the checks below report against.
fn record() -> DeclarationName {
    DeclarationName::Action(1)
}

/// A manifest's `[remotes]` declaring `ids`; their contents do not matter.
fn declaring(ids: &[&str]) -> BTreeMap<ItemId, Remote> {
    ids.iter()
        .map(|id| {
            let remote = toml::from_str("type = \"git\"\nurl = \"https://e.example/r.git\"\n")
                .expect("a well-formed remote");
            (
                ItemId::try_from((*id).to_owned()).expect("valid ID"),
                remote,
            )
        })
        .collect()
}

/// A path in the declaring repository, built rather than parsed so that a
/// Windows spelling reaches the check with its backslashes intact.
fn local(source: &str) -> RepoPath {
    RepoPath::local(source.to_owned())
}

fn source_error(source: &str) -> ManifestError {
    check_source(&local(source), SourceShape::Any, &record(), &declaring(&[]))
        .expect_err("expected the source to be refused")
}

#[test]
fn every_kind_of_record_is_named_the_way_its_document_names_it() {
    assert_eq!(DeclarationName::Action(3).to_string(), "action 3");
    assert_eq!(
        DeclarationName::Candidate {
            kind: ItemKind::Group,
            number: 1
        }
        .to_string(),
        "default-disabled group 1"
    );
    let core = ItemId::try_from("core".to_owned()).expect("valid ID");
    assert_eq!(DeclarationName::Remote(core).to_string(), "remote `core`");
}

#[test]
fn a_source_names_a_path_within_its_repository() {
    for accepted in ["shell/zshrc", "editor/nvim", "a/../b", "./bin/batgrep"] {
        assert!(
            check_source(
                &local(accepted),
                SourceShape::Any,
                &record(),
                &declaring(&[])
            )
            .is_ok(),
            "`{accepted}` was refused"
        );
    }
}

#[test]
fn a_source_may_not_leave_its_repository() {
    for escaping in ["/etc/passwd", "../secrets", "files/../../secrets"] {
        assert!(
            matches!(
                source_error(escaping),
                ManifestError::SourceNotRelative { .. } | ManifestError::SourceOutsideTree { .. }
            ),
            "`{escaping}` was accepted"
        );
    }
}

#[test]
fn a_component_that_starts_a_path_over_is_never_ordinary_depth() {
    assert_eq!(depth_within_tree("/etc/hosts"), None);
    assert_eq!(depth_within_tree("shell/zshrc"), Some(2));
}

/// Reject Windows rooted and drive-relative source paths.
#[cfg(windows)]
#[test]
fn a_windows_rooted_or_drive_relative_source_is_refused() {
    // Windows rooted or drive-relative paths can override `join` without being absolute.
    for anchored in [r"\etc\hosts", "/etc/hosts", "C:config"] {
        assert!(
            !Path::new(anchored).is_absolute(),
            "`{anchored}` is absolute after all, so this test proves nothing"
        );
        assert!(
            matches!(
                source_error(anchored),
                ManifestError::SourceNotRelative { .. }
            ),
            "`{anchored}` was accepted"
        );
    }
    assert!(matches!(
        source_error(r"C:\config"),
        ManifestError::SourceNotRelative { .. }
    ));
}

#[test]
fn a_source_may_not_name_the_whole_repository() {
    assert!(matches!(
        source_error(""),
        ManifestError::SourceEmpty { .. }
    ));
    for whole in [".", "./", "shell/.."] {
        assert!(
            matches!(source_error(whole), ManifestError::SourceIsWholeTree { .. }),
            "`{whole}` was not recognized as the whole repository"
        );
    }
}

/// A remote reference, read the way a manifest hands one over.
fn remote_path(written: &str) -> RepoPath {
    toml::from_str::<BTreeMap<String, RepoPath>>(&format!("source = \"{written}\"\n"))
        .expect("a well-formed reference")
        .remove("source")
        .expect("the value that was just read")
}

fn remote_error(written: &str) -> ManifestError {
    check_source(
        &remote_path(written),
        SourceShape::Any,
        &record(),
        &declaring(&["core"]),
    )
    .expect_err("expected the source to be refused")
}

#[test]
fn a_remote_reference_follows_the_same_rules_as_a_local_source() {
    for accepted in ["@core/shell/zshrc", "@core/a/../b", "@core/./bin/batgrep"] {
        assert!(
            check_source(
                &remote_path(accepted),
                SourceShape::Any,
                &record(),
                &declaring(&["core"])
            )
            .is_ok(),
            "`{accepted}` was refused"
        );
    }
}

#[test]
fn a_source_may_only_name_a_remote_the_manifest_declares() {
    let written = remote_path("@core/shell/zshrc");
    assert!(check_source(&written, SourceShape::Any, &record(), &declaring(&["core"])).is_ok());
    let refused = check_source(&written, SourceShape::Any, &record(), &declaring(&["work"]))
        .expect_err("expected an undeclared remote to be refused");
    assert!(matches!(
        refused,
        ManifestError::SourceRemoteUndeclared { .. }
    ));
    let message = refused.to_string();
    assert!(message.contains("does not declare"), "{message}");
    assert!(message.contains("[remotes.core]"), "{message}");
}

#[test]
fn a_path_that_breaks_a_rule_is_reported_as_that_rather_than_as_an_undeclared_remote() {
    // Both faults at once: the path rules come first.
    assert!(matches!(
        check_source(
            &remote_path("@core/../secrets"),
            SourceShape::Any,
            &record(),
            &declaring(&[])
        )
        .expect_err("expected the source to be refused"),
        ManifestError::SourceOutsideTree { .. }
    ));
}

#[test]
fn a_refused_remote_reference_names_the_remote_rather_than_the_repository() {
    for (refused, expected) in [
        ("@core/../secrets", "resolves outside remote `core`"),
        (
            "@core//etc/passwd",
            "is not relative to the root of remote `core`",
        ),
        ("@core", "names the whole of remote `core`"),
        ("@core/shell/..", "names the whole of remote `core`"),
    ] {
        let message = remote_error(refused).to_string();
        assert!(message.contains(expected), "`{refused}`: {message}");
    }
}

#[test]
fn a_path_within_a_remote_may_not_start_the_reference_over() {
    assert!(matches!(
        remote_error("@core/@other/zshrc"),
        ManifestError::SourceStartsWithRemotePrefix { .. }
    ));
    let structured: BTreeMap<String, RepoPath> =
        toml::from_str("source = { remote = \"core\", path = \"@other/zshrc\" }\n")
            .expect("a well-formed reference");
    assert!(matches!(
        check_source(
            &structured["source"],
            SourceShape::Any,
            &record(),
            &declaring(&["core"])
        )
        .expect_err("expected the source to be refused"),
        ManifestError::SourceStartsWithRemotePrefix { .. }
    ));
}

/// A manifest's `[remotes]` declaring one file remote, `pathogen`.
fn declaring_a_file() -> BTreeMap<ItemId, Remote> {
    let remote = toml::from_str("type = \"file\"\nurl = \"https://e.example/p.vim\"\n")
        .expect("a well-formed remote");
    BTreeMap::from([(
        ItemId::try_from("pathogen".to_owned()).expect("valid ID"),
        remote,
    )])
}

#[test]
fn a_file_remote_is_named_whole_and_never_as_a_directory() {
    let remotes = declaring_a_file();
    let check =
        |written: &str, shape| check_source(&remote_path(written), shape, &record(), &remotes);
    assert!(check("@pathogen", SourceShape::Any).is_ok());
    let structured: BTreeMap<String, RepoPath> =
        toml::from_str("source = { remote = \"pathogen\" }\n").expect("a well-formed reference");
    assert!(check_source(&structured["source"], SourceShape::Any, &record(), &remotes).is_ok());

    let inside = check("@pathogen/autoload/pathogen.vim", SourceShape::Any)
        .expect_err("a file remote has no paths inside it")
        .to_string();
    assert!(inside.contains("which is a single file"), "{inside}");
    assert!(inside.contains("write `@pathogen`"), "{inside}");

    let directory = check("@pathogen", SourceShape::Directory)
        .expect_err("a file remote is not a directory")
        .to_string();
    assert!(
        directory.contains("installs from a directory"),
        "{directory}"
    );
}

#[test]
fn an_inclusion_names_a_git_remote() {
    let pathogen = ItemId::try_from("pathogen".to_owned()).expect("valid ID");
    let message = check_inclusion_remote(&pathogen, &record(), &declaring_a_file())
        .expect_err("a file remote has no manifest")
        .to_string();
    assert!(
        message.contains("remote `pathogen` is of type `file`"),
        "{message}"
    );
    assert!(message.contains("manifest of a `git` remote"), "{message}");
}

#[test]
fn an_inclusion_may_only_name_a_remote_the_manifest_declares() {
    let core = ItemId::try_from("core".to_owned()).expect("valid ID");
    assert!(check_inclusion_remote(&core, &record(), &declaring(&["core"])).is_ok());
    let refused = check_inclusion_remote(&core, &record(), &declaring(&["work"]))
        .expect_err("expected an undeclared remote to be refused");
    let message = refused.to_string();
    assert!(message.contains("is not declared"), "{message}");
    assert!(message.contains("[remotes.core]"), "{message}");
}

/// Parse and validate an inclusion's filters.
fn filters(written: &str) -> Result<(), ManifestError> {
    let action: IncludeRemoteAction =
        toml::from_str(&format!("id = \"corp\"\nremote = \"core\"\n{written}"))
            .expect("the record should parse");
    check_inclusion_filters(&action, &record())
}

#[test]
fn an_inclusion_may_select_once_and_then_narrow_it() {
    for accepted in [
        "",
        "install-actions = [\"zshrc\"]",
        "install-groups = [\"shell\"]",
        "exclude-groups = [\"gui\"]",
        "exclude-actions = [\"p10k\"]",
        "install-groups = [\"shell\"]\nexclude-actions = [\"p10k\"]",
        "exclude-groups = [\"gui\"]\nexclude-actions = [\"p10k\"]",
    ] {
        assert!(
            filters(accepted).is_ok(),
            "`{accepted}` was refused: {:?}",
            filters(accepted).expect_err("just checked")
        );
    }
}

#[test]
fn an_inclusion_may_not_describe_its_selection_twice() {
    for (refused, one, other) in [
        (
            "install-actions = [\"zshrc\"]\ninstall-groups = [\"shell\"]",
            "install-actions",
            "install-groups",
        ),
        (
            "install-actions = [\"zshrc\"]\nexclude-groups = [\"gui\"]",
            "install-actions",
            "exclude-groups",
        ),
        (
            "install-groups = [\"shell\"]\nexclude-groups = [\"gui\"]",
            "install-groups",
            "exclude-groups",
        ),
        (
            "install-actions = [\"zshrc\"]\nexclude-actions = [\"p10k\"]",
            "install-actions",
            "exclude-actions",
        ),
    ] {
        let message = filters(refused)
            .expect_err("the combination should be refused")
            .to_string();
        assert!(message.contains(&format!("`{one}`")), "{message}");
        assert!(message.contains(&format!("`{other}`")), "{message}");
    }
}

#[test]
fn an_empty_filter_is_written_as_much_as_a_full_one() {
    // Even an empty allow-list counts as a declared selector.
    assert!(filters("install-actions = []").is_ok());
    assert!(filters("install-actions = []\ninstall-groups = []").is_err());
}

#[test]
fn an_included_action_installs_from_the_repository_that_declared_it() {
    assert!(check_included_source(&local("files/zshrc"), &record()).is_ok());
    let message = check_included_source(&remote_path("@shared/vimrc"), &record())
        .expect_err("expected a remote reference to be refused")
        .to_string();
    assert!(message.contains("`@shared/vimrc`"), "{message}");
    assert!(message.contains("names a remote"), "{message}");
    assert!(message.contains("leaf repository"), "{message}");
}

#[test]
fn a_dest_may_be_anywhere_the_selected_home_can_reach() {
    for accepted in ["~", "~/.zshrc", ".zshrc", "/etc/hosts", "~/../shared/rc"] {
        assert!(
            ValueRule::Destination.check(accepted).is_ok(),
            "`{accepted}` was refused"
        );
    }
}

#[test]
fn a_dest_is_written_out_rather_than_left_empty() {
    assert!(matches!(
        ValueRule::Destination
            .check("")
            .expect_err("expected an empty dest to be refused"),
        ValueError::DestinationEmpty
    ));
}

#[test]
fn a_fetched_source_names_a_scheme_batfiles_can_fetch() {
    for accepted in [
        "http://example.com/a",
        "https://example.com/a",
        "HTTPS://EXAMPLE.COM/A",
        "https://example.com/a b?c=d#e",
    ] {
        assert!(
            ValueRule::Url.check(accepted).is_ok(),
            "`{accepted}` was refused"
        );
    }
}

#[test]
fn a_fetched_source_that_is_not_a_url_is_refused_as_written() {
    for refused in [
        "files/ackrc",
        "example.com/a",
        "",
        "https://",
        "ftp://a/b",
        "💥💥x",
        "日本語のパス",
        "ht💥p://example.com/a",
    ] {
        assert!(
            matches!(
                ValueRule::Url
                    .check(refused)
                    .expect_err("expected the source to be refused"),
                ValueError::NotAUrl
            ),
            "`{refused}` was accepted"
        );
    }
}

#[test]
#[cfg(unix)]
fn a_file_url_names_a_file_on_this_machine() {
    assert!(ValueRule::Url.check("file:///etc/hosts").is_ok());
    let message = ValueRule::Url
        .check("file://server/etc/hosts")
        .expect_err("expected a file URL naming a host to be refused")
        .to_string();
    assert!(
        message.starts_with("names a host other than `localhost`"),
        "{message}"
    );
}

#[test]
fn a_digest_is_sixty_four_hexadecimal_digits_or_absent() {
    let sha = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    assert!(ValueRule::Digest.check(sha).is_ok());
    assert!(
        ValueRule::Digest.check(&sha.to_uppercase()).is_ok(),
        "a digest written in capitals is the same digest"
    );
    for refused in [&sha[..63], &format!("{sha}0")[..], &sha.replace('e', "g")] {
        assert!(
            matches!(
                ValueRule::Digest
                    .check(refused)
                    .expect_err("expected the digest to be refused"),
                ValueError::DigestNotSha256
            ),
            "`{refused}` was accepted"
        );
    }
}

#[test]
fn a_release_host_is_a_base_the_release_path_can_be_appended_to() {
    for accepted in [
        "https://github.com",
        "HTTPS://ghe.example/",
        "http://127.0.0.1:8080",
        "https://[::1]:8443/github",
        "https://ghe.example/a@b",
    ] {
        assert!(
            ValueRule::ReleaseHost.check(accepted).is_ok(),
            "`{accepted}` was refused"
        );
    }
    for (refused, reason) in [
        ("ftp://ghe.example", "is not an http:// or https:// URL"),
        ("https://", "is not an http:// or https:// URL"),
        ("https:///", "names no host"),
        ("https://:443/x", "names no host"),
        ("https://user:token@ghe.example", "contains credentials"),
        ("https://token@ghe.example/x?y", "contains credentials"),
        ("https://ghe.example?a=1", "has a query or fragment"),
        ("https://ghe.example/path#top", "has a query or fragment"),
    ] {
        let message = ValueRule::ReleaseHost
            .check(refused)
            .expect_err("expected the host to be refused")
            .to_string();
        assert!(message.starts_with(reason), "`{refused}`: {message}");
        assert!(!message.contains("token"), "{message}");
    }
}

#[test]
fn an_archive_root_names_a_prefix_or_asks_for_the_only_one() {
    for accepted in [
        None,
        Some("*"),
        Some("tool-1.0"),
        Some("tool-1.0/bin"),
        Some("./tool-1.0"),
    ] {
        assert!(
            check_archive_root(accepted, &record()).is_ok(),
            "`{accepted:?}` was refused"
        );
    }
}

#[test]
fn an_archive_root_that_could_match_no_entry_is_refused_as_written() {
    for refused in ["", "/tool", "../tool", ".", "tool/..", "releases/../tool"] {
        assert!(
            matches!(
                check_archive_root(Some(refused), &record())
                    .expect_err("expected the archive root to be refused"),
                ManifestError::ArchiveRootNotInside { .. }
            ),
            "`{refused}` was accepted"
        );
    }
}

#[test]
fn only_the_selected_home_is_spelled_with_a_tilde() {
    assert!(matches!(
        ValueRule::Destination
            .check("~other/.zshrc")
            .expect_err("expected another user's home to be refused"),
        ValueError::DestinationOtherHome
    ));
}
