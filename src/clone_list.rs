//! The line-oriented manifest a `git-clone-list` action reads.
//!
//! One repository per line, its metadata written beside it as `key=value`. Not
//! TOML, because the file it replaces is a list of URLs somebody maintains by
//! hand and pasting a new one on its own line has to stay the whole of adding a
//! repository.
//!
//! Reading is all this module does: it never clones, so it never writes.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use thiserror::Error;

use crate::error::Error;
use crate::item::{ItemId, ItemIdError};

/// One repository the list names.
///
/// `name` is the directory the clone lands in, under the action's `dest-dir`:
/// derived from the URL, or taken from `dest-name=`. It is settled here rather
/// than where the clone is made, because two entries deriving one directory is
/// a fault of the document and is caught by reading it.
#[derive(Debug)]
pub(crate) struct Entry {
    /// The repository, exactly as git is given it.
    #[expect(dead_code, reason = "cloned at 4.5")]
    pub url: String,
    /// The one directory component the clone lands in.
    pub name: String,
    /// Makes the entry addressable as `<action>.<entry>`.
    pub id: Option<ItemId>,
    /// The branch, tag, or commit the entry follows.
    #[expect(dead_code, reason = "honored at 4.5, with `ref` on `git-clone`")]
    pub git_ref: Option<String>,
    /// Which line of the list declared it, for a diagnostic that has to point
    /// at one. A fault found while reading carries its own line and does not
    /// come from here; this is for the entry that reads correctly and then
    /// fails to clone.
    #[expect(dead_code, reason = "names a failing entry at 4.5")]
    pub line: usize,
}

