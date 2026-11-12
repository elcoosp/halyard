//! The `halyard-plan/v1` wire format (§5.8) — the public contract between the
//! CLI, the GitHub Action, Release PR bodies, tests and third-party tools.
//!
//! These are the *wire* types: identities, versions and OIDs are strings here
//! (as they appear in serialized JSON). The richer in-memory model (interned
//! package ids, parsed conventional commits, byte-span edit sites) is layered
//! on top by the planning pipeline in later milestones and maps 1:1 onto
//! these types when a `Plan` is emitted or consumed.

use serde::{Deserialize, Serialize};

/// Value of the `schema` field emitted by every plan produced by this major
/// line of the tool (§5.8).
pub const PLAN_SCHEMA_V1: &str = "halyard-plan/v1";

/// A complete, serializable release plan (§5.8).
///
/// The plan is hashable (blake3 over its canonical JSON) and embedded in the
/// Release PR body so later runs can verify it and `release`/`publish` can
/// trust it (invariant I8). Producing the hash is [`crate::canonical::plan_hash`]'s
/// job; these types carry it as [`Plan::plan_hash`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
pub struct Plan {
    /// Always [`PLAN_SCHEMA_V1`] for documents handled by this code line.
    #[serde(default = "default_plan_schema")]
    pub schema: String,
    /// Branch the plan was computed against (e.g. `main`).
    pub base_branch: String,
    /// Hex commit SHA the plan was computed from.
    pub head: String,
    /// `blake3:<hex>` over the canonical JSON of the plan without this field.
    pub plan_hash: String,
    /// Packages to release, in discovery order (see [`Plan::order`] for the
    /// publish order).
    pub releases: Vec<PlanRelease>,
    /// Byte patches to apply so the working tree reflects the plan.
    pub edits: Vec<PlanEdit>,
    /// Topological publish order: package names, dependencies first.
    pub order: Vec<String>,
    /// Packages considered but not released, with the reason.
    pub skipped: Vec<PlanSkip>,
}

fn default_plan_schema() -> String {
    PLAN_SCHEMA_V1.to_owned()
}

/// One package release inside a [`Plan`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
pub struct PlanRelease {
    /// Package name as discovered (crate name or npm package name).
    pub package: String,
    /// Ecosystem identifier: `cargo`, `npm`, … (open set; §5.2).
    pub ecosystem: String,
    /// Current version in the manifest, or `"0.0.0"` when the package has no
    /// version yet (unpublishable, initial state).
    pub from: String,
    /// Next version after applying the bump.
    pub to: String,
    /// Bump level derived from commits/cascade/policy (§10.1).
    pub bump: Bump,
    /// Tag that will be created for this release (rendered tag template,
    /// appendix A.3).
    pub tag: String,
    /// Why the bump happened; the reason chain that powers `halyard explain`
    /// (§5.6).
    pub reasons: Vec<PlanReason>,
    /// Changelog update rendered for this release, when the package has a
    /// changelog file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changelog: Option<PlanChangelog>,
    /// Registry publishing decision for this release (§13).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publish: Option<PlanPublish>,
}

/// Bump level, totally ordered `None < Patch < Minor < Major` (§5.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum Bump {
    /// No version change.
    None,
    /// `z` component bumps.
    Patch,
    /// `y` component bumps.
    Minor,
    /// `x` component bumps.
    Major,
}

impl Bump {
    /// The strongest of two bumps (§10.1 precedence: higher level wins).
    #[must_use]
    pub const fn max(self, other: Self) -> Self {
        match (self, other) {
            (Self::Major, _) | (_, Self::Major) => Self::Major,
            (Self::Minor, _) | (_, Self::Minor) => Self::Minor,
            (Self::Patch, _) | (_, Self::Patch) => Self::Patch,
            (Self::None, Self::None) => Self::None,
        }
    }

    /// True when this bump does not change the version.
    pub const fn is_none(self) -> bool {
        matches!(self, Self::None)
    }
}

/// One entry of a reason chain (§5.6), internally tagged by `type`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PlanReason {
    /// A parsed conventional commit contributed to the bump.
    Commit {
        /// Hex SHA of the commit.
        oid: String,
        /// Commit type as parsed (`feat`, `fix`, …); open set.
        kind: String,
        /// Whether the commit was breaking (`!` or `BREAKING CHANGE` footer).
        breaking: bool,
    },
    /// A `Release-As: <version>` footer forced this version (honoured only
    /// from commits on the protected base branch, invariant I7).
    ReleaseAs {
        /// Hex SHA of the commit carrying the footer.
        oid: String,
        /// Forced version string.
        version: String,
    },
    /// A change file (`.halyard/changes/*.md`) contributed the bump (§9.4).
    Changeset {
        /// Repo-relative path of the change file.
        file: String,
    },
    /// The bump cascaded from a dependency (§10.4).
    Cascade {
        /// Name of the upstream package that was bumped.
        via: String,
        /// Dependency edge kind that connects the packages (`normal`,
        /// `build`, `dev`, `peer`, `optional`).
        edge: String,
        /// Why the cascade fired: `requirement_unsatisfied`, `policy_always`
        /// or `lockstep_group`.
        why: String,
    },
    /// The package has no release base yet; this is its initial release.
    InitialRelease,
}

