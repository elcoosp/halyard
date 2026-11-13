//! The Cargo.lock patcher (M3.2, §7.3 lockfile subset + §9.2): rewrites
//! the rows cargo itself would rewrite for the plan's version moves,
//! byte-in place (invariant I4).
//!
//! §9.2 direct-patch rules — everything else declines to
//! [`LockfileOutcome::NeedsNative`] so the orchestrator can run the
//! native refresh effect step (`cargo update --workspace --offline`):
//!
//! 1. For each changed workspace member (a `[[package]]` row **without**
//!    `source`), rewrite its `version` literal.
//! 2. For dependents referencing it in the disambiguation form
//!    `"name version"` (optionally `"name version source"`), rewrite only
//!    the version token's bytes; plain `"name"` entries need no change.
//! 3. Lockfile format `version = 3|4` is handled; any other header
//!    version (or a missing one) declines.
//! 4. Anything unrecognized — unparseable TOML, a `[[package]]` row
//!    without `name`/`version`, a changed name whose row carries a
//!    `source` (a same-named *registry* package, not the member), a
//!    duplicate member row, a member missing from the lockfile, or a
//!    version neither at `from` nor at `to` (drift) — declines to
//!    `NeedsNative`; only a non-UTF-8/unparseable file is a hard
//!    `E-LOCK-PARSE` failure.
//!
//! Rewrites are byte replacements over the source text (spans from a
//! `toml_edit` parse), never a re-serialization: every byte outside the
//! replaced literals — comments, ordering, whitespace, EOLs — survives
//! verbatim. A read-back re-parse verifies every rewritten row (§9.1
//! apply protocol); the pass is all-or-nothing and idempotent.

use std::collections::{BTreeMap, BTreeSet};

use halyard_core::lockfile::NativeRefreshError;
use halyard_core::lockfile::{LockfileFailure, LockfileOutcome, LockfileVersionChange};

/// One planned byte replacement over the source text (I4: only these
/// ranges change; everything outside survives verbatim).
struct Replacement {
    /// Inclusive start byte offset (over the whole lockfile text).
    start: usize,
    /// Exclusive end byte offset.
    end: usize,
    /// Replacement text (no surrounding syntax; callers pre-quote).
    text: String,
}

/// Applies the plan's version moves to one Cargo.lock (M3.2). `raw` must
/// be the lockfile text as it exists in the working tree.
///
/// # Errors
///
/// [`LockfileFailure::Parse`] when the lockfile is not valid UTF-8, does
/// not parse as TOML, or a planned change is not a usable exact version.
pub fn patch_lockfile(
    raw: &str,
    file: &str,
    changes: &[LockfileVersionChange],
) -> Result<LockfileOutcome, LockfileFailure> {
    validate_changes(changes, file)?;
    if changes.is_empty() {
        // Nothing to move: the byte-identical no-op (I1/I4).
        return Ok(LockfileOutcome::Patched {
            content: raw.as_bytes().to_vec(),
            applied: 0,
            skipped: 0,
        });
    }
    let doc = toml_edit::Document::parse(raw).map_err(|e| LockfileFailure::Parse {
        file: file.to_owned(),
        message: format!("lockfile does not parse as TOML: {e}"),
    })?;
    let header = doc
        .get("version")
        .and_then(toml_edit::Item::as_value)
        .and_then(toml_edit::Value::as_integer);
    let decline = |reason: String| Ok(LockfileOutcome::NeedsNative { reason });
    match header {
        Some(3 | 4) => {}
        Some(other) => {
            return decline(format!(
                "lockfile format version {other} is not handled (expected 3 or 4); refresh \
                 natively instead of guessing"
            ));
        }
        None => {
            return decline(
                "lockfile has no format version header (expected `version = 3` or \
                 `version = 4`); refresh natively instead of guessing"
                    .to_owned(),
            );
        }
    }
    let Some(packages) = doc
        .get("package")
        .and_then(toml_edit::Item::as_array_of_tables)
    else {
        return decline(
            "lockfile has no [[package]] tables; refresh natively instead of guessing".to_owned(),
        );
    };

    // Deterministic lookup by member name (I3); the state accumulates
    // the planned replacements and counts over the document-order walk.
    let mut wanted: BTreeMap<&str, &LockfileVersionChange> = BTreeMap::new();
    for change in changes {
        wanted.insert(change.name.as_str(), change);
    }
    let mut state = PatchState {
        matched: BTreeSet::new(),
        replacements: Vec::new(),
        applied: 0,
        skipped: 0,
    };

    for table in packages {
        let Some(row) = package_row(table) else {
            return decline(
                "a [[package]] row carries no `name`/`version` string pair; refresh natively \
                 instead of guessing"
                    .to_owned(),
            );
        };
        if let Err(reason) = patch_member_row(&row, raw, &wanted, &mut state) {
            return decline(reason);
        }
        if let Err(reason) = patch_dep_entries(table, raw, &wanted, &mut state) {
            return decline(reason);
        }
    }
    // §9.2 rule 1: every changed member must exist as a source-less row.
    for change in changes {
        if !state.matched.contains(change.name.as_str()) {
            return decline(format!(
                "member {:?} has no source-less [[package]] row in the lockfile (members are \
                 always recorded); refresh natively",
                change.name
            ));
        }
    }

    let text = apply_replacements(raw, file, &state.replacements)?;
    verify(&text, file, changes)?;
    Ok(LockfileOutcome::Patched {
        content: text.into_bytes(),
        applied: state.applied,
        skipped: state.skipped,
    })
}

