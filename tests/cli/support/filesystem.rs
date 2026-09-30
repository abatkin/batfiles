//! Filesystem snapshots and fixture copying.

use std::fs;
use std::path::{Path, PathBuf};

pub(crate) fn display(path: &Path) -> String {
    path.display().to_string()
}

/// Return the path to a committed fixture repository.
pub(crate) fn fixture_tree(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
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

/// Return sorted names of direct directory children.
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

/// The backups batfiles has made of `path`, beside it, sorted by name.
pub(crate) fn backups_of(path: &Path) -> Vec<PathBuf> {
    let parent = path.parent().expect("a path with a parent");
    let prefix = format!(
        "{}.batfiles-backup-",
        path.file_name()
            .expect("a path with a name")
            .to_string_lossy()
    );
    entries(parent)
        .into_iter()
        .filter(|name| name.starts_with(&prefix))
        .map(|name| parent.join(name))
        .collect()
}

/// The one backup batfiles has made of `path`.
pub(crate) fn backup_of(path: &Path) -> PathBuf {
    let found = backups_of(path);
    let [backup] = &found[..] else {
        panic!("expected one backup of {}, found {found:?}", path.display());
    };
    backup.clone()
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
