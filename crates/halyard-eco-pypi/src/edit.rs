//! The `pyproject.toml` patcher (M8 step 1, §7.3 edit subset, ADR-0056):
//! re-applies the plan's byte patches to a PEP 621 manifest
//! **semantically and in place**, preserving every byte outside the
//! edited literals (invariant I4).
//!
//! The planner emits patches whose spans come from discovery's
//! [`inner_span`](crate::inner_span) sites: the span covers the version
//! string's **inner bytes** (npm `json_span` convention; the quoted
//! literal without its delimiters), and the patch text is the plan's
//! semver target. This module:
//!
//! 1. **Validates** every patch against a span-carrying `toml_edit`
//!    parse. The only editable slot in the pypi grammar is the
//!    `[project] version` value (PEP 621's static version row);
//!    requirement rows are `ReqSpec::Other` (ADR-0006) and the planner
//!    never patches them. A patch span must match that slot exactly —
//!    a drifted manifest fails closed (`E-EDIT-PATCHES`).
//! 2. **Renders** the plan's semver target through the PEP 440 strategy
//!    ([`crate::pep440::render_pep440`]): `1.2.3-rc.1` arrives, `1.2.3rc1`
//!    lands — the semver hyphen form is not valid PEP 440 and never
//!    reaches the manifest. A target that cannot render is
//!    `E-EDIT-UNREWRITABLE` (required-to-edit by construction).
//! 3. **Rewrites** only the matched slot's inner bytes — the quote
//!    character and every byte outside the span (decor, comments,
//!    ordering, CRLF, BOM) survive untouched. Like the cargo patcher,
//!    this is deliberately a byte replacement over the source text,
//!    NOT a `toml_edit::DocumentMut` re-serialization (which normalizes
//!    CRLF to LF and would violate invariant I4).
//! 4. **Verifies** by re-parse that the rewritten value reads back as
//!    the rendered target and that untouched slots are unchanged.
//!
//! All patches validate before any byte is rewritten — a failure leaves
//! the text untouched (§9.1 apply protocol: all or none).

use halyard_core::edit::{EditFailure, EditOutcome};
use halyard_core::package::RelPath;
use halyard_core::plan::PlanPatch;
use halyard_core::rewrite::VersionError;

use crate::pep440;
use crate::{inner_span, parse_document};

/// One validated patch: its input, the matched slot and the decision.
struct PlannedEdit<'a> {
    /// The patch being applied.
    patch: &'a PlanPatch,
    /// The matched slot (current inner text + inner span).
    slot: Slot,
    /// `true` = rewrite the value; `false` = skip (the row already
    /// carries the target version).
    applied: bool,
}

/// The only editable slot in the pypi grammar: the own `[project]`
/// version value.
#[derive(Clone)]
struct Slot {
    /// The current inner text (PEP 440 spelling as written).
    current: String,
    /// The inner byte span (excludes the string delimiters).
    span: halyard_core::package::Span,
}

/// Applies the plan's patches to one `pyproject.toml`.
pub fn edit_versions(
    file: &RelPath,
    content: &[u8],
    patches: &[PlanPatch],
) -> Result<EditOutcome, EditFailure> {
    if patches.is_empty() {
        return Ok(EditOutcome {
            content: content.to_vec(),
            applied: 0,
            skipped: 0,
        });
    }
    let raw = std::str::from_utf8(content).map_err(|_| EditFailure::Parse {
        file: file.to_string(),
        message: "manifest is not valid UTF-8".to_owned(),
    })?;
    let doc = parse_document(raw, &file.to_string()).map_err(|e| EditFailure::Parse {
        file: file.to_string(),
        message: e.to_string(),
    })?;
    let slot = collect_slot(&doc, raw, file)?;
    let plan = validate(patches, slot.as_ref(), file)?;
    let applied = plan.iter().filter(|edit| edit.applied).count();
    let skipped = plan.len() - applied;
    let content = rewrite_bytes(raw, file, &plan)?;
    verify(&content, file, &plan)?;
    Ok(EditOutcome {
        content: content.into_bytes(),
        applied,
        skipped,
    })
}

