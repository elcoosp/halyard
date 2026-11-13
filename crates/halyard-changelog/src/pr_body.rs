//! The §11.3 Release-PR body renderer (M4.3a): the stable, parseable
//! Release-PR body the §11.1 state machine carries in its `PrSpec` —
//! and the [`extract_embedded_plan`] inverse that lets `release` /
//! `publish` verify I8 by reading the plan back out of a merged Release
//! PR.
//!
//! The body is deterministic (I3): a given [`Plan`] renders byte-for-byte
//! identically on every run, so a re-planning pass that derives the same
//! plan produces the same body and the machine's no-op effect
//! (`MachineEffect::PrUnchanged` on the forge side) holds.
//!
//! Layout (§11.3 skeleton, top to bottom):
//!
//! ```markdown
//! ## 🚀 Release plan
//!
//! | Package | Current | Next | Bump | Why |
//! |---|---|---|---|---|
//! | `core` | 0.4.1 | **0.5.0** | minor | `feat` (ab12cd3) |
//!
//! <details><summary>Changelog preview</summary>
//!
//! …the planner's per-release `entry_md` sections, verbatim…
//!
//! </details>
//!
//! > Override: add `Release-As: 2.0.0` to a commit on `main`, or edit
//! > labels. Run `halyard explain core` locally.
//!
//! <!-- halyard:plan v1 blake3:… <compressed base64 plan JSON> -->
//! ```
//!
//! The **Why** column renders the first entry of each release's reason
//! chain through a fixed, deterministic map (the wire reason carries no
//! prose): `Commit` → `` `kind` `` (with `!` for breaking and the short
//! OID), `Cascade` → `cascade: `via``, `ReleaseAs` → `` `Release-As: v` ``,
//! `Changeset` → `changeset: `file``, `InitialRelease` → `initial
//! release`. The full chain stays available through `halyard explain`
//! (§5.6); the table is the skimmable summary.
//!
//! The **embedded plan** closes the body: the plan's canonical JSON
//! (ADR-0005), DEFLATE-compressed and base64-encoded, prefixed by the
//! plan hash — the same `blake3:` digest [`plan_hash`] computes. The
//! renderer recomputes the hash from the plan itself and never trusts a
//! carried `plan_hash` field. [`extract_embedded_plan`] decodes,
//! decompresses, re-parses, re-hashes and rejects any mismatch — a
//! tampered or truncated body yields no plan (I8 fail-closed), only
//! [`ChangelogError::PrBody`].

use std::fmt::Write as _;
use std::io::Read as _;
use std::io::Write as _;

use base64::Engine as _;
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;

use halyard_core::canonical::{canonical_json, plan_hash};
use halyard_core::plan::{Plan, PlanReason};

use crate::ChangelogError;

/// The machine-readable plan marker's schema token (§11.3: the embedded
/// plan is `halyard-plan/v1` JSON). The marker reads
/// `<!-- halyard:plan v1 <hash> <payload> -->`.
const MARKER_SCHEMA: &str = "halyard:plan";

/// The stable override footer (§11.3): points at the escape hatches the
/// state machine honours — a `Release-As:` footer on the protected base
/// branch (invariant I7) or the PR labels.
const OVERRIDE_FOOTER: &str = "> Override: add `Release-As: 2.0.0` to a commit on `main`, \
     or edit labels. Run `halyard explain <package>` locally.";