/// Accumulated patch state of one pass (§9.2 rules 1–2).
struct PatchState<'a> {
    /// Members that have a source-less row in this lockfile.
    matched: BTreeSet<&'a str>,
    /// Planned byte replacements (applied right-to-left at the end).
    replacements: Vec<Replacement>,
    /// Rows/entries rewritten.
    applied: usize,
    /// Rows/entries already at the target (idempotent re-apply).
    skipped: usize,
}

/// §9.2 rule 1 for one row: a changed name must be a source-less row
/// (the member) at `from` (rewrite), at `to` (skip) — anything else is a
/// decline. `Err(reason)` = decline (`NeedsNative`).
fn patch_member_row<'doc>(
    row: &PackageRow<'doc>,
    raw: &str,
    wanted: &BTreeMap<&str, &LockfileVersionChange>,
    state: &mut PatchState<'doc>,
) -> Result<(), String> {
    let Some(change) = wanted.get(row.name) else {
        return Ok(());
    };
    if row.sourced {
        return Err(format!(
            "package {:?} carries a `source` (a registry package, not a workspace member); \
             rewriting it would corrupt a foreign row — refresh natively",
            row.name
        ));
    }
    if !state.matched.insert(row.name) {
        return Err(format!(
            "duplicate source-less [[package]] row for member {:?}; refresh natively instead of \
             guessing",
            row.name
        ));
    }
    if row.version == change.to {
        state.skipped += 1;
    } else if row.version == change.from {
        let text = requoted(raw, &row.version_span, &change.to)?;
        state.replacements.push(Replacement {
            start: row.version_span.start,
            end: row.version_span.end,
            text,
        });
        state.applied += 1;
    } else {
        return Err(format!(
            "member {:?} is at version {:?}, which is neither the planned from {:?} nor the \
             target {:?} — the lockfile drifted from the manifests (drift); refresh natively",
            row.name, row.version, change.from, change.to,
        ));
    }
    Ok(())
}

/// §9.2 rule 2 for one row: disambiguated `dependencies` entries follow
/// their member's move; plain `name` entries (one token) need no change.
fn patch_dep_entries(
    table: &toml_edit::Table,
    raw: &str,
    wanted: &BTreeMap<&str, &LockfileVersionChange>,
    state: &mut PatchState<'_>,
) -> Result<(), String> {
    let Some(deps) = table
        .get("dependencies")
        .and_then(toml_edit::Item::as_value)
        .and_then(toml_edit::Value::as_array)
    else {
        return Ok(());
    };
    for item in deps {
        let Some(text) = item.as_str() else {
            return Err(
                "a `dependencies` entry is not a string; refresh natively instead of guessing"
                    .to_owned(),
            );
        };
        let Some(span) = item.span() else {
            return Err(
                "a `dependencies` entry has no span; refresh natively instead of guessing"
                    .to_owned(),
            );
        };
        let Some(entry) = EntryTokens::parse(raw, &span) else {
            return Err(format!(
                "dependencies entry {text:?} is not a quoted `name [version [source]]` token \
                 list; refresh natively"
            ));
        };
        let Some(change) = wanted.get(entry.name) else {
            continue;
        };
        let Some(current) = entry.version else {
            // Plain `name` entry: no version to rewrite.
            continue;
        };
        if current == change.to {
            state.skipped += 1;
        } else if current == change.from {
            let (start, end) = entry.version_range;
            state.replacements.push(Replacement {
                start,
                end,
                text: change.to.clone(),
            });
            state.applied += 1;
        } else {
            return Err(format!(
                "dependency entry {text:?} references member {} at {current:?}, which is neither \
                 the planned from {:?} nor the target {:?} (drift); refresh natively",
                change.name, change.from, change.to,
            ));
        }
    }
    Ok(())
}

