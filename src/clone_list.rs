//! The line-oriented manifest a `git-clone-list` action reads, and the form a
//! run reads it into.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use thiserror::Error;

use crate::condition::{Bindings, Condition, ConditionError, Exclusion, Gate};
use crate::error::Error;
use crate::item::{ItemId, ItemIdError};
use crate::manifest::action::GitCloneListAction;

/// One repository the list names.
#[derive(Debug)]
pub(crate) struct Entry {
    /// The repository, exactly as git is given it. Not necessarily a URL:
    /// `git@host:path` and a plain directory are repositories too, and which
    /// of them a line names is git's question rather than batfiles'.
    pub repository: String,
    /// The one directory component the clone lands in.
    pub dest_name: String,
    /// Identifies the entry in diagnostics. Individual entry selection is not
    /// implemented; see the clone-list address enhancement in the roadmap.
    pub id: Option<ItemId>,
    /// The branch, tag, or commit the entry follows.
    pub git_ref: Option<String>,
    /// The condition admitting the entry, if it is written with one.
    pub when: Option<Condition>,
    /// The condition excluding it. An entry writes at most one of the two.
    pub unless: Option<Condition>,
    /// Which line of the list declared it, for a diagnostic that has to point
    /// at one. A fault found while reading carries its own line and does not
    /// come from here; this is for the entry that reads correctly and then
    /// fails to clone.
    pub line: usize,
}

impl Entry {
    /// The gate the entry's condition makes, if it was written with one.
    pub fn gate(&self) -> Option<Gate<'_>> {
        Gate::declared(self.when.as_ref(), self.unless.as_ref())
    }

    /// Where the entry is written, for a diagnostic that has to send a reader
    /// to it. `list` is the action's `source`, as the manifest spells it.
    pub fn written_at(&self, list: &str) -> String {
        match &self.id {
            Some(id) => format!("id={id}, {list} line {}", self.line),
            None => format!("{list} line {}", self.line),
        }
    }
}

/// One list a run read, and what that run made of each entry's own condition.
///
/// [`prepare`](Self::prepare) is the only thing that makes one and it makes one
/// only by reading the list, so holding a value of this type is what says the
/// list was read. An empty one is a list that declares no repositories, which
/// is a different answer from a list nothing opened — and that second one has
/// no value here at all, which is why nothing downstream has to ask.
pub(crate) struct PreparedList<'a> {
    /// The record that named the list: where the clones are made, and the name
    /// every line the action reports calls the list by.
    action: &'a GitCloneListAction,
    entries: Vec<PreparedEntry>,
}

/// One entry of a prepared list: the line as it was read, and what this run
/// made of the condition on it.
pub(crate) struct PreparedEntry {
    /// What the line declares, which reading it settles once and for all.
    pub declared: Entry,
    /// Why this run is not cloning the entry, if it is not: the verdict of its
    /// own condition. A closed entry stays on the list rather than being
    /// dropped from it, so that the action can report it under its own heading
    /// instead of having it vanish.
    pub exclusion: Option<Exclusion>,
}

impl<'a> PreparedList<'a> {
    /// Read the list `action` names, found at `path`, and decide each entry's
    /// own condition against `bindings`.
    ///
    /// A missing or malformed list is an error here, which is what keeps it
    /// ahead of the first action rather than partway through a run.
    pub fn prepare(
        action: &'a GitCloneListAction,
        path: &Path,
        bindings: &Bindings<'_>,
    ) -> Result<Self, Error> {
        let entries = read(path)?
            .into_iter()
            .map(|declared| PreparedEntry {
                exclusion: exclusion(&declared, bindings),
                declared,
            })
            .collect();
        Ok(Self { action, entries })
    }

    /// The directory the clones are made in, as the record wrote it.
    pub fn dest_dir(&self) -> &str {
        &self.action.dest_dir
    }

    /// How a line about the list names it: the path as the manifest wrote it,
    /// which for a list held by a remote is the reference including the remote
    /// rather than wherever on this machine it was materialized.
    pub fn name(&self) -> String {
        self.action.source.to_string()
    }

    /// Whether the list declares no repositories at all.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Every entry, in list order.
    pub fn entries(&self) -> &[PreparedEntry] {
        &self.entries
    }
}

/// Evaluate an entry's condition, returning `None` when it may be cloned.
/// The caller supplies the `not cloning` prefix when reporting an exclusion.
fn exclusion(entry: &Entry, bindings: &Bindings<'_>) -> Option<Exclusion> {
    entry.gate()?.exclusion(bindings, None)
}

