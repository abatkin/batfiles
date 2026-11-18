//! Parse line-oriented `git-clone-list` manifests. [`crate::run::clone_list`] decides a
//! parsed list's entries for one run.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use thiserror::Error;

use crate::error::Error;
use crate::manifest::condition::{Condition, Gate, SyntaxError};
use crate::manifest::item::{ItemId, ItemIdError};

/// One repository the list names.
#[derive(Debug)]
pub(crate) struct CloneListEntry {
    /// The repository, passed to git as written; not necessarily a URL.
    pub repository: String,
    /// The one directory component the clone lands in.
    pub dest_name: String,
    /// Identifies the entry in diagnostics, and addresses it under its list.
    pub id: Option<ItemId>,
    /// The branch, tag, or commit the entry follows.
    pub git_ref: Option<String>,
    /// The condition admitting the entry, if it is written with one.
    pub when: Option<Condition>,
    /// The condition excluding it. An entry writes at most one of the two.
    pub unless: Option<Condition>,
    /// The list line declaring it, for failures after the list was read.
    pub line: usize,
}

impl CloneListEntry {
    /// Return the entry's condition gate, if declared.
    pub fn gate(&self) -> Option<Gate<'_>> {
        Gate::from_fields(self.when.as_ref(), self.unless.as_ref())
    }

    /// Format the entry's list, line, and optional ID for diagnostics. `list` is the action's
    /// source as declared.
    pub fn written_at(&self, list: &str) -> String {
        match &self.id {
            Some(id) => format!("id={id}, {list} line {}", self.line),
            None => format!("{list} line {}", self.line),
        }
    }
}

/// Read and check the list at `path`. A missing or malformed list is an error.
pub(crate) fn read(path: &Path) -> Result<Vec<CloneListEntry>, Error> {
    let text = fs::read_to_string(path).map_err(Error::reading(path))?;
    parse(&text).map_err(|(line, source)| Error::CloneList {
        path: path.to_path_buf(),
        line,
        source,
    })
}

/// Every entry the text declares, or the first fault and the line it is on.
fn parse(text: &str) -> Result<Vec<CloneListEntry>, (usize, CloneListError)> {
    let mut entries: Vec<CloneListEntry> = Vec::new();
    let mut names: BTreeMap<String, (usize, String)> = BTreeMap::new();
    let mut ids: BTreeMap<String, usize> = BTreeMap::new();

    for (index, text) in text.lines().enumerate() {
        let line = index + 1;
        let Some(entry) = entry(text, line).map_err(|invalid| (line, invalid))? else {
            continue;
        };

        if let Some((first, written)) = names.insert(
            case_folded(&entry.dest_name),
            (line, entry.dest_name.clone()),
        ) {
            return Err((
                line,
                if written == entry.dest_name {
                    CloneListError::RepeatedName {
                        name: entry.dest_name,
                        first,
                    }
                } else {
                    CloneListError::AliasedName {
                        name: entry.dest_name,
                        written,
                        first,
                    }
                },
            ));
        }
        if let Some(id) = &entry.id
            && let Some(first) = ids.insert(id.to_string(), line)
        {
            return Err((
                line,
                CloneListError::RepeatedEntryId {
                    id: id.clone(),
                    first,
                },
            ));
        }
        entries.push(entry);
    }
    Ok(entries)
}

/// Parse a line; return `None` for blank or comment-only lines.
fn entry(text: &str, line: usize) -> Result<Option<CloneListEntry>, CloneListError> {
    let mut fields = fields(text)?.into_iter();
    let Some(repository) = fields.next() else {
        return Ok(None);
    };
    if repository.is_empty() {
        return Err(CloneListError::RepositoryEmpty);
    }

    let mut metadata: BTreeMap<String, String> = BTreeMap::new();
    for field in fields {
        let Some((key, value)) = field.split_once('=') else {
            return Err(CloneListError::NotMetadata { field });
        };
        match key {
            "id" | "ref" | "dest-name" | "when" | "unless" => {}
            _ => {
                return Err(CloneListError::UnknownKey {
                    key: key.to_owned(),
                });
            }
        }
        if value.is_empty() {
            return Err(CloneListError::EmptyValue {
                key: key.to_owned(),
            });
        }
        if metadata.insert(key.to_owned(), value.to_owned()).is_some() {
            return Err(CloneListError::RepeatedKey {
                key: key.to_owned(),
            });
        }
    }

    if metadata.contains_key("when") && metadata.contains_key("unless") {
        return Err(CloneListError::BothConditions);
    }

    let name = match metadata.get("dest-name") {
        Some(written) => {
            if !is_one_component(written) {
                return Err(CloneListError::DestNameUnusable {
                    name: written.clone(),
                });
            }
            written.clone()
        }
        None => {
            let derived = derive_name(&repository);
            if !is_one_component(&derived) {
                return Err(CloneListError::DerivedNameUnusable {
                    repository,
                    name: derived,
                });
            }
            derived
        }
    };

    Ok(Some(CloneListEntry {
        repository,
        dest_name: name,
        id: metadata
            .get("id")
            .map(|id| ItemId::try_from(id.clone()))
            .transpose()?,
        git_ref: metadata.get("ref").cloned(),
        when: condition(&metadata, "when")?,
        unless: condition(&metadata, "unless")?,
        line,
    }))
}

