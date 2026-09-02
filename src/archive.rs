//! Unpacking a downloaded archive, for `fetch-archive`.
//!
//! Everything written lands inside the staging tree [`crate::install`] created,
//! so an archive that turns out to be malformed or hostile leaves the
//! destination exactly as it found it: the whole extraction is abandoned by
//! discarding one path.
//!
//! **The archive is read twice, and that is the point.** The first pass reads
//! every entry's path and refuses the archive if any of them would be written
//! outside the tree; only then does the second pass write anything. What it
//! costs is decompressing a local file again, and what it buys is that a hostile
//! entry is caught before its neighbors have been unpacked.
//!
//! Downstream of a mode reader, and structurally so: `install::seed` creates no
//! staging node under `DryRun`, so nothing here is reachable in that mode and no
//! `RunMode` is consulted (`guidance.md`, "Two lists, and why they are not the
//! same one").

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use flate2::read::MultiGzDecoder;
use thiserror::Error;

// Making a symlink is platform-specific, as it is in `action/symlink.rs`, and
// for the same reason it is not built for Windows. An archive entry that is one
// is refused there by name, so the stand-in below exists to keep the crate
// compiling rather than to be called.
#[cfg(unix)]
use std::os::unix::fs::symlink;

use crate::error::Error;

/// The first bytes of a gzip stream.
const GZIP_MAGIC: [u8; 2] = [0x1f, 0x8b];

/// How much of the file has to be read to tell one format from another: one tar
/// header block, which carries its own checksum.
const HEADER: usize = 512;

/// Where a tar header keeps the checksum of the block it is in.
const CHECKSUM: std::ops::Range<usize> = 148..156;

/// The value `archive-root` takes to mean "whatever the single top-level
/// directory turns out to be".
const DETECT_ROOT: &str = "*";

/// The permission bits an unpacked entry may carry.
///
/// The low nine and nothing above them: setuid, setgid, and the sticky bit are
/// dropped rather than honored. An archive comes from a URL and is unpacked into
/// the user's home, and nothing a dotfiles repository installs has any business
/// arriving with elevated privileges.
#[cfg(unix)]
const PERMISSION_BITS: u32 = 0o777;

/// The mode an entry is created with, before the archive's own is applied.
///
/// Closed, then widened once the entry is complete, the way every other staging
/// node in the crate is made (`guidance.md`, rule 15).
#[cfg(unix)]
const BUILDING_MODE: u32 = 0o600;

/// The mode the unpacked tree lands with when the archive does not say.
///
/// Most archives carry a directory entry for the root being stripped, and its
/// mode is the one that wins; this is for the ones that list only files. It has
/// to be *something*, because the staging directory is created closed and
/// publishing it as it stands would install a directory the owner alone could
/// enter. The umask is not consulted, for the same reason `fetch-file`'s `0644`
/// does not consult it: there is no portable way to read one without a libc
/// dependency, and this is what an ordinary umask would have produced.
#[cfg(unix)]
const UNSTATED_ROOT_MODE: u32 = 0o755;

/// Where a mode means something other than it does on unix, nothing is applied
/// and the value is never read.
#[cfg(not(unix))]
const UNSTATED_ROOT_MODE: u32 = 0;

#[cfg(not(unix))]
fn symlink(_target: &Path, _at: &Path) -> io::Result<()> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}

/// What an archive can be that stops it being unpacked.
///
/// Its own enum rather than seven more variants on the crate error: extraction
/// has vocabulary of its own — entries, roots, formats — none of which means
/// anything to a caller that is not unpacking something (`guidance.md`, rule 5).
///
/// Every variant names the URL rather than the file, because the file is a
/// staging path the user never chose and will never see again.
#[derive(Debug, Error)]
pub(crate) enum Invalid {
    /// Bytes that are not an archive batfiles unpacks, named by what they look
    /// like where that is recognizable: "a zip archive" tells the author what to
    /// do about it, and "not a tar archive" does not.
    #[error("{url} is {saw}, and `fetch-archive` unpacks tar archives, gzipped or plain")]
    Format { url: String, saw: &'static str },

    /// An archive of the right format that does not read as one.
    #[error("could not read the archive from {url}: {source}")]
    Unreadable { url: String, source: io::Error },

    /// An entry naming a path outside the tree being unpacked: an absolute path,
    /// one climbing out with `..`, or a link whose target does either. The whole
    /// extraction stops, because an archive carrying one of these is not one to
    /// install part of.
    #[error("the archive from {url} has an entry that would be written outside it: `{entry}`")]
    EscapingEntry { url: String, entry: String },

    /// An entry that is not a file, a directory, or a link. A device node or a
    /// fifo in a dotfiles archive is a bug or an attack, and skipping it would
    /// publish an incomplete tree as a finished one.
    #[error(
        "the archive from {url} has an entry that is neither a file, a directory, \
         nor a link: `{entry}`"
    )]
    UnsupportedEntry { url: String, entry: String },

    /// A symlink entry on a platform where batfiles does not make symlinks. Its
    /// own variant rather than an unsupported entry, because the archive is
    /// fine and the machine is what cannot take it.
    #[error(
        "the archive from {url} holds the symlink `{entry}`, and symlinks are not \
         supported on this platform"
    )]
    SymlinkEntry { url: String, entry: String },

    /// `archive-root = "*"` over an archive with no single top-level directory
    /// to strip. Which ones it has, because that is what the author writes in
    /// place of the `*`.
    #[error(
        "the archive from {url} has no single top-level directory for \
         `archive-root = \"{DETECT_ROOT}\"` to strip; it has {}. Name one of them instead",
        .found.join(", ")
    )]
    AmbiguousRoot { url: String, found: Vec<String> },

    /// An `archive-root` over an archive holding nothing under it.
    #[error("the archive from {url} has nothing under `{root}`")]
    EmptyRoot { url: String, root: String },

    /// An archive with no entries at all, which no `archive-root` is to blame
    /// for.
    #[error("the archive from {url} is empty")]
    Empty { url: String },
}

