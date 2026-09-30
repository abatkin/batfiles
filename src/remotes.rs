//! Evaluate remote conditions and materialize declared remotes: clone or update
//! a Git repository, and fetch a file or unpack an archive where the manifest
//! declares one that is not already there as declared.
//! Sync materializes all non-excluded declarations, including unreferenced ones.
//! Apply commands read existing materializations through `RunContext`.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::action::RunContext;
use crate::archive;
use crate::condition::{Bindings, Exclusion};
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

/// What the warning for an undecidable remote condition says the run did about
/// it: the remote is not brought onto the machine, and nothing reads the tree
/// where an earlier run left one.
const NOT_MATERIALIZED: &str = "it is not materialized";

/// The suffix naming a fetched materialization's stamp, beside it.
const STAMP_SUFFIX: &str = ".batfiles-source";

/// Where the declared remote `id` is materialized: `remotes/<id>` inside the
/// leaf repository at the anchored path `repository`. The ID is a validated
/// path segment, so the result stays in the tree.
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

/// Materialize non-excluded remotes in ID order: clone or update a Git remote
/// under the Git update policy, and fetch a file or archive remote whose
/// materialization is missing or was fetched from another declaration, or
/// that `refresh` asks for regardless.
/// Report excluded remotes without touching their existing trees.
/// Dry runs report intent without launching Git or fetching. A failure stops
/// the run.
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
                // What is there now is a clone, so a stamp an earlier
                // declaration of another type left claims nothing.
                if context.mode().writes() {
                    tomlfile::remove(&stamp_path(&dest))?;
                }
            }
            Remote::File(remote) => fetch(id, Stamp::file(remote), &dest, refresh, context)?,
            Remote::Archive(remote) => {
                fetch(id, Stamp::archive(remote), &dest, refresh, context)?;
            }
        }
    }
    Ok(())
}

/// Bring a file or archive remote to `dest` unless the stamp beside it says it
/// is already there as `wanted` and `refresh` does not ask for it anyway, and
/// refuse a node there that no stamp claims.
fn fetch(
    id: &ItemId,
    wanted: Stamp,
    dest: &Path,
    refresh: bool,
    context: &RunContext<'_>,
) -> Result<(), Error> {
    let stamp = stamp_path(dest);
    let found = Found::at(dest)?;
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
    match &wanted {
        Stamp::File { sha256, .. } => {
            install::rebuild_file(dest, &context.tool_owned(), |file, at| {
                fetch::download_file(url, sha256.as_deref(), file, at)
            })?
        }
        Stamp::Archive {
            sha256,
            archive_root,
            ..
        } => install::rebuild_directory(dest, &context.tool_owned(), |staging| {
            install::with_scratch(dest, reporter, |scratch| {
                let at = scratch.path().to_path_buf();
                fetch::download(url, sha256.as_deref(), scratch, &at)?;
                archive::extract(scratch.file(), staging, archive_root.as_deref(), url)
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
    reporter.info(&format!("{} {} from {url}", verb.say(mode), dest.display()));
    Ok(())
}

/// Where the stamp for the materialization at `dest` is kept: beside it, under
/// a name no remote ID can take, since an ID holds no `.`.
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
    },
    Archive {
        url: String,
        sha256: Option<String>,
        archive_root: Option<String>,
    },
}

impl Stamp {
    /// What a materialization fetched for a file remote records. A digest is
    /// recorded in lowercase, so rewriting one in capitals refetches nothing.
    fn file(remote: &FileRemote) -> Self {
        Self::File {
            url: remote.url.clone(),
            sha256: digest(remote.sha256.as_deref()),
        }
    }

    /// What one fetched for an archive remote records, on the same terms.
    fn archive(remote: &ArchiveRemote) -> Self {
        Self::Archive {
            url: remote.url.clone(),
            sha256: digest(remote.sha256.as_deref()),
            archive_root: remote.archive_root.clone(),
        }
    }

    fn url(&self) -> &str {
        match self {
            Self::File { url, .. } | Self::Archive { url, .. } => url,
        }
    }

    /// Whether `found` is the kind of node a materialization of this type is.
    fn describes(&self, found: &Found) -> bool {
        matches!(
            (self, found),
            (Self::File { .. }, Found::File) | (Self::Archive { .. }, Found::Directory)
        )
    }
}

/// A declared digest as a stamp records it.
fn digest(sha256: Option<&str>) -> Option<String> {
    sha256.map(str::to_ascii_lowercase)
}

/// What is at a materialization's path, where something is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Found {
    File,
    Directory,
    Symlink,
    Other,
}

impl Found {
    /// Inspect `dest` without following a final symlink. Only the kind of node
    /// matters, since the stamp decides whose it is, so where a symlink points
    /// is never read.
    fn at(dest: &Path) -> Result<Option<Self>, Error> {
        Ok(paths::node_at(dest)?.map(|node| {
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

impl fmt::Display for Found {
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
    Refuse(Found),
}

/// Decide from what is at the materialization's path and the stamp beside it.
/// Only a node of the kind its stamp records is batfiles' own, and `refresh`
/// replaces one that is current.
fn decide(found: Option<&Found>, recorded: Option<&Stamp>, wanted: &Stamp, refresh: bool) -> Plan {
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
        }
    }

    fn archive(url: &str) -> Stamp {
        Stamp::Archive {
            url: url.to_owned(),
            sha256: None,
            archive_root: Some("*".to_owned()),
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
            decide(Some(&Found::File), Some(&wanted), &wanted, false),
            Plan::Unchanged
        );
        assert_eq!(
            decide(
                Some(&Found::File),
                Some(&file("https://e.example/old")),
                &wanted,
                false
            ),
            Plan::Replace
        );
        // An archive fetched under the same ID before the record became a file.
        assert_eq!(
            decide(Some(&Found::Directory), Some(&archive("x")), &wanted, false),
            Plan::Replace
        );
    }

    #[test]
    fn a_refresh_replaces_a_current_materialization_and_nothing_else() {
        let wanted = file("https://e.example/a");
        assert_eq!(
            decide(Some(&Found::File), Some(&wanted), &wanted, true),
            Plan::Replace
        );
        assert_eq!(decide(None, None, &wanted, true), Plan::Fetch);
        assert_eq!(
            decide(Some(&Found::File), None, &wanted, true),
            Plan::Refuse(Found::File),
            "a refresh is no licence to replace what batfiles did not fetch"
        );
    }

    #[test]
    fn what_no_stamp_claims_is_refused() {
        let wanted = archive("https://e.example/a.tar.gz");
        // A clone left by a Git remote declared under the same ID.
        assert_eq!(
            decide(Some(&Found::Directory), None, &wanted, false),
            Plan::Refuse(Found::Directory)
        );
        // A stamp beside a node of the other kind claims nothing either.
        assert_eq!(
            decide(Some(&Found::Directory), Some(&file("x")), &wanted, false),
            Plan::Refuse(Found::Directory)
        );
        assert_eq!(
            decide(Some(&Found::Symlink), Some(&wanted), &wanted, false),
            Plan::Refuse(Found::Symlink)
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
            }
        );
    }

    #[test]
    fn a_stamp_reads_back_as_it_was_written() {
        let stamp = archive("https://e.example/a.tar.gz");
        let written = toml::to_string(&stamp).expect("a stamp serializes");
        assert!(written.contains("archive-root = \"*\""), "{written}");
        assert_eq!(
            toml::from_str::<Stamp>(&written).expect("it reads back"),
            stamp
        );
    }
}
