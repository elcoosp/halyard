//! M8 step 1 edit contract tests (ADR-0056): the `halyard-eco-pypi`
//! adapter's `EcosystemAdapter::edit_versions` over `pyproject.toml`
//! (§7.3 edit subset, the M3.1/M3.3 discipline at the PEP 621 grammar).
//!
//! Plan pin: §17 M8 *"`PyPI` (`pyproject`, `uv`, PEP 440 strategy,
//! trusted publishing)"* — the **edit** half of the discover/edit
//! acceptance row ("each: discover/edit/publish/resume tests").

use halyard_core::EcosystemAdapter;
use halyard_core::package::RelPath;
use halyard_core::plan::PlanPatch;
use halyard_eco_pypi::PyPiAdapter;

type BoxError = Box<dyn std::error::Error>;

const MANIFEST: &str = "[project]\nname = \"albatross\"\nversion = \"0.1.0\"\nrequires-python = \">=3.12\"\ndependencies = [\"tqdm>=4,<5\"]\n\n[tool.uv]\npackage = true\n";

fn file() -> Option<RelPath> {
    RelPath::new("pyproject.toml")
}

/// The version row rewrites in place: every untouched byte survives
/// verbatim (invariant I4), the new value rides the PEP 440 spelling
/// (the semver hyphen form is not valid PEP 440), and a re-discovery of
/// the patched bytes reports the target version (verify-before-write).
#[test]
fn version_row_rewrites_in_place_and_round_trips() -> Result<(), BoxError> {
    let patch = PlanPatch {
        start: u32::try_from(MANIFEST.find("0.1.0").expect("span start")).unwrap_or(0),
        end: u32::try_from(MANIFEST.find("0.1.0").expect("span start") + "0.1.0".len())
            .unwrap_or(0),
        text: "0.2.0".to_owned(),
    };
    let outcome = PyPiAdapter.edit_versions(
        file().as_ref().expect("normalized"),
        MANIFEST.as_bytes(),
        &[patch],
    )?;
    assert_eq!(outcome.applied, 1);
    assert_eq!(outcome.skipped, 0);

    // Verify-before-write: the patched bytes re-discover to the target.
    let mut ctx = halyard_core::discover::DiscoverCtx::new();
    assert!(ctx.insert_file("pyproject.toml", outcome.content.clone()));
    let rows = PyPiAdapter.discover(&ctx)?;
    assert_eq!(
        rows.first()
            .expect("one row")
            .version
            .as_ref()
            .map(std::string::ToString::to_string),
        Some("0.2.0".into())
    );

    let patched = String::from_utf8(outcome.content)?;
    assert_eq!(
        patched,
        MANIFEST.replace("version = \"0.1.0\"", "version = \"0.2.0\"")
    );
    Ok(())
}

/// A pre-release target rides the PEP 440 spelling: semver
/// `1.2.3-rc.1` arrives at the version site as `1.2.3rc1` — the adapter
/// is the PEP 440 ⇄ semver boundary (the planner never sees TOML).
#[test]
fn semver_prerelease_target_renders_as_pep440() -> Result<(), BoxError> {
    let patch = PlanPatch {
        start: u32::try_from(MANIFEST.find("0.1.0").expect("span")).unwrap_or(0),
        end: u32::try_from(MANIFEST.find("0.1.0").expect("span") + 5).unwrap_or(0),
        text: "1.2.3-rc.1".to_owned(),
    };
    let outcome = PyPiAdapter.edit_versions(
        file().as_ref().expect("normalized"),
        MANIFEST.as_bytes(),
        &[patch],
    )?;
    assert_eq!(outcome.applied, 1);
    let patched = String::from_utf8(outcome.content)?;
    assert!(
        patched.contains("version = \"1.2.3rc1\""),
        "PEP 440 spelling expected, got: {patched}"
    );
    assert!(!patched.contains("1.2.3-rc.1"));
    Ok(())
}

/// Idempotence (M3.6's contract): applying the plan twice changes
/// nothing the second time (the row already carries the target text).
#[test]
fn reapplying_the_target_is_a_no_op() -> Result<(), BoxError> {
    let patch = PlanPatch {
        start: u32::try_from(MANIFEST.find("0.1.0").expect("span")).unwrap_or(0),
        end: u32::try_from(MANIFEST.find("0.1.0").expect("span") + 5).unwrap_or(0),
        text: "0.2.0".to_owned(),
    };
    let first = PyPiAdapter.edit_versions(
        file().as_ref().expect("normalized"),
        MANIFEST.as_bytes(),
        &[patch],
    )?;
    let patch2 = PlanPatch {
        start: u32::try_from(
            first
                .content
                .windows(5)
                .position(|w| w == b"0.2.0")
                .expect("target text"),
        )
        .unwrap_or(0),
        end: u32::try_from(
            first
                .content
                .windows(5)
                .position(|w| w == b"0.2.0")
                .expect("target text")
                + 5,
        )
        .unwrap_or(0),
        text: "0.2.0".to_owned(),
    };
    let second = PyPiAdapter.edit_versions(
        file().as_ref().expect("normalized"),
        &first.content,
        &[patch2],
    )?;
    assert_eq!(second.applied, 0);
    assert_eq!(second.skipped, 1, "already-target rows skip-and-count");
    assert_eq!(second.content, first.content);
    Ok(())
}

/// A drifted manifest (the bytes changed since the plan was computed)
/// fails closed on the exact patch span (`E-EDIT-PATCHES`) — nothing is
/// mutated. The drift here is a longer value, which shifts the inner
/// span the plan pinned.
#[test]
fn drifted_span_fails_closed() {
    let drifted = MANIFEST.replace("0.1.0", "0.10.0");
    let patch = PlanPatch {
        start: u32::try_from(MANIFEST.find("0.1.0").expect("span")).unwrap_or(0),
        end: u32::try_from(MANIFEST.find("0.1.0").expect("span") + 5).unwrap_or(0),
        text: "0.2.0".to_owned(),
    };
    let error = PyPiAdapter
        .edit_versions(
            file().as_ref().expect("normalized"),
            drifted.as_bytes(),
            &[patch],
        )
        .expect_err("drift must fail");
    assert_eq!(error.code(), "E-EDIT-PATCHES");
}

/// The quoted span discipline: single-quoted TOML values rewrite just as
/// the planner pinned them (quote-inclusive spans, the M3.1 rule).
#[test]
fn single_quoted_row_rewrites_verbatim() -> Result<(), BoxError> {
    let single = "[project]\nname = 'albatross'\nversion = '0.1.0'\n";
    let patch = PlanPatch {
        start: u32::try_from(single.find("0.1.0").expect("span")).unwrap_or(0),
        end: u32::try_from(single.find("0.1.0").expect("span") + 5).unwrap_or(0),
        text: "0.3.0".to_owned(),
    };
    let outcome = PyPiAdapter.edit_versions(
        file().as_ref().expect("normalized"),
        single.as_bytes(),
        &[patch],
    )?;
    assert_eq!(outcome.applied, 1);
    let patched = String::from_utf8(outcome.content)?;
    assert!(patched.contains("version = '0.3.0'"));
    assert!(patched.contains("name = 'albatross'"));
    Ok(())
}