/// Unpack an archive into a directory this run created.
///
/// `at` is where the downloaded archive is, `into` is the staging tree being
/// built, and `root` is the manifest's `archive-root`. `url` is carried for the
/// diagnostics alone.
pub(crate) fn extract(at: &Path, into: &Path, root: Option<&str>, url: &str) -> Result<(), Error> {
    let format = format_of(at, url)?;
    let entries = inspect(at, format, root, url)?;
    unpack(at, format, &entries, into, url)
}

/// How the archive's bytes are wrapped, decided by reading them.
///
/// Not decided from the URL: a release URL redirects, carries a query string,
/// and is named by whoever published it, so what it appears to end in is a hint
/// rather than an answer.
#[derive(Debug, Clone, Copy)]
enum Format {
    Gzip,
    Plain,
}

/// One entry, as the first pass left it.
///
/// Everything the second pass needs to write it, and nothing it would have to
/// decide again: what it is, and where it goes.
struct Entry {
    /// Where it goes, once the root has been stripped.
    placement: Placement,
    /// What it is, and where a link points.
    kind: Kind,
}

/// Where an entry ends up in the tree being built.
enum Placement {
    /// At this path under the tree.
    At(PathBuf),
    /// The tree itself: the archive's own root entry, or the directory
    /// `archive-root` stripped. It is already there, so the only thing it has
    /// left to say is what mode it should carry.
    TheTreeItself,
    /// Outside a named `archive-root`, and therefore not installed at all.
    NotInstalled,
}

impl Placement {
    fn path(&self) -> Option<&Path> {
        match self {
            Self::At(path) => Some(path),
            Self::TheTreeItself | Self::NotInstalled => None,
        }
    }
}

/// What an entry is, and what a link entry resolves to.
enum Kind {
    File,
    Directory,
    /// The target exactly as the archive writes it, since that is what a symlink
    /// stores.
    Symlink(PathBuf),
    /// The target's place in the tree being built, root already stripped: a tar
    /// hardlink names a path within the archive rather than one relative to the
    /// entry, so it is stripped the way an entry path is.
    Hardlink(PathBuf),
    /// A pax or GNU extension header, which describes the entry after it rather
    /// than being one. Carried as a kind so that both passes skip the same
    /// entries by asking the same question.
    Metadata,
}

/// Read enough of the archive to say what it is, or refuse it by name.
fn format_of(at: &Path, url: &str) -> Result<Format, Error> {
    let head = head_of(at)?;
    if head.starts_with(&GZIP_MAGIC) {
        return Ok(Format::Gzip);
    }
    if is_tar_header(&head) {
        return Ok(Format::Plain);
    }
    Err(Invalid::Format {
        url: url.to_owned(),
        saw: looks_like(&head),
    }
    .into())
}

