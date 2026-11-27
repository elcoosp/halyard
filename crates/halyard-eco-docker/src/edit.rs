//! The docker adapter's edit half — the semantic in-place OCI
//! version-label patch (M2 of the §17 `Docker image tags` cell,
//! ADR-0063, the release half).
//!
//! The plan's own-version byte patches re-apply **semantically and in
//! place** (invariant I4): each patch span must land on the LAST
//! `org.opencontainers.image.version` label's value span (the site the
//! discovery row emitted — the OCI annotations table's *"version of the
//! packaged software"*; Docker's own *"the most-recently-applied value
//! overrides any previously-set value"* makes the last row the site),
//! the value's inner bytes swap and every other byte — the quotes, the
//! sibling labels, the `FROM` rows, the comments — survives verbatim.
//! A re-scan read-back verifies the file now carries the target before
//! the patch is accepted (the §9.1 semantic check; the verify pins the
//! **text**, never the offset — a length-changing swap legitimately
//! moves the span); any other outcome fails closed (`E-EDIT-PATCHES` /
//! `E-EDIT-VERIFY`) with the input bytes untouched.
//!
//! A Dockerfile with no version label row (absent or non-canonical —
//! the fail-closed discipline) has no slot: a patch aimed at its
//! manifest refuses. A patch whose text already equals the current
//! value is the idempotent re-run (I1): matched, counted as skipped,
//! bytes verbatim.

use halyard_core::edit::{EditFailure, EditOutcome};
use halyard_core::package::RelPath;
use halyard_core::plan::PlanPatch;

use crate::VERSION_LABEL;
use crate::grammar;

/// The semantic patch: validate every patch span against the LAST
/// version label's value span, swap the inner bytes right-to-left, and
/// verify by re-scan before returning.
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
        message: "the Dockerfile is not valid UTF-8".to_owned(),
    })?;
    let parsed = grammar::parse_dockerfile(text).map_err(|error| EditFailure::Parse {
        file: file_string.clone(),
        message: error.to_string(),
    })?;
    // The editable slot: the LAST version label's value span (one slot —
    // the documented override rule makes the last row the site; a
    // Dockerfile without the label has none).
    let slot = version_slot(&parsed);
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
    // The read-back verify (§9.1): the re-scanned Dockerfile's LAST
    // version label carries the target the patch aimed at — the verify
    // pins the text, never the offset (a length-changing swap moves the
    // span) — never a guessed half-application.
    let bumped = std::str::from_utf8(&out).map_err(|_| EditFailure::Parse {
        file: file_string.clone(),
        message: "the patched Dockerfile is not valid UTF-8".to_owned(),
    })?;
    let reparsed = grammar::parse_dockerfile(bumped).map_err(|error| EditFailure::Parse {
        file: file_string.clone(),
        message: format!("read-back re-parse failed: {error}"),
    })?;
    for patch in patches {
        let carries = version_slot(&reparsed).is_some_and(|span| {
            reparsed
                .labels
                .iter()
                .rev()
                .find(|pair| pair.key == VERSION_LABEL && pair.span == span)
                .is_some_and(|pair| pair.value == patch.text)
        });
        if !carries {
            return Err(EditFailure::Parse {
                file: file_string.clone(),
                message: format!(
                    "read-back verify: the Dockerfile's {} label carries {:?} after the patch, \
                     expected {:?} — nothing was mutated",
                    VERSION_LABEL,
                    last_value(&reparsed),
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

/// The LAST version label's value span (Docker's documented override
/// rule); `None` when the manifest carries no version label.
fn version_slot(parsed: &grammar::Dockerfile) -> Option<halyard_core::package::Span> {
    parsed
        .labels
        .iter()
        .rev()
        .find(|pair| pair.key == VERSION_LABEL)
        .map(|pair| pair.span)
}

/// The LAST version label's current value (the verify message's
/// subject); `None` when the manifest carries no version label.
fn last_value(parsed: &grammar::Dockerfile) -> Option<&str> {
    parsed
        .labels
        .iter()
        .rev()
        .find(|pair| pair.key == VERSION_LABEL)
        .map(|pair| pair.value.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = concat!(
        "FROM alpine:3.20\n",
        "LABEL org.opencontainers.image.version=\"1.0.0\"\n",
        "CMD [\"/app\"]\n",
    );

    #[test]
    fn the_label_value_swaps_in_place() {
        let file = RelPath::new("Dockerfile").expect("fixture path");
        let start = usize_to_u32(FIXTURE.find("1.0.0"), "1.0.0");
        let end = start + 5;
        let patches = [PlanPatch {
            start,
            end,
            text: "1.1.0".to_owned(),
        }];
        let out = edit_versions(&file, FIXTURE.as_bytes(), &patches).expect("applies");
        assert_eq!((out.applied, out.skipped), (1, 0));
        assert_eq!(
            String::from_utf8(out.content).expect("valid UTF-8"),
            FIXTURE.replace("1.0.0", "1.1.0")
        );
    }

    #[test]
    fn patches_fail_closed_off_the_slot() {
        let file = RelPath::new("Dockerfile").expect("fixture path");
        let patches = [PlanPatch {
            start: 8,
            end: 13,
            text: "1.1.0".to_owned(),
        }];
        let err = edit_versions(&file, FIXTURE.as_bytes(), &patches)
            .expect_err("a span off the slot refuses");
        assert_eq!(err.code(), "E-EDIT-PATCHES");
    }

    #[test]
    fn empty_passes_return_the_verbatim_bytes() {
        let file = RelPath::new("Dockerfile").expect("fixture path");
        let out = edit_versions(&file, FIXTURE.as_bytes(), &[]).expect("an empty pass");
        assert_eq!(out.content, FIXTURE.as_bytes());
        assert_eq!((out.applied, out.skipped), (0, 0));
    }

    /// The fixture offset as a plan-contract span coordinate (the
    /// sibling bench posture — the failure stays loud and panic-free).
    fn usize_to_u32(value: Option<usize>, needle: &str) -> u32 {
        if let Some(value) = value.and_then(|value| u32::try_from(value).ok()) {
            return value;
        }
        eprintln!("the fixture must carry {needle}");
        std::process::exit(101);
    }
}