/// Renders the §11.3 Release-PR body for `plan`.
///
/// Total over plans: an empty plan renders the heading with a
/// "No pending releases." note (the §11.1 machine never *upserts* such a
/// body — it closes a stale PR instead — but tools previewing any plan
/// document get a well-formed body, and [`extract_embedded_plan`]
/// round-trips it).
///
/// # Errors
///
/// [`ChangelogError::PrBody`] when the embedded plan cannot be
/// canonicalized for hashing (a frozen-wire-type serialization failure —
/// unreachable by construction, kept fail-closed).
pub fn render_release_body(plan: &Plan) -> Result<String, ChangelogError> {
    let hash = embedded_hash(plan);
    let mut body = String::with_capacity(2048);
    body.push_str("## 🚀 Release plan\n\n");
    if plan.releases.is_empty() {
        body.push_str("No pending releases.\n\n");
    } else {
        body.push_str("| Package | Current | Next | Bump | Why |\n");
        body.push_str("|---|---|---|---|---|\n");
        for release in &plan.releases {
            let _ = writeln!(
                body,
                "| `{}` | {} | **{}** | {} | {} |",
                release.package,
                release.from,
                release.to,
                bump_name(release.bump),
                why_of(release.reasons.first()),
            );
        }
        body.push('\n');
    }
    // §16.3: the body shows what was considered and skipped, so a
    // reviewer never has to guess why a package is absent from the
    // table (the plan's `skipped` rows, deterministically rendered).
    if !plan.skipped.is_empty() {
        let list = plan
            .skipped
            .iter()
            .map(|skip| format!("`{}` ({})", skip.package, skip.reason))
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(body, "> Skipped: {list}\n");
    }
    // Changelog preview: the planner's own entry sections, verbatim
    // (plan and PR body can never disagree, I8). Skipped when no
    // release carries a changelog entry.
    let preview: Vec<&str> = plan
        .releases
        .iter()
        .filter_map(|release| release.changelog.as_ref().map(|c| c.entry_md.as_str()))
        .collect();
    if !preview.is_empty() {
        body.push_str("<details><summary>Changelog preview</summary>\n\n");
        for entry in &preview {
            let _ = writeln!(body, "{entry}");
        }
        body.push_str("</details>\n\n");
    }
    body.push_str(OVERRIDE_FOOTER);
    body.push_str("\n\n");
    // The machine-readable close: schema, recomputed plan hash, then the
    // compressed canonical JSON (I8: `release`/`publish` read the plan
    // back out of the merged PR instead of trusting prose).
    let blob = embed_plan(plan)?;
    let _ = writeln!(body, "<!-- {MARKER_SCHEMA} v1 {hash} {blob} -->");
    Ok(body)
}

/// Reads the embedded plan back out of a rendered Release-PR body (§11.3;
/// the I8 verifier behind `release`/`publish`).
///
/// The marker must be well-formed, the payload must decode
/// (base64 → DEFLATE → canonical JSON → [`Plan`]) and the decoded plan's
/// recomputed hash must equal both the plan's own `plan_hash` field and
/// the hash the marker carries. Anything else is
/// [`ChangelogError::PrBody`] — a tampered body never yields a plan.
///
/// # Errors
///
/// [`ChangelogError::PrBody`] for a missing marker, a malformed payload,
/// an undecodable document or any hash mismatch.
pub fn extract_embedded_plan(body: &str) -> Result<Plan, ChangelogError> {
    let marker = body
        .lines()
        .rev()
        .find(|line| line.contains("<!-- halyard:plan "))
        .ok_or_else(|| {
            ChangelogError::pr_body(
                "no `<!-- halyard:plan v1 … -->` marker; this body was not written by \
             render_release_body (or the marker was stripped)",
            )
        })?;
    let inner = marker
        .strip_prefix("<!-- ")
        .and_then(|rest| rest.strip_suffix(" -->"))
        .ok_or_else(|| ChangelogError::pr_body("the plan marker is not a complete HTML comment"))?;
    let mut fields = inner.split_whitespace();
    let schema = fields.next().unwrap_or_default();
    if schema != MARKER_SCHEMA {
        return Err(ChangelogError::pr_body(format!(
            "unknown plan marker schema {schema:?}; expected {MARKER_SCHEMA:?}"
        )));
    }
    let version = fields.next().unwrap_or_default();
    if version != "v1" {
        return Err(ChangelogError::pr_body(format!(
            "unknown plan marker version {version:?}; expected \"v1\""
        )));
    }
    let hash = fields.next().unwrap_or_default();
    if !hash.starts_with("blake3:") {
        return Err(ChangelogError::pr_body(
            "the plan marker carries no `blake3:` hash field",
        ));
    }
    let blob = fields
        .next()
        .ok_or_else(|| ChangelogError::pr_body("the plan marker carries no payload field"))?;
    if fields.next().is_some() {
        return Err(ChangelogError::pr_body(
            "the plan marker carries unexpected trailing fields",
        ));
    }

    let compressed = base64::engine::general_purpose::STANDARD
        .decode(blob)
        .map_err(|reason| ChangelogError::pr_body(format!("payload is not base64: {reason}")))?;
    let mut decoder = DeflateDecoder::new(compressed.as_slice());
    let mut json = String::new();
    decoder.read_to_string(&mut json).map_err(|reason| {
        ChangelogError::pr_body(format!("payload is not DEFLATE text: {reason}"))
    })?;
    let plan: Plan = serde_json::from_str(&json).map_err(|reason| {
        ChangelogError::pr_body(format!(
            "payload is not a halyard-plan/v1 document: {reason}"
        ))
    })?;

    // I8 fail-closed: the decoded plan must be self-consistent (its own
    // hash field) and bound to this body (the marker's hash).
    let recomputed = embedded_hash(&plan);
    if plan.plan_hash.is_empty() || plan.plan_hash != recomputed {
        return Err(ChangelogError::pr_body(
            "the embedded plan's own plan_hash does not match its content (re-plan instead \
             of trusting a tampered document)",
        ));
    }
    if plan.plan_hash != hash {
        return Err(ChangelogError::pr_body(format!(
            "the marker hash {hash:?} does not match the embedded plan's hash; the body was \
             edited after rendering (re-plan instead of trusting it)"
        )));
    }
    Ok(plan)
}