/// Whether a block is a tar header, by the checksum it carries of itself.
///
/// Not by looking for `ustar` at offset 257, which is a *format* rather than the
/// format: a V7 archive carries no magic there at all, so requiring it would
/// refuse an archive the reader after this one goes on to read perfectly well.
/// The checksum covers the whole block with its own field blanked, which makes
/// this a test the archive answers rather than a guess about what wrote it.
fn is_tar_header(head: &[u8]) -> bool {
    let Some(block) = head.get(..HEADER) else {
        return false;
    };
    let Some(declared) = octal(&block[CHECKSUM]) else {
        return false;
    };
    let (unsigned, signed) =
        block
            .iter()
            .enumerate()
            .fold((0u32, 0i32), |(unsigned, signed), (offset, &byte)| {
                // The field reads as spaces while it is being summed, since it
                // cannot hold the answer and be part of the question.
                let byte = if CHECKSUM.contains(&offset) {
                    b' '
                } else {
                    byte
                };
                (unsigned + u32::from(byte), signed + i32::from(byte as i8))
            });
    // Two answers because implementations disagreed about whether a byte is
    // signed, and a header holding a high byte hashes differently under each.
    // Either one is the archive agreeing with itself.
    declared == unsigned || i64::from(declared) == i64::from(signed)
}

/// A tar header's octal number field, which is digits, then padding.
fn octal(field: &[u8]) -> Option<u32> {
    let mut value: u32 = 0;
    let mut digits = 0;
    for byte in field.iter().skip_while(|byte| **byte == b' ') {
        if !(b'0'..=b'7').contains(byte) {
            break;
        }
        value = value.checked_mul(8)?.checked_add(u32::from(byte - b'0'))?;
        digits += 1;
    }
    (digits > 0).then_some(value)
}

/// What a run of leading bytes is, in the words a diagnostic uses.
///
/// The formats worth naming are the ones somebody plausibly wrote a
/// `fetch-archive` for: each is an archive, and each wants a different answer
/// from "this is not an archive at all".
fn looks_like(head: &[u8]) -> &'static str {
    for (magic, name) in [
        (&b"PK\x03\x04"[..], "a zip archive"),
        (&b"BZh"[..], "a bzip2 archive"),
        (&b"\xfd7zXZ\x00"[..], "an xz archive"),
        (&b"\x28\xb5\x2f\xfd"[..], "a zstd archive"),
        (&b"7z\xbc\xaf\x27\x1c"[..], "a 7-zip archive"),
    ] {
        if head.starts_with(magic) {
            return name;
        }
    }
    "not an archive batfiles recognizes"
}

/// The archive's first block, or as much of it as there is.
fn head_of(at: &Path) -> Result<Vec<u8>, Error> {
    use std::io::Read as _;

    let mut head = Vec::with_capacity(HEADER);
    fs::File::open(at)
        .and_then(|file| file.take(HEADER as u64).read_to_end(&mut head))
        .map_err(|source| Error::Read {
            path: at.to_path_buf(),
            source,
        })?;
    Ok(head)
}

/// Read every entry, check what will be written, and settle what the root
/// strips.
///
/// **Nothing is written here.** What comes back is one record per entry in
/// archive order, so the second pass creates without deciding anything.
///
/// Three sweeps over the same list, because each needs the answer before it.
/// The paths have to be read before the root can be detected, the root has to be
/// stripped before anything can be said about where an entry lands, and *every*
/// entry's placement has to be known before any one of them can be checked: what
/// makes a target safe depends on which paths the archive declares as symlinks,
/// and an archive is free to declare one after the entry that walks through it.
///
/// A path that leaves the archive is refused wherever it turns up, selected or
/// not — one spelled `../../.ssh/authorized_keys` is hostile whatever the
/// `archive-root` picks. The rest is asked only of the entries being installed,
/// so a fifo in a part of the archive nobody asked for is not a reason to refuse
/// the part they did.
fn inspect(at: &Path, format: Format, root: Option<&str>, url: &str) -> Result<Vec<Entry>, Error> {
    let mut read: Vec<(PathBuf, Kind)> = Vec::new();
    for_each_entry(at, format, url, |entry| {
        let written = archive_path(entry, url)?;
        let kind = kind_of(entry, url)?;
        if matches!(kind, Kind::Metadata) {
            read.push((written, kind));
            return Ok(());
        }
        // Named in the diagnostic as the archive writes it, so the entry can be
        // found in the archive it came from.
        let cleaned = entry_path(&written).ok_or_else(|| escaping(url, &written))?;
        read.push((cleaned, kind));
        Ok(())
    })?;

    let strip = root_prefix(&read, root, url)?;
    let placed: Vec<(PathBuf, Placement, Kind)> = read
        .into_iter()
        .map(|(written, kind)| {
            let placement = match kind {
                Kind::Metadata => Placement::NotInstalled,
                _ => place(&written, strip.as_deref()),
            };
            (written, placement, kind)
        })
        .collect();

    // Every symlink that will exist in the finished tree, which is what decides
    // whether a `..` elsewhere can be trusted.
    let links: BTreeSet<PathBuf> = placed
        .iter()
        .filter(|(_, _, kind)| matches!(kind, Kind::Symlink(_)))
        .filter_map(|(_, placement, _)| placement.path().map(Path::to_path_buf))
        .collect();

    let mut entries = Vec::with_capacity(placed.len());
    for (written, placement, kind) in placed {
        let kind = match placement.path() {
            None => kind,
            Some(inside) => resolve(kind, inside, strip.as_deref(), &links, &written, url)?,
        };
        entries.push(Entry { placement, kind });
    }

    if !entries
        .iter()
        .any(|entry| matches!(entry.placement, Placement::At(_)))
    {
        return Err(match strip {
            Some(root) => Invalid::EmptyRoot {
                url: url.to_owned(),
                root: display(&root),
            },
            None => Invalid::Empty {
                url: url.to_owned(),
            },
        }
        .into());
    }
    Ok(entries)
}

