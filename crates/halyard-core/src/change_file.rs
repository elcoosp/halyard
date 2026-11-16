//! The change-file planning input (§9.4, ADR-0028): the optional
//! `.halyard/changes/*.md` files ride the pure planner as plain data —
//! one explicit bump per package (replacing the commit-derived level)
//! and one markdown note per package (merged verbatim into the release's
//! changelog preview).
//!
//! The type is pure data like [`crate::history::ReleaseRefs`]: the
//! imperative shell materializes it (the strict front-matter grammar is
//! parsed in `halyard-changelog`, the §9.4 crate) and the planner reads
//! it — no I/O here (invariants I3/I6). One file may name several
//! packages; several files may name one package (the strongest bump
//! wins, the notes merge in path order — both deterministic, I3).

use std::collections::BTreeMap;

use crate::plan::Bump;

/// The change-file input of one planning run: per-package explicit
/// bumps and per-package merged notes. Empty input plans exactly as a
/// repo without change files.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChangeFiles {
    /// Explicit bump per package name; the strongest recorded bump wins.
    bumps: BTreeMap<String, ChangeBump>,
    /// Merged notes per package name, in record (path) order.
    notes: BTreeMap<String, Vec<ChangeNote>>,
}

/// One package's explicit change-file bump: the level and the file that
/// named it (the reason chain and `E-PLAN-CHANGE-FILE` cite the file).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangeBump {
    /// The explicit bump level (`Major`/`Minor`/`Patch` — the grammar
    /// admits no other).
    pub bump: Bump,
    /// Repo-relative path of the change file that recorded the winning
    /// bump.
    pub file: String,
}

/// One merged note: the file it came from and its trimmed body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangeNote {
    /// Repo-relative path of the change file carrying the note.
    pub file: String,
    /// The note body (markdown, trimmed of its edge whitespace).
    pub body: String,
}

impl ChangeFiles {
    /// Empty input — the zero-config shape (no `.halyard/changes/`
    /// directory).
    pub fn new() -> Self {
        Self::default()
    }

    /// True when no file named anything (the planner treats this
    /// exactly like a repo without change files).
    pub fn is_empty(&self) -> bool {
        self.bumps.is_empty()
    }

    /// Records one parsed change file: its per-package bumps and its
    /// note body. Called in path order by the loader; a repeated name
    /// keeps the strongest bump (and the file that carried it), and the
    /// note is appended after the existing ones — both merge orders are
    /// therefore deterministic. An empty note records nothing.
    pub fn record(
        &mut self,
        file: &str,
        packages: impl IntoIterator<Item = (String, Bump)>,
        note: &str,
    ) {
        let note = note.trim();
        let has_note = !note.is_empty();
        for (name, bump) in packages {
            match self.bumps.entry(name.clone()) {
                std::collections::btree_map::Entry::Occupied(mut occupied) => {
                    if occupied.get().bump < bump {
                        occupied.insert(ChangeBump {
                            bump,
                            file: file.to_owned(),
                        });
                    }
                }
                std::collections::btree_map::Entry::Vacant(vacant) => {
                    vacant.insert(ChangeBump {
                        bump,
                        file: file.to_owned(),
                    });
                }
            }
            if has_note {
                self.notes.entry(name).or_default().push(ChangeNote {
                    file: file.to_owned(),
                    body: note.to_owned(),
                });
            }
        }
    }

    /// The explicit bump of `name`, when a change file named it.
    pub fn bump_of(&self, name: &str) -> Option<&ChangeBump> {
        self.bumps.get(name)
    }

    /// The merged notes of `name`, in record (path) order.
    pub fn notes_for(&self, name: &str) -> &[ChangeNote] {
        self.notes.get(name).map_or(&[], Vec::as_slice)
    }

    /// Every package name any file named (bump entries and note
    /// carriers) — the planner's unknown-package validation walks this.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.bumps
            .keys()
            .chain(self.notes.keys())
            .map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_keeps_the_strongest_bump_and_the_winning_file() {
        let mut changes = ChangeFiles::new();
        changes.record(
            ".halyard/changes/b.md",
            [("app".to_owned(), Bump::Patch)],
            "",
        );
        changes.record(
            ".halyard/changes/a.md",
            [("app".to_owned(), Bump::Minor)],
            "",
        );
        let bump = changes.bump_of("app").expect("recorded");
        assert_eq!(bump.bump, Bump::Minor);
        assert_eq!(bump.file, ".halyard/changes/a.md");
    }

    #[test]
    fn a_weaker_bump_never_lowers_a_recorded_one() {
        let mut changes = ChangeFiles::new();
        changes.record(
            ".halyard/changes/a.md",
            [("app".to_owned(), Bump::Minor)],
            "",
        );
        changes.record(
            ".halyard/changes/b.md",
            [("app".to_owned(), Bump::Patch)],
            "",
        );
        let bump = changes.bump_of("app").expect("recorded");
        assert_eq!(bump.bump, Bump::Minor);
        assert_eq!(bump.file, ".halyard/changes/a.md");
    }

    #[test]
    fn notes_merge_in_record_order_and_empty_notes_record_nothing() {
        let mut changes = ChangeFiles::new();
        changes.record(
            ".halyard/changes/b.md",
            [("app".to_owned(), Bump::Patch)],
            "b note",
        );
        changes.record(
            ".halyard/changes/a.md",
            [("app".to_owned(), Bump::Minor)],
            "a note",
        );
        changes.record(
            ".halyard/changes/c.md",
            [("app".to_owned(), Bump::Patch)],
            "",
        );
        let notes = changes.notes_for("app");
        assert_eq!(notes.len(), 2, "the empty note records nothing");
        let bodies: Vec<&str> = notes.iter().map(|note| note.body.as_str()).collect();
        assert_eq!(bodies, ["b note", "a note"]);
    }

    #[test]
    fn empty_input_plans_like_a_repo_without_change_files() {
        let changes = ChangeFiles::new();
        assert!(changes.is_empty());
        assert!(changes.bump_of("app").is_none());
        assert_eq!(changes.notes_for("app").len(), 0);
        assert_eq!(changes.names().count(), 0);
    }
}
