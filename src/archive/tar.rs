//! Tar entries, from a plain or decompressed stream, in archive order.

use std::io;

use super::compressed::Compression;
use super::{Archive, ArchiveError, Entry, EntryKind, display};
use crate::error::Error;

/// Read the tar in `reader`, from its start, handing each entry to `visit`.
pub(super) fn for_each_entry(
    archive: &Archive<'_>,
    compression: Compression,
    reader: impl io::Read + 'static,
    visit: &mut dyn FnMut(Entry<'_>) -> Result<(), Error>,
) -> Result<(), Error> {
    let mut tar = ::tar::Archive::new(compression.decoder(reader));
    for entry in tar.entries().map_err(|source| archive.unreadable(source))? {
        let mut entry = entry.map_err(|source| archive.unreadable(source))?;
        let path = entry
            .path()
            .map(|path| path.into_owned())
            .map_err(|source| archive.unreadable(source))?;
        let kind = kind_of(&entry, archive)?;
        let mode = entry.header().mode().ok();
        visit(Entry {
            path,
            kind,
            mode,
            content: &mut entry,
        })?;
    }
    Ok(())
}

/// Which kind an entry is, refusing the ones batfiles has no way to install.
fn kind_of<R: io::Read>(
    entry: &::tar::Entry<'_, R>,
    archive: &Archive<'_>,
) -> Result<EntryKind, Error> {
    let entry_type = entry.header().entry_type();
    if entry_type.is_pax_global_extensions()
        || entry_type.is_pax_local_extensions()
        || entry_type.is_gnu_longname()
        || entry_type.is_gnu_longlink()
    {
        return Ok(EntryKind::Metadata);
    }
    if entry_type.is_dir() {
        return Ok(EntryKind::Directory);
    }
    if entry_type.is_file() {
        return Ok(EntryKind::File);
    }
    let unsupported = || {
        archive.fault(ArchiveError::UnsupportedEntry {
            entry: entry
                .path()
                .map_or_else(|_| String::from("?"), |path| display(&path)),
        })
    };
    let Some(target) = entry.link_name().ok().flatten() else {
        return Err(unsupported());
    };
    if entry_type.is_symlink() {
        Ok(EntryKind::Symlink(target.into_owned()))
    } else if entry_type.is_hard_link() {
        Ok(EntryKind::Hardlink(target.into_owned()))
    } else {
        Err(unsupported())
    }
}