/// Check an entry that is going to be installed, and settle where a link points.
///
/// Two things are left to ask of one. **Nothing is written through a symlink the
/// archive itself declares**, so no ancestor of the path may be one: the kernel
/// follows a link before it creates what is under it, and where that link goes
/// is not where the archive said the entry was.
///
/// And a link's target has to reach somewhere inside, which is a question about
/// the archive as a whole rather than about the target's spelling. The two link
/// kinds answer to different rules because they mean different things: a
/// symlink's target is read from the directory the link ends up in, and a
/// hardlink's names another entry of the archive.
///
/// The hardlink's target is resolved here rather than at the write, because this
/// is where the root prefix is known: doing it later would mean recovering the
/// prefix from an entry that no longer carries it.
fn resolve(
    kind: Kind,
    inside: &Path,
    strip: Option<&Path>,
    links: &BTreeSet<PathBuf>,
    written: &Path,
    url: &str,
) -> Result<Kind, Error> {
    if walks_through_a_link(inside, links) {
        return Err(escaping(url, written));
    }
    match kind {
        Kind::File | Kind::Directory | Kind::Metadata => Ok(kind),
        Kind::Symlink(target) => {
            if cfg!(not(unix)) {
                return Err(Invalid::SymlinkEntry {
                    url: url.to_owned(),
                    entry: display(written),
                }
                .into());
            }
            // Read from where the link ends up, which is after the root strip
            // and therefore shallower than where the archive wrote it. Asking at
            // the archive's own depth would let `../../x` out of a stripped
            // tree.
            let from = inside.parent().unwrap_or_else(|| Path::new(""));
            if stays_inside(&from.join(&target), links) {
                Ok(Kind::Symlink(target))
            } else {
                Err(escaping(url, written))
            }
        }
        // Three ways for one of these to point at nothing this run will create,
        // and they are one refusal: a target that leaves the archive altogether,
        // one that is merely outside the selected root, and one reached through
        // a symlink. An archive whose links do not resolve is not one to
        // install.
        //
        // A hardlink's target is an archive path, spelled the way the entry it
        // names is spelled, so it answers to the entry rule rather than the
        // symlink one.
        Kind::Hardlink(target) => entry_path(&target)
            .map(|target| place(&target, strip))
            .and_then(|placement| placement.path().map(Path::to_path_buf))
            .filter(|target| !walks_through_a_link(target, links))
            .map(Kind::Hardlink)
            .ok_or_else(|| escaping(url, written)),
    }
}

/// The prefix every entry is written without, resolved from what the manifest
/// asked for.
///
/// `*` is the only value that has to look at the archive: it means the single
/// top-level directory a release tarball usually has, so an archive with two of
/// them is refused rather than guessed at.
fn root_prefix(
    read: &[(PathBuf, Kind)],
    root: Option<&str>,
    url: &str,
) -> Result<Option<PathBuf>, Error> {
    let Some(root) = root else {
        return Ok(None);
    };
    if root != DETECT_ROOT {
        // Cleaned the way an entry path is, because that is what it is matched
        // against: a root written `./tool` carries a component no entry path has
        // and would match nothing at all. The panic states the agreement with
        // the manifest rather than defending against it — the two rules are
        // written against each other, and a root this could reject was refused
        // before anything was fetched.
        return Ok(Some(entry_path(Path::new(root)).expect(
            "the manifest refuses an archive-root that is not a path inside an archive",
        )));
    }
    let tops: BTreeSet<String> = read
        .iter()
        .filter(|(_, kind)| !matches!(kind, Kind::Metadata))
        .filter_map(|(path, _)| path.components().next())
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect();
    match tops.len() {
        0 => Err(Invalid::Empty {
            url: url.to_owned(),
        }
        .into()),
        1 => Ok(tops.into_iter().next().map(PathBuf::from)),
        _ => Err(Invalid::AmbiguousRoot {
            url: url.to_owned(),
            found: tops.into_iter().collect(),
        }
        .into()),
    }
}

