//! Example: the `uv.lock` lockfile half (the §17 M8 `PyPI` cell's
//! recorded-open remainder, ADR-0056).
//!
//! Run: `cargo run -p halyard-eco-pypi --example patch_uvlock`
//!
//! Demonstrates the seam's native-only shape at uv's own documented
//! posture (*"uv.lock is a human-readable TOML file but is managed by
//! uv and should not be edited manually"* —
//! `docs/research/pypi-uv-grammars.md` §3): the adapter's
//! `patch_lockfile` declines to `NeedsNative` naming the refresh argv,
//! the §9.2 rule-4 refresh runs `uv lock --offline` (the tool's own
//! documented refresh, `refresh_command()`), and the caller verifies
//! the effect with `verify_native_refresh` — a member version bump
//! moves exactly the member's `version` row (the observed uv 0.13.10
//! fidelity); anything else refuses `E-LOCK-NATIVE`.

use halyard_core::EcosystemAdapter;
use halyard_core::lockfile::LockfileVersionChange;
use halyard_core::package::RelPath;
use halyard_eco_pypi::{PyPiAdapter, uvlock};

/// The lockfile uv wrote for a workspace with one member (`app_a`) and
/// a virtual root — the uv 0.13.10 shape.
const LOCKFILE: &str = r#"version = 1
revision = 5
requires-python = ">=3.9"

[manifest]
members = [
    "app-a",
    "root",
]

[[package]]
name = "app-a"
version = "0.4.1"
source = { editable = "app_a" }

[[package]]
name = "root"
version = "0.0.0"
source = { virtual = "." }
"#;

/// The same lockfile after the member's pyproject moved to 0.4.2 and
/// the native refresh ran: exactly the member's `version` row changed.
const REFRESHED: &str = r#"version = 1
revision = 5
requires-python = ">=3.9"

[manifest]
members = [
    "app-a",
    "root",
]

[[package]]
name = "app-a"
version = "0.4.2"
source = { editable = "app_a" }

[[package]]
name = "root"
version = "0.0.0"
source = { virtual = "." }
"#;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let changes = [LockfileVersionChange {
        name: "app-a".to_owned(),
        from: "0.4.1".to_owned(),
        to: "0.4.2".to_owned(),
    }];
    let file = RelPath::new("uv.lock").ok_or("uv.lock is a valid relative path")?;

    // The decline is the half's honest shape: no in-memory patcher —
    // uv owns the file and the refresh argv names the native pass.
    let outcome = PyPiAdapter::new().patch_lockfile(&file, LOCKFILE.as_bytes(), &changes)?;
    println!("patch_lockfile: {outcome:?}");

    // The refresh argv the orchestrator spawns (§9.2 rule 4).
    let (program, args, rendered) = uvlock::refresh_command();
    println!("native refresh: {rendered} (spawn {program} {args:?})");

    // The caller verifies the effect: only member rows may move.
    let moved = uvlock::verify_native_refresh(
        LOCKFILE.as_bytes(),
        REFRESHED.as_bytes(),
        &["app-a".to_owned(), "root".to_owned()],
    )?;
    println!("verified: members moved = {moved:?}");

    // A refresh that touched a non-member row refuses fail-closed.
    let tampered = REFRESHED.replace(
        "name = \"root\"\nversion = \"0.0.0\"",
        "name = \"root\"\nversion = \"9.9.9\"",
    );
    match uvlock::verify_native_refresh(
        LOCKFILE.as_bytes(),
        tampered.as_bytes(),
        &["app-a".to_owned()],
    ) {
        Err(error) => println!("a non-member move refuses: {error}"),
        Ok(moved) => return Err(format!("a non-member move must refuse, got {moved:?}").into()),
    }
    Ok(())
}
