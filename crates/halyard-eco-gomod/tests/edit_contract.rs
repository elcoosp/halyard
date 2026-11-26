//! The M8 Go-modules cell's edit contract — the **tag-only** posture at
//! the edit seam (`docs/HALYARD_ARCHITECTURE.md` §17 YEAR 2, M8 row,
//! **Go modules** cell). The `go.mod` grammar has no version field for
//! the module itself (go.dev/ref/mod — versions are semantic version
//! tags), so the planner emits no own-version edit for a Go package, and
//! any patch that reaches the adapter is a plan-contract violation that
//! fails closed without touching the bytes (invariant I4: untouched
//! bytes survive verbatim — here, *all* bytes).

use halyard_core::EcosystemAdapter;
use halyard_core::edit::{EditFailure, EditOutcome};
use halyard_core::package::RelPath;
use halyard_core::plan::PlanPatch;
use halyard_eco_gomod::GoModAdapter;

const GOMOD: &[u8] =
    b"module example.com/halyard\n\ngo 1.23.0\n\nrequire example.com/halyard/api v0.1.2\n";

/// Any patch handed to the Go adapter fails closed (`E-EDIT-PATCHES`):
/// a go.mod has no editable version row, ever — there is no span that
/// could legitimately match.
#[test]
fn a_patch_on_a_go_mod_fails_closed_without_touching_the_bytes() {
    let file = RelPath::new("go.mod").expect("fixture path");
    let patches = [PlanPatch {
        start: 8,
        end: 28,
        text: "example.com/halyard/v2".to_owned(),
    }];
    let err = GoModAdapter::new()
        .edit_versions(&file, GOMOD, &patches)
        .expect_err("tag-only manifests carry no editable row");
    assert!(matches!(err, EditFailure::SpanMismatch { .. }));
    assert_eq!(err.code(), "E-EDIT-PATCHES");
}

/// The failure names the manifest and the offending span.
#[test]
fn the_span_mismatch_carries_the_patch_bounds() {
    let file = RelPath::new("api/go.mod").expect("fixture path");
    let patches = [PlanPatch {
        start: 4,
        end: 9,
        text: "0.2.0".to_owned(),
    }];
    let err = GoModAdapter::new()
        .edit_versions(&file, GOMOD, &patches)
        .expect_err("tag-only manifests carry no editable row");
    match err {
        EditFailure::SpanMismatch { file, start, end } => {
            assert_eq!(file.as_str(), "api/go.mod");
            assert_eq!((start, end), (4, 9));
        }
        other => panic!("expected SpanMismatch, got {other:?}"),
    }
}

/// An empty patch set applies nothing and returns the verbatim bytes
/// (the sibling adapters' posture for a legitimate empty pass — no
/// failure is invented for no input).
#[test]
fn an_empty_patch_set_returns_the_verbatim_bytes() {
    let file = RelPath::new("go.mod").expect("fixture path");
    let outcome: Result<EditOutcome, EditFailure> =
        GoModAdapter::new().edit_versions(&file, GOMOD, &[]);
    let out = outcome.expect("an empty pass applies nothing");
    assert_eq!(out.content, GOMOD.to_vec(), "all bytes survive verbatim");
    assert_eq!((out.applied, out.skipped), (0, 0));
}

/// The adapter's identity is the plan contract's ecosystem name.
#[test]
fn the_adapter_id_is_go() {
    assert_eq!(GoModAdapter::new().id(), "go");
}