/// Where an entry lands once the root has been stripped.
///
/// Two ways to be left with nothing, and they are not the same thing: an entry
/// that *is* the root is the tree, which already exists and still has a mode to
/// give it, and one outside a named prefix is not being installed at all.
fn place(path: &Path, strip: Option<&Path>) -> Placement {
    let remainder = match strip {
        Some(prefix) => match path.strip_prefix(prefix) {
            Ok(remainder) => remainder,
            Err(_) => return Placement::NotInstalled,
        },
        None => path,
    };
    match remainder.components().next() {
        Some(_) => Placement::At(remainder.to_path_buf()),
        None => Placement::TheTreeItself,
    }
}

/// A path inside an archive, with its `.` components dropped, or `None` where it
/// is not one batfiles will write.
///
/// An empty result is the archive's own root: `tar czf x.tgz .` — which is how
/// most archives are made — writes it as `./`, and every entry under it with a
/// leading `./` that has to come off before anything is matched against
/// anything. `archive-root = "*"` would otherwise find `.` at the top of every
/// path and strip that instead of the directory it was meant to.
///
/// **`..` is refused rather than cancelled.** Cancelling it on paper is right
/// only when the component before it is a real directory, and an archive is free
/// to declare a symlink there instead — after which the kernel goes up from
/// wherever the link landed and the paper answer is somewhere else entirely. No
/// archive worth installing writes one: GNU tar will not create one, and neither
/// will the library this reads them with. So there is nothing to weigh against
/// refusing them outright, and one rule beats a cancellation that is right most
/// of the time.
fn entry_path(path: &Path) -> Option<PathBuf> {
    let mut cleaned = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(name) => cleaned.push(name),
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(cleaned)
}

/// Whether a path passes *through* one of the archive's own symlinks.
///
/// Nothing is created under one. The kernel follows a link before it creates
/// what is below it, so an entry written there does not land where the archive
/// says it does — and where the link leads is a question this cannot answer by
/// looking at the path.
fn walks_through_a_link(path: &Path, links: &BTreeSet<PathBuf>) -> bool {
    path.ancestors()
        .skip(1)
        .any(|ancestor| links.contains(ancestor))
}

/// Whether a symlink target reaches somewhere inside the tree, resolved the way
/// the operating system would rather than the way `..` cancels on paper.
///
/// Asked of the target already joined to the directory the link sits in, which
/// is where a relative target is read from — `../lib/x` on its own says nothing
/// about whether it stays inside, and `bin/../lib/x` says it does.
///
/// A target needs `..` — `../lib/libfoo.so` is ordinary, and so is a link to
/// another link — so unlike an entry path this cannot simply refuse it. What it
/// refuses instead is the one case where cancelling is wrong: a `..` that would
/// cancel a component the archive declares as a symlink.
///
/// That is the whole of the escape, and it takes two entries to build. `a/b ->
/// ../x` is honest and stays inside. `escape -> a/b/../../outside` cancels on
/// paper to `outside`, which is inside; on disk the kernel resolves `a/b` to
/// `x`, goes up twice from there, and lands beside the destination. Anything
/// later written under `escape/` would then be written outside `dest` — through
/// a link, past every check that looked only at spellings.
fn stays_inside(path: &Path, links: &BTreeSet<PathBuf>) -> bool {
    let mut walked = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(name) => walked.push(name),
            Component::ParentDir => {
                // `walked` still holds the component this `..` would cancel.
                // Above the tree there is nothing to cancel, and where the
                // archive declares that component a symlink there is nothing
                // this can say about where going up from it leads.
                if walked.components().next().is_none() || links.contains(&walked) {
                    return false;
                }
                walked.pop();
            }
            Component::RootDir | Component::Prefix(_) => return false,
        }
    }
    walked.components().next().is_some()
}

