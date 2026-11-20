//! Install each direct child of a source directory into a destination directory.

use std::ffi::{OsStr, OsString};
use std::path::Path;

use crate::error::Error;
use crate::filesystem::paths;
use crate::output::Verb;
use crate::run::context::RunContext;

/// Source directory, destination directory, and naming/reporting options for child
/// installation.
pub(super) struct ChildInstall<'a> {
    pub source_dir: &'a Path,
    pub dest_dir: &'a Path,
    pub dot_prefix: bool,
    /// Verb used to report each installation.
    pub verb: Verb,
}

/// What [`for_each_child`] did.
pub(super) struct Children {
    /// Whether the children were enumerated, which a skipped conflict prevents.
    pub enumerated: bool,
    /// Whether the destination directory was created, or any `install_one` installed or replaced
    /// a child.
    pub changed: bool,
}

/// Ensure the destination directory exists, then call `install_one(source, dest)` for each
/// direct child `selects` accepts, in name order; it returns whether it installed or replaced
/// the child. Stop if the directory conflict is skipped or a child fails.
pub(super) fn for_each_child(
    context: &RunContext,
    install: &ChildInstall,
    mut selects: impl FnMut(&OsStr) -> bool,
    install_one: impl Fn(&Path, &Path) -> Result<bool, Error>,
) -> Result<Children, Error> {
    let directory = context.ensure_directory(install.dest_dir)?;
    if !directory.usable() {
        return Ok(Children {
            enumerated: false,
            changed: false,
        });
    }

    let children = paths::children_of(install.source_dir)?;
    let selected: Vec<OsString> = children
        .iter()
        .filter(|child| selects(child))
        .cloned()
        .collect();
    if selected.is_empty() {
        let none = if children.is_empty() {
            "no children"
        } else {
            "no selected children"
        };
        context.reporter().detail(
            1,
            &format!(
                "{none} to {} in {}",
                install.verb.infinitive(),
                install.source_dir.display()
            ),
        );
    }
    // Creating the destination is a change even when no child is installed into it.
    let mut changed = directory.created();
    for child in selected {
        let installed = installed_name(&child, install.dot_prefix)?;
        changed |= install_one(
            &install.source_dir.join(&child),
            &install.dest_dir.join(installed),
        )?;
    }
    Ok(Children {
        enumerated: true,
        changed,
    })
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
