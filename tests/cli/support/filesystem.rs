//! Filesystem snapshots and fixture copying.

use std::fs;
use std::path::{Path, PathBuf};

pub(crate) fn display(path: &Path) -> String {
    path.display().to_string()
}

/// Copy a directory tree, creating `to` and everything beneath it.
pub(crate) fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("a destination directory");
    for entry in fs::read_dir(from).expect("a fixture directory") {
        let entry = entry.expect("a directory entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("a file type").is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).expect("a fixture file");
        }
    }
}

/// The names directly inside a directory, sorted, for asserting that a run
/// installed everything it should have and nothing else.
pub(crate) fn entries(dir: &Path) -> Vec<String> {
    let mut found: Vec<String> = fs::read_dir(dir)
        .expect("a readable directory")
        .map(|entry| {
            entry
                .expect("a directory entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    found.sort();
    found
}

/// Everything under a directory: names, types, symlink targets as written, and
/// file contents, sorted.
pub(crate) fn snapshot(root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    record_into(root, root, &mut found);
    found.sort();
    found
}

fn record_into(root: &Path, dir: &Path, found: &mut Vec<String>) {
    for entry in fs::read_dir(dir).expect("a readable directory") {
        let entry = entry.expect("a directory entry");
        let path = entry.path();
        let name = display(path.strip_prefix(root).expect("a path under the root"));
        // Never followed: a symlink is a thing that is there, and what it
        // reaches is somebody else's part of the tree.
        let kind = entry.file_type().expect("a file type");
        if kind.is_symlink() {
            found.push(format!("{name} -> {}", display(&link_target(&path))));
        } else if kind.is_dir() {
            found.push(format!("{name}/"));
            record_into(root, &path, found);
        } else {
            let contents = fs::read(&path).expect("a readable file");
            found.push(format!("{name} = {}", String::from_utf8_lossy(&contents)));
        }
    }
}

/// Where a symlink points, without following it.
pub(crate) fn link_target(path: &Path) -> PathBuf {
    fs::read_link(path)
        .unwrap_or_else(|error| panic!("{} is not a symlink: {error}", path.display()))
}