/// Write the entries into the tree being built.
///
/// Every path has already been checked, so what is left is creation, in archive
/// order, with directory modes held back to the end.
fn unpack(
    at: &Path,
    format: Format,
    entries: &[Entry],
    into: &Path,
    url: &str,
) -> Result<(), Error> {
    // The tree's own mode, first in the list so that anything the archive says
    // about it lands later and wins — the sort below is stable, and the tree is
    // the shortest path there is, so it is applied last either way. An archive
    // that says nothing about its root leaves this: the staging node was created
    // closed, and publishing it as it stands would install a directory nobody
    // but its owner could enter (`guidance.md`, rule 15's widen-at-the-end).
    let mut directories: Vec<(PathBuf, u32)> = vec![(into.to_path_buf(), UNSTATED_ROOT_MODE)];
    let mut records = entries.iter();

    for_each_entry(at, format, url, |entry| {
        // The same archive read the same way, so the records line up with the
        // entries one for one and nothing here has to be decided again.
        let Some(record) = records.next() else {
            return Ok(());
        };
        let built_at = match &record.placement {
            Placement::At(inside) => into.join(inside),
            // Already there, and the only thing it has left to say is its mode.
            // This is the usual `archive-root = "*"` case: the directory being
            // stripped is the one carrying the mode the destination should end
            // up with.
            Placement::TheTreeItself => {
                if matches!(record.kind, Kind::Directory) {
                    directories.push((into.to_path_buf(), mode_of(entry)));
                }
                return Ok(());
            }
            Placement::NotInstalled => return Ok(()),
        };

        match &record.kind {
            Kind::Directory => {
                create_directory(&built_at)?;
                directories.push((built_at, mode_of(entry)));
            }
            Kind::File => {
                create_parents(&built_at)?;
                let mut file = create_closed(&built_at)?;
                io::copy(entry, &mut file).map_err(|source| Error::Write {
                    path: built_at.clone(),
                    source,
                })?;
                // Last, so an interrupted run leaves nothing readable behind.
                set_mode(&built_at, mode_of(entry))?;
            }
            Kind::Symlink(target) => {
                create_parents(&built_at)?;
                symlink(target, &built_at).map_err(|source| Error::Write {
                    path: built_at,
                    source,
                })?;
            }
            // The target is already the path within the tree, resolved when the
            // root was known.
            Kind::Hardlink(target) => {
                create_parents(&built_at)?;
                fs::hard_link(into.join(target), &built_at).map_err(|source| Error::Write {
                    path: built_at,
                    source,
                })?;
            }
            Kind::Metadata => {}
        }
        Ok(())
    })?;

    // Deepest first, so a directory the archive marks unwritable takes that mode
    // only once nothing more is going into it.
    directories.sort_by(|(left, _), (right, _)| right.cmp(left));
    for (path, mode) in directories {
        set_mode(&path, mode)?;
    }
    Ok(())
}

/// Read the archive from the start, handing each entry to `visit`.
///
/// The reader is boxed so that both formats reach one loop: a gzipped archive
/// and a plain one differ in nothing after the bytes have been unwrapped.
fn for_each_entry(
    at: &Path,
    format: Format,
    url: &str,
    mut visit: impl FnMut(&mut tar::Entry<'_, Box<dyn io::Read>>) -> Result<(), Error>,
) -> Result<(), Error> {
    let file = fs::File::open(at).map_err(|source| Error::Read {
        path: at.to_path_buf(),
        source,
    })?;
    let reader: Box<dyn io::Read> = match format {
        // Multi-member, not single: a gzip stream may be several members
        // concatenated — which is what `pigz` writes and what `cat a.gz b.gz`
        // produces — and a decoder that stopped at the first would hand back
        // part of the archive as though it were all of it. That is the failure
        // rule 15 is about, arriving through the reader.
        Format::Gzip => Box::new(MultiGzDecoder::new(file)),
        Format::Plain => Box::new(file),
    };
    let mut archive = tar::Archive::new(reader);
    for entry in archive
        .entries()
        .map_err(|source| unreadable(url, source))?
    {
        let mut entry = entry.map_err(|source| unreadable(url, source))?;
        visit(&mut entry)?;
    }
    Ok(())
}

/// The path an entry names, as the archive writes it.
fn archive_path(entry: &tar::Entry<'_, Box<dyn io::Read>>, url: &str) -> Result<PathBuf, Error> {
    entry
        .path()
        .map(|path| path.into_owned())
        .map_err(|source| unreadable(url, source))
}

/// Which kind an entry is, refusing the ones batfiles has no way to install.
fn kind_of(entry: &tar::Entry<'_, Box<dyn io::Read>>, url: &str) -> Result<Kind, Error> {
    let entry_type = entry.header().entry_type();
    if entry_type.is_pax_global_extensions()
        || entry_type.is_pax_local_extensions()
        || entry_type.is_gnu_longname()
        || entry_type.is_gnu_longlink()
    {
        return Ok(Kind::Metadata);
    }
    if entry_type.is_dir() {
        return Ok(Kind::Directory);
    }
    if entry_type.is_file() {
        return Ok(Kind::File);
    }
    let unsupported = || {
        Invalid::UnsupportedEntry {
            url: url.to_owned(),
            entry: entry
                .path()
                .map_or_else(|_| String::from("?"), |path| display(&path)),
        }
        .into()
    };
    let Some(target) = entry.link_name().ok().flatten() else {
        return Err(unsupported());
    };
    if entry_type.is_symlink() {
        Ok(Kind::Symlink(target.into_owned()))
    } else if entry_type.is_hard_link() {
        Ok(Kind::Hardlink(target.into_owned()))
    } else {
        Err(unsupported())
    }
}

fn escaping(url: &str, entry: &Path) -> Error {
    Invalid::EscapingEntry {
        url: url.to_owned(),
        entry: display(entry),
    }
    .into()
}

fn unreadable(url: &str, source: io::Error) -> Error {
    Invalid::Unreadable {
        url: url.to_owned(),
        source,
    }
    .into()
}

/// Create a directory of the tree, and every directory above it the archive did
/// not name.
///
/// Everything created here is under the staging root, which is closed, so the
/// modes these take in the meantime are reachable by nobody.
fn create_directory(at: &Path) -> Result<(), Error> {
    fs::create_dir_all(at).map_err(|source| Error::Write {
        path: at.to_path_buf(),
        source,
    })
}

/// Create the directories an entry sits under, for an archive that names a file
/// without naming the directory holding it.
fn create_parents(at: &Path) -> Result<(), Error> {
    match at.parent() {
        Some(parent) => create_directory(parent),
        None => Ok(()),
    }
}

/// Create a file of the tree, closed, failing rather than truncating if the path
/// is taken.
///
/// Everything here is inside a staging node this run just made, so a name
/// already taken is an archive naming one entry twice — a failure, rather than
/// something to overwrite.
#[cfg(unix)]
fn create_closed(at: &Path) -> Result<fs::File, Error> {
    use std::os::unix::fs::OpenOptionsExt as _;

    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(BUILDING_MODE)
        .open(at)
        .map_err(|source| Error::Write {
            path: at.to_path_buf(),
            source,
        })
}

/// Where a mode means something other than it does on unix, this is ordinary
/// exclusive creation: the permissions batfiles carries across are the unix
/// ones, and there is nothing here to narrow.
#[cfg(not(unix))]
fn create_closed(at: &Path) -> Result<fs::File, Error> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(at)
        .map_err(|source| Error::Write {
            path: at.to_path_buf(),
            source,
        })
}

