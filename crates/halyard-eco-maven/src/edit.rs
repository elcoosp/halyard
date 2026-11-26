//! The maven adapter's edit half — the semantic in-place version patch
//! (M2 of ADR-0058, the release half).
//!
//! The plan's own-version byte patches re-apply **semantically and in
//! place** (invariant I4): each patch span must land on a
//! `<project>`-owned `<version>` element's value text of the owning pom
//! (the site the discovery row emitted), the value's inner bytes swap for
//! the plan's target, and the surrounding markup — comments, attribute
//! order, indentation, whitespace — never moves. A re-parse read-back
//! verifies the pom now carries the target before the patch is accepted
//! (the §9.1 semantic check); any other outcome fails closed
//! (`E-EDIT-PATCHES` / `E-EDIT-VERIFY`) with the input bytes untouched.
//!
//! Inherited members have no own `<version>` element: their one edit
//! rides the aggregator pom (the workspace-inheritance posture), so a
//! patch aimed at an inheritor's own manifest has no slot to match and
//! refuses. A patch whose text already equals the pom's current value is
//! the idempotent re-run (I1): matched, counted as skipped, bytes
//! verbatim.

use halyard_core::edit::{EditFailure, EditOutcome};
use halyard_core::package::RelPath;
use halyard_core::plan::PlanPatch;

use crate::grammar;

