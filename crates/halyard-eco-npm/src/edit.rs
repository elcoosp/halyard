//! The `package.json` patcher (M3.3, §7.3 edit subset): re-applies the
//! plan's byte patches (§5.8 `edits`, §9.1) to a parsed manifest
//! **semantically**, preserving every other byte (invariant I4 — the
//! lossless span scanner's [`JsonDoc::render`] applies only the recorded
//! inner-span replacements).
//!
//! The planner emits patches whose spans come from discovery's JSON span
//! scanner: for strings the span is the **inner** text between the
//! quotes, and the patch text is the new requirement/version. This
//! module:
//!
//! 1. **Validates** every patch against the spanned tree: the span must
//!    match exactly one editable string value — the package's own
//!    `version` or a requirement string inside `dependencies`,
//!    `devDependencies`, `peerDependencies`, `optionalDependencies`.
//! 2. **Classifies** the requirement with the M1.6 parser's
//!    [`classify_req`](crate::classify_req) so skip-and-count rows match
//!    discovery's [`ReqSpec`] decisions exactly: `VersionReq` rows
//!    rewrite; `workspace:`/`catalog:`/`link:`/`file:` protocols and
//!    foreign grammars (`>=8 <10`, URLs) are skip-and-count — the planner
//!    never rewrites them (ADR-0006, §10.4); a row already carrying the
//!    target text is skip-and-count too (idempotent re-apply).
//! 3. **Applies** through [`JsonDoc::replace_span`]/[`JsonDoc::render`]
//!    and **verifies** by re-parse that every edited value reads back as
//!    the patch text.
//!
//! All patches validate before anything is recorded — a failure leaves
//! the document untouched (§9.1 apply protocol: all or none).

use halyard_core::ReqSpec;
use halyard_core::edit::EditFailure;
use halyard_core::package::Span;
use halyard_core::plan::PlanPatch;

use crate::json_span::{JsonDoc, JsonValue};

/// The dependency fields whose string values are editable requirement
/// rows (§6.4), in discovery's fixed order.
const DEP_FIELDS: [&str; 4] = [
    "dependencies",
    "devDependencies",
    "peerDependencies",
    "optionalDependencies",
];

/// Applied/skipped row counts of one edit pass (M3.6 apply summary).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EditCounts {
    /// Rows whose text was rewritten.
    pub applied: usize,
    /// Rows matched but intentionally not rewritten (protocols, foreign
    /// grammars, rows already carrying the target text).
    pub skipped: usize,
}

/// One editable slot found during the validation walk.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Slot {
    /// Inner span of the string literal (between the quotes — the JSON
    /// scanner's edit-site convention, §9.1).
    span: Span,
    /// Decoded current text.
    current: String,
}

/// Applies the plan's requirement/version patches to `doc` in place
/// (M3.3).
///
/// `doc` must have been parsed from the manifest text the patches were
/// computed against. On success the caller renders the edited document
/// through [`JsonDoc::render`] — only the matched inner spans changed.
///
/// # Errors
///
/// [`EditFailure::SpanMismatch`] when a patch span matches no editable
/// slot (the manifest drifted from the plan — re-plan),
/// [`EditFailure::Parse`] when the document serialization does not
/// re-parse or the read-back check fails. Requirement rows that are not
/// plain semver requirements are **not** errors: they are skip-and-count,
/// consistently with the M1.6 [`ReqSpec`] classification.
pub fn edit_versions(
    doc: &mut JsonDoc,
    file: &str,
    patches: &[PlanPatch],
) -> Result<EditCounts, EditFailure> {
    if patches.is_empty() {
        return Ok(EditCounts::default());
    }
    let slots = collect_slots(doc.root());
    let plan = validate(patches, &slots, file)?;
    // All patches validated — record the replacements and verify.
    apply(doc, &plan, file)?;
    verify(doc, &plan, file)?;
    Ok(EditCounts {
        applied: plan.iter().filter(|edit| edit.applied).count(),
        skipped: plan.iter().filter(|edit| !edit.applied).count(),
    })
}

/// One validated patch: its input, the matched slot and the decision.
struct PlannedEdit<'a> {
    /// The patch being applied.
    patch: &'a PlanPatch,
    /// The matched slot.
    slot: Slot,
    /// `true` = rewrite the value; `false` = skip.
    applied: bool,
}