/// Parse an optional condition from entry metadata.
fn condition(
    metadata: &BTreeMap<String, String>,
    key: &str,
) -> Result<Option<Condition>, CloneListError> {
    metadata
        .get(key)
        .map(|source| Condition::new(source))
        .transpose()
        .map_err(|error| CloneListError::Condition(Box::new(error)))
}

/// The directory an entry clones into when it does not name one.
fn derive_name(repository: &str) -> String {
    let trimmed = repository.trim_end_matches('/');
    let tail = match trimmed.rfind(['/', ':']) {
        Some(at) => &trimmed[at + 1..],
        None => trimmed,
    };
    // Keep a bare `.git` name for destination-name validation to reject.
    match tail.strip_suffix(".git") {
        Some(stripped) if !stripped.is_empty() => stripped.to_owned(),
        _ => tail.to_owned(),
    }
}

/// Lowercase a destination name for portable collision detection.
fn case_folded(name: &str) -> String {
    name.to_lowercase()
}

/// Accept a nonempty destination name without path separators or a drive colon.
/// Reject `.`, `..`, and `.git`.
fn is_one_component(name: &str) -> bool {
    !name.is_empty() && !matches!(name, "." | ".." | ".git") && !name.contains(['/', '\\', ':'])
}

/// Split a line into whitespace-separated fields, honoring quotes and escapes.
/// An unquoted `#` starts a comment. Empty quoted fields are retained.
fn fields(line: &str) -> Result<Vec<String>, CloneListError> {
    let mut fields = Vec::new();
    let mut current = String::new();
    // Track empty quoted fields separately from absent fields.
    let mut started = false;
    let mut characters = line.chars();

    while let Some(character) = characters.next() {
        match character {
            '#' => break,
            _ if character.is_whitespace() => {
                if started {
                    fields.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            '\'' | '"' => {
                started = true;
                quoted(character, &mut characters, &mut current)?;
            }
            _ => {
                started = true;
                current.push(character);
            }
        }
    }
    if started {
        fields.push(current);
    }
    Ok(fields)
}

/// Read a quoted run into `value`, stopping at the quote that opened it.
fn quoted(
    opener: char,
    characters: &mut std::str::Chars<'_>,
    value: &mut String,
) -> Result<(), CloneListError> {
    while let Some(character) = characters.next() {
        match character {
            _ if character == opener => return Ok(()),
            '\\' => match characters.next() {
                Some(escaped @ ('\\' | '"' | '\'')) => value.push(escaped),
                Some(escaped) => return Err(CloneListError::UnsupportedEscape { escape: escaped }),
                None => return Err(CloneListError::UnterminatedQuote),
            },
            _ => value.push(character),
        }
    }
    Err(CloneListError::UnterminatedQuote)
}

/// Invalid clone-list syntax or conflicting entries.
#[derive(Debug, Error)]
pub(crate) enum CloneListError {
    /// An empty repository field.
    #[error("names no repository")]
    RepositoryEmpty,

    /// A field after the repository that is not `key=value`.
    #[error("has `{field}` after the repository, which is not `key=value` metadata")]
    NotMetadata { field: String },

    #[error("uses the unknown key `{key}`; the keys are id, ref, dest-name, when, and unless")]
    UnknownKey { key: String },

    /// An invalid `when` or `unless` expression, with the parser's diagnostic.
    #[error(transparent)]
    Condition(Box<SyntaxError>),

    /// An entry declaring both `when` and `unless`.
    #[error("writes both `when` and `unless`; a line has one condition or none")]
    BothConditions,

    #[error("writes `{key}` twice")]
    RepeatedKey { key: String },

    #[error("writes `{key}=` with no value")]
    EmptyValue { key: String },

    /// A quoted value with no closing quote.
    #[error("opens a quote that never closes")]
    UnterminatedQuote,

    /// An unsupported backslash escape in a quoted value.
    #[error("writes the escape `\\{escape}`; only `\\\\`, `\\\"`, and `\\'` are escapes")]
    UnsupportedEscape { escape: char },

    /// An invalid entry ID.
    #[error(transparent)]
    EntryId(#[from] ItemIdError),

    /// A repository whose final component cannot serve as a destination directory name.
    #[error(
        "names `{repository}`, which gives `{name}` as a directory name; \
         write `dest-name=` to say what to call the clone"
    )]
    DerivedNameUnusable { repository: String, name: String },

    /// A `dest-name` that is a path rather than a name.
    #[error("writes `dest-name={name}`, which is not one ordinary directory name")]
    DestNameUnusable { name: String },

    /// Two entries installing into one directory.
    #[error("clones into `{name}`, which line {first} already clones into")]
    RepeatedName { name: String, first: usize },

    /// Two entries whose names differ only in case.
    #[error(
        "clones into `{name}`, which differs only in case from the `{written}` on line {first}; \
         on a filesystem that ignores case they are one directory"
    )]
    AliasedName {
        name: String,
        written: String,
        first: usize,
    },

    /// Two entries declaring the same ID.
    #[error("repeats the id `{id}`, which line {first} already uses")]
    RepeatedEntryId { id: ItemId, first: usize },
}

#[cfg(test)]
mod tests;
