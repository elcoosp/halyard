//! The change-file grammar (§9.4, ADR-0028): the optional
//! `.halyard/changes/*.md` files are front-matter + note, parsed with
//! the same strict-input discipline as `halyard.toml` — unknown keys,
//! unknown bumps, duplicate names and malformed tables all refuse
//! ([`ChangelogError::ChangeFile`], `E-CHLOG-CHANGE-FILE`), never skip.
//!
//! The documented grammar is TOML-style (the config crate's dialect):
//!
//! ```markdown
//! ---
//! packages = { core = "minor", "@scope/pkg" = "patch" }
//! ---
//! Free-form markdown the release notes merge verbatim.
//! ```
//!
//! One file may name several packages; several files may name one
//! package (the strongest bump wins, the notes merge in path order —
//! the merge lives in [`halyard_core::change_file::ChangeFiles::record`],
//! so the loader only owns parsing and ordering). The parser is pure:
//! bytes in, entries out (§4) — the shell owns the directory scan.

use std::collections::BTreeMap;

use halyard_core::change_file::ChangeFiles;
use halyard_core::plan::Bump;

use crate::ChangelogError;

/// Parses one change file (§9.4, ADR-0028): the `---` delimited
/// front-matter must carry exactly one key — `packages`, a `{ name =
/// "bump" }` inline table with at least one entry — and everything
/// after the closing delimiter is the note body (edge-trimmed, may be
/// empty).
///
/// # Errors
///
/// [`ChangelogError::ChangeFile`] for any grammar violation; the
/// message names the file, what is wrong and the documented shape.
pub fn parse_change_file(
    path: &str,
    bytes: &[u8],
) -> Result<(BTreeMap<String, Bump>, String), ChangelogError> {
    let text = std::str::from_utf8(bytes).map_err(|_| {
        ChangelogError::change_file(
            path,
            "the file is not valid UTF-8; change files are markdown text",
        )
    })?;
    let lines: Vec<&str> = text.lines().collect();
    let opens = lines.first().is_some_and(|line| line.trim() == "---");
    if !opens {
        return Err(ChangelogError::change_file(
            path,
            "the file does not open with a `---` front-matter delimiter; every change \
             file starts with one",
        ));
    }
    let mut front_lines = Vec::new();
    let mut closer = None;
    for (index, line) in lines.iter().enumerate().skip(1) {
        if line.trim() == "---" {
            closer = Some(index);
            break;
        }
        front_lines.push(*line);
    }
    let Some(closer) = closer else {
        return Err(ChangelogError::change_file(
            path,
            "the front matter is never closed; add the closing `---` line",
        ));
    };
    let mut note_lines = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if index > closer {
            note_lines.push(*line);
        }
    }
    let note = note_lines.join("\n");
    let packages = parse_packages(path, front_lines.join("\n").trim())?;
    Ok((packages, note.trim().to_owned()))
}

/// Parses the `packages = { ... }` front-matter body (strict: one key,
/// one inline table, quoted documented bumps, bare package names).
fn parse_packages(path: &str, front: &str) -> Result<BTreeMap<String, Bump>, ChangelogError> {
    if front.is_empty() {
        return Err(ChangelogError::change_file(
            path,
            "the front matter is empty; declare `packages = { name = \"bump\" }`",
        ));
    }
    let Some((before, after_open)) = front.split_once('{') else {
        return Err(ChangelogError::change_file(
            path,
            "the front matter must be `packages = { name = \"bump\" }` — the packages \
             value is a `{ ... }` inline table",
        ));
    };
    let Some((inner, after)) = after_open.rsplit_once('}') else {
        return Err(ChangelogError::change_file(
            path,
            "the `packages` inline table is never closed; end it with `}`",
        ));
    };
    let key = before.trim();
    let Some(key) = key.strip_suffix('=') else {
        return Err(ChangelogError::change_file(
            path,
            "unknown front-matter content before the table; the only documented key is \
             `packages = { ... }`",
        ));
    };
    if key.trim() != "packages" {
        return Err(ChangelogError::change_file(
            path,
            "unknown front-matter key; the only documented key is `packages = { ... }`",
        ));
    }
    if !after.trim().is_empty() {
        return Err(ChangelogError::change_file(
            path,
            "unknown front-matter content after the `packages` table; the front matter \
             carries exactly one key",
        ));
    }
    let inner = inner.trim();
    if inner.is_empty() {
        return Err(ChangelogError::change_file(
            path,
            "the `packages` table is empty; name at least one package and its bump",
        ));
    }
    let mut packages = BTreeMap::new();
    for entry in inner.split(',') {
        let entry = entry.trim();
        let Some((name, value)) = entry.split_once('=') else {
            return Err(ChangelogError::change_file(
                path,
                "each `packages` entry is `name = \"bump\"` (major, minor or patch)",
            ));
        };
        let name = name.trim();
        validate_name(path, name)?;
        if packages.contains_key(name) {
            return Err(ChangelogError::change_file(
                path,
                format!("the package `{name}` is named twice in one file; merge the entries"),
            ));
        }
        let value = value.trim();
        let Some(bump) = value
            .strip_prefix('"')
            .and_then(|quoted| quoted.strip_suffix('"'))
            .and_then(|word| match word {
                "major" => Some(Bump::Major),
                "minor" => Some(Bump::Minor),
                "patch" => Some(Bump::Patch),
                _ => None,
            })
        else {
            return Err(ChangelogError::change_file(
                path,
                format!(
                    "the bump for `{name}` must be a quoted \"major\", \"minor\" or \
                     \"patch\" (got {value})"
                ),
            ));
        };
        packages.insert(name.to_owned(), bump);
    }
    Ok(packages)
}

/// Validates one package name (bare identifiers: alphanumerics plus the
/// npm scoped-name characters `@` `/` `.` `_` `-` — never whitespace).
fn validate_name(path: &str, name: &str) -> Result<(), ChangelogError> {
    let valid = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '@' | '/' | '.' | '_' | '-'));
    if valid {
        return Ok(());
    }
    Err(ChangelogError::change_file(
        path,
        format!(
            "the package name `{name}` is not a valid package name; names are bare \
             identifiers (letters, digits, `@`, `/`, `.`, `_`, `-`) — quote nothing, \
             split nothing on whitespace"
        ),
    ))
}

/// Parses and merges many change files into the planner's
/// [`ChangeFiles`] input (§9.4, ADR-0028): the files are processed in
/// path order (never the caller's iteration order), so the strongest-
/// bump-wins merge and the note order are deterministic (I3). One
/// malformed file fails the whole load — a bad file never silently
/// drops its siblings' input (fail closed, ADR-0023's discipline).
///
/// # Errors
///
/// [`ChangelogError::ChangeFile`] naming the first offending file in
/// path order.
pub fn load_change_files(
    files: impl IntoIterator<Item = (String, Vec<u8>)>,
) -> Result<ChangeFiles, ChangelogError> {
    let mut sorted: Vec<(String, Vec<u8>)> = files.into_iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let mut changes = ChangeFiles::new();
    for (path, bytes) in sorted {
        let (packages, note) = parse_change_file(&path, &bytes)?;
        changes.record(&path, packages, &note);
    }
    Ok(changes)
}