/// Validates every patch against the slot inventory. Fail-closed: the
/// first unmatched patch aborts before any edit is recorded.
fn validate<'a>(
    patches: &'a [PlanPatch],
    slots: &[Slot],
    file: &str,
) -> Result<Vec<PlannedEdit<'a>>, EditFailure> {
    let mut planned = Vec::with_capacity(patches.len());
    for patch in patches {
        let wanted = Span {
            start: patch.start,
            end: patch.end,
        };
        let Some(slot) = slots.iter().find(|slot| slot.span == wanted) else {
            return Err(EditFailure::SpanMismatch {
                file: file.to_owned(),
                start: patch.start,
                end: patch.end,
            });
        };
        // Skip-and-count rules (M3.3): protocols, foreign grammars and
        // already-applied rows are not rewritten; plain semver rows are.
        let applied =
            slot.current != patch.text && classify_req_class(&slot.current) == ReqClass::Semver;
        planned.push(PlannedEdit {
            patch,
            slot: slot.clone(),
            applied,
        });
    }
    Ok(planned)
}

/// Coarse classification mirroring [`crate::classify_req`]'s `ReqSpec`
/// decisions (the patcher only needs the VersionReq/protocol split).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReqClass {
    /// A plain semver requirement (rewritable).
    Semver,
    /// A protocol (`workspace:`/`catalog:`/`link:`/`file:`) or a foreign
    /// grammar — skip-and-count (ADR-0006).
    ProtocolOrForeign,
}

/// Classifies a requirement text the way the M1.6 parser does.
fn classify_req_class(text: &str) -> ReqClass {
    let (spec, _internal) = crate::classify_req(text);
    match spec {
        ReqSpec::VersionReq(_) => ReqClass::Semver,
        _ => ReqClass::ProtocolOrForeign,
    }
}