/// Read and check one list.
///
/// The path is attached here rather than threaded through the rules, which are
/// about a line and do not care what file it came from — the same division
/// [`crate::manifest::Manifest::load`] makes.
///
/// A path that names nothing is the caller's to report and not this function's:
/// a list is an action's `source`, and one the repository does not have has
/// already failed as a missing source by the time this is reached.
pub(crate) fn read(path: &Path) -> Result<Vec<Entry>, Error> {
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
///
/// **One problem at a time**, as a manifest's rules are, so a list with two
/// faults reports the earlier one and the next run reports the rest.
fn parse(text: &str) -> Result<Vec<Entry>, (usize, Invalid)> {
    let mut entries: Vec<Entry> = Vec::new();
    // What each name and id was first claimed on, which is the half of a repeat
    // that the second line cannot say for itself. A name is kept under its
    // folded spelling and beside the one that was written: what decides is the
    // first, and what a diagnostic has to quote is the second.
    let mut names: BTreeMap<String, (usize, String)> = BTreeMap::new();
    let mut ids: BTreeMap<String, usize> = BTreeMap::new();

    for (index, text) in text.lines().enumerate() {
        let line = index + 1;
        let Some(entry) = entry(text, line).map_err(|invalid| (line, invalid))? else {
            continue;
        };

        if let Some((first, written)) = names.insert(fold(&entry.name), (line, entry.name.clone()))
        {
            return Err((
                line,
                if written == entry.name {
                    Invalid::RepeatedName {
                        name: entry.name,
                        first,
                    }
                } else {
                    Invalid::AliasedName {
                        name: entry.name,
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
    let Some(url) = fields.next() else {
        return Ok(None);
    };
    if url.is_empty() {
        return Err(Invalid::UrlEmpty);
    }

    let mut metadata: BTreeMap<String, String> = BTreeMap::new();
    for field in fields {
        let Some((key, value)) = field.split_once('=') else {
            return Err(Invalid::NotMetadata { field });
        };
        match key {
            "id" | "ref" | "dest-name" => {}
            // Specified, and refused until the step that evaluates one, rather
            // than accepted and quietly never consulted.
            // CARRY(5.6): conditions arrive with `when` and `unless` on an
            // action; delete this arm and let the two keys through.
            "when" | "unless" => {
                return Err(Invalid::ConditionNotYet {
                    key: key.to_owned(),
                });
            }
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
            let derived = derive_name(&url);
            if !is_one_component(&derived) {
                return Err(Invalid::DerivedNameUnusable { url, name: derived });
            }
            derived
        }
    };

    Ok(Some(Entry {
        url,
        name,
        id: metadata
            .get("id")
            .map(|id| ItemId::try_from(id.clone()))
            .transpose()?,
        git_ref: metadata.get("ref").cloned(),
        line,
    }))
}

/// The directory an entry clones into when it does not name one.
///
/// Textual, and deliberately so: which of the several things git accepts a
/// source is is git's question, and the last component is the answer for all of
/// them. `/` and `:` are both separators here, which is what carries the
/// `scp`-style `git@host:repo.git` that has no slash to end on; a `ssh://`
/// URL's port colon comes before its last slash, so the later separator wins in
/// every form at once.
///
/// What comes back is not necessarily usable — [`is_one_component`] is what
/// decides that, and a source it cannot name a directory for is one the author
/// writes `dest-name=` for.
fn derive_name(url: &str) -> String {
    let trimmed = url.trim_end_matches('/');
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

/// A name as a filesystem that ignores case would see it.
///
/// **Only ever used to refuse a list, never to decide where a clone goes**, so
/// approximating is the right shape: what this has to catch is two entries that
/// *could* be one directory somewhere, and lowercasing catches more of those
/// than the case-folding rules of any one filesystem would. A clone still lands
/// at the name exactly as it was written.
fn fold(name: &str) -> String {
    name.to_lowercase()
}

/// Whether a name is one ordinary directory, which is all an entry may install
/// into.
///
/// `dest-name` is a name and not a path: an entry cannot reach out of the
/// directory the action declared, whether by climbing, by anchoring, or by
/// naming a directory of its own. `:` is refused with the separators because a
/// list is meant to be read on every machine that shares the repository, and a
/// name holding one cannot be a directory on all of them.
fn is_one_component(name: &str) -> bool {
    !name.is_empty() && !matches!(name, "." | ".." | ".git") && !name.contains(['/', '\\', ':'])
}

/// One line split into fields, with quoting honored and the comment dropped.
///
/// The first `#` outside a quoted value ends the line, so a comment may hold
/// anything at all — including text that looks like metadata. A `#` that is
/// part of a URL has to be written `%23`, which is what a URL means by it
/// anyway.
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
///
/// Three escapes and no others. A `\n` that meant a newline would be a second
/// way of writing something no field can hold, and one that silently meant `n`
/// would be worse; what the escapes exist for is a quote inside a quoted value.
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
///
/// **Every variant is a predicate rather than a sentence.** The subject is
/// supplied by [`Error::CloneList`], which names the file and the line, so
/// nothing below carries either.
#[derive(Debug, Error)]
pub(crate) enum Invalid {
    /// A line whose first field is empty, which is the one thing a repository
    /// cannot be. What the rest of it means is git's question, the same way it
    /// is for a `git-clone` record's `source`.
    #[error("names no repository")]
    UrlEmpty,

    /// A field after the repository that is not `key=value`. Very often a
    /// second repository written on the same line.
    #[error("has `{field}` after the repository, which is not `key=value` metadata")]
    NotMetadata { field: String },

    #[error("uses the unknown key `{key}`; the keys are id, ref, and dest-name")]
    UnknownKey { key: String },

    /// A condition on an entry, which the format specifies and nothing
    /// evaluates yet. Refused rather than accepted and ignored, so that a list
    /// never installs what it said to leave out.
    #[error("uses `{key}`, and conditions arrive at step 5.6")]
    ConditionNotYet { key: String },

    #[error("writes `{key}` twice")]
    RepeatedKey { key: String },

    #[error("writes `{key}=` with no value")]
    EmptyValue { key: String },

    /// A quoted value that never closes, which would otherwise swallow whatever
    /// follows it — including the `#` that was meant to start a comment.
    #[error("opens a quote that never closes")]
    UnterminatedQuote,

    /// A backslash inside a quoted value that escapes something the format has
    /// no meaning for. Refused rather than read as the character itself, since
    /// `\n` is far likelier to mean a newline to whoever wrote it.
    #[error("writes the escape `\\{escape}`; only `\\\\`, `\\\"`, and `\\'` are escapes")]
    UnsupportedEscape { escape: char },

    /// An `id` that is not an ID. The rejected value travels inside
    /// [`ItemIdError`], which already renders the whole complaint.
    #[error(transparent)]
    EntryId(#[from] ItemIdError),

    /// A repository whose last component is no directory name: a bare host, a
    /// path ending in a separator this cannot see past, a Windows path whose
    /// drive letter is the last separator.
    #[error(
        "names `{url}`, which gives `{name}` as a directory name; \
         write `dest-name=` to say what to call the clone"
    )]
    DerivedNameUnusable { url: String, name: String },

    /// A `dest-name` that is a path rather than a name. The entry is trying to
    /// install outside the directory its action declared, which is the one
    /// thing this field must not be able to do.
    #[error("writes `dest-name={name}`, which is not one ordinary directory name")]
    DestNameUnusable { name: String },

    /// Two entries installing into one directory. The second would clone over
    /// the first, or — once a clone is there — update it from the wrong
    /// repository on every run.
    #[error("clones into `{name}`, which line {first} already clones into")]
    RepeatedName { name: String, first: usize },

    /// Two entries whose names differ only in case. One directory on Windows
    /// and on a typical macOS volume, two on Linux — so this is refused
    /// everywhere rather than on the machines where it bites, a list being
    /// meant to read the same on all of them. Refused rather than merged: the
    /// second entry would find the first's clone, and an update never checks
    /// which repository a clone came from, so the wrong one would sit there
    /// reporting success on every run.
    #[error(
        "clones into `{name}`, which differs only in case from the `{written}` on line {first}; \
         on a filesystem that ignores case they are one directory"
    )]
    AliasedName {
        name: String,
        written: String,
        first: usize,
    },

    /// Two entries answering to one address, which would make either
    /// unreachable.
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
    fn name_of(url: &str) -> String {
        let parsed = entries(url);
        parsed.into_iter().next().expect("one entry").name
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
        for url in ["git@host:", "C:\\src\\repo", "https://e.example/.git"] {
            assert!(
                matches!(fault(url), (1, Invalid::DerivedNameUnusable { .. })),
                "`{url}` should not name a directory"
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
        assert_eq!(parsed[0].name, "a");
    }

    #[test]
    fn a_fault_is_reported_against_the_line_it_is_written_on() {
        // Physical lines, counted through the ones that declare nothing: a
        // diagnostic that named the entry's position in the list would send the
        // reader to the wrong line of the file.
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
        // A space, which would otherwise end the field, and a `#`, which would
        // otherwise end the line. Read back through `dest-name`, the one key
        // whose value 4.4 acts on.
        let parsed = entries("https://e.example/a.git dest-name=\"my plugin\"\n");
        assert_eq!(parsed[0].name, "my plugin");
        assert_eq!(name_of("https://e.example/a.git dest-name='a#b'"), "a#b");
    }

    #[test]
    fn a_ref_is_accepted_and_waits_for_the_step_that_follows_it() {
        // Validated as the list is read, carried out at 4.5. Nothing here reads
        // the value back, because nothing in this build does.
        assert_eq!(entries("https://e.example/a.git ref=main\n").len(), 1);
        assert_eq!(
            entries("https://e.example/a.git ref='refs/heads/main'\n").len(),
            1
        );
    }

    #[test]
    fn a_line_splits_into_fields_at_whitespace_a_quote_does_not_cover() {
        // The lexer at its own level, because what a quoted value may hold is
        // wider than what any key accepts today: a name cannot hold a
        // backslash, so reading one back through `dest-name` would be testing
        // the name rule rather than the quoting.
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
        assert!(matches!(fault("'' id=a\n"), (1, Invalid::UrlEmpty)));
    }

    #[test]
    fn a_condition_is_refused_until_something_can_evaluate_one() {
        for key in ["when", "unless"] {
            let written = format!("https://e.example/a.git {key}=\"os == 'linux'\"\n");
            assert!(
                matches!(fault(&written), (1, Invalid::ConditionNotYet { .. })),
                "`{key}` should be refused"
            );
        }
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
        // One directory on Windows and on a typical macOS volume, two on Linux.
        // Refusing everywhere is what keeps a list meaning the same thing on
        // every machine that shares the repository -- and on the machines where
        // they do collide, the second entry would find the first's clone and an
        // update would leave the wrong repository sitting there.
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
            parsed.iter().map(|entry| &entry.name).collect::<Vec<_>>(),
            ["zsh-syntax-highlighting", "vim-airline"]
        );
    }
}