/// Reads the `[project] version` slot (the single editable site).
fn collect_slot(
    doc: &toml_edit::Document<&str>,
    raw: &str,
    file: &RelPath,
) -> Result<Option<Slot>, EditFailure> {
    let file_s = file.to_string();
    let Some(project) = doc.get("project").and_then(toml_edit::Item::as_table) else {
        return Ok(None);
    };
    let Some(item) = project.get("version") else {
        return Ok(None);
    };
    let Some(current) = item.as_value().and_then(toml_edit::Value::as_str) else {
        return Err(EditFailure::Parse {
            file: file_s,
            message: "the [project] version is not a string; nothing to edit".to_owned(),
        });
    };
    let Some(span) = inner_span(item, raw) else {
        return Err(EditFailure::Parse {
            file: file_s,
            message: "the [project] version is not a plain quoted string; nothing to edit"
                .to_owned(),
        });
    };
    Ok(Some(Slot {
        current: current.to_owned(),
        span,
    }))
}

/// Validates every patch against the slot inventory. Fail-closed: the
/// first unmatched or unrenderable patch aborts before any byte changes.
fn validate<'a>(
    patches: &'a [PlanPatch],
    slot: Option<&Slot>,
    file: &RelPath,
) -> Result<Vec<PlannedEdit<'a>>, EditFailure> {
    let mut planned = Vec::with_capacity(patches.len());
    for patch in patches {
        let wanted = halyard_core::package::Span {
            start: patch.start,
            end: patch.end,
        };
        let Some(slot) = slot else {
            return Err(EditFailure::SpanMismatch {
                file: file.to_string(),
                start: patch.start,
                end: patch.end,
            });
        };
        if slot.span != wanted {
            return Err(EditFailure::SpanMismatch {
                file: file.to_string(),
                start: patch.start,
                end: patch.end,
            });
        }
        let applied = decide(patch, slot, file)?;
        planned.push(PlannedEdit {
            patch,
            slot: slot.clone(),
            applied,
        });
    }
    Ok(planned)
}

/// Classifies one matched patch: an already-applied row (the current
/// PEP 440 spelling pairs to the patch's semver target) skips; a
/// canonical current value rewrites to the rendered target; anything
/// else is `E-EDIT-UNREWRITABLE` (plan patches are required-to-edit by
/// construction — the planner only emits own-version rows for editable
/// sites).
fn decide(patch: &PlanPatch, slot: &Slot, file: &RelPath) -> Result<bool, EditFailure> {
    let unrewritable = || EditFailure::Unrewritable {
        file: file.to_string(),
        detail: VersionError::unparseable(&slot.current, &semver::Version::new(0, 0, 0)),
    };
    let current = pep440::parse_pep440(&slot.current).ok_or_else(unrewritable)?;
    let target = semver::Version::parse(&patch.text).map_err(|_| unrewritable())?;
    if current == target {
        // Idempotent re-apply: the manifest already carries the target
        // version (the rewriter returns `Unchanged` here too).
        return Ok(false);
    }
    if pep440::render_pep440(&target).is_none() {
        // The semver target cannot render as canonical PEP 440 (a
        // foreign pre-release identifier) — fail closed, never guess.
        return Err(unrewritable());
    }
    Ok(true)
}