/// The semantic patch: validate every patch span against the pom's
/// `<project>`-owned `<version>` value span, swap the inner bytes
/// right-to-left, and verify by re-parse before returning.
pub fn edit_versions(
    file: &RelPath,
    content: &[u8],
    patches: &[PlanPatch],
) -> Result<EditOutcome, EditFailure> {
    // The sibling adapters' posture for a legitimate empty pass (nothing
    // to apply, nothing changed).
    if patches.is_empty() {
        return Ok(EditOutcome {
            content: content.to_vec(),
            applied: 0,
            skipped: 0,
        });
    }
    let file_string = file.to_string();
    let text = std::str::from_utf8(content).map_err(|_| EditFailure::Parse {
        file: file_string.clone(),
        message: "pom.xml is not valid UTF-8".to_owned(),
    })?;
    let pom = grammar::parse_pom(text, &file_string).map_err(|error| EditFailure::Parse {
        file: file_string.clone(),
        message: error.to_string(),
    })?;
    // The editable slots: the `<project>`-owned `<version>` element's
    // value text (one slot — inherited poms carry none).
    let slot = pom.version.as_ref().map(|field| field.span);
    for patch in patches {
        let matches = slot.is_some_and(|span| span.start == patch.start && span.end == patch.end);
        if !matches {
            return Err(EditFailure::SpanMismatch {
                file: file_string.clone(),
                start: patch.start,
                end: patch.end,
            });
        }
    }
    // Validate the spans are in bounds before any byte moves (the
    // manifest drifted check — a patch beyond the file cannot match a
    // slot, but stay defensive about the splice bounds).
    let mut out = content.to_vec();
    let mut applied = 0usize;
    let mut skipped = 0usize;
    let mut ordered: Vec<&PlanPatch> = patches.iter().collect();
    ordered.sort_by_key(|patch| patch.start);
    for patch in ordered.into_iter().rev() {
        let start = patch.start as usize;
        let end = patch.end as usize;
        if end > out.len() || start > end {
            return Err(EditFailure::SpanMismatch {
                file: file_string.clone(),
                start: patch.start,
                end: patch.end,
            });
        }
        let carried = out
            .get(start..end)
            .is_some_and(|current| current == patch.text.as_bytes());
        if carried {
            // The idempotent re-run (I1): the value already carries the
            // target — matched, nothing to rewrite.
            skipped += 1;
            continue;
        }
        out.splice(start..end, patch.text.as_bytes().iter().copied());
        applied += 1;
    }
    // The read-back verify (§9.1): the re-parsed pom carries the target
    // the patch aimed at — never a guessed half-application.
    let bumped = std::str::from_utf8(&out).map_err(|_| EditFailure::Parse {
        file: file_string.clone(),
        message: "the patched pom is not valid UTF-8".to_owned(),
    })?;
    let reparsed =
        grammar::parse_pom(bumped, &file_string).map_err(|error| EditFailure::Parse {
            file: file_string.clone(),
            message: format!("read-back re-parse failed: {error}"),
        })?;
    for patch in patches {
        let carries = reparsed
            .version
            .as_ref()
            .is_some_and(|field| field.text == patch.text);
        if !carries {
            return Err(EditFailure::Parse {
                file: file_string.clone(),
                message: format!(
                    "read-back verify: pom carries {:?} after the patch, expected {:?} — nothing \
                     was mutated",
                    reparsed.version.as_ref().map(|field| field.text.as_str()),
                    patch.text
                ),
            });
        }
    }
    Ok(EditOutcome {
        content: out,
        applied,
        skipped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const POM: &str =
        "<project xmlns=\"u\"><artifactId>a</artifactId><version>1.2.3</version></project>";

    #[test]
    fn swaps_the_value_and_preserves_the_markup() {
        let file = RelPath::new("pom.xml").expect("fixture path");
        // The version value's span: 48..53 ("1.2.3" between the tags).
        let start = u32::try_from(POM.find("<version>").expect("fixture")).expect("fits") + 9;
        let end = start + 5;
        let patches = [PlanPatch {
            start,
            end,
            text: "1.3.0".to_owned(),
        }];
        let out = edit_versions(&file, POM.as_bytes(), &patches).expect("applies");
        assert_eq!((out.applied, out.skipped), (1, 0));
        assert_eq!(
            String::from_utf8(out.content).expect("utf-8"),
            POM.replace("<version>1.2.3</version>", "<version>1.3.0</version>")
        );
    }

    #[test]
    fn a_span_off_the_version_row_refuses() {
        let file = RelPath::new("pom.xml").expect("fixture path");
        let start = u32::try_from(POM.find("a</artifactId>").expect("fixture")).expect("fits");
        let patches = [PlanPatch {
            start,
            end: start + 1,
            text: "1.3.0".to_owned(),
        }];
        let err = edit_versions(&file, POM.as_bytes(), &patches).expect_err("refuses");
        assert_eq!(err.code(), "E-EDIT-PATCHES");
    }

    #[test]
    fn an_inherited_pom_has_no_slot() {
        let member = "<project xmlns=\"u\"><parent><groupId>g</groupId><artifactId>a</artifactId><version>1.2.3</version></parent><artifactId>m</artifactId></project>";
        let file = RelPath::new("member/pom.xml").expect("fixture path");
        let patches = [PlanPatch {
            start: 4,
            end: 9,
            text: "1.3.0".to_owned(),
        }];
        let err = edit_versions(&file, member.as_bytes(), &patches).expect_err("refuses");
        assert_eq!(err.code(), "E-EDIT-PATCHES");
    }

    #[test]
    fn the_idempotent_rerun_skips_the_matched_value() {
        let file = RelPath::new("pom.xml").expect("fixture path");
        let start = u32::try_from(POM.find("<version>").expect("fixture")).expect("fits") + 9;
        let patches = [PlanPatch {
            start,
            end: start + 5,
            text: "1.2.3".to_owned(),
        }];
        let out = edit_versions(&file, POM.as_bytes(), &patches).expect("applies");
        assert_eq!((out.applied, out.skipped), (0, 1), "already carried");
        assert_eq!(out.content, POM.as_bytes(), "the bytes are verbatim");
    }

    #[test]
    fn empty_passes_return_the_verbatim_bytes() {
        let file = RelPath::new("pom.xml").expect("fixture path");
        let out = edit_versions(&file, POM.as_bytes(), &[]).expect("an empty pass");
        assert_eq!(out.content, POM.as_bytes());
        assert_eq!((out.applied, out.skipped), (0, 0));
    }

    #[test]
    fn a_slot_is_only_the_project_owned_version() {
        // A pom whose `<parent>` block carries a `<version>`: the patch
        // aimed at the parent's value refuses — the project's own row is
        // the only slot.
        let pom = "<project xmlns=\"u\"><parent><groupId>g</groupId><artifactId>a</artifactId><version>9.9.9</version></parent><artifactId>c</artifactId><version>1.2.3</version></project>";
        let file = RelPath::new("pom.xml").expect("fixture path");
        let parent_version = u32::try_from(pom.find("9.9.9").expect("fixture")).expect("fits");
        let patches = [PlanPatch {
            start: parent_version,
            end: parent_version + 5,
            text: "9.9.10".to_owned(),
        }];
        let err = edit_versions(&file, pom.as_bytes(), &patches).expect_err("refuses");
        assert_eq!(err.code(), "E-EDIT-PATCHES");
    }
}