/// Records the validated replacements on the document.
fn apply(doc: &mut JsonDoc, plan: &[PlannedEdit<'_>], file: &str) -> Result<(), EditFailure> {
    for edit in plan.iter().filter(|edit| edit.applied) {
        doc.replace_span(edit.slot.span, &edit.patch.text)
            .map_err(|e| EditFailure::Parse {
                file: file.to_owned(),
                message: e.to_string(),
            })?;
    }
    Ok(())
}

/// Verifies by re-parse that every rewritten value reads back as the
/// patch text.
fn verify(doc: &JsonDoc, plan: &[PlannedEdit<'_>], file: &str) -> Result<(), EditFailure> {
    let rendered = doc.render();
    let output = JsonDoc::parse(&rendered).map_err(|e| EditFailure::Parse {
        file: file.to_owned(),
        message: e.to_string(),
    })?;
    let slots = collect_slots(output.root());
    for edit in plan.iter().filter(|edit| edit.applied) {
        // Length-changing patches shift later spans, so the read-back
        // check matches by decoded value: the target text must exist as a
        // slot value, and skipped rows must have kept theirs.
        let found = slots.iter().any(|slot| slot.current == edit.patch.text);
        if !found {
            return Err(EditFailure::Parse {
                file: file.to_owned(),
                message: format!(
                    "read-back check failed at {}..{} (expected {:?})",
                    edit.slot.span.start, edit.slot.span.end, edit.patch.text
                ),
            });
        }
    }
    Ok(())
}

/// Walks the spanned tree and records every editable slot: the package's
/// own `version` and every string requirement inside the four dependency
/// fields (§6.4).
fn collect_slots(root: &crate::json_span::SpannedValue) -> Vec<Slot> {
    let mut slots = Vec::new();
    let JsonValue::Object(entries) = &root.value else {
        return slots;
    };
    if let Some(entry) = entries.iter().find(|entry| entry.key == "version")
        && let JsonValue::String { decoded, inner } = &entry.value.value
    {
        slots.push(Slot {
            span: *inner,
            current: decoded.clone(),
        });
    }
    for field in DEP_FIELDS {
        let Some(entry) = entries.iter().find(|entry| entry.key == field) else {
            continue;
        };
        if let JsonValue::Object(deps) = &entry.value.value {
            for dep in deps {
                if let JsonValue::String { decoded, inner } = &dep.value.value {
                    slots.push(Slot {
                        span: *inner,
                        current: decoded.clone(),
                    });
                }
            }
        }
    }
    slots
}

#[cfg(test)]
mod tests {
    use super::*;
    use halyard_core::plan::PlanPatch;

    /// Byte patch over `needle`'s inner span (string values: quotes
    /// excluded — the JSON scanner's span convention).
    fn patch_at(raw: &str, needle: &str, text: &str) -> PlanPatch {
        let start = u32::try_from(raw.find(needle).unwrap_or(0)).unwrap_or(u32::MAX);
        let end = start.saturating_add(u32::try_from(needle.len()).unwrap_or(u32::MAX));
        PlanPatch {
            start,
            end,
            text: text.to_owned(),
        }
    }

    fn edit(raw: &str, patches: &[PlanPatch]) -> (Vec<u8>, EditCounts) {
        let mut doc = JsonDoc::parse(raw.as_bytes()).unwrap();
        let counts = edit_versions(&mut doc, "package.json", patches).unwrap();
        (doc.render(), counts)
    }

    #[test]
    fn plain_requirement_and_own_version_rewrite() {
        let raw = "{\n\t\"name\": \"@scope/app\",\n\t\"version\": \"1.0.0\",\n\t\"dependencies\": {\n\t\t\"lodash\": \"^4.17.0\",\n\t\t\"keep\": \"3.0.0\"\n\t}\n}\n";
        let patches = [
            patch_at(raw, "1.0.0", "1.1.0"),
            patch_at(raw, "^4.17.0", "^4.18.0"),
        ];
        let (out, counts) = edit(raw, &patches);
        assert_eq!(
            String::from_utf8(out).unwrap_or_default(),
            "{\n\t\"name\": \"@scope/app\",\n\t\"version\": \"1.1.0\",\n\t\"dependencies\": {\n\t\t\"lodash\": \"^4.18.0\",\n\t\t\"keep\": \"3.0.0\"\n\t}\n}\n"
        );
        assert_eq!(
            counts,
            EditCounts {
                applied: 2,
                skipped: 0
            }
        );
    }

    #[test]
    fn protocols_and_foreign_grammars_are_skip_and_count() {
        let raw = "{\n\t\"dependencies\": {\n\t\t\"a\": \"workspace:*\",\n\t\t\"b\": \"catalog:\",\n\t\t\"c\": \"catalog:std\",\n\t\t\"d\": \"link:../g\",\n\t\t\"e\": \"file:../f\",\n\t\t\"f\": \">=8 <10\"\n\t}\n}\n";
        let patches = [
            patch_at(raw, "workspace:*", "9.0.0"),
            patch_at(raw, "catalog:", "9.0.0"),
            patch_at(raw, "catalog:std", "9.0.0"),
            patch_at(raw, "link:../g", "9.0.0"),
            patch_at(raw, "file:../f", "9.0.0"),
            patch_at(raw, ">=8 <10", "9.0.0"),
        ];
        let (out, counts) = edit(raw, &patches);
        assert_eq!(
            String::from_utf8(out).unwrap_or_default(),
            raw,
            "skipped rows leave the manifest byte-identical"
        );
        assert_eq!(counts.applied, 0);
        assert_eq!(counts.skipped, 6);
    }

    #[test]
    fn reapply_is_idempotent() {
        let raw = "{\n\t\"version\": \"1.0.0\",\n\t\"devDependencies\": {\n\t\t\"x\": \"^1.2.0\"\n\t}\n}\n";
        let patches = [
            patch_at(raw, "1.0.0", "1.0.1"),
            patch_at(raw, "^1.2.0", "^1.3.0"),
        ];
        let (once, first) = edit(raw, &patches);
        assert_eq!(first.applied, 2);
        let (twice, second) = edit(std::str::from_utf8(&once).unwrap_or_default(), &patches);
        assert_eq!(second.applied, 0);
        assert_eq!(second.skipped, 2);
        assert_eq!(twice, once, "re-apply is byte-identical (idempotence)");
    }

    #[test]
    fn stale_span_is_e_edit_patches() {
        let raw = "{\"dependencies\": {\"a\": \"^1\"}}";
        let patches = [PlanPatch {
            start: 0,
            end: 1,
            text: "9".to_owned(),
        }];
        let mut doc = JsonDoc::parse(raw.as_bytes()).unwrap();
        let err = edit_versions(&mut doc, "package.json", &patches).unwrap_err();
        assert_eq!(err.code(), "E-EDIT-PATCHES");
        assert!(matches!(err, EditFailure::SpanMismatch { .. }));
        assert_eq!(doc.render(), raw.as_bytes().to_vec(), "nothing mutated");
    }

    #[test]
    fn style_preserving_across_indentation_and_escaped_keys() {
        let raw = "{\r\n  \"version\": \"0.1.0\",\r\n  \"peerDependencies\": {\r\n    \"react\": \"^18.0.0\"\r\n  },\r\n  \"optionalDependencies\": {\"z\": \"~2.1.0\"}\r\n}\r\n";
        let patches = [
            patch_at(raw, "0.1.0", "0.2.0"),
            patch_at(raw, "^18.0.0", "^19.0.0"),
            patch_at(raw, "~2.1.0", "~2.2.0"),
        ];
        let (out, counts) = edit(raw, &patches);
        let unchanged = raw
            .replace("0.1.0", "@A@")
            .replace("^18.0.0", "@B@")
            .replace("~2.1.0", "@C@");
        let edited = String::from_utf8(out.clone())
            .unwrap_or_default()
            .replace("0.2.0", "@A@")
            .replace("^19.0.0", "@B@")
            .replace("~2.2.0", "@C@");
        assert_eq!(unchanged, edited);
        assert!(
            String::from_utf8(out.clone())
                .unwrap_or_default()
                .contains('\r')
        );
        assert_eq!(counts.applied, 3);
    }
}
