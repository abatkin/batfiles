//! End-to-end tests of the built `batfiles` binary, grouped by behavior with shared fixtures in
//! `support`.

mod support;

mod apply;
mod bootstrap;
mod clone;
mod clone_list_addresses;
mod clone_lists;
mod conditions;
mod create_dir_and_copy;
mod decompressing;
mod disabled;
mod documentation;
mod fetch_actions;
mod git_clone_action;
mod git_remotes;
mod github_release;
mod groups;
mod inclusion;
mod inclusion_addresses;
mod inclusion_clone_lists;
mod inclusion_filters;
mod inclusion_vars;
mod init;
mod locations;
mod manifest;
mod refresh_content;
mod run_launch;
mod run_lock;
mod selection;
mod surface;
mod templates;
mod unzipping;
mod update;
mod vars;

#[cfg(unix)]
mod conflicts;
#[cfg(unix)]
mod dry_run;
#[cfg(unix)]
mod dynamic_vars;
#[cfg(unix)]
mod file_and_archive_remotes;
// Symlink actions, which are Unix-only.
#[cfg(unix)]
mod inclusion_composition;
#[cfg(unix)]
mod linking;
#[cfg(unix)]
mod remote_origins;
// Commands run through `sh`.
#[cfg(unix)]
mod run_action;
#[cfg(unix)]
mod vars_refresh;
