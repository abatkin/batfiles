//! Install each direct child of a source directory into a destination directory.

use std::ffi::OsString;
use std::path::Path;

use super::RunContext;
use crate::error::Error;
use crate::output::Verb;
use crate::paths;

/// Source directory, destination directory, and naming/reporting options for child
/// installation.
pub(super) struct ChildInstall<'a> {
    pub source_dir: &'a Path,
    pub dest_dir: &'a Path,
    pub dot_prefix: bool,
    /// Verb used to report each installation.
    pub verb: Verb,
}

/// Ensure the destination directory exists, then call `install_one(source, dest)` for each
/// direct child in name order. Stop if the directory conflict is skipped or a child fails.
pub(super) fn for_each_child(
    context: &RunContext,
    install: &ChildInstall,
    install_one: impl Fn(&Path, &Path) -> Result<(), Error>,
) -> Result<(), Error> {
    if !context.ensure_directory(install.dest_dir)? {
        return Ok(());
    }

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

/// Return the installed child name. With `dot_prefix`, prepend `.` or fail if the name already
/// starts with a dot.
fn installed_name(child: &OsString, dot_prefix: bool) -> Result<OsString, Error> {
    if !dot_prefix {
        return Ok(child.clone());
    }
    // Decode lossily only for the error message; construct paths from the original bytes.
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
