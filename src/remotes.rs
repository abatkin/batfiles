//! Evaluate remote conditions and materialize Git repositories, files, and archives. Sync
//! processes all admitted remotes, including unreferenced ones; apply commands use existing
//! materializations.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::action::RunContext;
use crate::archive;
use crate::condition::{Bindings, Exclusion};
use crate::entry_filter::{EntryFilter, Executable, GlobFilter};
use crate::error::Error;
use crate::fetch;
use crate::git;
use crate::install;
use crate::item::ItemId;
use crate::manifest::remote::{ArchiveRemote, FileRemote, Remote};
use crate::output::Verb;
use crate::paths;
use crate::tomlfile;

/// The tool-owned directory inside the leaf repository that every
/// materialization sits under.
pub(crate) const DIRECTORY: &str = "remotes";

/// Warning suffix for a remote whose condition cannot be evaluated.
const NOT_MATERIALIZED: &str = "it is not materialized";

/// The suffix naming a fetched materialization's stamp, beside it.
const STAMP_SUFFIX: &str = ".batfiles-source";

/// Return `remotes/<id>` under the anchored leaf root `repository`.
pub(crate) fn materialization(repository: &Path, id: &ItemId) -> PathBuf {
    repository.join(DIRECTORY).join(id.as_str())
}

/// Evaluate declared remote conditions without I/O.
/// Returns excluded remotes and their reasons, even when an old tree exists.
/// Used by sync and apply commands to prevent reads from excluded remotes.
pub(crate) fn excluded(
    remotes: &BTreeMap<ItemId, Remote>,
    bindings: &Bindings<'_>,
) -> BTreeMap<ItemId, Exclusion> {
    remotes
        .iter()
        .filter_map(|(id, remote)| {
            let exclusion = remote.gate()?.exclusion(bindings, Some(NOT_MATERIALIZED))?;
            Some((id.clone(), exclusion))
        })
        .collect()
}

/// Materialize admitted remotes in ID order. Update Git clones; fetch missing or changed
/// file/archive declarations, or all of them when `refresh` is set.
///
/// Report exclusions without reading their trees. Dry runs report intent without Git or
/// downloads. Stop on the first failure.
pub(crate) fn materialize(
    remotes: &BTreeMap<ItemId, Remote>,
    context: &RunContext<'_>,
    refresh: bool,
) -> Result<(), Error> {
    for (id, remote) in remotes {
        let heading = format!("remote {id}");
        if let Some(exclusion) = context.excluded_remote(id) {
            exclusion.report_heading(context.reporter(), &heading);
            continue;
        }
        context.reporter().detail(1, &heading);
        let dest = context.materialization(id);
        match remote {
            Remote::Git(remote) => {
                git::clone_or_update(
                    &remote.url,
                    &dest,
                    remote.git_ref.as_deref(),
                    context.repository(),
                    &context.tool_owned(),
                )?;
                // A successful Git materialization invalidates any earlier file/archive stamp.
                if context.mode().writes() {
                    tomlfile::remove(&stamp_path(&dest))?;
                }
            }
            Remote::File(remote) => {
                fetch(id, Stamp::file(remote), None, None, &dest, refresh, context)?;
            }
            Remote::Archive(remote) => {
                let filter = EntryFilter::new(remote.include.as_ref(), remote.exclude.as_ref());
                let executable = Executable::new(remote.executable.as_ref());
                let wanted = Stamp::archive(remote);
                fetch(id, wanted, filter, executable, &dest, refresh, context)?;
            }
        }
    }
    Ok(())
}

