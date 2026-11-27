//! The `uv.lock` lockfile half (the §17 M8 `PyPI` cell's recorded-open
//! remainder, ADR-0056): *"the uv.lock lockfile half (the trait's
//! documented `NeedsNative`/`Unsupported` declines stand)"* — the plan
//! line these tests pin. The half is the lockfile seam for `uv.lock`:
//! the adapter declines the in-memory patch honestly at uv's own
//! documented posture (*"uv.lock is a human-readable TOML file but is
//! managed by uv and should not be edited manually"*,
//! `docs/research/pypi-uv-grammars.md` §3) and the native refresh effect
//! step (`uv lock --offline`, the tool's documented refresh) exists so
//! the orchestrator's §9.2 rule-4 fallback stops erroring
//! `E-LOCK-NATIVE Unsupported` — the same native-only shape the
//! npm-family lockfiles (package-lock.json/yarn.lock/bun.lock) ride.
//!
//! The verifier's fidelity contract was proven empirically against the
//! installed tool (uv 0.13.10): a member version bump rewrites exactly
//! the member's `version` row — every other byte of the lockfile (the
//! `version`/`revision`/`requires-python` header, the `[manifest]`
//! members, sibling `[[package]]` rows, `requires-dist`) survives
//! verbatim. Anything else the refresh does is a resolution change the
//! plan cannot express and refuses.

use halyard_core::EcosystemAdapter;
use halyard_core::lockfile::{LockfileOutcome, LockfileVersionChange, NativeRefreshError};
use halyard_core::package::RelPath;
use halyard_eco_pypi::{PyPiAdapter, uvlock};

type BoxError = Box<dyn std::error::Error>;

/// The lockfile the scratch uv workspace carries (uv 0.13.10 shape):
/// the `[manifest]` members, the root's virtual row and the two
/// editable member rows.
const UV_LOCK_BEFORE: &str = "version = 1\nrevision = 5\nrequires-python = \">=3.9\"\n\n[manifest]\nmembers = [\n    \"app-a\",\n    \"app-b\",\n    \"root\",\n]\n\n[[package]]\nname = \"app-a\"\nversion = \"0.1.0\"\nsource = { editable = \"app_a\" }\ndependencies = [\n    { name = \"app-b\" },\n]\n\n[package.metadata]\nrequires-dist = [{ name = \"app-b\", editable = \"app_b\" }]\n\n[[package]]\nname = \"app-b\"\nversion = \"0.1.0\"\nsource = { editable = \"app_b\" }\n\n[[package]]\nname = \"root\"\nversion = \"0.0.0\"\nsource = { virtual = \".\" }\n";

/// The same lockfile after `uv lock --offline` moved `app-a` 0.1.0 →
/// 0.2.0 (the observed diff: exactly the member's `version` row).
const UV_LOCK_AFTER: &str = "version = 1\nrevision = 5\nrequires-python = \">=3.9\"\n\n[manifest]\nmembers = [\n    \"app-a\",\n    \"app-b\",\n    \"root\",\n]\n\n[[package]]\nname = \"app-a\"\nversion = \"0.2.0\"\nsource = { editable = \"app_a\" }\ndependencies = [\n    { name = \"app-b\" },\n]\n\n[package.metadata]\nrequires-dist = [{ name = \"app-b\", editable = \"app_b\" }]\n\n[[package]]\nname = \"app-b\"\nversion = \"0.1.0\"\nsource = { editable = \"app_b\" }\n\n[[package]]\nname = \"root\"\nversion = \"0.0.0\"\nsource = { virtual = \".\" }\n";

fn change() -> Vec<LockfileVersionChange> {
    vec![LockfileVersionChange {
        name: "app-a".to_owned(),
        from: "0.1.0".to_owned(),
        to: "0.2.0".to_owned(),
    }]
}

#[test]
fn the_uv_lock_decline_names_the_native_refresh() -> Result<(), BoxError> {
    let file = RelPath::new("uv.lock").ok_or("uv.lock is a valid relative path")?;
    let outcome = PyPiAdapter::new().patch_lockfile(&file, UV_LOCK_BEFORE.as_bytes(), &change())?;
    let LockfileOutcome::NeedsNative { reason } = outcome else {
        return Err(format!("expected the native decline, got {outcome:?}").into());
    };
    // The decline is the lockfile-aware one: uv's own documented
    // posture and the refresh argv, not the generic adapter fallback.
    assert!(
        reason.contains("uv lock"),
        "the decline names the refresh argv: {reason}"
    );
    assert!(
        reason.contains("should not be edited manually"),
        "the decline cites uv's documented management posture: {reason}"
    );
    Ok(())
}

