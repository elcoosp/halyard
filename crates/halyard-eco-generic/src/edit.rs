//! The generic adapter's edit half (M8 ADR-0062's release half): the
//! plan's own-version byte patches re-apply **semantically and in
//! place** — the adapter re-locates the declared pattern's named
//! `version` capture group in the current bytes, the patch span must
//! equal the capture's span (a drifted file refuses, `E-EDIT-PATCHES`),
//! the captured bytes swap in place (every other byte survives
//! verbatim, invariant I4), and a re-match read-back verifies the file
//! now carries the target before the patch is accepted
//! (`E-EDIT-VERIFY` posture). A patch whose text already equals the
//! current capture is the idempotent re-run (I1): matched, skipped,
//! bytes verbatim. The generic adapter edits only its declared version
//! files — a file without a spec refuses fail-closed, never a guess.

use halyard_core::discover::GenericFileSpec;
use halyard_core::edit::{EditFailure, EditOutcome};
use halyard_core::package::RelPath;
use halyard_core::plan::PlanPatch;

/// The semantic edit seam over one declared version file. `spec` is
/// `None` when the file is not a declared version file at all (the
/// apply shell routed a foreign file here — the same refusal either
/// way). Pure parse+mutate over the given bytes: no process spawn, no
/// I/O.
pub fn edit_versions(
    spec: Option<&GenericFileSpec>,
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
    let Some(spec) = spec else {
        // The first patch's span names the refusal site: the routed file
        // carries no declared locator, so no editable row exists.
        let Some(first) = patches.first() else {
            return Ok(EditOutcome {
                content: content.to_vec(),
                applied: 0,
                skipped: 0,
            });
        };
        return Err(span_mismatch(file, first));
    };
    let pattern = regex::Regex::new(&spec.pattern).map_err(|error| EditFailure::Parse {
        file: file.as_str().to_owned(),
        message: format!("the declared pattern does not compile: {error}"),
    })?;
    let mut bytes = content.to_vec();
    let mut applied = 0;
    let mut skipped = 0;
    for patch in patches {
        let text = std::str::from_utf8(&bytes).map_err(|_| EditFailure::Parse {
            file: file.as_str().to_owned(),
            message: "the declared version file is not valid UTF-8".to_owned(),
        })?;
        let capture = locate(&pattern, text).ok_or_else(|| span_mismatch(file, patch))?;
        let (start, end) = capture;
        if (start, end) != (patch.start, patch.end) {
            return Err(span_mismatch(file, patch));
        }
        if text
            .get(start as usize..end as usize)
            .is_some_and(|current| current == patch.text)
        {
            // The idempotent re-run (I1): the row already carries the
            // target text — matched, skipped, bytes verbatim.
            skipped += 1;
            continue;
        }
        // The captured bytes swap in place: the range is a UTF-8
        // boundary set (the capture's own span) and the replacement is
        // UTF-8, so the buffer stays valid UTF-8.
        let replacement = patch.text.as_bytes().to_vec();
        bytes.splice(start as usize..end as usize, replacement);
        // The read-back verify re-matches before acceptance: the file
        // must now carry the target at the declared locator.
        let verify = std::str::from_utf8(&bytes).map_err(|_| EditFailure::Parse {
            file: file.as_str().to_owned(),
            message: "the edited version file is not valid UTF-8".to_owned(),
        })?;
        let carried = locate(&pattern, verify).ok_or_else(|| EditFailure::Parse {
            file: file.as_str().to_owned(),
            message: format!(
                "the edited file no longer matches the declared pattern — the target \
                 {:?} is a spelling the pattern's `version` group cannot carry",
                patch.text
            ),
        })?;
        let carried_text = verify
            .get(carried.0 as usize..carried.1 as usize)
            .unwrap_or_default();
        if carried_text != patch.text {
            // A same-length swap keeps the span; a length-changing swap
            // legitimately moves it — the verify pins the TEXT the
            // declared locator now carries, never the offset.
            return Err(EditFailure::Parse {
                file: file.as_str().to_owned(),
                message: format!(
                    "the read-back verify failed: the capture carries {carried_text:?}, the \
                     plan targets {:?}",
                    patch.text
                ),
            });
        }
        applied += 1;
    }
    Ok(EditOutcome {
        content: bytes,
        applied,
        skipped,
    })
}

/// Locates the declared pattern's named `version` capture group in the
/// leftmost match as a plan-contract span; `None` when the file matches
/// nothing or the group is missing (unreachable through the validated
/// loader — the adapter still refuses instead of guessing).
fn locate(pattern: &regex::Regex, text: &str) -> Option<(u32, u32)> {
    let caps = pattern.captures(text)?;
    let group = caps.name("version")?;
    let start = u32::try_from(group.start()).ok()?;
    let end = u32::try_from(group.end()).ok()?;
    Some((start, end))
}

/// The `E-EDIT-PATCHES` refusal at the patch's own span (the working
/// tree drifted from the file the plan was computed against — re-plan
/// instead of forcing the patch).
fn span_mismatch(file: &RelPath, patch: &PlanPatch) -> EditFailure {
    EditFailure::SpanMismatch {
        file: file.as_str().to_owned(),
        start: patch.start,
        end: patch.end,
    }
}