/// Fetch a file or archive unless its stamp matches `wanted` and `refresh` is false. Refuse
/// occupied destinations without a matching ownership stamp.
fn fetch(
    id: &ItemId,
    wanted: Stamp,
    mut filter: Option<EntryFilter<'_>>,
    mut executable: Option<Executable<'_>>,
    dest: &Path,
    refresh: bool,
    context: &RunContext<'_>,
) -> Result<(), Error> {
    let stamp = stamp_path(dest);
    let found = FilesystemEntryKind::at(dest)?;
    let recorded = read_stamp(&stamp)?;
    let (mode, reporter) = (context.mode(), context.reporter());

    let replacing = match decide(found.as_ref(), recorded.as_ref(), &wanted, refresh) {
        Plan::Unchanged => {
            reporter.detail(1, &format!("unchanged {}", dest.display()));
            return Ok(());
        }
        Plan::Refuse(found) => {
            return Err(Error::MaterializationNotFetched {
                remote: id.clone(),
                path: dest.to_path_buf(),
                found,
            });
        }
        Plan::Fetch => false,
        Plan::Replace => true,
    };

    let url = wanted.url();
    let mut extracted = false;
    match &wanted {
        Stamp::File {
            sha256,
            executable,
            decompress,
            ..
        } => install::rebuild_file(dest, &context.tool_owned(), |file, at| {
            let source = fetch::FileSource {
                url,
                sha256: sha256.as_deref(),
                executable: *executable,
                decompress: *decompress,
            };
            fetch::fetch_file(&source, file, at, dest, reporter)
        })?,
        Stamp::Archive {
            sha256,
            archive_root,
            ..
        } => install::rebuild_directory(dest, &context.tool_owned(), |staging| {
            install::with_scratch(dest, reporter, |scratch| {
                let at = scratch.path().to_path_buf();
                fetch::download(url, sha256.as_deref(), scratch, &at)?;
                extracted = true;
                archive::extract(
                    scratch.file(),
                    staging,
                    archive_root.as_deref(),
                    filter.as_mut(),
                    executable.as_mut(),
                    url,
                )
            })
        })?,
    }
    if mode.writes() {
        tomlfile::write(&stamp, &wanted).map_err(|error| Error::StampNotWritten {
            remote: id.clone(),
            path: dest.to_path_buf(),
            source: Box::new(error),
        })?;
    }

    let verb = match (&wanted, replacing) {
        (_, true) => Verb::Refetch,
        (Stamp::File { .. }, false) => Verb::Fetch,
        (Stamp::Archive { .. }, false) => Verb::Extract,
    };
    reporter.info(&format!(
        "{} {} from {url}",
        verb.for_mode(mode),
        dest.display()
    ));
    if extracted {
        if let Some(filter) = &filter {
            filter.report_unmatched(url, reporter);
        }
        if let Some(executable) = &executable {
            executable.report_unmatched(url, reporter);
        }
    }
    Ok(())
}

/// Return the stamp path beside a materialization.
fn stamp_path(dest: &Path) -> PathBuf {
    let mut name = dest.file_name().unwrap_or_default().to_os_string();
    name.push(STAMP_SUFFIX);
    dest.with_file_name(name)
}

/// The stamp at `path`, or `None` where there is none or it is not one this
/// build wrote. Any other failure to read it is an error.
fn read_stamp(path: &Path) -> Result<Option<Stamp>, Error> {
    match tomlfile::read(path) {
        Ok(stamp) => Ok(Some(stamp)),
        Err(error) if error.is_not_found() => Ok(None),
        Err(Error::Parse { .. }) => Ok(None),
        Err(error) => Err(error),
    }
}

/// The declaration a file or archive materialization was fetched from, written
/// beside it once it is in place. A materialization whose stamp matches the
/// manifest is current; one with no stamp is not batfiles' to replace.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "kebab-case",
    deny_unknown_fields
)]
enum Stamp {
    File {
        url: String,
        sha256: Option<String>,
        #[serde(default, skip_serializing_if = "is_false")]
        executable: bool,
        #[serde(default, skip_serializing_if = "is_false")]
        decompress: bool,
    },
    Archive {
        url: String,
        sha256: Option<String>,
        archive_root: Option<String>,
        include: Option<Vec<String>>,
        exclude: Option<Vec<String>>,
        executable: Option<Vec<String>>,
    },
}

impl Stamp {
    /// Create a file-remote stamp with a lowercase digest.
    fn file(remote: &FileRemote) -> Self {
        Self::File {
            url: remote.url.clone(),
            sha256: digest(remote.sha256.as_deref()),
            executable: remote.executable,
            decompress: remote.decompress,
        }
    }

    /// Create an archive-remote stamp with a lowercase digest.
    fn archive(remote: &ArchiveRemote) -> Self {
        Self::Archive {
            url: remote.url.clone(),
            sha256: digest(remote.sha256.as_deref()),
            archive_root: remote.archive_root.clone(),
            include: written(remote.include.as_ref()),
            exclude: written(remote.exclude.as_ref()),
            executable: written(remote.executable.as_ref()),
        }
    }

    fn url(&self) -> &str {
        match self {
            Self::File { url, .. } | Self::Archive { url, .. } => url,
        }
    }

    /// Whether `found` is the kind of node a materialization of this type is.
    fn describes(&self, found: &FilesystemEntryKind) -> bool {
        matches!(
            (self, found),
            (Self::File { .. }, FilesystemEntryKind::File)
                | (Self::Archive { .. }, FilesystemEntryKind::Directory)
        )
    }
}