/// One `[[package]]` row's fields as read from the source text.
struct PackageRow<'a> {
    /// The `name` value as written (no delimiters).
    name: &'a str,
    /// The `version` value as written (no delimiters).
    version: &'a str,
    /// Span of the `version` quoted literal.
    version_span: std::ops::Range<usize>,
    /// True when the row carries a `source` (a registry/git package).
    sourced: bool,
}

/// Reads one `[[package]]` row; `None` when the name/version pair is
/// missing or not a plain quoted string (structural surprise → decline).
fn package_row(table: &toml_edit::Table) -> Option<PackageRow<'_>> {
    let name_value = table.get("name")?.as_value()?;
    let name_text = name_value.as_str()?;
    // The name literal must carry a span (discovery's span convention);
    // the range itself is not needed since patches match members by name.
    name_value.span()?;
    let version_value = table.get("version")?.as_value()?;
    let version_text = version_value.as_str()?;
    let version_span = version_value.span()?;
    Some(PackageRow {
        name: name_text,
        version: version_text,
        version_span,
        sourced: table.get("source").is_some(),
    })
}

/// Token list of one `dependencies` entry, resolved into byte ranges over
/// the source text. Entries are `"name"`, `"name version"` or
/// `"name version source"` (§9.2 rule 2).
struct EntryTokens<'a> {
    /// First token (the dependency name).
    name: &'a str,
    /// Second token (the exact version), when present.
    version: Option<&'a str>,
    /// Byte range of the version token over the source text (inside the
    /// quoted literal), when present.
    version_range: (usize, usize),
}

