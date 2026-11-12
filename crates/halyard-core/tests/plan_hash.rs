//! `plan_hash` + canonical JSON determinism (§5.8, invariant I3).
//!
//! The plan hash is blake3 over the *canonical JSON* of the plan (sorted
//! keys, no insignificant whitespace) with the `plan_hash` field scrubbed to
//! the empty string. These tests pin the format with an insta snapshot and
//! prove the byte-identical determinism requirement.

use halyard_core::plan::{Bump, Plan, PlanEdit, PlanPatch, PlanReason, PlanRelease};
use halyard_core::{canonical_json, plan_hash};
use insta::assert_snapshot;

/// The §5.8 spec sample plan, rebuilt as a concrete value.
fn sample_plan() -> Plan {
    Plan {
        schema: halyard_core::PLAN_SCHEMA_V1.to_owned(),
        base_branch: "main".to_owned(),
        head: "9f2ca67".to_owned(),
        plan_hash: "blake3:deadbeef".to_owned(),
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
            changelog: None,
            publish: None,
        }],
        edits: vec![PlanEdit {
            file: "crates/core/Cargo.toml".to_owned(),
            patches: vec![PlanPatch {
                start: 123,
                end: 128,
                text: "0.5.0".to_owned(),
            }],
        }],
        order: vec!["core".to_owned()],
        skipped: vec![],
    }
}

#[test]
fn plan_hash_is_blake3_prefixed_64_hex() {
    let hash = plan_hash(&sample_plan());
    let hex = hash.strip_prefix("blake3:").unwrap_or_default();
    assert_eq!(hash.len(), 7 + 64, "unexpected hash shape: {hash}");
    assert!(
        hex.bytes().all(|b| b.is_ascii_hexdigit()),
        "hash must be lowercase hex: {hash}"
    );
}

#[test]
fn plan_hash_is_deterministic_and_scrubs_plan_hash_field() {
    let mut variant = sample_plan();
    let first = plan_hash(&variant);
    variant.plan_hash = "totally different".to_owned();
    assert_eq!(
        first,
        plan_hash(&variant),
        "plan_hash field must be scrubbed"
    );
    assert_eq!(
        first,
        plan_hash(&sample_plan()),
        "re-run must be byte-identical"
    );
}

#[test]
fn plan_hash_changes_with_content() {
    let mut variant = sample_plan();
    variant.base_branch = "develop".to_owned();
    assert_ne!(plan_hash(&variant), plan_hash(&sample_plan()));
}

#[test]
fn sample_plan_canonical_json_snapshot() {
    let value =
        serde_json::to_value(sample_plan()).unwrap_or_else(|e| panic!("plan to value: {e}"));
    assert_snapshot!("sample_plan_canonical_json", canonical_json(&value));
}

#[test]
fn canonical_json_sorts_object_keys() {
    let text = r#"{"zeta":1,"alpha":{"nested_b":true,"nested_a":null},"mid":[3,1]}"#;
    let value: serde_json::Value =
        serde_json::from_str(text).unwrap_or_else(|e| panic!("parse fixture: {e}"));
    let canonical = canonical_json(&value);
    assert_eq!(
        canonical,
        r#"{"alpha":{"nested_a":null,"nested_b":true},"mid":[3,1],"zeta":1}"#
    );
}
