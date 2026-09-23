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
pub(crate) use filesystem::{copy_tree, display, entries, fixture_tree, snapshot};
pub(crate) use git::{BareRepo, git};
pub(crate) use http::{Reply, Server, server_that_hangs_up};
pub(crate) use manifests::{
    CORPORATE_ACTIONS, CorporateAction, LEAF_ORDERED_PAIR, assert_leaf_portable_actions,
    installed_corporate, one_copy, one_copy_dir, one_create_dir, one_symlink, one_symlink_dir,
    rejected, seeded_repository_in_the_home,
};
pub(crate) use tree::Tree;

use assert_cmd::Command;

/// A command with nothing selected, for the cases that resolve no roots:
/// `version`, `--help`, and anything clap rejects before dispatch.
pub(crate) fn batfiles() -> Command {
    let mut command = Command::cargo_bin("batfiles").expect("the batfiles binary should be built");
    // The tests must not inherit the developer's own color environment.
    command.env_remove("BATFILES_COLOR").env_remove("NO_COLOR");
    // Nor their `BATFILES_VAR_*` variables. Read with `vars_os`, as batfiles
    // does, so one non-UTF-8 variable cannot panic the suite.
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

pub(crate) fn stderr_of(assertion: &assert_cmd::assert::Assert) -> String {
    String::from_utf8_lossy(&assertion.get_output().stderr).into_owned()
}

pub(crate) fn stdout_of(assertion: &assert_cmd::assert::Assert) -> String {
    String::from_utf8_lossy(&assertion.get_output().stdout).into_owned()
}