impl<'a> EntryTokens<'a> {
    /// Parses the entry at `span` (which covers the quoted literal).
    /// `None` when the literal is not a `'…'`/`"…"` pair or the tokens do
    /// not split on single spaces.
    fn parse(raw: &'a str, span: &std::ops::Range<usize>) -> Option<EntryTokens<'a>> {
        let literal = raw.get(span.clone())?;
        let quote = literal.as_bytes().first()?;
        if literal.len() < 2 || literal.as_bytes().last() != Some(quote) {
            return None;
        }
        if *quote != b'"' && *quote != b'\'' {
            return None;
        }
        let content_start = span.start + 1;
        let content = literal.get(1..literal.len() - 1)?;
        // Token offsets are over `content`; single-space separators are
        // cargo's writer format. Any double space is a shape we decline
        // (the caller falls back rather than guessing).
        let mut tokens: Vec<(&str, usize)> = Vec::new();
        let mut offset = 0usize;
        for token in content.split(' ') {
            tokens.push((token, offset));
            offset += token.len() + 1;
        }
        if tokens.iter().any(|(t, _)| t.is_empty()) {
            return None;
        }
        let (name, name_off) = *tokens.first()?;
        let name = resolve(content, name_off, name.len())?;
        let version = tokens.get(1).map(|(t, off)| (t, *off));
        let Some((version_text, version_off)) = version else {
            return Some(EntryTokens {
                name,
                version: None,
                version_range: (0, 0),
            });
        };
        let version = resolve(content, version_off, version_text.len())?;
        Some(EntryTokens {
            name,
            version: Some(version),
            version_range: (
                content_start + version_off,
                content_start + version_off + version_text.len(),
            ),
        })
    }
}

/// Slices one token out of `content` (defensive: offsets come from the
/// same split, so they always round-trip).
fn resolve(content: &str, offset: usize, len: usize) -> Option<&str> {
    content.get(offset..offset + len)
}

/// Validates the planned changes before any byte is examined: exact
/// semver versions, a real move, non-empty names. A malformed change is a
/// plan bug, not a lockfile problem — hard `E-LOCK-PARSE`, never a
/// rewrite.
fn validate_changes(changes: &[LockfileVersionChange], file: &str) -> Result<(), LockfileFailure> {
    for change in changes {
        if change.name.is_empty() {
            return Err(LockfileFailure::Parse {
                file: file.to_owned(),
                message: "planned change has an empty package name — re-plan".to_owned(),
            });
        }
        let from = semver::Version::parse(&change.from).ok();
        let to = semver::Version::parse(&change.to).ok();
        if from.is_none() || to.is_none() {
            return Err(LockfileFailure::Parse {
                file: file.to_owned(),
                message: format!(
                    "planned change for {:?} carries a non-semver version (from {:?}, to {:?}) — \
                     re-plan",
                    change.name, change.from, change.to
                ),
            });
        }
        if change.from == change.to {
            return Err(LockfileFailure::Parse {
                file: file.to_owned(),
                message: format!(
                    "planned change for {:?} is not a version move (from == to == {:?}); \
                     no-move releases must not touch the lockfile",
                    change.name, change.from
                ),
            });
        }
    }
    Ok(())
}

/// Re-quotes `text` with the literal's own quote character (the span
/// covers the quoted literal, so the replacement must carry the same
/// delimiters). `Err(message)` is a defensive span round-trip failure —
/// the step helpers surface it as a decline, and the file is left
/// untouched either way.
fn requoted(raw: &str, span: &std::ops::Range<usize>, text: &str) -> Result<String, String> {
    let literal = raw
        .get(span.clone())
        .ok_or_else(|| "version literal span does not round-trip".to_owned())?;
    let quote = match literal.as_bytes().first() {
        Some(b'\'') => '\'',
        _ => '"',
    };
    Ok(format!("{quote}{text}{quote}"))
}

/// Applies the replacements right-to-left so earlier spans stay valid.
/// Everything outside the replaced ranges is copied verbatim (I4).
fn apply_replacements(
    raw: &str,
    file: &str,
    replacements: &[Replacement],
) -> Result<String, LockfileFailure> {
    let mut ordered: Vec<&Replacement> = replacements.iter().collect();
    ordered.sort_by(|a, b| b.start.cmp(&a.start).then(b.end.cmp(&a.end)));
    let mut out = raw.as_bytes().to_vec();
    for replacement in ordered {
        let (start, end) = (replacement.start, replacement.end);
        if start > end || end > out.len() {
            return Err(LockfileFailure::Parse {
                file: file.to_owned(),
                message: format!("replacement span {start}..{end} does not fit the lockfile"),
            });
        }
        let head = out.get(..start).unwrap_or(&[]).to_vec();
        let tail = out.get(end..).unwrap_or(&[]).to_vec();
        out = head;
        out.extend_from_slice(replacement.text.as_bytes());
        out.extend_from_slice(&tail);
    }
    String::from_utf8(out).map_err(|_| LockfileFailure::Parse {
        file: file.to_owned(),
        message: "rewritten lockfile is not valid UTF-8".to_owned(),
    })
}

/// Verifies by re-parse that every changed member row and every
/// disambiguated entry referencing a changed member reads back the target
/// version (M3.1's read-back acceptance, applied to the lockfile).
fn verify(
    text: &str,
    file: &str,
    changes: &[LockfileVersionChange],
) -> Result<(), LockfileFailure> {
    let doc = toml_edit::Document::parse(text).map_err(|e| LockfileFailure::Parse {
        file: file.to_owned(),
        message: format!("read-back re-parse failed: {e}"),
    })?;
    let packages = doc
        .get("package")
        .and_then(toml_edit::Item::as_array_of_tables)
        .ok_or_else(|| LockfileFailure::Parse {
            file: file.to_owned(),
            message: "read-back check failed: rewritten lockfile has no [[package]] tables"
                .to_owned(),
        })?;
    for change in changes {
        // Member row reads back the target version.
        let mut member_rows = 0usize;
        for table in packages {
            let Some(row) = package_row(table) else {
                return Err(LockfileFailure::Parse {
                    file: file.to_owned(),
                    message: "read-back check failed: a [[package]] row lost its name/version \
                              pair"
                        .to_owned(),
                });
            };
            if row.name == change.name && !row.sourced {
                member_rows += 1;
                if row.version != change.to {
                    return Err(LockfileFailure::Parse {
                        file: file.to_owned(),
                        message: format!(
                            "read-back check failed for member {:?} (expected version {:?}, got \
                             {:?})",
                            change.name, change.to, row.version
                        ),
                    });
                }
            }
            // Every disambiguated entry referencing the member reads back
            // the target version.
            let entries = table
                .get("dependencies")
                .and_then(toml_edit::Item::as_value)
                .and_then(toml_edit::Value::as_array);
            if let Some(entries) = entries {
                for item in entries {
                    let Some(entry_text) = item.as_str() else {
                        continue;
                    };
                    let Some(tokens) = parse_entry_for_verify(entry_text) else {
                        continue;
                    };
                    if tokens.0 == change.name {
                        let Some(version) = tokens.1 else {
                            continue;
                        };
                        if version != change.to {
                            return Err(LockfileFailure::Parse {
                                file: file.to_owned(),
                                message: format!(
                                    "read-back check failed for dependency entry {entry_text:?} \
                                     (expected version {:?}, got {version:?})",
                                    change.to
                                ),
                            });
                        }
                    }
                }
            }
        }
        if member_rows != 1 {
            return Err(LockfileFailure::Parse {
                file: file.to_owned(),
                message: format!(
                    "read-back check failed: member {:?} has {member_rows} source-less rows \
                     (expected exactly 1)",
                    change.name
                ),
            });
        }
    }
    Ok(())
}

/// Splits one entry text into `(name, version)` for the read-back check
/// (no byte ranges needed here).
fn parse_entry_for_verify(entry: &str) -> Option<(&str, Option<&str>)> {
    let mut tokens = entry.split(' ');
    let name = tokens.next()?;
    if name.is_empty() {
        return None;
    }
    let version = tokens.next().filter(|t| !t.is_empty());
    Some((name, version))
}

/// One `[[package]]` row's identity + dependency entries, for the
/// before/after comparison.
struct RowSnapshot {
    /// The `version` value (empty when the row carries none — never
    /// produced by cargo, tolerated defensively).
    version: String,
    /// True when the row carries a `source`.
    sourced: bool,
    /// The `dependencies` entry texts in document order.
    deps: Vec<String>,
}

/// Parses a lockfile into `name → row snapshots` (owned: the snapshots
/// outlive the parse). The format header rides along under a sentinel
/// key no real package name can occupy.
fn parse_snapshot(bytes: &[u8]) -> Result<BTreeMap<String, Vec<RowSnapshot>>, NativeRefreshError> {
    let text = std::str::from_utf8(bytes).map_err(|_| NativeRefreshError::Verification {
        reason: "the lockfile is not valid UTF-8".to_owned(),
    })?;
    let doc = toml_edit::Document::parse(text).map_err(|e| NativeRefreshError::Verification {
        reason: format!("the lockfile does not parse as TOML: {e}"),
    })?;
    let header = doc
        .get("version")
        .and_then(toml_edit::Item::as_value)
        .and_then(toml_edit::Value::as_integer);
    let packages = doc
        .get("package")
        .and_then(toml_edit::Item::as_array_of_tables)
        .ok_or(NativeRefreshError::Verification {
            reason: "the lockfile has no [[package]] tables".to_owned(),
        })?;
    let mut rows: BTreeMap<String, Vec<RowSnapshot>> = BTreeMap::new();
    for table in packages {
        let name = table
            .get("name")
            .and_then(toml_edit::Item::as_value)
            .and_then(toml_edit::Value::as_str)
            .ok_or(NativeRefreshError::Verification {
                reason: "a [[package]] row carries no `name` string".to_owned(),
            })?
            .to_owned();
        let version = table
            .get("version")
            .and_then(toml_edit::Item::as_value)
            .and_then(toml_edit::Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let sourced = table.get("source").is_some();
        let deps = table
            .get("dependencies")
            .and_then(toml_edit::Item::as_value)
            .and_then(toml_edit::Value::as_array)
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(toml_edit::Value::as_str)
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        rows.entry(name).or_default().push(RowSnapshot {
            version,
            sourced,
            deps,
        });
    }
    let header_version = match header {
        Some(value) => value.to_string(),
        None => "missing".to_owned(),
    };
    rows.entry("\0format-header".to_owned())
        .or_default()
        .push(RowSnapshot {
            version: header_version,
            sourced: false,
            deps: Vec::new(),
        });
    Ok(rows)
}

/// Post-refresh verification of the §9.2 rule-4 fallback (M3.2): the
/// native tool may only move **workspace members** (their `version`
/// rows, plus the dependency entries referencing them). Anything else —
/// a registry row moving, a row appearing or disappearing, a changed
/// format header — is a resolution change and fails closed
/// (`E-LOCK-NATIVE`): re-plan instead of trusting a shifted tree.
///
/// Returns the sorted names of the members that moved.
///
/// # Errors
///
/// [`NativeRefreshError::Verification`] when the refresh moved rows
/// beyond the members or produced an unrecognizable lockfile.
pub fn verify_native_refresh(
    before: &[u8],
    after: &[u8],
    members: &[String],
) -> Result<Vec<String>, NativeRefreshError> {
    let before_rows = parse_snapshot(before)?;
    let after_rows = parse_snapshot(after)?;
    let member = |name: &str| members.iter().any(|m| m == name);
    let mut moved: BTreeSet<String> = BTreeSet::new();

    // Rows appearing, disappearing, or changing identity/version.
    for (name, after_list) in &after_rows {
        let Some(before_list) = before_rows.get(name) else {
            if member(name) {
                moved.insert(name.clone());
                continue;
            }
            return Err(NativeRefreshError::Verification {
                reason: format!("row for {name:?} appeared (a resolution change)"),
            });
        };
        if before_list.len() != after_list.len() {
            return Err(NativeRefreshError::Verification {
                reason: format!("row count for {name:?} changed"),
            });
        }
        let same_versions = before_list
            .iter()
            .zip(after_list.iter())
            .all(|(b, a)| b.version == a.version && b.sourced == a.sourced);
        if !same_versions && !member(name) {
            return Err(NativeRefreshError::Verification {
                reason: format!(
                    "row for {name:?} moved (version/source changed) but it is not a workspace \
                     member"
                ),
            });
        }
        if same_versions && !same_deps(before_list, after_list) {
            // Entry-only changes are attributed to the referenced names:
            // the mover must be a member.
            for referenced in moved_references(before_list, after_list) {
                if !member(&referenced) {
                    return Err(NativeRefreshError::Verification {
                        reason: format!(
                            "dependencies entry for {referenced:?} changed but it is not a \
                             workspace member"
                        ),
                    });
                }
                moved.insert(referenced);
            }
        } else if !same_versions {
            moved.insert(name.clone());
            // Entries may follow the member; attribute them too.
            for referenced in moved_references(before_list, after_list) {
                if !member(&referenced) {
                    return Err(NativeRefreshError::Verification {
                        reason: format!(
                            "dependencies entry for {referenced:?} changed but it is not a \
                             workspace member"
                        ),
                    });
                }
                if &referenced != name {
                    moved.insert(referenced);
                }
            }
        }
    }
    for name in before_rows.keys() {
        if !after_rows.contains_key(name) {
            if member(name) {
                moved.insert(name.clone());
                continue;
            }
            return Err(NativeRefreshError::Verification {
                reason: format!("row for {name:?} disappeared (a resolution change)"),
            });
        }
    }
    Ok(moved.into_iter().collect())
}

/// True when both row lists carry identical dependency entries.
fn same_deps(before: &[RowSnapshot], after: &[RowSnapshot]) -> bool {
    before
        .iter()
        .zip(after.iter())
        .all(|(b, a)| b.deps == a.deps)
}

/// The names referenced by entries that differ between the before/after
/// row lists (first token of each asymmetric entry).
fn moved_references(before: &[RowSnapshot], after: &[RowSnapshot]) -> Vec<String> {
    fn collect(rows: &[RowSnapshot]) -> BTreeMap<&str, usize> {
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for row in rows {
            for entry in &row.deps {
                let name = entry.split(' ').next().unwrap_or_default();
                if !name.is_empty() {
                    *counts.entry(name).or_default() += 1;
                }
            }
        }
        counts
    }
    let before_counts = collect(before);
    let after_counts = collect(after);
    let mut moved: Vec<String> = Vec::new();
    for (name, after_count) in &after_counts {
        if before_counts.get(name) != Some(after_count) {
            moved.push((*name).to_owned());
        }
    }
    for name in before_counts.keys() {
        if !after_counts.contains_key(name) {
            moved.push((*name).to_owned());
        }
    }
    moved.sort();
    moved.dedup();
    moved
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A representative v4 lock: two members (`app`, `core`), one
    /// same-named-registry hazard (`shared` has a source), one foreign
    /// registry package, plain and disambiguated dependency entries.
    const LOCK_V4: &str = r#"# This file is automatically @generated by Cargo.
# It is not intended for manual editing.
version = 4

[[package]]
name = "app"
version = "0.4.1"
dependencies = [
 "core",
 "shared 0.4.1",
]

[[package]]
name = "core"
version = "0.4.1"

[[package]]
name = "other"
version = "9.9.9"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
dependencies = [
 "core 0.4.1",
 "shared 0.4.1 registry+https://github.com/rust-lang/crates.io-index",
]

[[package]]
name = "shared"
version = "0.4.1"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
"#;

    fn change(name: &str, from: &str, to: &str) -> LockfileVersionChange {
        LockfileVersionChange {
            name: name.to_owned(),
            from: from.to_owned(),
            to: to.to_owned(),
        }
    }

    fn patch(raw: &str, changes: &[LockfileVersionChange]) -> LockfileOutcome {
        patch_lockfile(raw, "Cargo.lock", changes)
            .unwrap_or_else(|f| unreachable!("fixture patch failed: {f}"))
    }

    #[test]
    fn member_rows_and_disambiguated_entries_rewrite() {
        let out = patch(
            LOCK_V4,
            &[
                change("app", "0.4.1", "0.5.0"),
                change("core", "0.4.1", "0.5.0"),
            ],
        );
        let LockfileOutcome::Patched {
            content,
            applied,
            skipped,
        } = out
        else {
            unreachable!("v4 lock with plain members must patch, got {out:?}")
        };
        let text = String::from_utf8(content).unwrap_or_else(|e| unreachable!("utf8: {e}"));
        // Member rows rewritten.
        assert!(text.contains("name = \"app\"\nversion = \"0.5.0\""));
        assert!(text.contains("name = \"core\"\nversion = \"0.5.0\""));
        // Disambiguated dependency entries follow the member.
        assert!(text.contains("\"core 0.5.0\","));
        // Plain entries, foreign rows and the same-named registry package
        // keep their bytes (shared stays at 0.4.1 everywhere).
        assert!(text.contains("\"shared 0.4.1\","));
        assert!(text.contains("name = \"shared\"\nversion = \"0.4.1\""));
        assert!(text.contains("name = \"other\"\nversion = \"9.9.9\""));
        assert!(text.contains("checksum = \"bbbb"));
        // Two member rows + one disambiguated entry (`other` lists
        // `core 0.4.1`; the `shared 0.4.1` entries reference an unchanged
        // package and never rewrite).
        assert_eq!(applied, 3, "app row, core row, 1 dep entry");
        assert_eq!(skipped, 0);
    }

    #[test]
    fn unedited_bytes_survive_verbatim_outside_replaced_literals() {
        let out = patch(LOCK_V4, &[change("core", "0.4.1", "0.5.0")]);
        let LockfileOutcome::Patched { content, .. } = out else {
            unreachable!("must patch")
        };
        let text = String::from_utf8(content).unwrap_or_else(|e| unreachable!("utf8: {e}"));
        // Mask the two rewritten literals on both sides: everything else
        // must compare byte-for-byte.
        let before = LOCK_V4
            .replace("name = \"core\"\nversion = \"0.4.1\"", "@ROW@")
            .replace("\"core 0.4.1\"", "@ENTRY@");
        let after = text
            .replace("name = \"core\"\nversion = \"0.5.0\"", "@ROW@")
            .replace("\"core 0.5.0\"", "@ENTRY@");
        assert_eq!(before, after);
    }

    #[test]
    fn lockfile_v3_is_handled() {
        let lock = LOCK_V4.replace("version = 4", "version = 3");
        let out = patch(&lock, &[change("core", "0.4.1", "0.5.0")]);
        assert!(matches!(out, LockfileOutcome::Patched { applied: 2, .. }));
    }

    #[test]
    fn already_applied_changes_are_skipped_idempotent_reapply() {
        let first = patch(LOCK_V4, &[change("core", "0.4.1", "0.5.0")]);
        let LockfileOutcome::Patched { content, .. } = first else {
            unreachable!("must patch")
        };
        let once = String::from_utf8(content).unwrap_or_else(|e| unreachable!("utf8: {e}"));
        let second = patch(&once, &[change("core", "0.4.1", "0.5.0")]);
        match second {
            LockfileOutcome::Patched {
                content,
                applied,
                skipped,
            } => {
                assert_eq!(applied, 0, "rows already at target are skipped");
                assert_eq!(skipped, 2, "member row + dep entry");
                assert_eq!(
                    String::from_utf8(content).unwrap_or_else(|e| unreachable!("utf8: {e}")),
                    once,
                    "re-apply is byte-identical (idempotence)"
                );
            }
            other @ LockfileOutcome::NeedsNative { .. } => {
                unreachable!("second pass must not decline: {other:?}")
            }
        }
    }

    #[test]
    fn unknown_or_missing_format_header_declines() {
        for header in ["version = 5", "version = 2", ""] {
            let lock = LOCK_V4.replace("version = 4", header);
            let out = patch(&lock, &[change("core", "0.4.1", "0.5.0")]);
            let LockfileOutcome::NeedsNative { reason } = out else {
                unreachable!("header {header:?} must decline, got {out:?}")
            };
            assert!(reason.contains("format"), "reason explains: {reason}");
        }
    }

    #[test]
    fn changed_name_with_a_source_row_declines() {
        // `shared` exists as a registry package: rewriting "its" row would
        // corrupt a foreign package (§9.2 rule 4).
        let out = patch(LOCK_V4, &[change("shared", "0.4.1", "0.5.0")]);
        let LockfileOutcome::NeedsNative { reason } = out else {
            unreachable!("sourced row must decline, got {out:?}")
        };
        assert!(reason.contains("source"), "reason explains: {reason}");
    }

    #[test]
    fn member_missing_from_lockfile_declines() {
        let out = patch(LOCK_V4, &[change("ghost", "1.0.0", "1.1.0")]);
        let LockfileOutcome::NeedsNative { reason } = out else {
            unreachable!("missing member must decline, got {out:?}")
        };
        assert!(
            reason.contains("ghost"),
            "reason names the member: {reason}"
        );
    }

    #[test]
    fn drifted_versions_decline() {
        // Member row at neither `from` nor `to`.
        let lock = LOCK_V4.replace(
            "name = \"core\"\nversion = \"0.4.1\"",
            "name = \"core\"\nversion = \"0.3.0\"",
        );
        let out = patch(&lock, &[change("core", "0.4.1", "0.5.0")]);
        let LockfileOutcome::NeedsNative { reason } = out else {
            unreachable!("drifted member row must decline, got {out:?}")
        };
        assert!(reason.contains("drift"), "reason explains: {reason}");
        // Disambiguated entry at neither `from` nor `to`.
        let lock = LOCK_V4.replace("\"core 0.4.1\"", "\"core 0.3.0\"");
        let out = patch(&lock, &[change("core", "0.4.1", "0.5.0")]);
        assert!(
            matches!(out, LockfileOutcome::NeedsNative { .. }),
            "drifted entry must decline, got {out:?}"
        );
    }

    #[test]
    fn duplicate_member_rows_decline() {
        let dup = LOCK_V4.replace(
            "[[package]]\nname = \"other\"",
            "[[package]]\nname = \"core\"\nversion = \"0.4.1\"\n\n[[package]]\nname = \"other\"",
        );
        let out = patch(&dup, &[change("core", "0.4.1", "0.5.0")]);
        assert!(
            matches!(out, LockfileOutcome::NeedsNative { .. }),
            "duplicate member rows must decline, got {out:?}"
        );
    }

    /// House-style failure extraction (no `unwrap`/`unwrap_err` in
    /// tests; impossible paths are `unreachable!`).
    fn expect_failure(result: Result<LockfileOutcome, LockfileFailure>) -> LockfileFailure {
        match result {
            Err(failure) => failure,
            Ok(outcome) => unreachable!("expected a hard failure, got {outcome:?}"),
        }
    }

    #[test]
    fn unparseable_lockfile_is_e_lock_parse() {
        // Not TOML.
        let err = expect_failure(patch_lockfile(
            "not toml [[",
            "Cargo.lock",
            &[change("core", "0.4.1", "0.5.0")],
        ));
        assert_eq!(err.code(), "E-LOCK-PARSE");
    }

    #[test]
    fn invalid_planned_version_is_e_lock_parse() {
        let err = expect_failure(patch_lockfile(
            LOCK_V4,
            "Cargo.lock",
            &[change("core", "0.4.1", "not-semver")],
        ));
        assert_eq!(err.code(), "E-LOCK-PARSE");
        let err = expect_failure(patch_lockfile(
            LOCK_V4,
            "Cargo.lock",
            &[change("core", "0.4.1", "0.4.1")],
        ));
        assert_eq!(err.code(), "E-LOCK-PARSE", "from == to is not a move");
    }

    #[test]
    fn empty_changes_patch_nothing() {
        let out = patch(LOCK_V4, &[]);
        let LockfileOutcome::Patched {
            content,
            applied,
            skipped,
        } = out
        else {
            unreachable!("empty changes must be a no-op patch, got {out:?}")
        };
        assert_eq!(applied, 0);
        assert_eq!(skipped, 0);
        assert_eq!(content, LOCK_V4.as_bytes(), "byte-identical no-op");
    }

    #[test]
    fn crlf_lockfile_keeps_its_line_endings() {
        let lock = LOCK_V4.replace('\n', "\r\n");
        let out = patch(&lock, &[change("core", "0.4.1", "0.5.0")]);
        let LockfileOutcome::Patched { content, .. } = out else {
            unreachable!("must patch")
        };
        let text = String::from_utf8(content).unwrap_or_else(|e| unreachable!("utf8: {e}"));
        assert!(text.contains("version = \"0.5.0\"\r\n"));
        assert_eq!(
            text.matches("\r\n").count(),
            LOCK_V4.matches('\n').count(),
            "CRLF count preserved"
        );
    }
}
