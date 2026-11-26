//! The Go adapter's edit half — the **tag-only** fail-closed posture
//! (M8 ADR-0057).
//!
//! The `go.mod` grammar has no version field for the module itself
//! (`go.dev/ref/mod`: the version is the repository's semantic version
//! tag), so the planner emits no own-version edit for a Go package and
//! requirement rewrites never touch Go rows (foreign-grammar
//! requirements ride verbatim, ADR-0006). The edit seam therefore has no
//! legitimate input; any patch that reaches it is a plan-contract
//! violation and fails closed (`E-EDIT-PATCHES`) without touching the
//! bytes — invariant I4 trivially: *all* bytes survive.

use halyard_core::edit::{EditFailure, EditOutcome};
use halyard_core::package::RelPath;
use halyard_core::plan::PlanPatch;

/// The tag-only edit seam: `go.mod` carries no editable version row, so
/// every patch fails closed and the content is never mutated.
pub fn edit_versions(
    file: &RelPath,
    content: &[u8],
    patches: &[PlanPatch],
) -> Result<EditOutcome, EditFailure> {
    // The sibling adapters' posture for a legitimate empty pass (nothing
    // to apply, nothing changed) — a Go plan never carries go.mod edits,
    // so this is the only `Ok` shape the seam returns.
    if patches.is_empty() {
        return Ok(EditOutcome {
            content: content.to_vec(),
            applied: 0,
            skipped: 0,
        });
    }
    let first = &patches[0];
    Err(EditFailure::SpanMismatch {
        file: file.to_string(),
        start: first.start,
        end: first.end,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patches_fail_closed_untouched() {
        let file = RelPath::new("go.mod").expect("fixture path");
        let patches = [PlanPatch {
            start: 8,
            end: 28,
            text: "example.com/m/v2".to_owned(),
        }];
        let err = edit_versions(&file, b"module example.com/m\n", &patches)
            .expect_err("tag-only manifests carry no editable row");
        assert_eq!(err.code(), "E-EDIT-PATCHES");
    }

    #[test]
    fn empty_passes_return_the_verbatim_bytes() {
        let file = RelPath::new("go.mod").expect("fixture path");
        let out = edit_versions(&file, b"module example.com/m\n", &[])
            .expect("an empty pass applies nothing");
        assert_eq!(out.content, b"module example.com/m\n");
        assert_eq!((out.applied, out.skipped), (0, 0));
    }
}