/// The permission bits an entry asks for, with the ones batfiles will not grant
/// removed.
#[cfg(unix)]
fn mode_of(entry: &tar::Entry<'_, Box<dyn io::Read>>) -> u32 {
    entry.header().mode().unwrap_or(0o644) & PERMISSION_BITS
}

/// Where a mode means something other than it does on unix, an entry's bits are
/// not carried across, so there is nothing to read and nothing to apply.
#[cfg(not(unix))]
fn mode_of(_entry: &tar::Entry<'_, Box<dyn io::Read>>) -> u32 {
    0
}

#[cfg(unix)]
fn set_mode(at: &Path, mode: u32) -> Result<(), Error> {
    use std::os::unix::fs::PermissionsExt as _;

    fs::set_permissions(at, fs::Permissions::from_mode(mode)).map_err(|source| Error::Write {
        path: at.to_path_buf(),
        source,
    })
}

#[cfg(not(unix))]
fn set_mode(_at: &Path, _mode: u32) -> Result<(), Error> {
    Ok(())
}

fn display(path: &Path) -> String {
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn linking(paths: &[&str]) -> BTreeSet<PathBuf> {
        paths.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn an_entry_path_keeps_its_components_and_drops_its_dots() {
        assert_eq!(entry_path(Path::new("a/b")), Some(PathBuf::from("a/b")));
        assert_eq!(entry_path(Path::new("./a/b")), Some(PathBuf::from("a/b")));
        assert_eq!(entry_path(Path::new("a/./b")), Some(PathBuf::from("a/b")));
        // What `tar czf x.tgz .` writes for the archive's own root, which names
        // the tree rather than anything in it.
        assert_eq!(entry_path(Path::new("./")), Some(PathBuf::new()));
        assert_eq!(entry_path(Path::new("")), Some(PathBuf::new()));
    }

    #[test]
    fn an_entry_path_that_could_leave_the_tree_is_refused_rather_than_cancelled() {
        // `a/../b` names `b` only if `a` is a real directory. It is refused
        // rather than cancelled because the same archive may declare `a` a
        // symlink, and then it names something else entirely.
        for refused in ["/etc/passwd", "..", "../a", "a/../b", "a/../../b"] {
            assert_eq!(entry_path(Path::new(refused)), None, "`{refused}`");
        }
    }

    #[test]
    fn a_root_is_stripped_off_what_is_under_it_and_nothing_else() {
        let strip = Some(Path::new("tool-1.0"));
        let at = |path: &str| match place(Path::new(path), strip) {
            Placement::At(inside) => Some(inside),
            Placement::TheTreeItself | Placement::NotInstalled => None,
        };
        assert_eq!(at("tool-1.0/bin/tool"), Some(PathBuf::from("bin/tool")));
        // Outside the prefix, so not installed. A sibling whose name merely
        // starts with the prefix is not under it.
        assert_eq!(at("other/x"), None);
        assert_eq!(at("tool-1.0-docs/x"), None);
        // The root's own entry is the tree, told apart from the entries that are
        // not installed at all because it still has a mode to give.
        assert!(matches!(
            place(Path::new("tool-1.0"), strip),
            Placement::TheTreeItself
        ));
        assert!(matches!(
            place(Path::new("other/x"), strip),
            Placement::NotInstalled
        ));
        // With no root, every entry keeps the path it was written with, and the
        // archive's own root is still the tree.
        assert!(matches!(
            place(Path::new("bin/tool"), None),
            Placement::At(_)
        ));
        assert!(matches!(
            place(Path::new(""), None),
            Placement::TheTreeItself
        ));
    }

    #[test]
    fn nothing_is_written_under_a_symlink_the_archive_declares() {
        let links = linking(&["bin/tool"]);
        assert!(walks_through_a_link(Path::new("bin/tool/x"), &links));
        assert!(walks_through_a_link(Path::new("bin/tool/deep/x"), &links));
        // The link itself is not *through* the link: creating it is the entry.
        assert!(!walks_through_a_link(Path::new("bin/tool"), &links));
        assert!(!walks_through_a_link(Path::new("bin/other"), &links));
    }

    #[test]
    fn a_symlink_target_may_climb_past_a_directory_and_not_past_a_link() {
        let links = linking(&["a/b", "lib/libfoo.so.1"]);
        // Targets already joined to the directory their link sits in, which is
        // the form this is asked about. The last is the chained link a library
        // tarball ships — `lib/libfoo.so -> libfoo.so.1`, pointing at another
        // link — which is fine, because neither hop climbs past one.
        for inside in ["x", "bin/../lib/x", "a/b", "lib/libfoo.so.1"] {
            assert!(stays_inside(Path::new(inside), &links), "`{inside}`");
        }
        // The escape, which takes two entries: `a/b -> ../x` is honest, and this
        // target cancels down to `outside` on paper while the kernel resolves
        // `a/b` first and lands beside the tree.
        assert!(!stays_inside(Path::new("a/b/../../outside"), &links));
        // One `..` past the link is already wrong, before it leaves the tree.
        assert!(!stays_inside(Path::new("a/b/../c"), &links));
        // And the ordinary ways out.
        for refused in ["/etc/shadow", "../../outside", ".."] {
            assert!(!stays_inside(Path::new(refused), &links), "`{refused}`");
        }
    }

    #[test]
    fn leading_bytes_are_named_where_they_are_recognizable() {
        assert_eq!(looks_like(b"PK\x03\x04rest"), "a zip archive");
        assert_eq!(looks_like(b"BZh9"), "a bzip2 archive");
        assert_eq!(
            looks_like(b"<!DOCTYPE html>"),
            "not an archive batfiles recognizes"
        );
        // An empty body is not an archive either, and asking about its first
        // bytes must not panic.
        assert_eq!(looks_like(b""), "not an archive batfiles recognizes");
    }

    /// One header block, checksummed, with `magic` written where a `ustar`
    /// archive carries one and a V7 archive carries nothing.
    fn header(magic: &[u8]) -> Vec<u8> {
        let mut block = vec![0u8; HEADER];
        block[..8].copy_from_slice(b"a/b\0\0\0\0\0");
        block[100..108].copy_from_slice(b"000644 \0");
        block[124..136].copy_from_slice(b"00000000002\0");
        block[257..257 + magic.len()].copy_from_slice(magic);
        block[CHECKSUM].fill(b' ');
        let sum: u32 = block.iter().map(|&byte| u32::from(byte)).sum();
        let written = format!("{sum:06o}\0 ");
        block[CHECKSUM].copy_from_slice(written.as_bytes());
        block
    }

    #[test]
    fn a_tar_is_recognized_by_its_checksum_rather_than_by_its_format() {
        // POSIX, GNU, and V7 — the last of which carries no magic at all, and is
        // the one a check for `ustar` at offset 257 would refuse.
        for magic in [&b"ustar\0"[..], b"ustar  \0", b""] {
            assert!(is_tar_header(&header(magic)), "{magic:?}");
        }
    }

    #[test]
    fn something_that_is_not_a_tar_header_is_not_taken_for_one() {
        // A block that is the right length and says the wrong thing about
        // itself, which is what the checksum is for.
        let mut wrong = header(b"ustar\0");
        wrong[0] = b'z';
        assert!(!is_tar_header(&wrong));
        // Too short to hold a header, all zeroes, and ordinary text.
        assert!(!is_tar_header(b"ustar"));
        assert!(!is_tar_header(&[0u8; HEADER]));
        assert!(!is_tar_header(&[b'x'; HEADER]));
    }
}
