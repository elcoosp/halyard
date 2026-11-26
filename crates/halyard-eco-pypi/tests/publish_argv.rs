//! M8 step 2 publish-argv contract tests (ADR-0056): the `uv publish`
//! grammar the publish effect spawns (§13.4's tool-grammar convention).
//!
//! Plan pin: §17 M8 *"`PyPI` (`pyproject`, `uv`, PEP 440 strategy,
//! trusted publishing)"* — the **publish** half of the discover/edit/
//! publish/resume acceptance row. The grammar is VERIFIED from the
//! installed tool (`uv 0.13.9`, `uv publish --help`,
//! `docs/research/pypi-uv-grammars.md` §2): `--dry-run` is uv's own
//! forecast flag, and halyard invents no flag the config did not set —
//! the bare argv is the whole contract (ADR-0033's unset-is-semantic
//! rule at uv's grammar; under trusted publishing uv decides on its
//! own).

use halyard_eco_pypi::uv_publish_args;

/// The live argv is bare: `uv publish` with no invented flags — the
/// tool's own posture governs credentials, trusted publishing and the
/// index.
#[test]
fn live_argv_is_bare() {
    assert_eq!(uv_publish_args(false), vec!["publish".to_owned()]);
}

/// The dry argv forecasts without uploading (`--dry-run` is uv's own:
/// "Perform a dry run without uploading files"). Deterministic (I3):
/// the forecast shows exactly what the live run would spawn.
#[test]
fn dry_argv_forecasts_without_uploading() {
    assert_eq!(
        uv_publish_args(true),
        vec!["publish".to_owned(), "--dry-run".to_owned()]
    );
}
