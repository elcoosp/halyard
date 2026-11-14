//! Preview tag-phase tests (§11.1 step 2, the offline forecast): the
//! dry-run writer probes the **real** repository state (validation,
//! target existence, existing-tag classification) but never mutates —
//! mutations are deterministic forecasts. Pins:
//!
//! * the forecast reports `Created` for pending directives while writing
//!   **nothing** (zero refs);
//! * the forecast is deterministic: the same directive forecasts the same
//!   shape oid (the neutral-identity tag object hash) across runs;
//! * existing tags classify exactly like the live run (`AlreadyTagged`,
//!   `E-GIT-TAG-CONFLICT`), so the forecast can never disagree with what
//!   the live tag phase would do.

use halyard_git::gix::hash::ObjectId;
use halyard_git::tag::{PreviewTagWriter, TagOutcome, TagWriter};
use halyard_test_support::fixture::GitFixture;

type BoxError = Box<dyn std::error::Error>;

/// A fixture with one commit; returns the fixture and HEAD's oid.
fn fixture_one_commit(
    label: &str,
) -> Result<(GitFixture, ObjectId), halyard_test_support::FixtureError> {
    let mut fx = GitFixture::new(label)?;
    fx.write(
        "Cargo.toml",
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n",
    )?;
    fx.stage_all()?;
    let head = fx.commit("feat(app): initial")?;
    let oid =
        ObjectId::from_hex(head.as_bytes()).unwrap_or_else(|e| unreachable!("fixture oid: {e}"));
    Ok((fx, oid))
}

#[test]
fn forecast_reports_created_but_never_writes() -> Result<(), BoxError> {
    let (fx, head) = fixture_one_commit("tag-preview-forecast")?;
    let writer = PreviewTagWriter::open(fx.root())?;
    let directive = halyard_git::tag::TagDirective {
        name: "v0.1.0".to_owned(),
        target: head,
        message: "app 0.1.0".to_owned(),
    };
    let outcome = writer.create_annotated_tag(&directive)?;
    let TagOutcome::Created { tag_oid } = outcome else {
        return Err("an untagged target must forecast Created".into());
    };
    assert_ne!(
        tag_oid, head,
        "the forecast oid is the tag object, not the commit"
    );
    assert!(
        fx.ls_tags()?.is_empty(),
        "the preview writer must never create a ref"
    );
    Ok(())
}

#[test]
fn forecast_is_deterministic_across_runs_and_repositories() -> Result<(), BoxError> {
    // I3: the forecast oid is a pure function of the directive — no
    // clock, no identity leakage, same in any repository.
    let (a, head_a) = fixture_one_commit("tag-preview-det-a")?;
    let (b, head_b) = fixture_one_commit("tag-preview-det-b")?;
    let wa = PreviewTagWriter::open(a.root())?;
    let wb = PreviewTagWriter::open(b.root())?;
    let make = |target| halyard_git::tag::TagDirective {
        name: "v0.1.0".to_owned(),
        target,
        message: "app 0.1.0".to_owned(),
    };
    let TagOutcome::Created { tag_oid: first } = wa.create_annotated_tag(&make(head_a))? else {
        return Err("expected a forecast".into());
    };
    for writer in [&wa, &wb] {
        let TagOutcome::Created { tag_oid } = writer.create_annotated_tag(&make(head_b))? else {
            return Err("expected a forecast".into());
        };
        assert_eq!(
            tag_oid, first,
            "the forecast oid must not depend on run or repo"
        );
    }
    Ok(())
}

#[test]
fn forecast_respects_existing_tags_like_the_live_run() -> Result<(), BoxError> {
    // Probe-real, mutate-never: an existing tag at the target forecasts
    // AlreadyTagged (the live no-op), elsewhere the fail-closed conflict.
    let (mut fx, _first) = fixture_one_commit("tag-preview-existing")?;
    fx.commit_with(&halyard_test_support::CommitSpec::files(
        vec![(
            "src/lib.rs".to_owned(),
            "pub const APP: usize = 2;\n".to_owned(),
        )],
        "feat: extend",
    ))?;
    let head_hex = fx.resolve("HEAD")?;
    let head = ObjectId::from_hex(head_hex.trim().as_bytes())
        .unwrap_or_else(|e| unreachable!("fixture oid: {e}"));
    let older_hex = fx.resolve("HEAD~1")?;
    let older = ObjectId::from_hex(older_hex.trim().as_bytes())
        .unwrap_or_else(|e| unreachable!("fixture oid: {e}"));
    fx.tag_annotated("v0.1.0", "already shipped")?;
    let writer = PreviewTagWriter::open(fx.root())?;
    let at_target = halyard_git::tag::TagDirective {
        name: "v0.1.0".to_owned(),
        target: head,
        message: "app 0.1.0".to_owned(),
    };
    let outcome = writer.create_annotated_tag(&at_target)?;
    assert_eq!(outcome, TagOutcome::AlreadyTagged { target: head });

    let conflicting = halyard_git::tag::TagDirective {
        name: "v0.1.0".to_owned(),
        target: older,
        message: "app 0.1.0".to_owned(),
    };
    let error = writer
        .create_annotated_tag(&conflicting)
        .expect_err("the live run would fail closed, so must the forecast");
    assert_eq!(error.code(), "E-GIT-TAG-CONFLICT");
    assert_eq!(fx.ls_tags()?, vec!["v0.1.0".to_owned()], "nothing moved");
    Ok(())
}

#[test]
fn forecast_fails_closed_like_the_live_run() -> Result<(), BoxError> {
    // Validation and target checks are forecasts too: the same E-GIT-TAG
    // failures the live writer raises, raised before any render.
    let (fx, head) = fixture_one_commit("tag-preview-invalid")?;
    let writer = PreviewTagWriter::open(fx.root())?;
    let bad_name = halyard_git::tag::TagDirective {
        name: "v..1".to_owned(),
        target: head,
        message: "app 0.1.0".to_owned(),
    };
    let error = writer
        .create_annotated_tag(&bad_name)
        .expect_err("invalid names forecast the live failure");
    assert_eq!(error.code(), "E-GIT-TAG");
    let ghost = ObjectId::from_hex(b"1111111111111111111111111111111111111111")
        .unwrap_or_else(|e| unreachable!("static hex: {e}"));
    let bad_target = halyard_git::tag::TagDirective {
        name: "v0.1.0".to_owned(),
        target: ghost,
        message: "app 0.1.0".to_owned(),
    };
    let error = writer
        .create_annotated_tag(&bad_target)
        .expect_err("unknown targets forecast the live failure");
    assert_eq!(error.code(), "E-GIT-TAG");
    Ok(())
}