/// Read and check one list.
fn read(path: &Path) -> Result<Vec<Entry>, Error> {
    let text = fs::read_to_string(path).map_err(|source| Error::Read {
        path: path.to_path_buf(),
        source,
    })?;
    parse(&text).map_err(|(line, source)| Error::CloneList {
        path: path.to_path_buf(),
        line,
        source,
    })
}

/// Every entry the text declares, or the first fault and the line it is on.
fn parse(text: &str) -> Result<Vec<Entry>, (usize, Invalid)> {
    let mut entries: Vec<Entry> = Vec::new();
    let mut names: BTreeMap<String, (usize, String)> = BTreeMap::new();
    let mut ids: BTreeMap<String, usize> = BTreeMap::new();

    for (index, text) in text.lines().enumerate() {
        let line = index + 1;
        let Some(entry) = entry(text, line).map_err(|invalid| (line, invalid))? else {
            continue;
        };

        if let Some((first, written)) =
            names.insert(fold(&entry.dest_name), (line, entry.dest_name.clone()))
        {
            return Err((
                line,
                if written == entry.dest_name {
                    Invalid::RepeatedName {
                        name: entry.dest_name,
                        first,
                    }
                } else {
                    Invalid::AliasedName {
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
                Invalid::RepeatedEntryId {
                    id: id.clone(),
                    first,
                },
            ));
        }
        entries.push(entry);
    }
    Ok(entries)
}

/// One line: an entry, or nothing where the line declares none.
fn entry(text: &str, line: usize) -> Result<Option<Entry>, Invalid> {
    let mut fields = fields(text)?.into_iter();
    // A blank line and a line holding only a comment both declare nothing,
    // which is one case rather than two: the comment is gone by now.
    let Some(repository) = fields.next() else {
        return Ok(None);
    };
    if repository.is_empty() {
        return Err(Invalid::RepositoryEmpty);
    }

    let mut metadata: BTreeMap<String, String> = BTreeMap::new();
    for field in fields {
        let Some((key, value)) = field.split_once('=') else {
            return Err(Invalid::NotMetadata { field });
        };
        match key {
            "id" | "ref" | "dest-name" | "when" | "unless" => {}
            _ => {
                return Err(Invalid::UnknownKey {
                    key: key.to_owned(),
                });
            }
        }
        if value.is_empty() {
            return Err(Invalid::EmptyValue {
                key: key.to_owned(),
            });
        }
        if metadata.insert(key.to_owned(), value.to_owned()).is_some() {
            return Err(Invalid::RepeatedKey {
                key: key.to_owned(),
            });
        }
    }

    // The same rule a manifest record follows, checked here because a line is
    // where a reader would go and fix it.
    if metadata.contains_key("when") && metadata.contains_key("unless") {
        return Err(Invalid::BothConditions);
    }

    let name = match metadata.get("dest-name") {
        Some(written) => {
            if !is_one_component(written) {
                return Err(Invalid::DestNameUnusable {
                    name: written.clone(),
                });
            }
            written.clone()
        }
        None => {
            let derived = derive_name(&repository);
            if !is_one_component(&derived) {
                return Err(Invalid::DerivedNameUnusable {
                    repository,
                    name: derived,
                });
            }
            derived
        }
    };

    Ok(Some(Entry {
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

/// One of the two condition keys, parsed where the line is read so that a
/// malformed one names the file and the line rather than surfacing partway
/// through a run.
fn condition(metadata: &BTreeMap<String, String>, key: &str) -> Result<Option<Condition>, Invalid> {
    metadata
        .get(key)
        .map(|source| Condition::new(source))
        .transpose()
        .map_err(Invalid::from)
}

/// The directory an entry clones into when it does not name one.
fn derive_name(repository: &str) -> String {
    let trimmed = repository.trim_end_matches('/');
    let tail = match trimmed.rfind(['/', ':']) {
        Some(at) => &trimmed[at + 1..],
        None => trimmed,
    };
    // Only where something is left: a repository called `.git` keeps the name
    // it has, and is refused for being one rather than for being empty.
    match tail.strip_suffix(".git") {
        Some(stripped) if !stripped.is_empty() => stripped.to_owned(),
        _ => tail.to_owned(),
    }
}

/// Lowercase a destination name for portable collision detection.
fn fold(name: &str) -> String {
    name.to_lowercase()
}

/// Accept a nonempty destination name without path separators or a drive colon.
/// Reject `.`, `..`, and `.git`.
fn is_one_component(name: &str) -> bool {
    !name.is_empty() && !matches!(name, "." | ".." | ".git") && !name.contains(['/', '\\', ':'])
}

/// Split a line into whitespace-separated fields, honoring quotes and escapes.
/// An unquoted `#` starts a comment. Empty quoted fields are retained.
fn fields(line: &str) -> Result<Vec<String>, Invalid> {
    let mut fields = Vec::new();
    let mut current = String::new();
    // Kept apart from `current.is_empty()`, so that `dest-name=""` is a field
    // holding nothing rather than no field at all.
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
) -> Result<(), Invalid> {
    while let Some(character) = characters.next() {
        match character {
            _ if character == opener => return Ok(()),
            '\\' => match characters.next() {
                Some(escaped @ ('\\' | '"' | '\'')) => value.push(escaped),
                Some(escaped) => return Err(Invalid::UnsupportedEscape { escape: escaped }),
                None => return Err(Invalid::UnterminatedQuote),
            },
            _ => value.push(character),
        }
    }
    Err(Invalid::UnterminatedQuote)
}

/// What a line of a clone list can be that stops it naming a repository.
#[derive(Debug, Error)]
pub(crate) enum Invalid {
    /// A line whose first field is empty, which is the one thing a repository cannot
    /// be.
    #[error("names no repository")]
    RepositoryEmpty,

    /// A field after the repository that is not `key=value`.
    #[error("has `{field}` after the repository, which is not `key=value` metadata")]
    NotMetadata { field: String },

    #[error("uses the unknown key `{key}`; the keys are id, ref, dest-name, when, and unless")]
    UnknownKey { key: String },

    /// A `when` that is not a condition. The message is the parser's own, which
    /// names the text and the character it stopped at.
    #[error(transparent)]
    Condition(#[from] ConditionError),

    /// A line writing both spellings of a condition, refused on the same terms
    /// as the manifest record that does.
    #[error("writes both `when` and `unless`; a line has one condition or none")]
    BothConditions,

    #[error("writes `{key}` twice")]
    RepeatedKey { key: String },

    #[error("writes `{key}=` with no value")]
    EmptyValue { key: String },

    /// A quoted value that never closes, which would otherwise swallow whatever follows
    /// it — including the `#` that was meant to start a comment.
    #[error("opens a quote that never closes")]
    UnterminatedQuote,

    /// A backslash inside a quoted value that escapes something the format has no
    /// meaning for.
    #[error("writes the escape `\\{escape}`; only `\\\\`, `\\\"`, and `\\'` are escapes")]
    UnsupportedEscape { escape: char },

    /// An `id` that is not an ID.
    #[error(transparent)]
    EntryId(#[from] ItemIdError),

    /// A repository whose last component is no directory name: a bare host, a path
    /// ending in a separator this cannot see past, a Windows path whose drive letter is
    /// the last separator.
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

    /// Two entries answering to one address, which would make either unreachable.
    #[error("repeats the id `{id}`, which line {first} already uses")]
    RepeatedEntryId { id: ItemId, first: usize },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(text: &str) -> Vec<Entry> {
        parse(text).expect("the list should parse")
    }

    fn fault(text: &str) -> (usize, Invalid) {
        parse(text).expect_err("the list should be refused")
    }

    /// What every name in the list comes out as, which is the whole of the
    /// derivation rule.
    fn name_of(repository: &str) -> String {
        let parsed = entries(repository);
        parsed.into_iter().next().expect("one entry").dest_name
    }

    #[test]
    fn a_name_is_the_last_component_without_its_git_suffix() {
        assert_eq!(name_of("https://github.com/mileszs/ack.vim.git"), "ack.vim");
        // Nothing requires the suffix: half of a real list is written without
        // it.
        assert_eq!(
            name_of("https://github.com/vim-airline/vim-airline"),
            "vim-airline"
        );
        assert_eq!(name_of("https://github.com/user/repo.git/"), "repo");
        assert_eq!(name_of("/srv/git/repo"), "repo");
    }

    #[test]
    fn a_name_ends_at_a_colon_where_that_is_the_last_separator() {
        // The `scp`-style form, which has no slash to end on.
        assert_eq!(name_of("git@github.com:repo.git"), "repo");
        // The same form with a path: the slash comes later, so it wins.
        assert_eq!(name_of("git@github.com:user/repo.git"), "repo");
        // A port, whose colon comes before the last slash and decides nothing.
        assert_eq!(name_of("ssh://git@host:2222/user/repo.git"), "repo");
    }

    #[test]
    fn a_repository_whose_name_is_not_a_directory_asks_for_dest_name() {
        // A source ending at its separator, one whose last separator is a
        // drive letter, and one whose whole last component is the suffix.
        for repository in ["git@host:", "C:\\src\\repo", "https://e.example/.git"] {
            assert!(
                matches!(fault(repository), (1, Invalid::DerivedNameUnusable { .. })),
                "`{repository}` should not name a directory"
            );
        }
    }

    #[test]
    fn a_dest_name_replaces_the_derived_one_and_is_still_only_a_name() {
        assert_eq!(
            name_of("https://github.com/romkatv/powerlevel10k.git dest-name=p10k"),
            "p10k"
        );
        for name in ["../elsewhere", "/etc", "a/b", "..", ".", ".git", "C:x"] {
            let written = format!("https://e.example/a.git dest-name='{name}'");
            assert!(
                matches!(fault(&written), (1, Invalid::DestNameUnusable { .. })),
                "`{name}` should not be a destination name"
            );
        }
    }

    #[test]
    fn blank_lines_and_comments_declare_nothing() {
        let parsed = entries(
            "# a whole-line comment, with an = and a \"quote\" in it\n\
             \n   \n\
             https://e.example/a.git # trailing, id=not-metadata\n\
             \t# indented\n",
        );
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].dest_name, "a");
    }

    #[test]
    fn a_fault_is_reported_against_the_line_it_is_written_on() {
        assert!(matches!(
            fault(
                "# a comment\n\
                 \n\
                 https://e.example/a.git\n\
                 https://e.example/b.git colour=blue\n"
            ),
            (4, Invalid::UnknownKey { .. })
        ));
    }

    #[test]
    fn a_value_may_be_quoted_and_hold_what_a_bare_one_cannot() {
        let parsed = entries("https://e.example/a.git dest-name=\"my plugin\"\n");
        assert_eq!(parsed[0].dest_name, "my plugin");
        assert_eq!(name_of("https://e.example/a.git dest-name='a#b'"), "a#b");
    }

    #[test]
    fn a_ref_is_accepted_in_the_shapes_git_resolves() {
        // Validated as the list is read; what following one does is
        // `crate::git`'s and is tested through the binary.
        assert_eq!(entries("https://e.example/a.git ref=main\n").len(), 1);
        assert_eq!(
            entries("https://e.example/a.git ref='refs/heads/main'\n").len(),
            1
        );
    }

    #[test]
    fn a_line_splits_into_fields_at_whitespace_a_quote_does_not_cover() {
        assert_eq!(
            fields(r#"url dest-name="a\"b\\c" ref='x y' # "not a field""#).expect("fields"),
            ["url", r#"dest-name=a"b\c"#, "ref=x y"]
        );
        // Adjacent quoted and bare runs are one field, which is what makes
        // `key="value"` a field rather than two.
        assert_eq!(fields(r#"a'b'"c"d"#).expect("fields"), ["abcd"]);
        assert_eq!(fields("   ").expect("fields"), Vec::<String>::new());
    }

    #[test]
    fn the_only_escapes_are_the_quotes_and_the_backslash() {
        assert!(matches!(
            fault("https://e.example/a.git ref=\"a\\nb\"\n"),
            (1, Invalid::UnsupportedEscape { escape: 'n' })
        ));
        assert!(matches!(
            fault("https://e.example/a.git ref=\"unfinished\n"),
            (1, Invalid::UnterminatedQuote)
        ));
    }

    #[test]
    fn a_field_after_the_repository_has_to_be_metadata() {
        // The mistake this is written for: a second repository on one line.
        assert!(matches!(
            fault("https://e.example/a.git https://e.example/b.git\n"),
            (1, Invalid::NotMetadata { .. })
        ));
        assert!(matches!(
            fault("https://e.example/a.git colour=blue\n"),
            (1, Invalid::UnknownKey { .. })
        ));
        assert!(matches!(
            fault("https://e.example/a.git ref=\n"),
            (1, Invalid::EmptyValue { .. })
        ));
        assert!(matches!(
            fault("https://e.example/a.git ref=main ref=next\n"),
            (1, Invalid::RepeatedKey { .. })
        ));
        assert!(matches!(fault("'' id=a\n"), (1, Invalid::RepositoryEmpty)));
    }

    #[test]
    fn an_entry_takes_one_condition_in_either_spelling() {
        // Parsed as the list is read, like a manifest record's; what a run makes
        // of one is settled during preparation and tested through the binary.
        let parsed = entries("https://e.example/a.git when=\"work\"\n");
        assert_eq!(
            parsed[0].gate().expect("a gate").exclusion_reason(),
            "when \"work\" is false"
        );
        let parsed = entries("https://e.example/a.git unless=\"facts.os == 'windows'\"\n");
        assert!(parsed[0].when.is_none());
        assert!(parsed[0].unless.is_some());
    }

    #[test]
    fn an_entry_writes_one_condition_or_none() {
        assert!(matches!(
            fault("https://e.example/a.git when=\"work\" unless=\"school\"\n"),
            (1, Invalid::BothConditions)
        ));
    }

    #[test]
    fn a_malformed_condition_is_refused_where_the_line_is_read() {
        // The alternative is a list that reads correctly and fails partway
        // through a run, on a line nothing has named yet.
        let (line, fault) = fault(
            "https://e.example/a.git\n\
             https://e.example/b.git when=\"work &&\"\n",
        );
        assert_eq!(line, 2);
        assert!(
            matches!(fault, Invalid::Condition(_)),
            "{fault:?}: a condition that does not parse should be refused"
        );
        assert!(
            fault.to_string().contains("is not a valid condition"),
            "{fault}"
        );
    }

    #[test]
    fn an_entry_id_follows_the_id_rule_and_not_the_directory_one() {
        assert_eq!(
            entries("https://e.example/a.git id=p10k\n")[0]
                .id
                .as_ref()
                .expect("an id")
                .as_str(),
            "p10k"
        );
        // A dot composes an address, so `ack.vim` is a fine directory and not a
        // fine id. That is why an id is never derived from a name.
        assert!(matches!(
            fault("https://github.com/mileszs/ack.vim.git id=ack.vim\n"),
            (1, Invalid::EntryId(_))
        ));
    }

    #[test]
    fn two_entries_may_not_claim_one_directory_or_one_id() {
        // Two spellings of one repository, which is how this happens in a list
        // somebody has been editing for years.
        assert!(matches!(
            fault(
                "https://github.com/user/repo.git\n\
                 https://gitlab.example/user/repo\n"
            ),
            (2, Invalid::RepeatedName { first: 1, .. })
        ));
        assert!(matches!(
            fault(
                "https://e.example/a.git id=plugin\n\
                 https://e.example/b.git id=plugin\n"
            ),
            (2, Invalid::RepeatedEntryId { first: 1, .. })
        ));
        // The same repository under two names is not a repeat: the directories
        // differ, which is the thing that has to.
        assert_eq!(
            entries(
                "https://e.example/a.git\n\
                 https://e.example/a.git dest-name=second\n"
            )
            .len(),
            2
        );
    }

    #[test]
    fn two_names_that_differ_only_in_case_are_refused_on_every_platform() {
        let (line, fault) = fault(
            "https://e.example/Plugin.git\n\
             https://elsewhere.example/plugin.git\n",
        );
        assert_eq!(line, 2);
        assert!(
            matches!(&fault, Invalid::AliasedName { written, first: 1, .. } if written == "Plugin"),
            "{fault:?}"
        );
        // Both spellings are quoted, since neither alone says what to change.
        let said = fault.to_string();
        assert!(
            said.contains("`plugin`") && said.contains("`Plugin`"),
            "{said}"
        );

        // Case that carries a real difference is still fine: these are two
        // repositories under two names on any filesystem.
        assert_eq!(
            entries(
                "https://e.example/YouCompleteMe.git\n\
                 https://e.example/youcompleteme-config.git\n"
            )
            .len(),
            2
        );
    }

    #[test]
    fn a_real_list_reads_as_the_shell_script_read_it() {
        // The file this action exists to take over, in the two shapes it is
        // actually written in.
        let parsed = entries(
            "https://github.com/zsh-users/zsh-syntax-highlighting.git\n\
             https://github.com/vim-airline/vim-airline\n",
        );
        assert_eq!(
            parsed
                .iter()
                .map(|entry| &entry.dest_name)
                .collect::<Vec<_>>(),
            ["zsh-syntax-highlighting", "vim-airline"]
        );
    }
}