/// Replaces each applied patch's inner bytes with the rendered PEP 440
/// target, right-to-left so earlier spans stay valid. Everything
/// outside the matched spans is copied verbatim (invariant I4).
fn rewrite_bytes(
    raw: &str,
    file: &RelPath,
    plan: &[PlannedEdit<'_>],
) -> Result<String, EditFailure> {
    let mut text = raw.to_owned();
    for edit in plan.iter().rev() {
        if !edit.applied {
            continue;
        }
        let target = semver::Version::parse(&edit.patch.text).map_err(|_| EditFailure::Parse {
            file: file.to_string(),
            message: "plan target stopped parsing as semver".to_owned(),
        })?;
        let rendered = pep440::render_pep440(&target).ok_or_else(|| EditFailure::Unrewritable {
            file: file.to_string(),
            detail: VersionError::unparseable(&edit.slot.current, &semver::Version::new(0, 0, 0)),
        })?;
        let start = edit.patch.start as usize;
        let end = edit.patch.end as usize;
        if start > end || end > text.len() {
            return Err(EditFailure::SpanMismatch {
                file: file.to_string(),
                start: edit.patch.start,
                end: edit.patch.end,
            });
        }
        text.replace_range(start..end, &rendered);
    }
    Ok(text)
}

/// The read-back check (M3.1 acceptance, semantic): every rewritten slot
/// reads back as the rendered target through a fresh parse.
fn verify(content: &str, file: &RelPath, plan: &[PlannedEdit<'_>]) -> Result<(), EditFailure> {
    let doc = parse_document(content, &file.to_string()).map_err(|e| EditFailure::Parse {
        file: file.to_string(),
        message: e.to_string(),
    })?;
    for edit in plan.iter().filter(|e| e.applied) {
        let Some(slot) = doc
            .get("project")
            .and_then(toml_edit::Item::as_table)
            .and_then(|project| project.get("version"))
            .and_then(toml_edit::Item::as_value)
            .and_then(toml_edit::Value::as_str)
        else {
            return Err(EditFailure::Parse {
                file: file.to_string(),
                message: "the [project] version row vanished during the rewrite".to_owned(),
            });
        };
        let target = semver::Version::parse(&edit.patch.text).map_err(|_| EditFailure::Parse {
            file: file.to_string(),
            message: "plan target stopped parsing as semver".to_owned(),
        })?;
        let rendered = pep440::render_pep440(&target).unwrap_or_default();
        if slot != rendered {
            return Err(EditFailure::Parse {
                file: file.to_string(),
                message: format!("read-back {slot:?} != target {rendered:?}"),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use halyard_core::package::RelPath;

    const MANIFEST: &str = "[project]\nname = \"p\"\nversion = \"0.1.0\"\n";

    fn patch(text: &str) -> PlanPatch {
        PlanPatch {
            start: u32::try_from(MANIFEST.find("0.1.0").expect("span")).unwrap_or(0),
            end: u32::try_from(MANIFEST.find("0.1.0").expect("span") + 5).unwrap_or(0),
            text: text.to_owned(),
        }
    }

    #[test]
    fn release_target_rewrites_in_place() {
        let file = RelPath::new("pyproject.toml").expect("rel");
        let outcome = edit_versions(&file, MANIFEST.as_bytes(), &[patch("0.2.0")]).expect("edit");
        assert_eq!(outcome.applied, 1);
        assert_eq!(
            String::from_utf8(outcome.content).expect("utf8"),
            "[project]\nname = \"p\"\nversion = \"0.2.0\"\n"
        );
    }

    #[test]
    fn semver_prerelease_lands_as_pep440() {
        let file = RelPath::new("pyproject.toml").expect("rel");
        let outcome =
            edit_versions(&file, MANIFEST.as_bytes(), &[patch("1.2.3-rc.1")]).expect("edit");
        let text = String::from_utf8(outcome.content).expect("utf8");
        assert!(text.contains("version = \"1.2.3rc1\""), "{text}");
        assert!(!text.contains("-rc.1"));
    }

    #[test]
    fn already_target_row_skips() {
        let file = RelPath::new("pyproject.toml").expect("rel");
        let first = edit_versions(&file, MANIFEST.as_bytes(), &[patch("0.2.0")]).expect("edit");
        let patch = PlanPatch {
            start: u32::try_from(MANIFEST.find("0.1.0").expect("span")).unwrap_or(0),
            end: u32::try_from(MANIFEST.find("0.1.0").expect("span") + 5).unwrap_or(0),
            text: "0.2.0".to_owned(),
        };
        let second = edit_versions(&file, &first.content, &[patch]).expect("edit");
        assert_eq!(second.applied, 0);
        assert_eq!(second.skipped, 1);
        assert_eq!(second.content, first.content);
    }

    #[test]
    fn drifted_span_fails_closed() {
        let file = RelPath::new("pyproject.toml").expect("rel");
        // A longer value shifts the inner span (the manifest changed
        // since the plan was computed): the patch matches nothing and
        // must fail closed before any byte moves.
        let drifted = MANIFEST.replace("0.1.0", "0.10.0");
        let error =
            edit_versions(&file, drifted.as_bytes(), &[patch("0.2.0")]).expect_err("must fail");
        assert_eq!(error.code(), "E-EDIT-PATCHES");
    }
}