/// A declared filter's patterns as a stamp records them: a list, however the manifest
/// spelled it.
fn written(filter: Option<&GlobFilter>) -> Option<Vec<String>> {
    filter.map(|filter| {
        filter
            .as_slice()
            .iter()
            .map(|pattern| pattern.as_str().to_owned())
            .collect()
    })
}

/// Whether a flag is unset, and so left out of a stamp.
fn is_false(flag: &bool) -> bool {
    !flag
}

/// A declared digest as a stamp records it.
fn digest(sha256: Option<&str>) -> Option<String> {
    sha256.map(str::to_ascii_lowercase)
}

/// What is at a materialization's path, where something is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FilesystemEntryKind {
    File,
    Directory,
    Symlink,
    Other,
}

impl FilesystemEntryKind {
    /// Inspect the node kind at `dest` without following its final symlink.
    fn at(dest: &Path) -> Result<Option<Self>, Error> {
        Ok(paths::symlink_metadata_if_present(dest)?.map(|node| {
            if node.is_symlink() {
                Self::Symlink
            } else if node.is_file() {
                Self::File
            } else if node.is_dir() {
                Self::Directory
            } else {
                Self::Other
            }
        }))
    }
}

impl fmt::Display for FilesystemEntryKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::File => "a regular file",
            Self::Directory => "a directory",
            Self::Symlink => "a symlink",
            Self::Other => "neither a regular file nor a directory",
        })
    }
}

/// What to do about one file or archive remote.
#[derive(Debug, PartialEq, Eq)]
enum Plan {
    /// Nothing is there: fetch it.
    Fetch,
    /// There as declared.
    Unchanged,
    /// An earlier fetch of another declaration: fetch again and replace it.
    Replace,
    /// Something no stamp claims, which is not batfiles' to replace.
    Refuse(FilesystemEntryKind),
}

