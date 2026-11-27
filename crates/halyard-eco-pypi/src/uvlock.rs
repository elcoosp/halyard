//! The `uv.lock` lockfile half (the §17 M8 `PyPI` cell's recorded-open
//! remainder, ADR-0056): the seam's native-refresh arm plus the refresh
//! verifier, at uv's own documented posture.
//!
//! uv's documentation is explicit about who owns the file (*"uv.lock is
//! a human-readable TOML file but is managed by uv and should not be
//! edited manually"*, *"The uv.lock format is specific to uv and not
//! usable by other tools"* — `docs/research/pypi-uv-grammars.md` §3),
//! so there is **no in-memory patcher** for `uv.lock`: the adapter's
//! `patch_lockfile` declines
//! ([`halyard_core::lockfile::LockfileOutcome::NeedsNative`]) at that
//! posture and the orchestrator's §9.2 rule-4 native refresh runs the
//! tool's own documented refresh (`uv lock` — *"Update the project's
//! lockfile"*; `--offline` — *"Disable network access"*, the
//! cargo/pnpm offline discipline, verified against the installed tool,
//! uv 0.13.10). This is the same native-only shape the npm family
//! rides for package-lock.json/yarn.lock/bun.lock (§9.3).
//!
//! The caller verifies the refresh's effect
//! ([`verify_native_refresh`], §9.2: "only workspace members may
//! change"): the fidelity contract was proven empirically against the
//! installed tool — a member version bump rewrites exactly the member's
//! `version` row; the header (`version`/`revision`/
//! `requires-python`), the `[manifest]` members and every sibling
//! `[[package]]` row survive verbatim. Anything else the refresh did is
//! a resolution change the plan cannot express and refuses
//! (`E-LOCK-NATIVE`).
//!
//! Layer note (§7.2): plain data over given bytes — the effectful
//! subprocess stays in the adapter (`refresh_lockfile_native`), the
//! verification is a pure parse.

use std::collections::{BTreeMap, BTreeSet};

use halyard_core::lockfile::NativeRefreshError;

/// The pypi-family lockfile name the orchestrator stages (the root
/// `uv.lock` uv writes for the workspace).
pub const UV_LOCKFILE: &str = "uv.lock";

/// Why the in-memory patcher declines (rendered in the apply summary):
/// uv's own documented management posture and the refresh argv that
/// owns the file.
pub const DECLINE_REASON: &str = "uv.lock is managed by uv and should not be edited manually \
     (uv's documented posture) — refresh the lockfile natively with `uv lock --offline`";

/// The native refresh argv (§9.2 rule 4): `uv lock` — *"Update the
/// project's lockfile"* — with `--offline` — *"Disable network access"*
/// (the installed tool's own help, uv 0.13.10; the cargo/pnpm offline
/// discipline: the refresh never touches the network, and the tool's
/// own error governs when the cache cannot resolve offline).
#[must_use]
pub fn refresh_command() -> (&'static str, &'static [&'static str], &'static str) {
    ("uv", &["lock", "--offline"], "uv lock --offline")
}

/// One lockfile's parsed shape: the rendered bytes of everything
/// outside the `[[package]]` rows (the header uv owns) plus each
/// package row's rendered bytes keyed by its `name`.
struct Snapshot {
    header: String,
    rows: BTreeMap<String, String>,
}

/// Parses one `uv.lock` into its verification snapshot. Anything the
/// TOML grammar cannot read is uv's business — the verifier refuses
/// rather than guesses.
fn parse_snapshot(raw: &[u8]) -> Result<Snapshot, String> {
    let text = std::str::from_utf8(raw).map_err(|e| format!("the lockfile is not UTF-8: {e}"))?;
    let doc = toml_edit::Document::parse(text)
        .map_err(|e| format!("the lockfile does not parse as TOML: {e}"))?;
    let mut header = String::new();
    let mut rows = BTreeMap::new();
    for (key, item) in doc.iter() {
        if key == "package" {
            let Some(array) = item.as_array_of_tables() else {
                return Err(format!("the `[[package]]` rows are not an array: {key}"));
            };
            for table in array {
                let name = table
                    .get("name")
                    .and_then(toml_edit::Item::as_value)
                    .and_then(toml_edit::Value::as_str)
                    .ok_or("a `[[package]]` row carries no `name`")?;
                let rendered = toml_edit::Item::Table(table.clone()).to_string();
                if rows.insert(name.to_owned(), rendered).is_some() {
                    return Err(format!("the package {name:?} appears more than once"));
                }
            }
        } else {
            header.push_str(key);
            header.push('\u{0}');
            header.push_str(&item.to_string());
            header.push('\u{0}');
        }
    }
    Ok(Snapshot { header, rows })
}

/// Verifies the native refresh's effect (§9.2: "only workspace members
/// may change"): the header and every non-member row must be
/// byte-identical; a member row may move (uv rewrote it) and joins the
/// returned moved set. Everything else is a resolution change the plan
/// cannot express and refuses (`E-LOCK-NATIVE`).
///
/// # Errors
///
/// [`NativeRefreshError::Verification`] when either snapshot does not
/// parse, the header changed, a non-member row moved, or a row
/// appeared/disappeared for a non-member.
pub fn verify_native_refresh(
    before: &[u8],
    after: &[u8],
    members: &[String],
) -> Result<Vec<String>, NativeRefreshError> {
    let verification = |reason: String| NativeRefreshError::Verification { reason };
    let before = parse_snapshot(before).map_err(verification)?;
    let after = parse_snapshot(after).map_err(verification)?;
    let member = |name: &str| members.iter().any(|m| m == name);
    let mut moved: BTreeSet<String> = BTreeSet::new();

    // The header (`version`/`revision`/`requires-python`, the
    // `[manifest]` members) is uv's format business: a change is a
    // format or requirement change the plan cannot express.
    if before.header != after.header {
        return Err(verification(
            "the lockfile header changed (a format or requirement change uv owns)".to_owned(),
        ));
    }
    for (name, after_row) in &after.rows {
        let Some(before_row) = before.rows.get(name) else {
            if member(name) {
                moved.insert(name.clone());
                continue;
            }
            return Err(verification(format!(
                "row for {name:?} appeared (a resolution change)"
            )));
        };
        if before_row == after_row {
            continue;
        }
        if member(name) {
            moved.insert(name.clone());
        } else {
            return Err(verification(format!(
                "row for {name:?} moved but it is not a workspace member"
            )));
        }
    }
    for name in before.rows.keys() {
        if !after.rows.contains_key(name) {
            if member(name) {
                moved.insert(name.clone());
            } else {
                return Err(verification(format!(
                    "row for {name:?} disappeared (a resolution change)"
                )));
            }
        }
    }
    Ok(moved.into_iter().collect())
}