#[test]
fn the_decline_leaves_the_bytes_untouched() -> Result<(), BoxError> {
    // The decline is a report, not a mutation: nothing else may change
    // the lockfile's bytes (the native refresh owns them).
    let file = RelPath::new("uv.lock").ok_or("uv.lock is a valid relative path")?;
    let outcome = PyPiAdapter::new().patch_lockfile(&file, UV_LOCK_BEFORE.as_bytes(), &change())?;
    assert!(matches!(outcome, LockfileOutcome::NeedsNative { .. }));
    Ok(())
}

#[test]
fn refresh_command_is_the_documented_offline_refresh() {
    // `uv lock` — "Update the project's lockfile"; `--offline` —
    // "Disable network access" (the installed tool's own help, uv
    // 0.13.10; the cargo/pnpm offline discipline).
    let (program, args, rendered) = uvlock::refresh_command();
    assert_eq!(program, "uv");
    assert_eq!(args, &["lock", "--offline"]);
    assert_eq!(rendered, "uv lock --offline");
}

#[test]
fn verify_accepts_a_member_only_move() -> Result<(), BoxError> {
    let moved = uvlock::verify_native_refresh(
        UV_LOCK_BEFORE.as_bytes(),
        UV_LOCK_AFTER.as_bytes(),
        &["app-a".to_owned(), "app-b".to_owned(), "root".to_owned()],
    )?;
    assert_eq!(moved, vec!["app-a".to_owned()]);
    Ok(())
}

#[test]
fn verify_refuses_a_non_member_row_change() {
    // A sibling registry row moved: a resolution change the plan
    // cannot express.
    let tampered = UV_LOCK_BEFORE.replace(
        "name = \"app-b\"\nversion = \"0.1.0\"",
        "name = \"app-b\"\nversion = \"0.3.0\"",
    );
    let error = uvlock::verify_native_refresh(
        UV_LOCK_BEFORE.as_bytes(),
        tampered.as_bytes(),
        &["app-a".to_owned(), "root".to_owned()],
    )
    .expect_err("a non-member move refuses");
    assert_eq!(error.code(), NativeRefreshError::CODE);
    assert!(
        error.to_string().contains("app-b"),
        "the refusal names the moved package: {error}"
    );
}

#[test]
fn verify_refuses_a_header_change() {
    // `revision`/`version` bumps are uv's format business — a refresh
    // that rewrote the header rewrote more than the plan's moves.
    let tampered = UV_LOCK_AFTER.replace("revision = 5", "revision = 6");
    let error = uvlock::verify_native_refresh(
        UV_LOCK_BEFORE.as_bytes(),
        tampered.as_bytes(),
        &["app-a".to_owned(), "app-b".to_owned(), "root".to_owned()],
    )
    .expect_err("a header change refuses");
    assert_eq!(error.code(), NativeRefreshError::CODE);
}

#[test]
fn verify_refuses_an_unparseable_after() {
    let error = uvlock::verify_native_refresh(
        UV_LOCK_BEFORE.as_bytes(),
        b"not toml at all".as_slice(),
        &["app-a".to_owned()],
    )
    .expect_err("unparseable bytes refuse");
    assert_eq!(error.code(), NativeRefreshError::CODE);
}

#[test]
fn verify_refuses_a_missing_member_row() -> Result<(), BoxError> {
    // The member's row vanished: uv dropped it (a resolution change —
    // the manifest would no longer claim the member).
    let dropped = UV_LOCK_BEFORE
        .split("[[package]]\nname = \"app-b\"")
        .next()
        .ok_or("prefix")?
        .to_owned();
    let error = uvlock::verify_native_refresh(
        UV_LOCK_BEFORE.as_bytes(),
        dropped.as_bytes(),
        &["app-a".to_owned(), "app-b".to_owned()],
    )
    .expect_err("a vanished member row refuses");
    assert_eq!(error.code(), NativeRefreshError::CODE);
    Ok(())
}