/// Changelog entry preview for a release (§9.4).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
pub struct PlanChangelog {
    /// Repo-relative path of the changelog file that will be edited.
    pub file: String,
    /// Rendered markdown entry (one version section).
    pub entry_md: String,
}

/// Publishing decision for a release (§13).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
pub struct PlanPublish {
    /// Registry identifier (`crates.io`, `npm`, …; open set).
    pub registry: String,
    /// Whether publishing is enabled for this package in this plan.
    pub enabled: bool,
}

/// Byte patches for one file (§5.8 `edits`, §9.1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
pub struct PlanEdit {
    /// Repo-relative, forward-slash file path.
    pub file: String,
    /// Non-overlapping byte patches, applied right-to-left; byte offsets are
    /// over the file as it exists at [`Plan::head`].
    pub patches: Vec<PlanPatch>,
}

/// A single byte-range replacement (§9.1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
pub struct PlanPatch {
    /// Inclusive start byte offset.
    pub start: u32,
    /// Exclusive end byte offset.
    pub end: u32,
    /// Replacement text (UTF-8).
    pub text: String,
}

/// A package considered but not released (§5.8 `skipped`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
pub struct PlanSkip {
    /// Package name.
    pub package: String,
    /// Machine-readable reason (`no_changes`, `release_disabled`, …).
    pub reason: String,
}

/// JSON Schema document (`draft 2020-12`) for `halyard-plan/v1`, generated
/// from these types. Used by `xtask schema-check` to keep
/// `schemas/halyard-plan-v1.json` in lockstep (M0.7 drift test).
#[cfg(feature = "schema-gen")]
pub fn schema_document() -> Result<serde_json::Value, String> {
    const ID: &str = "https://halyard.dev/schemas/halyard-plan-v1.json";
    const NOTE: &str = "Generated by `cargo run -p xtask -- schema-check --write` from the \
                        halyard-core plan types. Do not edit by hand; an ADR is required for \
                        format changes.";
    let mut generator =
        schemars::SchemaGenerator::new(schemars::generate::SchemaSettings::draft2020_12());
    let schema = generator.root_schema_for::<Plan>();
    let mut value = serde_json::to_value(&schema)
        .map_err(|e| format!("plan schema serialization failed: {e}"))?;
    let obj = value
        .as_object_mut()
        .ok_or_else(|| "plan root schema is not a JSON object".to_owned())?;
    obj.insert("$id".to_owned(), serde_json::Value::String(ID.to_owned()));
    obj.insert(
        "$comment".to_owned(),
        serde_json::Value::String(NOTE.to_owned()),
    );
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_plan_json_shape_matches_spec_example() {
        let plan = Plan {
            schema: PLAN_SCHEMA_V1.to_owned(),
            base_branch: "main".to_owned(),
            head: "9f2ca67".to_owned(),
            plan_hash: "blake3:ab12".to_owned(),
            releases: vec![PlanRelease {
                package: "core".to_owned(),
                ecosystem: "cargo".to_owned(),
                from: "0.4.1".to_owned(),
                to: "0.5.0".to_owned(),
                bump: Bump::Minor,
                tag: "core-v0.5.0".to_owned(),
                reasons: vec![PlanReason::Commit {
                    oid: "ab12cd34".to_owned(),
                    kind: "feat".to_owned(),
                    breaking: false,
                }],
                changelog: Some(PlanChangelog {
                    file: "crates/core/CHANGELOG.md".to_owned(),
                    entry_md: "## 0.5.0\n\n### Features\n\n- …".to_owned(),
                }),
                publish: Some(PlanPublish {
                    registry: "crates.io".to_owned(),
                    enabled: true,
                }),
            }],
            edits: vec![PlanEdit {
                file: "crates/core/Cargo.toml".to_owned(),
                patches: vec![PlanPatch {
                    start: 123,
                    end: 128,
                    text: "0.5.0".to_owned(),
                }],
            }],
            order: vec!["core".to_owned(), "cli".to_owned()],
            skipped: vec![PlanSkip {
                package: "docs".to_owned(),
                reason: "no_changes".to_owned(),
            }],
        };

        let json = serde_json::to_value(&plan).expect("sample plan serializes");
        let obj = json.as_object().expect("plan is an object");
        assert_eq!(
            obj.get("schema").and_then(serde_json::Value::as_str),
            Some(PLAN_SCHEMA_V1)
        );
        assert_eq!(
            obj.get("base_branch").and_then(serde_json::Value::as_str),
            Some("main")
        );
        let release = obj
            .get("releases")
            .and_then(serde_json::Value::as_array)
            .and_then(|a| a.first())
            .and_then(serde_json::Value::as_object)
            .expect("one release");
        assert_eq!(
            release.get("bump").and_then(serde_json::Value::as_str),
            Some("minor")
        );
        let reason = release
            .get("reasons")
            .and_then(serde_json::Value::as_array)
            .and_then(|a| a.first())
            .and_then(serde_json::Value::as_object)
            .expect("one reason");
        assert_eq!(
            reason.get("type").and_then(serde_json::Value::as_str),
            Some("commit")
        );
        assert_eq!(
            reason.get("kind").and_then(serde_json::Value::as_str),
            Some("feat")
        );

        let roundtrip: Plan = serde_json::from_value(json).expect("plan deserializes");
        assert_eq!(roundtrip, plan);
    }
}
