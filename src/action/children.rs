//! The shape both `-dir` actions are: one thing installed per direct child of a
//! source directory, all of them into one destination directory.
//!
//! `symlink-dir` and `copy-dir` differ only in what they do with each child, so
//! everything up to that point is here and each of them supplies the rest.

use std::ffi::OsString;
use std::path::Path;

use super::Context;
use crate::error::Error;
use crate::mode::Verb;
use crate::paths;

/// What a `-dir` action installs, and where.
///
/// The three fields both records carry, resolved, plus the one word their
/// reports differ by. A struct rather than four parameters because `source_dir`
/// and `dest_dir` are both `&Path` and neighbours: transposing them compiles,
/// and would empty a repository directory's children into itself.
///
/// Not shared with the manifest records, which repeat these fields for a
/// separate reason — `#[serde(flatten)]` silently disables
/// `deny_unknown_fields` ([`crate::manifest::action`]).
pub(super) struct ChildInstall<'a> {
    pub source_dir: &'a Path,
    pub dest_dir: &'a Path,
    pub dot_prefix: bool,
    /// How the action says what it did, in the one line both of them report.
    pub verb: Verb,
}

/// Do one action's work once per direct child of its source directory.
///
/// Make the destination, then install every direct child of the source into it
/// under the name [`installed_name`] gives. Not recursive, in either action — a
/// child that is itself a directory is one thing installed, and what is inside
/// it is reached through what was installed rather than decided entry by entry.
pub(super) fn for_each_child(
    context: &Context,
    install: &ChildInstall,
    install_one: impl Fn(&Path, &Path) -> Result<(), Error>,
) -> Result<(), Error> {
    context.ensure_directory(install.dest_dir)?;

    let children = paths::children_of(install.source_dir)?;
    if children.is_empty() {
        context.reporter().detail(
            1,
            &format!(
                "no children to {} in {}",
                install.verb.infinitive(),
                install.source_dir.display()
            ),
        );
    }
    for child in children {
        let installed = installed_name(&child, install.dot_prefix)?;
        install_one(
            &install.source_dir.join(&child),
            &install.dest_dir.join(installed),
        )?;
    }
    Ok(())
}

/// What a child of a `source-dir` is called once installed.
///
/// The dot-prefix rule and its one refusal, shared by both actions that install
/// a directory's children: a child already starting with `.` would arrive as
/// `..name`, which is a legal file name and never the one that was meant.
fn installed_name(child: &OsString, dot_prefix: bool) -> Result<OsString, Error> {
    if !dot_prefix {
        return Ok(child.clone());
    }
    // Lossy only where a name is not UTF-8, and only for the refusal's message;
    // the paths themselves are joined from the original `OsString`.
    let name = child.to_string_lossy();
    if name.starts_with('.') {
        return Err(Error::DotPrefixOnDotfile {
            child: name.into_owned(),
        });
    }
    let mut dotted = OsString::from(".");
    dotted.push(child);
    Ok(dotted)
}
