//! Shared fixtures for the CLI test target.

mod archive;
mod filesystem;
mod git;
mod http;
mod manifests;
mod tree;

pub(crate) use archive::{Member, multi_member_tarball, plain_tarball, tarball, v7_tarball};
#[cfg(unix)]
pub(crate) use filesystem::link_target;
pub(crate) use filesystem::{
    backup_of, backups_of, copy_tree, display, entries, fixture_tree, snapshot,
};
pub(crate) use git::{BareRepo, git};
pub(crate) use http::{Reply, Server, server_that_hangs_up};
pub(crate) use manifests::{
    CORPORATE_ACTIONS, CorporateAction, LEAF_ORDERED_PAIR, assert_leaf_portable_actions,
    installed_corporate, one_copy, one_copy_dir, one_create_dir, one_symlink, one_symlink_dir,
    rejected, seeded_repository_in_the_home,
};
pub(crate) use tree::Tree;

use assert_cmd::Command;

/// Build a batfiles command with color, variable-override, and proxy environment inputs cleared.
pub(crate) fn batfiles() -> Command {
    let mut command = Command::cargo_bin("batfiles").expect("the batfiles binary should be built");
    // The tests must not inherit the developer's own color environment.
    command.env_remove("BATFILES_COLOR").env_remove("NO_COLOR");
    // Remove inherited overrides, including non-UTF-8 variables.
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("BATFILES_VAR_") {
            command.env_remove(&key);
        }
    }
    // Keep fixture requests on the loopback interface.
    for proxy in [
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
    ] {
        command.env_remove(proxy);
    }
    command
}

/// Encode `path` as a `file://` URL, escaping percent signs, spaces, `#`, and `?`.
pub(crate) fn file_url(path: &std::path::Path) -> String {
    let path = path.to_str().expect("fixture paths are UTF-8");
    let mut url = String::from("file://");
    // On Windows an absolute path starts with its drive, and a URL path with `/`.
    if !path.starts_with('/') {
        url.push('/');
    }
    for character in path.replace('\\', "/").chars() {
        match character {
            '%' => url.push_str("%25"),
            ' ' => url.push_str("%20"),
            '?' => url.push_str("%3F"),
            '#' => url.push_str("%23"),
            other => url.push(other),
        }
    }
    url
}

pub(crate) fn stderr_of(assertion: &assert_cmd::assert::Assert) -> String {
    String::from_utf8_lossy(&assertion.get_output().stderr).into_owned()
}

pub(crate) fn stdout_of(assertion: &assert_cmd::assert::Assert) -> String {
    String::from_utf8_lossy(&assertion.get_output().stdout).into_owned()
}
