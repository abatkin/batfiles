//! The shape both `-dir` actions are: one thing installed per direct child of a
//! source directory, all of them into one destination directory.

use std::ffi::OsString;
use std::path::Path;

use super::RunContext;
use crate::error::Error;
use crate::output::Verb;
use crate::paths;

/// What a `-dir` action installs, and where.
pub(super) struct ChildInstall<'a> {
    pub source_dir: &'a Path,
    pub dest_dir: &'a Path,
    pub dot_prefix: bool,
    /// How the action says what it did, in the one line both of them report.
    pub verb: Verb,
}

/// Do one action's work once per direct child of its source directory.
pub(super) fn for_each_child(
    context: &RunContext,
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