/// The hash the marker carries: recomputed from the plan itself
/// (`plan_hash` never trusts a carried field).
fn embedded_hash(plan: &Plan) -> String {
    plan_hash(plan)
}

/// DEFLATE-compresses and base64-encodes the plan's canonical JSON (the
/// §11.3 "compressed base64 plan JSON" payload).
fn embed_plan(plan: &Plan) -> Result<String, ChangelogError> {
    // The embedded document is the *complete* `halyard-plan/v1` JSON:
    // the plan with its `plan_hash` field filled from the recomputed
    // digest, so `extract_embedded_plan` can check the document against
    // itself (I8) before checking it against the marker.
    let hash = plan_hash(plan);
    let mut embedded = plan.clone();
    embedded.plan_hash = hash;
    let value = serde_json::to_value(&embedded).map_err(|reason| {
        ChangelogError::pr_body(format!(
            "plan does not serialize (wire types are frozen): {reason}"
        ))
    })?;
    let json = canonical_json(&value);
    let mut encoder = DeflateEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(json.as_bytes()).map_err(|reason| {
        ChangelogError::pr_body(format!("plan payload compression failed: {reason}"))
    })?;
    let compressed = encoder.finish().map_err(|reason| {
        ChangelogError::pr_body(format!("plan payload compression failed: {reason}"))
    })?;
    Ok(base64::engine::general_purpose::STANDARD.encode(compressed))
}

/// The lowercase bump word the table renders (`Bump` serializes
/// lowercase on the wire; the table matches it verbatim).
fn bump_name(bump: halyard_core::plan::Bump) -> &'static str {
    match bump {
        halyard_core::plan::Bump::None => "none",
        halyard_core::plan::Bump::Patch => "patch",
        halyard_core::plan::Bump::Minor => "minor",
        halyard_core::plan::Bump::Major => "major",
    }
}

/// The Why-column text for a reason chain's head (deterministic map —
/// the wire reason carries kind/oid/edge, never prose; the full chain
/// lives behind `halyard explain`, §5.6).
fn why_of(reason: Option<&PlanReason>) -> String {
    match reason {
        Some(PlanReason::Commit {
            oid,
            kind,
            breaking,
        }) => {
            let short = &oid[..oid.len().min(7)];
            if *breaking {
                format!("`{kind}!` ({short})")
            } else {
                format!("`{kind}` ({short})")
            }
        }
        Some(PlanReason::ReleaseAs { version, .. }) => format!("`Release-As: {version}`"),
        Some(PlanReason::Changeset { file }) => format!("changeset: `{file}`"),
        Some(PlanReason::Cascade { via, .. }) => format!("cascade: `{via}`"),
        Some(PlanReason::InitialRelease) | None => "initial release".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn why_of_maps_every_reason_variant() {
        let cases: [(PlanReason, &str); 5] = [
            (
                PlanReason::Commit {
                    oid: "ab12cd34ef".to_owned(),
                    kind: "fix".to_owned(),
                    breaking: false,
                },
                "`fix` (ab12cd3)",
            ),
            (
                PlanReason::Commit {
                    oid: "ab12cd34ef".to_owned(),
                    kind: "feat".to_owned(),
                    breaking: true,
                },
                "`feat!` (ab12cd3)",
            ),
            (
                PlanReason::Cascade {
                    via: "core".to_owned(),
                    edge: "normal".to_owned(),
                    why: "policy_always".to_owned(),
                },
                "cascade: `core`",
            ),
            (
                PlanReason::ReleaseAs {
                    oid: "1234567".to_owned(),
                    version: "2.0.0".to_owned(),
                },
                "`Release-As: 2.0.0`",
            ),
            (PlanReason::InitialRelease, "initial release"),
        ];
        for (reason, expected) in cases {
            assert_eq!(why_of(Some(&reason)), expected, "{reason:?}");
        }
        assert_eq!(why_of(None), "initial release");
    }
}