/// Decide from what is at the materialization's path and the stamp beside it.
/// Only a node of the kind its stamp records is batfiles' own, and `refresh`
/// replaces one that is current.
fn decide(
    found: Option<&FilesystemEntryKind>,
    recorded: Option<&Stamp>,
    wanted: &Stamp,
    refresh: bool,
) -> Plan {
    let Some(found) = found else {
        return Plan::Fetch;
    };
    match recorded {
        Some(recorded) if recorded.describes(found) => {
            if recorded == wanted && !refresh {
                Plan::Unchanged
            } else {
                Plan::Replace
            }
        }
        _ => Plan::Refuse(*found),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(url: &str) -> Stamp {
        Stamp::File {
            url: url.to_owned(),
            sha256: None,
            executable: false,
            decompress: false,
        }
    }

    fn archive(url: &str) -> Stamp {
        Stamp::Archive {
            url: url.to_owned(),
            sha256: None,
            archive_root: Some("*".to_owned()),
            include: None,
            exclude: None,
            executable: None,
        }
    }

    #[test]
    fn nothing_there_is_fetched_whatever_a_stamp_says() {
        let wanted = file("https://e.example/a");
        assert_eq!(decide(None, None, &wanted, false), Plan::Fetch);
        assert_eq!(
            decide(None, Some(&archive("x")), &wanted, false),
            Plan::Fetch
        );
    }

    #[test]
    fn a_stamped_materialization_is_current_or_replaced() {
        let wanted = file("https://e.example/a");
        assert_eq!(
            decide(
                Some(&FilesystemEntryKind::File),
                Some(&wanted),
                &wanted,
                false
            ),
            Plan::Unchanged
        );
        assert_eq!(
            decide(
                Some(&FilesystemEntryKind::File),
                Some(&file("https://e.example/old")),
                &wanted,
                false
            ),
            Plan::Replace
        );
        // An archive fetched under the same ID before the record became a file.
        assert_eq!(
            decide(
                Some(&FilesystemEntryKind::Directory),
                Some(&archive("x")),
                &wanted,
                false
            ),
            Plan::Replace
        );
    }

    #[test]
    fn a_refresh_replaces_a_current_materialization_and_nothing_else() {
        let wanted = file("https://e.example/a");
        assert_eq!(
            decide(
                Some(&FilesystemEntryKind::File),
                Some(&wanted),
                &wanted,
                true
            ),
            Plan::Replace
        );
        assert_eq!(decide(None, None, &wanted, true), Plan::Fetch);
        assert_eq!(
            decide(Some(&FilesystemEntryKind::File), None, &wanted, true),
            Plan::Refuse(FilesystemEntryKind::File),
            "a refresh is no licence to replace what batfiles did not fetch"
        );
    }

    #[test]
    fn what_no_stamp_claims_is_refused() {
        let wanted = archive("https://e.example/a.tar.gz");
        // A clone left by a Git remote declared under the same ID.
        assert_eq!(
            decide(Some(&FilesystemEntryKind::Directory), None, &wanted, false),
            Plan::Refuse(FilesystemEntryKind::Directory)
        );
        // A stamp beside a node of the other kind claims nothing either.
        assert_eq!(
            decide(
                Some(&FilesystemEntryKind::Directory),
                Some(&file("x")),
                &wanted,
                false
            ),
            Plan::Refuse(FilesystemEntryKind::Directory)
        );
        assert_eq!(
            decide(
                Some(&FilesystemEntryKind::Symlink),
                Some(&wanted),
                &wanted,
                false
            ),
            Plan::Refuse(FilesystemEntryKind::Symlink)
        );
    }

    #[test]
    fn a_stamp_records_a_digest_however_the_manifest_capitalized_it() {
        let remote: FileRemote =
            toml::from_str("url = \"https://e.example/a\"\nsha256 = \"ABC\"\nwhen = \"work\"\n")
                .expect("a well-formed remote");
        assert_eq!(
            Stamp::file(&remote),
            Stamp::File {
                url: "https://e.example/a".to_owned(),
                sha256: Some("abc".to_owned()),
                executable: false,
                decompress: false,
            }
        );
    }

    #[test]
    fn an_executable_file_is_stamped_so_and_an_ordinary_one_says_nothing() {
        let executable: FileRemote =
            toml::from_str("url = \"https://e.example/a\"\nexecutable = true\n")
                .expect("a well-formed remote");
        let stamp = Stamp::file(&executable);
        assert_ne!(stamp, file("https://e.example/a"));
        let written = toml::to_string(&stamp).expect("a stamp");
        assert!(written.contains("executable = true"), "{written}");
        assert_eq!(
            toml::from_str::<Stamp>(&written).expect("it reads back"),
            stamp
        );
        let ordinary = toml::to_string(&file("https://e.example/a")).expect("a stamp");
        assert!(!ordinary.contains("executable"), "{ordinary}");
    }

    #[test]
    fn a_stamp_records_filters_as_lists_however_the_manifest_spelled_them() {
        let one: ArchiveRemote =
            toml::from_str("url = \"https://e.example/a.tar.gz\"\ninclude = \"bin\"\n")
                .expect("a well-formed remote");
        let listed: ArchiveRemote =
            toml::from_str("url = \"https://e.example/a.tar.gz\"\ninclude = [\"bin\"]\n")
                .expect("a well-formed remote");
        assert_eq!(Stamp::archive(&one), Stamp::archive(&listed));
        assert_ne!(Stamp::archive(&one), archive("https://e.example/a.tar.gz"));
    }

    #[test]
    fn a_stamp_without_filters_does_not_mention_them() {
        let written = toml::to_string(&archive("https://e.example/a.tar.gz")).expect("a stamp");
        assert!(!written.contains("include"), "{written}");
        assert!(!written.contains("exclude"), "{written}");
        assert!(!written.contains("executable"), "{written}");
    }

    #[test]
    fn marking_files_executable_is_part_of_what_an_archive_was_fetched_from() {
        let marking: ArchiveRemote = toml::from_str(
            "url = \"https://e.example/a.tar.gz\"\narchive-root = \"*\"\nexecutable = \"bin\"\n",
        )
        .expect("a well-formed remote");
        assert_ne!(
            Stamp::archive(&marking),
            archive("https://e.example/a.tar.gz")
        );
        let earlier =
            "type = \"archive\"\nurl = \"https://e.example/a.tar.gz\"\narchive-root = \"*\"\n";
        assert_eq!(
            toml::from_str::<Stamp>(earlier).expect("a stamp from before `executable`"),
            archive("https://e.example/a.tar.gz")
        );
    }

    #[test]
    fn a_stamp_reads_back_as_it_was_written() {
        let stamp = Stamp::Archive {
            url: "https://e.example/a.tar.gz".to_owned(),
            sha256: None,
            archive_root: Some("*".to_owned()),
            include: Some(vec!["bin/*".to_owned()]),
            exclude: Some(vec![]),
            executable: Some(vec!["bin".to_owned()]),
        };
        let written = toml::to_string(&stamp).expect("a stamp serializes");
        assert!(written.contains("archive-root = \"*\""), "{written}");
        assert_eq!(
            toml::from_str::<Stamp>(&written).expect("it reads back"),
            stamp
        );
    }
}
