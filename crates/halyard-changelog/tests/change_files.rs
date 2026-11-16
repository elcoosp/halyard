//! The change-file grammar (§9.4, ADR-0028): `.halyard/changes/*.md`
//! files are front-matter + note — the strict parser rejects anything
//! else with `E-CHLOG-CHANGE-FILE`, and [`load_change_files`] merges
//! many files deterministically (path order, strongest bump wins).

use std::collections::BTreeMap;

use halyard_changelog::ChangelogError;
use halyard_changelog::change_files::{load_change_files, parse_change_file};
use halyard_core::plan::Bump;

/// The documented grammar: `---` delimiters, one `packages` inline
/// table, then the note body.
#[test]
fn parses_front_matter_and_note() {
    let source = b"---\npackages = { app = \"minor\", @scope/pkg = \"patch\" }\n---\n\nUpgrade notes:\n\n- run the migration\n";
    let (packages, note) = parse_change_file(".halyard/changes/next.md", source).expect("parses");
    assert_eq!(packages.len(), 2, "one file may name several packages");
    assert_eq!(packages.get("app"), Some(&Bump::Minor));
    assert_eq!(packages.get("@scope/pkg"), Some(&Bump::Patch));
    assert_eq!(note, "Upgrade notes:\n\n- run the migration");
}

/// A bump-only file (no note body) is legal — the bump is the message.
#[test]
fn accepts_a_bump_only_file() {
    let source = b"---\npackages = { app = \"patch\" }\n---\n";
    let (packages, note) = parse_change_file(".halyard/changes/fix.md", source).expect("parses");
    assert_eq!(packages.get("app"), Some(&Bump::Patch));
    assert_eq!(note, "");
}

/// Blank lines around the body are trimmed; the note stays verbatim
/// inside.
#[test]
fn trims_only_the_note_edges() {
    let source = b"---\npackages = { app = \"minor\" }\n---\n\n\nFirst line.\n\nSecond line.\n\n\n";
    let (_, note) = parse_change_file("a.md", source).expect("parses");
    assert_eq!(note, "First line.\n\nSecond line.");
}

fn assert_rejects(source: &[u8], needle: &str) {
    let Err(error) = parse_change_file(".halyard/changes/bad.md", source) else {
        unreachable!("the grammar is strict");
    };
    assert_eq!(error.code(), "E-CHLOG-CHANGE-FILE", "{error}");
    assert!(error.to_string().contains(needle), "{error}");
    assert!(
        error.to_string().contains(".halyard/changes/bad.md"),
        "the file is named: {error}"
    );
}

/// No front-matter at all — a stray markdown file in the directory is a
/// mistake worth surfacing, never a silent skip.
#[test]
fn rejects_a_file_without_front_matter() {
    assert_rejects(b"just a note\n", "front-matter delimiter");
}

/// Missing the closing delimiter.
#[test]
fn rejects_an_unclosed_front_matter() {
    assert_rejects(b"---\npackages = { app = \"minor\" }\n", "front matter");
}

/// Unknown front-matter keys refuse (the `E-CFG-UNKNOWN-KEY` discipline).
#[test]
fn rejects_unknown_front_matter_keys() {
    assert_rejects(
        b"---\npackages = { app = \"minor\" }\nextra = true\n---\n",
        "unknown",
    );
}

/// Only `major`, `minor` and `patch` are documented bumps.
#[test]
fn rejects_an_unknown_bump_value() {
    assert_rejects(b"---\npackages = { app = \"mega\" }\n---\n", "major");
}

/// An empty `packages` table says nothing — refuse rather than merge a
/// note into every release.
#[test]
fn rejects_an_empty_packages_table() {
    assert_rejects(b"---\npackages = { }\n---\nnote\n", "empty");
}

/// One file cannot name a package twice.
#[test]
fn rejects_a_duplicate_name_in_one_file() {
    assert_rejects(
        b"---\npackages = { app = \"minor\", app = \"patch\" }\n---\n",
        "twice",
    );
}

/// Non-UTF-8 bytes refuse — the files are markdown, not payloads.
#[test]
fn rejects_non_utf8_bytes() {
    assert_rejects(&[b'-', b'-', b'-', b'\n', 0xff, b'\n'], "UTF-8");
}

/// Package names: bare identifiers with the npm scoped-name characters;
/// whitespace is never a name.
#[test]
fn rejects_names_with_spaces() {
    assert_rejects(b"---\npackages = { my app = \"minor\" }\n---\n", "name");
}

/// Many files merge deterministically: the iteration order never leaks
/// into the result (path order), the strongest bump wins per package
/// and every note lands in path order.
#[test]
fn load_merges_in_path_order_with_strongest_bump_wins() {
    let files = [
        (
            ".halyard/changes/b-patch.md".to_owned(),
            b"---\npackages = { app = \"patch\" }\n---\nb note\n".to_vec(),
        ),
        (
            ".halyard/changes/a-minor.md".to_owned(),
            b"---\npackages = { app = \"minor\" }\n---\na note\n".to_vec(),
        ),
    ];
    let changes = load_change_files(files).expect("parses");
    let bump = changes.bump_of("app").expect("the bump is recorded");
    assert_eq!(bump.bump, Bump::Minor, "the strongest bump wins");
    assert_eq!(bump.file, ".halyard/changes/a-minor.md");
    let notes = changes.notes_for("app");
    assert_eq!(notes.len(), 2);
    let first = notes.first().expect("two notes were merged");
    let second = notes.get(1).expect("two notes were merged");
    assert_eq!(first.file, ".halyard/changes/a-minor.md", "path order");
    assert_eq!(first.body, "a note");
    assert_eq!(second.file, ".halyard/changes/b-patch.md");
}

/// Different packages from different files each keep their own bump.
#[test]
fn load_keeps_per_package_bumps() {
    let files = [
        (
            ".halyard/changes/one.md".to_owned(),
            b"---\npackages = { app = \"minor\" }\n---\n".to_vec(),
        ),
        (
            ".halyard/changes/two.md".to_owned(),
            b"---\npackages = { cli = \"patch\" }\n---\n".to_vec(),
        ),
    ];
    let changes = load_change_files(files).expect("parses");
    assert_eq!(changes.bump_of("app").map(|b| b.bump), Some(Bump::Minor));
    assert_eq!(changes.bump_of("cli").map(|b| b.bump), Some(Bump::Patch));
    assert!(changes.bump_of("ghost").is_none());
    assert!(!changes.is_empty());
}

/// A malformed file fails the whole load — one bad file never silently
/// drops its siblings' input.
#[test]
fn load_fails_closed_on_any_bad_file() {
    let files = [
        (
            ".halyard/changes/good.md".to_owned(),
            b"---\npackages = { app = \"minor\" }\n---\n".to_vec(),
        ),
        (
            ".halyard/changes/bad.md".to_owned(),
            b"---\npackages = { app = \"banana\" }\n---\n".to_vec(),
        ),
    ];
    let error = load_change_files(files).expect_err("fails closed");
    assert!(
        matches!(error, ChangelogError::ChangeFile { .. }),
        "{error:?}"
    );
}

/// The parse map is a plain [`BTreeMap`] so shell callers can build the
/// [`halyard_core::change_file::ChangeFiles`] input without a round-trip
/// through the changelog crate.
#[test]
fn parse_returns_a_plain_map() {
    let (packages, _) =
        parse_change_file("a.md", b"---\npackages = { app = \"patch\" }\n---\n").expect("parses");
    let plain: BTreeMap<String, Bump> = packages;
    assert_eq!(plain.get("app"), Some(&Bump::Patch));
}
